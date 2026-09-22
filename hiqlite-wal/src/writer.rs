use crate::error::Error;
use crate::lockfile::LockFile;
use crate::log_store_impl::{deserialize, serialize};
use crate::metadata::Metadata;
use crate::reader::LogReadMemo;
use crate::wal::WalFileSet;
use openraft::{LeaderId, LogId};
use std::borrow::Cow;
use std::fmt::{Debug, Formatter};
use std::io;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use thread_priority::ThreadPriority;
use tokio::sync::{oneshot, watch};
use tokio::time::Interval;
use tokio::{task, time};
use tracing::{debug, error, warn};

/// The log I/O completion notification for one dispatched append.
///
/// It carries a result so that the writer can report *which* of the three outcomes in
/// [`complete_append`] occurred. The OpenRaft adapter forwards the value verbatim to
/// `LogFlushed::log_io_completed`. The parameter makes the failure *expressible*, which the
/// previous `FnOnce()` did not; it does not make ignoring the failure impossible, because a
/// callback may still discard its argument. That the real adapter does not is a property of the
/// adapter, held by `log_store_impl::tests::append_adapter_forwards_a_persistence_failure_to_openraft`.
pub type AppendCompletion = Box<dyn FnOnce(Result<(), io::Error>) + Send>;

pub enum Action {
    Append {
        rx: flume::Receiver<Option<(u64, Vec<u8>)>>,
        callback: AppendCompletion,
        ack: oneshot::Sender<Result<(), Error>>,
    },
    Remove {
        from: u64,
        until: u64,
        last_log: Option<Vec<u8>>,
        ack: oneshot::Sender<Result<(), Error>>,
    },
    Vote {
        value: Vec<u8>,
        ack: oneshot::Sender<Result<(), Error>>,
    },
    Sync,
    Shutdown(oneshot::Sender<()>),
}

impl Debug for Action {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Append { .. } => write!(f, "Action::Append"),
            Action::Remove { .. } => write!(f, "Action::Remove"),
            Action::Vote { .. } => write!(f, "Action::Vote"),
            Action::Sync => write!(f, "Action::Sync"),
            Action::Shutdown(_) => write!(f, "Action::Shutdown"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogSync {
    Immediate,
    ImmediateAsync,
    IntervalMillis(u64),
}

impl TryFrom<&str> for LogSync {
    type Error = Error;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "immediate" => Ok(Self::Immediate),
            "immediate_async" => Ok(Self::ImmediateAsync),
            v => {
                if let Some(ms) = v.strip_prefix("interval_") {
                    let Ok(ms) = ms.parse::<u64>() else {
                        return Err(Error::Generic(
                            format!(
                                "Invalid value for log_sync interval, cannot parse as u64: {v}"
                            )
                            .into(),
                        ));
                    };
                    Ok(Self::IntervalMillis(ms))
                } else {
                    Err(Error::Generic(
                        format!("Cannot parse LogSync - invalid value: {v}").into(),
                    ))
                }
            }
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn spawn(
    base_path: String,
    lockfile: LockFile,
    sync: LogSync,
    wal_size: u32,
    wal_deep_integrity_check: bool,
    meta: Arc<RwLock<Metadata>>,
) -> Result<
    (
        flume::Sender<Action>,
        Arc<RwLock<WalFileSet>>,
        watch::Receiver<Option<String>>,
    ),
    Error,
> {
    let mut set = WalFileSet::read(base_path, wal_size)?;
    // TODO emit a warning log in that case and tell the user how to resolve or "force start" in
    // that case, or should be maybe `auto-heal` as much as possible?
    let mut buf = Vec::with_capacity(32);
    set.check_integrity(&mut buf, wal_deep_integrity_check)?;
    if set.files.is_empty() {
        buf.clear();
        set.add_file(wal_size, &mut buf)?;
    }
    let wal_locked = Arc::new(RwLock::new(set.clone_no_map()));

    // TODO remove with version <= 0.13
    // This is a fix for a bug from previous versions. Can be removed in later ones,
    // it would be safe to do probably around version >= 0.13.
    if meta.read()?.last_purged_log_id.is_none()
        && let Some(front) = set.files.front_mut()
        && front.wal_no > 1
        && front.id_from > 2
    {
        warn!("Trying to fix bad LogState for `last_purged_logid`");
        let mut buf = Vec::with_capacity(16);
        let mut memo: Option<LogReadMemo> = None;
        front.mmap()?;
        front.read_logs(front.id_from, front.id_until, &mut memo, &mut buf)?;
        let (_, bytes) = buf.first().unwrap();
        let log: openraft::log_id::LogId<u64> = deserialize(bytes)?;
        let log_id: openraft::log_id::LogId<u64> = LogId {
            leader_id: LeaderId {
                term: log.leader_id.term,
                node_id: log.leader_id.node_id,
            },
            index: log.index - 1,
        };
        front.mmap_drop();

        meta.write()?.last_purged_log_id = Some(serialize(&log_id)?);
        Metadata::write(meta.clone(), &set.base_path)?;
    }

    let (tx, rx) = flume::bounded::<Action>(1);
    let wal = wal_locked.clone();
    let snc = sync.clone();
    let reported_path = set.base_path.clone();

    // The termination report gets a consumer.
    //
    // `008` KD-3 recorded that the ERROR log below has none: nothing above this crate can tell
    // a writer that has ended from one that is running, so a node whose log storage had died
    // went on answering as though it were healthy. This channel is that consumer's half.
    //
    // It carries three states, and the third is the one that matters. `None` means the writer
    // is running. `Some(reason)` means it ended and said why. A **closed** channel means the
    // sender was dropped without a reason, which is what a panic unwinding past this closure
    // looks like from the outside: still a terminated writer, still not a healthy node, and
    // not something this crate can describe beyond that.
    let (failure_tx, failure_rx) = watch::channel(None::<String>);
    thread::spawn(move || {
        let _failure_tx = failure_tx;
        // The writer thread's `JoinHandle` is deliberately not retained: nothing in this crate
        // manages its lifecycle, and joining it would be lifecycle management rather than error
        // reporting. What was missing is the report itself. `run` only returns `Err` on a failure
        // it could not handle, and that ends the thread, so logging here is the smallest place
        // that observes that exit.
        //
        // What it observes is exactly `run` returning `Err`. A panic inside `run` unwinds past
        // this line and is reported by the panic hook instead, and an abort or a process kill is
        // reported by neither. This is an error report for the one termination the writer can
        // describe, not process supervision.
        // Kept past `run`, so the thread can still answer what is queued after a termination.
        let rx_after = rx.clone();
        if let Err(err) = run(lockfile, meta, wal, set, rx, snc, wal_size) {
            let reason = format!(
                "Raft logs WAL writer for `{reported_path}` terminated with an unrecoverable \
                error: {err} - all further appends will fail until this process is restarted"
            );
            error!("{reason}");
            // Best effort: if nobody is watching, the log line is still the report.
            let _ = _failure_tx.send(Some(reason.clone()));

            // F-114. A terminated writer used to stop reading its channel, but the adapter
            // still held senders, and a queued message is kept alive for as long as any sender
            // is. An `Append` that reached the one-slot queue just before the termination was
            // never read and never dropped, so the entry channel inside it stayed open and the
            // adapter blocked on it forever: "a terminal writer fails its callers" held for
            // every interleaving except that one. The thread now answers every action with the
            // termination until the last sender is gone or a shutdown arrives.
            while let Ok(action) = rx_after.recv() {
                if !refuse_after_termination(action, &reason) {
                    break;
                }
            }
            // A shutdown ends the loop, and the receiver drops with this thread, but a queued
            // message outlives that for as long as a sender exists. Refuse whatever is already
            // queued behind the shutdown. This narrows the window; a send racing this line can
            // still land after it, as it can after a healthy writer's shutdown.
            while let Ok(action) = rx_after.try_recv() {
                refuse_after_termination(action, &reason);
            }
        }
    });

    if let LogSync::IntervalMillis(millis) = &sync {
        let interval = time::interval(Duration::from_millis(*millis));
        spawn_syncer(tx.clone(), interval);
    }

    Ok((tx, wal_locked, failure_rx))
}

/// Answer one action that reached a writer after it terminated. `false` ends the thread.
fn refuse_after_termination(action: Action, reason: &str) -> bool {
    let err = || Error::Internal(reason.to_string().into());
    match action {
        Action::Append { rx, callback, ack } => {
            // Dropping the entry receiver first releases a producer that is blocked sending.
            drop(rx);
            let _ = ack.send(Err(err()));
            callback(Err(err().as_io_error()));
        }
        Action::Remove { ack, .. } | Action::Vote { ack, .. } => {
            let _ = ack.send(Err(err()));
        }
        Action::Sync => {}
        Action::Shutdown(ack) => {
            // Not acknowledged: the writer did not stop cleanly, it had already failed, and a
            // shutdown answered `()` here was reported as a clean stop all the way up (found in
            // review). The caller sees the closed channel as an error.
            drop(ack);
            return false;
        }
    }
    true
}

/// Flush the active WAL file so that everything written to it is on disk.
///
/// `flush_async` only starts the writeback and returns, so a WAL flushed that way
/// is not known to be on disk. Only a flush that returns may clear `is_dirty`.
fn flush_blocking(
    wal: &mut WalFileSet,
    buf: &mut Vec<u8>,
    is_dirty: &mut bool,
) -> Result<(), Error> {
    if !*is_dirty {
        return Ok(());
    }

    let active = wal.active();
    buf.clear();
    active.update_header(buf)?;
    active.flush()?;
    *is_dirty = false;

    Ok(())
}

/// Deterministic persistence-failure injection for this crate's own tests.
///
/// Compiled out entirely outside `cfg(test)`, so the writer carries no production branch for it.
///
/// Injections are held per WAL base path and are owned by the guard that armed them. A single
/// shared slot was not enough: arming a second WAL overwrote an unconsumed injection armed for a
/// first one, and the path comparison at consumption could only turn that into a silently missed
/// injection, never restore it. Keying the store by WAL identity lets two writers hold
/// outstanding injections at the same time, and the guard removes whatever its WAL did not
/// consume, so nothing survives into a later test that reuses the directory.
#[cfg(test)]
pub(crate) mod fault {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Unconsumed injections per WAL base path. `BTreeMap::new` is `const`, so the store needs no
    /// lazy initialization.
    static ARMED: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

    /// Ownership of the persistence-failure injections armed for one WAL base path.
    ///
    /// One guard owns the whole outstanding count for its path, so a test arms its own WAL and
    /// nothing else. Dropping it disarms whatever is left, which is the cleanup that keeps a test
    /// that ends early from leaving an injection behind for the next one.
    #[must_use = "the injection is disarmed when its guard drops"]
    pub(crate) struct ArmedPersistenceFailure {
        base_path: String,
    }

    impl ArmedPersistenceFailure {
        /// How many of this WAL's injections are still unconsumed.
        pub(crate) fn outstanding(&self) -> usize {
            ARMED
                .lock()
                .unwrap()
                .get(&self.base_path)
                .copied()
                .unwrap_or(0)
        }
    }

    impl Drop for ArmedPersistenceFailure {
        fn drop(&mut self) {
            ARMED.lock().unwrap().remove(&self.base_path);
        }
    }

    /// Arm one persistence failure for the writer serving `base_path`.
    ///
    /// The returned guard must be held for as long as the injection is wanted.
    pub(crate) fn arm_persistence_failure(base_path: &str) -> ArmedPersistenceFailure {
        *ARMED
            .lock()
            .unwrap()
            .entry(base_path.to_string())
            .or_insert(0) += 1;
        ArmedPersistenceFailure {
            base_path: base_path.to_string(),
        }
    }

    /// Consume one injection armed for `base_path`, if any is outstanding for it.
    ///
    /// Only this WAL's own injections are visible here: another WAL's outstanding injection is
    /// never consumed, and never consumed twice.
    pub(crate) fn take_persistence_failure(base_path: &str) -> bool {
        let mut armed = ARMED.lock().unwrap();
        let Some(outstanding) = armed.get_mut(base_path) else {
            return false;
        };
        *outstanding -= 1;
        if *outstanding == 0 {
            armed.remove(base_path);
        }
        true
    }
}

/// Report the append result, perform the mode-specific persistence step, and then notify log I/O
/// completion to openraft exactly once.
///
/// The append acknowledgement and the completion notification are distinct events, in that order.
/// The acknowledgement says that the writer accepted or rejected the bytes and is sent before the
/// persistence step, so a caller that returns on it has not waited for persistence. The
/// notification is the result-bearing event openraft consumes, and exactly one of these three
/// outcomes is delivered on every append this function is called for:
///
/// - **accepted, persisted**: `Ok(())`, after `persist` returned successfully.
/// - **rejected**: `Err` carrying the append rejection. The rejection is the cause the caller was
///   already given, so it takes precedence over anything `persist` reports on the same append.
/// - **accepted, persistence failed**: `Err` carrying the persistence cause, delivered *before*
///   that error is propagated out of this function.
///
/// Propagation is unchanged: a persistence failure still leaves the writer loop, which ends the
/// writer thread. What changes is that openraft now learns the cause first, instead of only
/// observing the dropped completion sender.
///
/// In async modes the supplied persistence step may only start writeback, so a successful
/// notification does not by itself imply stable storage.
fn complete_append<F>(
    append_result: Result<(), Error>,
    // `Some` only when the failure is one the writer cannot keep serving past: today that is a
    // truncated entry stream, where the writer holds a prefix of a batch whose extent it does
    // not know, so a later append could be a continuation of a batch that never fully arrived.
    // An ordinary rejection stays `None` and leaves the writer running, which is the policy
    // `008` section 3.1 recorded.
    terminal: Option<Error>,
    ack: oneshot::Sender<Result<(), Error>>,
    callback: AppendCompletion,
    persist: F,
) -> Result<(), Error>
where
    F: FnOnce() -> Result<(), Error>,
{
    // The ack keeps the typed original; `Error` is not `Clone`, so the notification carries a
    // reproduction of the same cause.
    let rejection = append_result.as_ref().err().map(Error::as_io_error);

    if let Err(err) = ack.send(append_result) {
        // this should usually not happen, but it may during an incorrect shutdown
        error!("error sending back ack after logs append: {err:?}");
    }

    // Unchanged on purpose: the persistence step runs for a rejected append too, because bytes
    // written before the rejection are already in the mapping and `is_dirty` is already set.
    let persisted = persist();

    match (rejection, persisted) {
        (Some(err), persisted) => {
            callback(Err(err));
            // A rejected append is not by itself a writer failure, so the writer keeps serving
            // unless the rejection is terminal, or the persistence step also failed, either of
            // which still ends it. The terminal reason wins, because it is the more specific
            // account of why this writer must stop.
            match terminal {
                Some(reason) => Err(reason),
                None => persisted,
            }
        }
        (None, Ok(())) => {
            callback(Ok(()));
            Ok(())
        }
        (None, Err(err)) => {
            callback(Err(err.as_io_error()));
            Err(err)
        }
    }
}

fn spawn_syncer(tx_writer: flume::Sender<Action>, mut interval: Interval) {
    task::spawn(async move {
        loop {
            interval.tick().await;
            if tx_writer.send_async(Action::Sync).await.is_err() {
                debug!("Error sending ActionWrite::Sync to LogStoreWriter - exiting");
                break;
            }
        }
    });
}

/// There are a lot of `unwrap()`s in this task. The reason is simply, if most of these fail, it can
/// only be because of a non-recoverable error anyway and the application should crash, so that
/// the next health check can restart it.
///
/// Everything related to locking and memory mapping is being `unwrap()`ped. If anything fails in
/// this regard, it's either a physical storage or OS issue and this code an do nothing about it.
/// A step that fails before a `Remove` or `Vote` is acknowledged ends the writer, as it always
/// did, but answers the caller with the cause first. It used to `?` straight out of `run`, so the
/// caller got "the writer thread is no longer running" instead of the I/O error, the same class
/// the `Append` rollover repair closed (found by the AI review of `b5039d2`).
macro_rules! answer_or_end {
    ($ack:ident, $step:expr) => {
        match $step {
            Ok(v) => v,
            Err(err) => {
                let _ = $ack.send(Err(Error::Internal(format!("{err}").into())));
                return Err(err.into());
            }
        }
    };
}

fn run(
    lockfile: LockFile,
    meta: Arc<RwLock<Metadata>>,
    wal_locked: Arc<RwLock<WalFileSet>>,
    mut wal: WalFileSet,
    rx: flume::Receiver<Action>,
    sync: LogSync,
    wal_size: u32,
) -> Result<(), Error> {
    let _ = ThreadPriority::Max.set_for_current();

    let mut is_dirty = false;
    let mut shutdown_ack: Option<oneshot::Sender<()>> = None;
    let data_len_limit = wal_size as usize - wal.active().offset_logs() - 2;

    // openraft will read chunks of 64 logs for bigger tasks
    let mut buf: Vec<u8> = Vec::with_capacity(64);
    let mut buf_logs: Vec<(u64, Vec<u8>)> = Vec::with_capacity(1);

    wal.active().mmap_mut()?;

    while let Ok(action) = rx.recv() {
        match action {
            Action::Append { rx, callback, ack } => {
                debug!("WAL Writer - Action::Append");

                let mut res: Result<(), Error> = Ok(());
                let mut terminal: Option<Error> = None;
                let mut received: u64 = 0;
                let mut appended: u64 = 0;
                {
                    let mut active = wal.active();
                    loop {
                        // Three cases, not two. `Ok(None)` is the producer's explicit
                        // end-of-stream marker and means the batch is complete. `Err` is the
                        // entry sender having been dropped, which means it is not, and the two
                        // used to be indistinguishable: both ended a `while let Ok(Some(..))`
                        // with the result still `Ok`, so a truncated append was acknowledged
                        // and notified as a success (F-028).
                        let (id, bytes) = match rx.recv() {
                            Ok(Some(entry)) => entry,
                            Ok(None) => break,
                            Err(_) => {
                                let msg: Cow<'static, str> = format!(
                                    "the entry stream for this append disconnected after \
                                     {received} entr{} without its end-of-stream marker, so the \
                                     batch is incomplete and its extent is unknown; \
                                     {appended} entr{} already persisted",
                                    if received == 1 { "y" } else { "ies" },
                                    if appended == 1 { "y is" } else { "ies are" },
                                )
                                .into();
                                res = Err(Error::IncompleteAppend(msg.clone()));
                                terminal = Some(Error::IncompleteAppend(msg));
                                break;
                            }
                        };
                        received += 1;

                        if bytes.len() > data_len_limit {
                            // A single raft entry cannot span WAL files, so an entry larger
                            // than the WAL cannot be written. **That is a rejected append, not
                            // a reason to end the writer**, and it is the default now.
                            //
                            // It used to be a `panic!`, on the reasoning that an oversized
                            // entry is a non-recoverable setup issue. The comment beside it
                            // said what is wrong with that: "With the default `wal_size` of
                            // 2MB this is easily reached by a single large INSERT, transaction
                            // or batch." An application's own data ending the storage thread,
                            // and under an aborting profile the whole process, is not a setup
                            // issue; it is a large write. The caller gets
                            // `Error::WalSizeExceeded` on the acknowledgement and on the
                            // completion, and the writer keeps serving.
                            //
                            // `oversized-entry-error` is kept as a no-op so a consumer that
                            // enables it still builds; it selects what is now the only
                            // behavior.
                            {
                                res = Err(Error::WalSizeExceeded(
                                    format!(
                                        "`data` length must not exceed `wal_size` -> data \
                                        length is {} vs wal_size (without header) is \
                                        {data_len_limit}",
                                        bytes.len(),
                                    )
                                    .into(),
                                ));
                                break;
                            }
                        }

                        if !active.has_space(bytes.len() as u32) {
                            buf.clear();
                            // A failed rollover used to `?` out of `run` from here, dropping the
                            // acknowledgement and the completion callback unfired, so openraft
                            // never heard about this append at all (found in review). It is a
                            // failed append now, answered once on both channels, and terminal,
                            // because the WAL's file set may be half rolled.
                            if let Err(err) = wal.roll_over(wal_size, &mut buf) {
                                let msg: Cow<'static, str> =
                                    format!("rolling over to a new WAL file failed: {err}").into();
                                res = Err(Error::Internal(msg));
                                terminal = Some(err);
                                break;
                            }
                            {
                                let mut lock = wal_locked.write().unwrap();
                                lock.active = wal.active;
                                lock.clone_files_from_no_mmap(&wal.files);
                            }
                            active = wal.active();
                        }

                        buf.clear();
                        if let Err(err) = active.append_log(id, &bytes, &mut buf) {
                            res = Err(err);
                            break;
                        }
                        appended += 1;
                        debug_assert_eq!(
                            active.id_until, id,
                            "active.id_until and id don't match: {} != {id}",
                            active.id_until
                        );
                    }
                }

                {
                    let mut lock = wal_locked.write().unwrap();
                    debug_assert_eq!(lock.active, wal.active);
                    lock.active().clone_from_no_mmap(wal.active());
                }

                // The WAL now holds bytes that are not known to be on disk, and only a
                // blocking flush can clear that state again. `flush_async` merely starts the
                // writeback, which is why `Action::Remove` and `Action::Vote` below still flush.
                //
                // Conditional, unlike before: a clean empty batch, and a stream that
                // disconnected before its first entry, write nothing, so there is nothing to
                // mark dirty and nothing for the persistence step to flush. Both still get
                // exactly one acknowledgement and exactly one completion, a success for the
                // first and a failure for the second.
                if appended > 0 {
                    is_dirty = true;
                }
                complete_append(res, terminal, ack, callback, || {
                    #[cfg(test)]
                    if fault::take_persistence_failure(&wal.base_path) {
                        return Err(Error::IO(io::Error::other("injected persistence failure")));
                    }
                    if sync == LogSync::Immediate {
                        flush_blocking(&mut wal, &mut buf, &mut is_dirty)?;
                    } else if sync == LogSync::ImmediateAsync {
                        wal.active().flush_async()?;
                    }
                    Ok(())
                })?;

                // Roll WAL pre-emptively if only very few space is left at this point, because
                // if we just wrote some chunks, me probably have a very short break now until the
                // next request comes in.
                //
                // TODO fixed 4kB -> make configurable?
                if wal.active().space_left() < 4 * 1024 {
                    buf.clear();
                    wal.roll_over(wal_size, &mut buf)?;
                    {
                        let mut lock = wal_locked.write().unwrap();
                        lock.active = wal.active;
                        lock.clone_files_from_no_mmap(&wal.files);
                    }
                }
            }
            Action::Remove {
                from,
                until,
                last_log,
                ack,
            } => {
                debug!(
                    "WAL Writer - Action::Remove from {from} until {until} / \
                    last_log: {last_log:?}\n{wal:?}"
                );

                // Before removing any logs, make sure that all in-memory buffers are flushed. If
                // at least headers and metadata are not up to date, and a crash happens in the
                // middle of removing logs, we could end up with a hole between Snapshot and latest
                // existing Raft Log, which must never happen.
                //
                // The flush has to block. `flush_async` only starts the writeback and returns,
                // so a crash during the removal below can still land after the deletions and
                // before the header reaches disk, which is the hole this guards against.
                answer_or_end!(ack, flush_blocking(&mut wal, &mut buf, &mut is_dirty));

                // Persist the purge frontier before deleting (too low = hole into deleted files;
                // too high = extra files). Revert it again if the deletion fails below.
                let previous_purged = answer_or_end!(ack, meta.read()).last_purged_log_id.clone();
                let persist_purged = last_log.is_some();
                if persist_purged {
                    answer_or_end!(ack, meta.write()).last_purged_log_id = last_log;
                    answer_or_end!(ack, Metadata::write(meta.clone(), &wal.base_path));
                }

                buf.clear();
                buf_logs.clear();
                match wal.shift_delete_logs(from, until, wal_size, &mut buf, &mut buf_logs) {
                    Ok(_) => {
                        {
                            let mut lock = wal_locked.write().unwrap();
                            lock.active = wal.active;
                            lock.clone_files_from_no_mmap(&wal.files);
                        }
                        // A requester that went away (a cancelled future during teardown) is
                        // not a reason to end the writer: that `unwrap` could abort the
                        // process under `panic = "abort"`, the same class as F-112.
                        let _ = ack.send(Ok(()));
                    }
                    Err(err) => {
                        if persist_purged {
                            // deletion failed: revert the frontier, but still report the error
                            let revert_res = {
                                // `Metadata::write` re-locks the meta, so release the guard first
                                let mut m = match meta.write() {
                                    Ok(m) => m,
                                    Err(poisoned) => poisoned.into_inner(),
                                };
                                m.last_purged_log_id = previous_purged;
                                drop(m);
                                Metadata::write(meta.clone(), &wal.base_path)
                            };
                            if let Err(revert_err) = revert_res {
                                error!(
                                    "Failed to revert last_purged_log_id after log deletion failure: {revert_err}"
                                );
                            }
                        }
                        let _ = ack.send(Err(err));
                    }
                }
            }
            Action::Vote { value, ack } => {
                debug!("WAL Writer - Action::Vote");

                // Blocking on purpose: clearing `is_dirty` while an msync is still in flight
                // would make the interval ticker skip a WAL that never reached disk.
                answer_or_end!(ack, flush_blocking(&mut wal, &mut buf, &mut is_dirty));

                answer_or_end!(ack, meta.write()).vote = Some(value);
                let res = Metadata::write(meta.clone(), &wal.base_path);

                let _ = ack.send(res);
            }
            Action::Sync => {
                // The ticker is the only flush in `IntervalMillis` mode and no append waits on
                // it, so it blocks: `msync(MS_ASYNC)` starts no writeback on Linux at all.
                flush_blocking(&mut wal, &mut buf, &mut is_dirty)?;
            }
            Action::Shutdown(ack) => {
                debug!("Raft logs store writer is being shut down");
                shutdown_ack = Some(ack);
                break;
            }
        }
    }

    debug!("Logs Writer exiting");

    let active = wal.active();
    buf.clear();
    active.update_header(&mut buf)?;
    active.flush()?;
    Metadata::write(meta, &wal.base_path)?;

    // drop the lockfile before trying to remove it to unlock it
    drop(lockfile);
    if let Err(err) = LockFile::remove(&wal.base_path) {
        // The lock itself was released by dropping it above; a leftover file only means the
        // next start takes the not-a-clean-start path. Not worth ending the process for.
        error!("Could not remove the WAL lock file in {}: {err}", wal.base_path);
    }

    if let Some(ack) = shutdown_ack {
        // A shutdown caller that stopped waiting is not a failure of the writer.
        let _ = ack.send(());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Capture `tracing` ERROR events so a test can assert on a report whose only channel is a
    /// log line. The writer runs on its own `std::thread`, which does not inherit a thread-local
    /// subscriber, so the capture has to be the process-wide default.
    mod capture {
        use std::io;
        use std::sync::{Arc, Mutex, OnceLock};

        #[derive(Clone, Default)]
        pub(super) struct Buffer(Arc<Mutex<Vec<u8>>>);

        impl Buffer {
            pub(super) fn contents(&self) -> String {
                String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
            }
        }

        impl io::Write for Buffer {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buffer {
            type Writer = Buffer;

            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        static CAPTURED: OnceLock<Buffer> = OnceLock::new();

        /// Install the capture once and hand back the shared buffer. Every test in this binary
        /// shares it, so assertions must look for a substring unique to their own case.
        pub(super) fn errors() -> Buffer {
            CAPTURED
                .get_or_init(|| {
                    let buf = Buffer::default();
                    let subscriber = tracing_subscriber::fmt()
                        .with_max_level(tracing::Level::ERROR)
                        .with_ansi(false)
                        .with_writer(buf.clone())
                        .finish();
                    let _ = tracing::subscriber::set_global_default(subscriber);
                    buf
                })
                .clone()
        }
    }

    /// Poll `check` until it holds or the budget runs out. Used where the observable is produced
    /// by the writer thread and there is no channel to await.
    fn eventually<F: FnMut() -> bool>(mut check: F) -> bool {
        for _ in 0..50 {
            if check() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// One dispatched append against a live writer, returning the acknowledgement and the log I/O
    /// completion notification separately so a test can assert on both.
    #[allow(clippy::type_complexity)]
    fn dispatch_append(
        tx: &flume::Sender<Action>,
        id: u64,
        bytes: Vec<u8>,
    ) -> (
        oneshot::Receiver<Result<(), Error>>,
        std::sync::mpsc::Receiver<Result<(), io::Error>>,
    ) {
        let (ack_tx, ack_rx) = oneshot::channel();
        let (entry_tx, entry_rx) = flume::bounded(1);
        let (note_tx, note_rx) = std::sync::mpsc::channel();

        // Every send is allowed to fail: a test that asserts the writer terminated dispatches
        // one more append afterwards, and by then the writer's receiver is gone. The assertion
        // there is that nothing is acknowledged, not that the send succeeded.
        let _ = tx.send(Action::Append {
            rx: entry_rx,
            callback: Box::new(move |res| {
                let _ = note_tx.send(res);
            }),
            ack: ack_tx,
        });
        let _ = entry_tx.send(Some((id, bytes)));
        let _ = entry_tx.send(None);

        (ack_rx, note_rx)
    }

    /// One dispatched append whose entry stream is **truncated**: `entries` are sent and then
    /// the entry sender is dropped without the `None` end-of-stream marker, which is exactly
    /// what the adapter does when a send fails partway through a batch or a serialization
    /// panic unwinds past it.
    #[allow(clippy::type_complexity)]
    fn dispatch_truncated_append(
        tx: &flume::Sender<Action>,
        entries: Vec<(u64, Vec<u8>)>,
    ) -> (
        oneshot::Receiver<Result<(), Error>>,
        std::sync::mpsc::Receiver<Result<(), io::Error>>,
    ) {
        let (ack_tx, ack_rx) = oneshot::channel();
        let (entry_tx, entry_rx) = flume::bounded(1);
        let (note_tx, note_rx) = std::sync::mpsc::channel();

        let _ = tx.send(Action::Append {
            rx: entry_rx,
            callback: Box::new(move |res| {
                let _ = note_tx.send(res);
            }),
            ack: ack_tx,
        });
        for entry in entries {
            let _ = entry_tx.send(Some(entry));
        }
        // No `None`. This is the whole point of the helper.
        drop(entry_tx);

        (ack_rx, note_rx)
    }

    /// One dispatched append with **no entries at all** and a clean end-of-stream marker.
    #[allow(clippy::type_complexity)]
    fn dispatch_empty_append(
        tx: &flume::Sender<Action>,
    ) -> (
        oneshot::Receiver<Result<(), Error>>,
        std::sync::mpsc::Receiver<Result<(), io::Error>>,
    ) {
        let (ack_tx, ack_rx) = oneshot::channel();
        let (entry_tx, entry_rx) = flume::bounded(1);
        let (note_tx, note_rx) = std::sync::mpsc::channel();

        let _ = tx.send(Action::Append {
            rx: entry_rx,
            callback: Box::new(move |res| {
                let _ = note_tx.send(res);
            }),
            ack: ack_tx,
        });
        let _ = entry_tx.send(None);

        (ack_rx, note_rx)
    }

    fn start_writer(base: &str, sync: LogSync) -> flume::Sender<Action> {
        let _ = std::fs::remove_dir_all(base);
        std::fs::create_dir_all(base).unwrap();

        let lockfile = LockFile::create(base).unwrap();
        lockfile.lock().unwrap();
        let meta = Arc::new(RwLock::new(Metadata::read_or_create(base).unwrap()));

        let (tx, _wal, _fail) =
            spawn(base.to_string(), lockfile, sync, 64 * 1024, false, meta).unwrap();
        tx
    }

    /// Two WALs hold outstanding injections at the same time, and each consumes exactly its own.
    ///
    /// This is the interleaving the previous single-slot mechanism could not represent: both
    /// paths are armed *before* either is consumed, so arming the second had to discard the
    /// first's unconsumed injection, and the path comparison at consumption could then only turn
    /// that into a missed injection. Everything here is a direct call, so the result does not
    /// depend on thread scheduling, test order, or how many test threads the harness runs.
    ///
    /// It establishes the isolation property of the mechanism. It does not establish anything
    /// about the writer loop; the live-writer tests below do that.
    #[test]
    fn injections_are_isolated_per_wal_and_consumed_exactly_once() {
        let first = "test_data/fault_isolation_first";
        let second = "test_data/fault_isolation_second";
        let unarmed = "test_data/fault_isolation_unarmed";

        let armed_first = fault::arm_persistence_failure(first);
        let armed_second = fault::arm_persistence_failure(second);

        assert_eq!(
            armed_first.outstanding(),
            1,
            "arming a second WAL must not discard the first WAL's injection"
        );
        assert_eq!(armed_second.outstanding(), 1);

        assert!(
            !fault::take_persistence_failure(unarmed),
            "a WAL nothing armed must never consume another WAL's injection"
        );

        assert!(
            fault::take_persistence_failure(first),
            "the first WAL must receive its own injection"
        );
        assert_eq!(
            armed_first.outstanding(),
            0,
            "the first WAL's injection is consumed exactly once"
        );
        assert!(
            !fault::take_persistence_failure(first),
            "a consumed injection must not be delivered twice"
        );

        assert_eq!(
            armed_second.outstanding(),
            1,
            "consuming the first WAL's injection must leave the second WAL's armed"
        );
        assert!(
            fault::take_persistence_failure(second),
            "the second WAL must still receive its own injection"
        );
        assert!(!fault::take_persistence_failure(second));
    }

    /// An injection nobody consumed does not outlive the guard that armed it, so a later test
    /// reusing the same WAL directory cannot inherit it.
    #[test]
    fn a_dropped_guard_disarms_an_unconsumed_injection() {
        let base = "test_data/fault_guard_cleanup";

        drop(fault::arm_persistence_failure(base));

        assert!(
            !fault::take_persistence_failure(base),
            "dropping the guard must disarm what its WAL never consumed"
        );
    }

    /// The acknowledgement is observable before the persistence step runs, and the completion
    /// notification only after it returns. This ordering is the contract `001` section 2 records
    /// and the repair does not change it.
    #[test]
    fn append_result_precedes_persistence_and_completion() {
        let (ack_tx, mut ack_rx) = oneshot::channel();
        let (callback_tx, callback_rx) = std::sync::mpsc::channel();

        complete_append(
            Ok(()),
            None,
            ack_tx,
            Box::new(move |res| callback_tx.send(res).unwrap()),
            || {
                assert!(matches!(ack_rx.try_recv(), Ok(Ok(()))));
                assert!(callback_rx.try_recv().is_err());
                Ok(())
            },
        )
        .unwrap();

        assert!(matches!(callback_rx.try_recv(), Ok(Ok(()))));
    }

    /// Replaces `append_failure_is_returned_but_completion_still_fires`, which pinned F-001: the
    /// old helper reported success to openraft for an append it had just rejected. The
    /// notification must now carry the rejection, and must never be a success.
    #[test]
    fn append_rejection_notifies_error_and_never_success() {
        let (ack_tx, ack_rx) = oneshot::channel();
        let (callback_tx, callback_rx) = std::sync::mpsc::channel();

        complete_append(
            Err(Error::IO(std::io::Error::other("injected append failure"))),
            None,
            ack_tx,
            Box::new(move |res| callback_tx.send(res).unwrap()),
            || Ok(()),
        )
        .unwrap();

        assert!(matches!(ack_rx.blocking_recv(), Ok(Err(Error::IO(_)))));

        let notified = callback_rx
            .try_recv()
            .expect("a rejected append must still notify exactly once");
        let err = notified.expect_err("a rejected append must never notify success");
        assert!(
            err.to_string().contains("injected append failure"),
            "the notification must carry the rejection cause, got: {err}"
        );
        assert!(
            callback_rx.try_recv().is_err(),
            "exactly one notification per append"
        );
    }

    /// Replaces `persistence_failure_suppresses_completion_callback`, which pinned F-002: the
    /// callback was dropped rather than invoked, so openraft only ever saw a closed channel. The
    /// cause must now be notified, and the error must still propagate so the failure policy is
    /// unchanged.
    #[test]
    fn persistence_failure_notifies_error_before_propagating() {
        let (ack_tx, ack_rx) = oneshot::channel();
        let (callback_tx, callback_rx) = std::sync::mpsc::channel();

        let err = complete_append(
            Ok(()),
            None,
            ack_tx,
            Box::new(move |res| callback_tx.send(res).unwrap()),
            || Err(Error::IO(std::io::Error::other("injected sync failure"))),
        )
        .unwrap_err();

        assert!(matches!(err, Error::IO(_)), "the error still propagates");
        assert!(matches!(ack_rx.blocking_recv(), Ok(Ok(()))));

        let notified = callback_rx
            .try_recv()
            .expect("a persistence failure must notify, not drop the callback");
        let notified = notified.expect_err("a persistence failure must not notify success");
        assert!(
            notified.to_string().contains("injected sync failure"),
            "the notification must carry the persistence cause, got: {notified}"
        );
        assert!(
            callback_rx.try_recv().is_err(),
            "exactly one notification per append"
        );
    }

    /// An append rejection that is followed by a failing persistence step still produces exactly
    /// one notification, and it names the rejection: that is the cause the caller was given on
    /// the acknowledgement channel.
    #[test]
    fn rejection_takes_precedence_over_a_failing_persistence_step() {
        let (ack_tx, ack_rx) = oneshot::channel();
        let (callback_tx, callback_rx) = std::sync::mpsc::channel();

        let err = complete_append(
            Err(Error::IO(std::io::Error::other("injected append failure"))),
            None,
            ack_tx,
            Box::new(move |res| callback_tx.send(res).unwrap()),
            || Err(Error::IO(std::io::Error::other("injected sync failure"))),
        )
        .unwrap_err();

        assert!(err.to_string().contains("injected sync failure"));
        assert!(matches!(ack_rx.blocking_recv(), Ok(Err(Error::IO(_)))));

        let notified = callback_rx.try_recv().unwrap().unwrap_err();
        assert!(
            notified.to_string().contains("injected append failure"),
            "the rejection is the notified cause, got: {notified}"
        );
        assert!(callback_rx.try_recv().is_err());
    }


    /// F-114, deterministically. An append queued behind one the writer is still reading, which
    /// then turns out truncated, used to be stranded: the writer terminated without reading it,
    /// the adapter's senders kept it alive in the queue, and whoever was sending its entries
    /// blocked forever. The same held for a shutdown sent to a terminated writer. Both must now
    /// be answered, and every wait here is bounded so the unrepaired writer fails rather than
    /// hangs.
    #[tokio::test(flavor = "multi_thread")]
    async fn work_queued_behind_a_terminating_append_is_answered_not_stranded() {
        let base = "test_data/queued_behind_termination".to_string();
        let tx = start_writer(&base, LogSync::ImmediateAsync);
        let bound = Duration::from_secs(5);

        // A: the writer takes it and waits for entries.
        let (ack_a_tx, ack_a) = oneshot::channel();
        let (entries_a, rx_a) = flume::bounded::<Option<(u64, Vec<u8>)>>(1);
        tx.send_async(Action::Append {
            rx: rx_a,
            callback: Box::new(|_| {}),
            ack: ack_a_tx,
        })
        .await
        .unwrap();
        entries_a.send_async(Some((1, b"first".to_vec()))).await.unwrap();

        // B: queued behind A in the one-slot channel while the writer is still inside A.
        let (ack_b_tx, ack_b) = oneshot::channel();
        let (entries_b, rx_b) = flume::bounded::<Option<(u64, Vec<u8>)>>(1);
        let (note_b_tx, note_b) = std::sync::mpsc::channel();
        tx.send_async(Action::Append {
            rx: rx_b,
            callback: Box::new(move |res| {
                let _ = note_b_tx.send(res);
            }),
            ack: ack_b_tx,
        })
        .await
        .unwrap();

        // A is truncated: the writer terminates.
        drop(entries_a);
        let a = tokio::time::timeout(bound, ack_a)
            .await
            .expect("A must be acknowledged")
            .unwrap();
        assert!(matches!(a, Err(Error::IncompleteAppend(_))), "got {a:?}");

        // B's producer must not block, and B must be refused, once, on both channels.
        let sent = tokio::time::timeout(bound, async {
            for id in 2..=4u64 {
                if entries_b.send_async(Some((id, b"more".to_vec()))).await.is_err() {
                    break;
                }
            }
            let _ = entries_b.send_async(None).await;
        })
        .await;
        assert!(sent.is_ok(), "the producer of an append queued behind a termination blocked");
        let b = tokio::time::timeout(bound, ack_b)
            .await
            .expect("an append queued behind a termination must be answered")
            .expect("the answer must not be a dropped channel");
        assert!(b.is_err(), "a terminated writer must refuse queued work, got {b:?}");
        assert!(
            note_b.recv_timeout(bound).expect("one completion").is_err(),
            "the completion is a failure too"
        );

        // And a shutdown sent to the terminated writer is answered rather than awaited forever,
        // and not as a clean stop: the writer had already failed.
        let (sd_tx, sd_rx) = oneshot::channel();
        tx.send_async(Action::Shutdown(sd_tx)).await.unwrap();
        let sd = tokio::time::timeout(bound, sd_rx)
            .await
            .expect("a shutdown of a terminated writer must be answered, not awaited forever");
        assert!(sd.is_err(), "a terminated writer must not acknowledge a clean shutdown");
    }

    /// F-028: a truncated entry stream was acknowledged and notified as a **successful**
    /// append, because `while let Ok(Some(..)) = rx.recv()` ends the same way for the `None`
    /// that marks a healthy end of stream and for the `Err` that a dropped sender produces.
    ///
    /// Both disconnection points are covered, before the first entry and after a prefix, and
    /// every supported `LogSync` mode is swept, because the defect is in the collection loop
    /// that runs before any mode-specific persistence step and W-07's bar names all three.
    ///
    /// What is asserted, per case: the acknowledgement carries `Error::IncompleteAppend` rather
    /// than success, exactly one completion notification arrives and it is an error naming the
    /// truncation, and the writer has ended. The last one is the adopted policy: the writer
    /// holds a prefix of a batch whose extent it cannot know, so it must not serve a later
    /// append that could be a continuation of a batch that never fully arrived.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_truncated_entry_stream_never_reports_success_in_any_log_sync_mode() {
        for (mode, label) in [
            (LogSync::Immediate, "immediate"),
            (LogSync::ImmediateAsync, "immediate_async"),
            (LogSync::IntervalMillis(50), "interval"),
        ] {
            for (prefix, case) in [(0usize, "before the first entry"), (2, "after a prefix")] {
                let base = format!("test_data/truncated_{label}_{prefix}");
                let tx = start_writer(&base, mode.clone());

                let entries: Vec<(u64, Vec<u8>)> = (1..=prefix as u64)
                    .map(|id| (id, format!("entry {id}").into_bytes()))
                    .collect();
                let (ack_rx, note_rx) = dispatch_truncated_append(&tx, entries);

                let ack = ack_rx
                    .await
                    .unwrap_or_else(|_| panic!("{label} / {case}: the append must be acknowledged, not left pending"));
                let err = ack.expect_err(&format!(
                    "{label} / {case}: a truncated append must never be acknowledged as a success"
                ));
                assert!(
                    matches!(err, Error::IncompleteAppend(_)),
                    "{label} / {case}: the acknowledgement must name the truncation, got: {err}"
                );

                let notified = note_rx
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap_or_else(|_| panic!("{label} / {case}: exactly one completion must arrive"));
                let notified = notified.expect_err(&format!(
                    "{label} / {case}: a truncated append must never notify success"
                ));
                assert!(
                    notified.to_string().contains("IncompleteAppend"),
                    "{label} / {case}: the completion must carry the truncation cause, got: {notified}"
                );
                assert!(
                    note_rx.recv_timeout(Duration::from_millis(50)).is_err(),
                    "{label} / {case}: exactly one completion per dispatched append"
                );

                // F-114: a terminated writer keeps answering, so termination is observed as a
                // refusal naming it, not as a closed channel (which used to strand queued work).
                assert!(
                    later_append_is_refused_as_terminated(&tx).await,
                    "{label} / {case}: the writer must end after a truncated append"
                );
            }
        }
    }

    /// A clean batch with no entries is a success, not a failure and not a silent nothing. It
    /// is the one case that ends the collection loop on its first iteration with `Ok(None)`,
    /// and it must be distinguishable from a stream that disconnected before its first entry,
    /// which is the other way to end that loop having received nothing.
    ///
    /// It also writes nothing, so nothing is marked dirty and the persistence step has nothing
    /// to flush. That is stated here because the old code forced `is_dirty` unconditionally and
    /// performed a full flush for a batch that had appended no bytes.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_clean_empty_batch_succeeds_and_notifies_once() {
        for (mode, label) in [
            (LogSync::Immediate, "immediate"),
            (LogSync::ImmediateAsync, "immediate_async"),
            (LogSync::IntervalMillis(50), "interval"),
        ] {
            let base = format!("test_data/empty_batch_{label}");
            let tx = start_writer(&base, mode.clone());

            let (ack_rx, note_rx) = dispatch_empty_append(&tx);

            assert!(
                matches!(ack_rx.await, Ok(Ok(()))),
                "{label}: a clean empty batch is acknowledged as a success"
            );
            let notified = note_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| panic!("{label}: an empty batch must still notify"));
            assert!(
                notified.is_ok(),
                "{label}: an empty batch notifies success, got: {notified:?}"
            );
            assert!(
                note_rx.recv_timeout(Duration::from_millis(50)).is_err(),
                "{label}: exactly one completion"
            );

            let (ack, _) = dispatch_append(&tx, 1, b"after an empty batch".to_vec());
            assert!(
                matches!(tokio::time::timeout(Duration::from_secs(5), ack).await, Ok(Ok(Ok(())))),
                "{label}: an empty batch is not a failure and must not end the writer"
            );

            let (ack, _) = oneshot::channel();
            let _ = tx.send(Action::Shutdown(ack));
        }
    }

    /// Every supported `LogSync` mode reports exactly one success through a live writer, in the
    /// documented order. `IntervalMillis` performs no per-append persistence at all and must
    /// still notify once.
    #[tokio::test(flavor = "multi_thread")]
    async fn success_notifies_once_per_append_in_every_log_sync_mode() {
        for (name, sync) in [
            ("immediate", LogSync::Immediate),
            ("immediate_async", LogSync::ImmediateAsync),
            ("interval", LogSync::IntervalMillis(50)),
        ] {
            let base = format!("test_data/notify_once_{name}");
            let tx = start_writer(&base, sync.clone());

            for id in 1..=3u64 {
                let (ack_rx, note_rx) = dispatch_append(&tx, id, format!("entry-{id}").into_bytes());

                ack_rx
                    .await
                    .unwrap_or_else(|err| panic!("{name}: ack channel closed: {err}"))
                    .unwrap_or_else(|err| panic!("{name}: append rejected: {err}"));

                let notified = note_rx
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap_or_else(|err| panic!("{name}: no completion notification: {err}"));
                assert!(
                    notified.is_ok(),
                    "{name}: a successful append must notify success, got {notified:?}"
                );
                assert!(
                    note_rx.recv_timeout(Duration::from_millis(50)).is_err(),
                    "{name}: exactly one notification per append"
                );
            }

            let (ack_tx, ack_rx) = oneshot::channel();
            tx.send(Action::Shutdown(ack_tx)).unwrap();
            ack_rx.await.unwrap();
        }
    }

    /// The whole F-002 chain through the live writer loop, which the helper-level test cannot
    /// see: the append is acknowledged, the injected persistence failure is notified with its
    /// cause, and the writer then stops receiving, as it did before the repair, so every later
    /// append goes unacknowledged.
    #[tokio::test(flavor = "multi_thread")]
    async fn persistence_failure_notifies_then_terminates_the_writer() {
        let base = "test_data/persistence_failure_terminates".to_string();
        let tx = start_writer(&base, LogSync::Immediate);

        // A healthy append first, so the disconnect asserted below is an observed change of state
        // rather than a condition that was already true.
        //
        // The wait is on the healthy append's *completion notification*, not on its
        // acknowledgement. `complete_append` sends the acknowledgement before it invokes the
        // persistence step, so an awaited acknowledgement leaves that append's `persist` call
        // still ahead of the writer: arming on it would let the healthy append consume the
        // injection meant for the next one. The notification is sent after `persist` returned,
        // and `Ok(())` says it returned successfully, so observing it places this test strictly
        // after the healthy append's only chance to take an injection. The writer serves actions
        // one at a time and `LogSync::Immediate` spawns no syncer, so the next `persist` to run
        // is the injected append's.
        let (healthy_ack, healthy_note) = dispatch_append(&tx, 1, b"healthy".to_vec());
        healthy_ack.await.unwrap().unwrap();
        assert_eq!(
            healthy_note
                .recv_timeout(Duration::from_secs(5))
                .expect("the healthy append must be notified")
                .map_err(|err| err.to_string()),
            Ok(()),
            "the healthy append must complete successfully before an injection is armed"
        );
        // The healthy append acknowledged above is what shows the writer serving. A channel
        // that is still connected no longer shows it: a terminated writer keeps its receiver.

        let _armed = fault::arm_persistence_failure(&base);
        let (ack_rx, note_rx) = dispatch_append(&tx, 2, b"entry".to_vec());

        assert!(
            matches!(ack_rx.await, Ok(Ok(()))),
            "the append is acknowledged before the persistence step"
        );

        let notified = note_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the persistence failure must be notified")
            .expect_err("it must not be notified as success");
        assert!(
            notified.to_string().contains("injected persistence failure"),
            "the notification must carry the injected cause, got: {notified}"
        );

        // Failure policy preserved, observed positively: a later append is refused, and the
        // refusal names the termination. It used to be observed as a closed channel and a later
        // append that was never acknowledged, which was F-114's defect seen from the side
        // where it looked like the policy working.
        assert!(
            later_append_is_refused_as_terminated(&tx).await,
            "the writer must stop serving after a persistence failure"
        );
    }

    /// A later append to a terminated writer is refused within five seconds, and says why.
    async fn later_append_is_refused_as_terminated(tx: &flume::Sender<Action>) -> bool {
        let (ack, note) = dispatch_append(tx, 999, b"later".to_vec());
        let refused = matches!(
            tokio::time::timeout(Duration::from_secs(5), ack).await,
            Ok(Ok(Err(Error::Internal(reason)))) if reason.contains("terminated")
        );
        refused && note.recv_timeout(Duration::from_secs(5)).is_ok_and(|r| r.is_err())
    }

    /// The termination itself is reported. The thread's `JoinHandle` is not retained; the report
    /// is a single ERROR log naming the WAL the writer served and the cause.
    #[tokio::test(flavor = "multi_thread")]
    async fn writer_termination_is_reported() {
        let captured = capture::errors();
        let base = "test_data/termination_reported".to_string();
        let tx = start_writer(&base, LogSync::Immediate);

        let _armed = fault::arm_persistence_failure(&base);
        let (ack_rx, _note_rx) = dispatch_append(&tx, 1, b"entry".to_vec());
        assert!(matches!(ack_rx.await, Ok(Ok(()))));

        assert!(
            eventually(|| {
                let log = captured.contents();
                log.contains(&base) && log.contains("terminated with an unrecoverable error")
            }),
            "the writer's termination must be reported; captured ERROR log was:\n{}",
            captured.contents()
        );
    }

    fn append(tx: &flume::Sender<Action>, id: u64, bytes: Vec<u8>) -> Result<(), Error> {
        let (ack_tx, ack_rx) = oneshot::channel();
        let (entry_tx, entry_rx) = flume::bounded(1);
        tx.send(Action::Append {
            rx: entry_rx,
            callback: Box::new(|_| {}),
            ack: ack_tx,
        })
        .unwrap();
        // The acknowledgement carries the verdict. A writer that rejects an entry drops the
        // entry receiver, and whether that happens before or after these sends is a race, so a
        // failed send is expected and not a test failure. It used to `unwrap`, which failed
        // this suite about once in forty full runs.
        let _ = entry_tx.send(Some((id, bytes)));
        let _ = entry_tx.send(None);
        ack_rx.blocking_recv().unwrap()
    }

    /// An entry larger than the WAL is a rejected append, and the writer keeps serving.
    ///
    /// This used to require the `oversized-entry-error` feature; the default was a `panic!`,
    /// which under an aborting profile ends the embedding application because its own write
    /// was too big. The comment beside that panic said the default `wal_size` of 2 MiB is
    /// "easily reached by a single large INSERT, transaction or batch", which is what makes it
    /// a large write rather than a setup issue. The test is now unconditional and the one that
    /// pinned the panic is gone.
    #[test]
    fn oversized_entry_errors_without_killing_writer() {
        let base = "test_data/oversized_entry".to_string();
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let lockfile = LockFile::create(&base).unwrap();
        lockfile.lock().unwrap();
        let meta = Arc::new(RwLock::new(Metadata::read_or_create(&base).unwrap()));

        let (tx, _wal, _fail) = spawn(
            base.clone(),
            lockfile,
            LogSync::Immediate,
            64 * 1024,
            false,
            meta.clone(),
        )
        .unwrap();

        // an entry larger than the whole WAL file must error, not panic the writer
        let res = append(&tx, 1, vec![0u8; 70 * 1024]);
        assert!(res.is_err(), "oversized entry must error, got {res:?}");

        // the writer must still be alive and accept a normal append afterwards
        let res = append(&tx, 2, b"ok".to_vec());
        assert!(res.is_ok(), "writer must survive: {res:?}");

        // graceful shutdown (removes the lockfile)
        let (ack_tx, ack_rx) = oneshot::channel();
        tx.send(Action::Shutdown(ack_tx)).unwrap();
        ack_rx.blocking_recv().unwrap();
    }

}
