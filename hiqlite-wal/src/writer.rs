use crate::error::Error;
use crate::lockfile::LockFile;
use crate::log_store_impl::{deserialize, serialize};
use crate::metadata::Metadata;
use crate::reader::LogReadMemo;
use crate::wal::WalFileSet;
use openraft::{LeaderId, LogId};
use std::fmt::{Debug, Formatter};
use std::io;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use thread_priority::ThreadPriority;
use tokio::sync::oneshot;
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
) -> Result<(flume::Sender<Action>, Arc<RwLock<WalFileSet>>), Error> {
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
    thread::spawn(move || {
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
        if let Err(err) = run(lockfile, meta, wal, set, rx, snc, wal_size) {
            error!(
                "Raft logs WAL writer for `{reported_path}` terminated with an unrecoverable \
                error: {err} - all further appends will fail until this process is restarted"
            );
        }
    });

    if let LogSync::IntervalMillis(millis) = &sync {
        let interval = time::interval(Duration::from_millis(*millis));
        spawn_syncer(tx.clone(), interval);
    }

    Ok((tx, wal_locked))
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
            // unless the persistence step also failed, which still ends it.
            persisted
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

                let mut res = Ok(());
                {
                    let mut active = wal.active();
                    while let Ok(Some((id, bytes))) = rx.recv() {
                        if bytes.len() > data_len_limit {
                            // A single raft entry cannot span WAL files. By default an
                            // oversized entry is a non-recoverable setup issue (it needs a
                            // config change with a full restart, or code changes), so the
                            // writer panics. With the `oversized-entry-error` feature the
                            // append instead fails with `Error::WalSizeExceeded` and the
                            // writer keeps serving subsequent requests. With the default
                            // `wal_size` of 2MB this is easily reached by a single large
                            // INSERT, transaction or batch.
                            #[cfg(feature = "oversized-entry-error")]
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
                            #[cfg(not(feature = "oversized-entry-error"))]
                            {
                                panic!(
                                    "`data` length must not exceed `wal_size` -> data length \
                                    is {} vs wal_size (without header) is {data_len_limit}",
                                    bytes.len(),
                                );
                            }
                        }

                        if !active.has_space(bytes.len() as u32) {
                            buf.clear();
                            wal.roll_over(wal_size, &mut buf)?;
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
                is_dirty = true;
                complete_append(res, ack, callback, || {
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
                flush_blocking(&mut wal, &mut buf, &mut is_dirty)?;

                // Persist the purge frontier before deleting (too low = hole into deleted files;
                // too high = extra files). Revert it again if the deletion fails below.
                let previous_purged = meta.read()?.last_purged_log_id.clone();
                let persist_purged = last_log.is_some();
                if persist_purged {
                    meta.write()?.last_purged_log_id = last_log;
                    Metadata::write(meta.clone(), &wal.base_path)?;
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
                        ack.send(Ok(())).unwrap();
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
                        ack.send(Err(err)).unwrap();
                    }
                }
            }
            Action::Vote { value, ack } => {
                debug!("WAL Writer - Action::Vote");

                // Blocking on purpose: clearing `is_dirty` while an msync is still in flight
                // would make the interval ticker skip a WAL that never reached disk.
                flush_blocking(&mut wal, &mut buf, &mut is_dirty)?;

                meta.write()?.vote = Some(value);
                let res = Metadata::write(meta.clone(), &wal.base_path);

                ack.send(res).unwrap();
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
    LockFile::remove(&wal.base_path).expect("LockFile removal failed");

    if let Some(ack) = shutdown_ack {
        ack.send(())
            .expect("Shutdown handler to always wait for ack from logs");
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

    fn start_writer(base: &str, sync: LogSync) -> flume::Sender<Action> {
        let _ = std::fs::remove_dir_all(base);
        std::fs::create_dir_all(base).unwrap();

        let lockfile = LockFile::create(base).unwrap();
        lockfile.lock().unwrap();
        let meta = Arc::new(RwLock::new(Metadata::read_or_create(base).unwrap()));

        let (tx, _wal) = spawn(base.to_string(), lockfile, sync, 64 * 1024, false, meta).unwrap();
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
        assert!(
            !tx.is_disconnected(),
            "the writer is serving before the injected failure"
        );

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

        // Failure policy preserved, observed positively. `run` owns the only `Receiver` for this
        // channel and holds it for as long as it is on the stack, so the sender reporting a
        // disconnect establishes that `run` has left: the writer can receive no further action.
        // It does not establish that the OS thread has finished, because the thread closure runs
        // its error report after `run` returns. That is a stronger claim than this test makes and
        // than the runtime supports.
        assert!(
            eventually(|| tx.is_disconnected()),
            "the writer must still stop serving after a persistence failure: its sole Action \
            receiver is dropped when `run` returns"
        );

        // And the consequence the policy is about: with the writer no longer receiving, a later
        // append is never acknowledged. This is a corollary of the disconnect above, not the
        // proof of it.
        let (mut later_ack, _later_note) = dispatch_append(&tx, 3, b"later".to_vec());
        assert!(
            later_ack.try_recv().is_err(),
            "a terminated writer must not acknowledge a later append"
        );
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

    #[cfg(feature = "oversized-entry-error")]
    fn append(tx: &flume::Sender<Action>, id: u64, bytes: Vec<u8>) -> Result<(), Error> {
        let (ack_tx, ack_rx) = oneshot::channel();
        let (entry_tx, entry_rx) = flume::bounded(1);
        tx.send(Action::Append {
            rx: entry_rx,
            callback: Box::new(|_| {}),
            ack: ack_tx,
        })
        .unwrap();
        entry_tx.send(Some((id, bytes))).unwrap();
        entry_tx.send(None).unwrap();
        ack_rx.blocking_recv().unwrap()
    }

    #[cfg(feature = "oversized-entry-error")]
    #[test]
    fn oversized_entry_errors_without_killing_writer() {
        let base = "test_data/oversized_entry".to_string();
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let lockfile = LockFile::create(&base).unwrap();
        lockfile.lock().unwrap();
        let meta = Arc::new(RwLock::new(Metadata::read_or_create(&base).unwrap()));

        let (tx, _wal) = spawn(
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

    #[cfg(not(feature = "oversized-entry-error"))]
    #[test]
    fn oversized_entry_panics_and_kills_writer() {
        let base = "test_data/oversized_entry_panic".to_string();
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let lockfile = LockFile::create(&base).unwrap();
        lockfile.lock().unwrap();
        let meta = Arc::new(RwLock::new(Metadata::read_or_create(&base).unwrap()));

        let (tx, _wal) = spawn(
            base.clone(),
            lockfile,
            LogSync::Immediate,
            64 * 1024,
            false,
            meta.clone(),
        )
        .unwrap();

        let send_append = |id: u64, bytes: Vec<u8>| {
            let (ack_tx, ack_rx) = oneshot::channel();
            let (entry_tx, entry_rx) = flume::bounded(1);
            // The writer thread may already be dead, so every send can fail and is ignored;
            // the assertion is that the append is never acked.
            let _ = tx.send(Action::Append {
                rx: entry_rx,
                callback: Box::new(|_| {}),
                ack: ack_tx,
            });
            let _ = entry_tx.send(Some((id, bytes)));
            let _ = entry_tx.send(None);
            ack_rx
        };

        // an oversized entry panics the writer thread by default; the append is never acked
        let mut ack = send_append(1, vec![0u8; 70 * 1024]);
        assert!(
            !recv_with_timeout(&mut ack),
            "oversized entry must panic the writer (no ack)"
        );

        // the writer thread is dead: a subsequent append is never acked either
        let mut ack = send_append(2, b"ok".to_vec());
        assert!(
            !recv_with_timeout(&mut ack),
            "writer must be dead after the panic"
        );
    }

    #[cfg(not(feature = "oversized-entry-error"))]
    fn recv_with_timeout(ack: &mut tokio::sync::oneshot::Receiver<Result<(), Error>>) -> bool {
        for _ in 0..10 {
            if ack.try_recv().is_ok() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        false
    }
}
