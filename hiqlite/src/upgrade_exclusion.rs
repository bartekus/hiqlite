//! `035`: exclude every live hiqlite node, of either version, before anything in the data
//! directory is mutated, and keep that exclusion until this node's last write.
//!
//! hiqlite 0.14 takes no owner lock. It holds advisory locks on `logs/lock.hql` and
//! `logs_cache/lock.hql` and nothing else, so `024`'s owner lock keeps out another node of this
//! version and nobody older. This module takes the WAL locks right after the owner lock, before
//! the unclean-stop check, the legacy cache check and its consent move, and hands them to the
//! log stores instead of releasing them (a second descriptor in the same process cannot take a
//! lock the process holds). They are released after every writer has stopped, lock files
//! unlinked while held, and only then is the owner lock released.
//!
//! The consent move (`027` B-10) is a resumable operation here: its destination is created as
//! `pre-upgrade-<secs>.partial/`, whose name is the operation's identity until it completes,
//! the replacement `logs_cache` is staged and locked before the legacy one moves, and the
//! legacy log, which is the evidence that makes a start take this path at all, moves last.

use crate::Error;
use hiqlite_wal::{LockFile, TryAcquire};
use std::fs;
use std::io;
use std::path::Path;
use tracing::{info, warn};

#[cfg(feature = "cache")]
use crate::store::logs::CACHE_LEGACY_MOVE_ASIDE_ENV as CONSENT_ENV;
/// Only a build with `cache` opens a cache raft log; the name is for messages.
#[cfg(not(feature = "cache"))]
const CONSENT_ENV: &str = "HQL_CACHE_LEGACY_MOVE_ASIDE";

/// The directory a consent move lands in, once it is complete. The same prefix as the published
/// build used, so an operator's existing instructions still find it.
pub(crate) const PRE_UPGRADE_DIR_PREFIX: &str = "pre-upgrade-";
/// The suffix of a consent move that has not completed. The directory's name is the
/// operation's identity: a later start resumes into it and never picks a new timestamp.
const PARTIAL_SUFFIX: &str = ".partial";
/// The replacement cache log directory, built and locked before the legacy one is moved.
const STAGED_CACHE_LOG_DIR: &str = "logs_cache.hiqlite-next";

/// The file in the cache raft's log directory that says which `CacheRequest` layout wrote it.
pub(crate) const CACHE_LOG_FORMAT_FILE: &str = "hiqlite-cache-log-format";
/// The `CacheRequest` layout this build reads and writes (upstream PR #362's).
pub(crate) const CACHE_LOG_FORMAT: &str = "2";

/// A WAL directory and the lock this start holds on it.
#[derive(Debug)]
struct WalGuard {
    dir: String,
    lock: LockFile,
    /// This start created the directory (it did not exist), so a refusal removes it again when
    /// it is still empty.
    created_dir: bool,
}

impl WalGuard {
    /// Create the directory if it is absent and take its lock, or say who holds it.
    fn acquire(dir: String) -> Result<Result<Self, bool>, io::Error> {
        let created_dir = create_dir_private(&dir)?;
        match LockFile::try_acquire(&dir).map_err(io::Error::other)? {
            TryAcquire::Acquired(lock) => Ok(Ok(Self {
                dir,
                lock,
                created_dir,
            })),
            TryAcquire::Held { created } => {
                // The file is the holder's now, whoever created it. Only the directory, if this
                // start created it and it is still empty, is ours to take back.
                let _ = created;
                if created_dir {
                    let _ = fs::remove_dir(&dir);
                }
                Ok(Err(created_dir))
            }
        }
    }

    /// Undo what this start created, while the lock is still held. Returns what could not be
    /// undone.
    fn undo_creations(self) -> Vec<String> {
        let mut left = Vec::new();
        if !self.lock.existed_before() {
            if let Err(err) = self.lock.unlink_while_held(&self.dir) {
                left.push(format!("{}/lock.hql ({err})", self.dir));
            }
        } else {
            drop(self.lock);
        }
        if self.created_dir {
            match fs::remove_dir(&self.dir) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(_) => left.push(format!("{}/ (no longer empty)", self.dir)),
            }
        }
        left
    }
}

/// The WAL locks of one start, from before its first mutation to its last write.
#[derive(Debug)]
pub(crate) struct UpgradeExclusion {
    data_dir: String,
    /// `logs/`, when this build opens the SQLite raft.
    db: Option<WalGuard>,
    /// `logs_cache/`, when this build opens a disk-backed cache raft.
    cache: Option<WalGuard>,
    /// A consent move completed during this start: `pre-upgrade-<secs>/`.
    moved_to: Option<String>,
}

/// What a refusal or a failure is, before the account of this start's own creations is added.
enum Stop {
    /// Refused before anything was moved or written.
    Refuse(fn(String) -> Error, String),
    /// A consent move started, by this start or an earlier one, and did not complete.
    Incomplete(String),
    /// A test's injected crash: no cleanup, exactly as a killed process leaves it.
    #[cfg(test)]
    Crash,
}

#[cfg(test)]
thread_local! {
    /// Where this start should stop, for `035` U-7. `None` in every build but a test's.
    pub(crate) static FAULT_POINT: std::cell::RefCell<Option<&'static str>> =
        const { std::cell::RefCell::new(None) };
    /// Called at every fault point, for `035` U-4's observations and contenders.
    #[allow(clippy::type_complexity)]
    pub(crate) static PROBE: std::cell::RefCell<Option<Box<dyn FnMut(&'static str)>>> =
        std::cell::RefCell::new(None);
}

/// A fault point (`035` B-5, U-7, X-5). In a test build, a crash the test asked for. With the
/// `__upgrade-fault-points` feature, an abort when `HQL_TEST_UPGRADE_FAULT` names this point,
/// which is how the real-version harness kills the candidate at an exact step. Otherwise
/// nothing.
#[allow(unused_variables)]
fn fault(point: &'static str) -> Result<(), Stop> {
    #[cfg(test)]
    PROBE.with(|p| {
        if let Some(probe) = p.borrow_mut().as_mut() {
            probe(point);
        }
    });
    #[cfg(test)]
    if FAULT_POINT.with(|p| *p.borrow() == Some(point)) {
        return Err(Stop::Crash);
    }
    #[cfg(feature = "__upgrade-fault-points")]
    if std::env::var("HQL_TEST_UPGRADE_FAULT").as_deref() == Ok(point) {
        eprintln!("HQL_TEST_UPGRADE_FAULT: aborting at {point}");
        std::process::abort();
    }
    Ok(())
}

/// `035` B-1 in the order a start runs it: the owner lock (step 1), the WAL locks, the
/// unclean-stop marker and the legacy cache check with its consent move (steps 2 to 4), and
/// only then the owner note.
pub(crate) fn acquire_storage(
    data_dir: &str,
    opens_db: bool,
    opens_cache_log: bool,
    consent: bool,
) -> Result<crate::storage_lock::StorageOwnership, Error> {
    let mut ownership = crate::storage_lock::StorageOwnership::acquire_without_note(data_dir)?;
    let exclusion = UpgradeExclusion::run(
        data_dir,
        opens_db,
        opens_cache_log,
        consent,
        ownership.created(),
    )?;
    ownership.record_owner_note(data_dir);
    ownership.attach_wal_exclusion(exclusion);
    Ok(ownership)
}

impl UpgradeExclusion {
    /// `035` B-1 steps 2 to 4, under the owner lock the caller already holds (step 1).
    ///
    /// On `Err` everything this call created is removed again, and the error says what is left,
    /// including whether `hiqlite-owner.lock` was created by this start (`owner_lock_created`).
    pub(crate) fn run(
        data_dir: &str,
        opens_db: bool,
        opens_cache_log: bool,
        consent: bool,
        owner_lock_created: bool,
    ) -> Result<Self, Error> {
        let mut slf = Self {
            data_dir: data_dir.to_string(),
            db: None,
            cache: None,
            moved_to: None,
        };
        match slf.run_inner(opens_db, opens_cache_log, consent) {
            Ok(()) => Ok(slf),
            Err(stop) => Err(slf.stop(stop, owner_lock_created)),
        }
    }

    fn run_inner(&mut self, opens_db: bool, opens_cache_log: bool, consent: bool) -> Result<(), Stop> {
        let data_dir = self.data_dir.clone();

        // Step 2. Every WAL directory this start will open, created if absent, so there is no
        // window before the log store starts in which a 0.14 process could create and lock it.
        if opens_db {
            self.db = Some(self.wal_guard(format!("{data_dir}/logs"))?);
        }
        fault("after-db-lock")?;
        if opens_cache_log {
            self.cache = Some(self.wal_guard(format!("{data_dir}/logs_cache"))?);
        }
        fault("after-cache-lock")?;

        // Step 3. The previous run of this directory, of either version, did not stop cleanly.
        // `auto-heal` keeps its own policy (the state machine rebuilds), and still only runs
        // after the locks above.
        #[cfg(all(feature = "sqlite", not(feature = "auto-heal")))]
        {
            let marker = format!("{data_dir}/state_machine/lock");
            if Path::new(&marker).exists() {
                return Err(Stop::Refuse(
                    |s| Error::Startup(s.into()),
                    format!(
                        "{marker} exists: the previous run of this data directory, of this or an \
                         older hiqlite version, did not stop cleanly, and this build refuses to \
                         open it without `auto-heal`. Nothing was moved. Before removing the \
                         marker, establish that no process uses {data_dir} and that the SQLite \
                         database can be rebuilt from, or is consistent with, the raft log, or \
                         restore a backup"
                    ),
                ));
            }
        }

        // Step 4.
        if opens_cache_log {
            self.cache_log_format(consent)?;
        }
        Ok(())
    }

    fn wal_guard(&self, dir: String) -> Result<WalGuard, Stop> {
        match WalGuard::acquire(dir.clone()) {
            Ok(Ok(guard)) => Ok(guard),
            Ok(Err(_)) => Err(Stop::Refuse(
                |s| Error::StorageInUse(s.into()),
                format!(
                    "{dir}/lock.hql is locked by another live process: a hiqlite node of this or \
                     an older version (0.14 takes only this lock) is using {}. This start \
                     refuses before changing anything. Stop that process first",
                    self.data_dir
                ),
            )),
            Err(err) => Err(Stop::Refuse(
                |s| Error::Startup(s.into()),
                format!("cannot take the WAL lock in {dir}: {err}"),
            )),
        }
    }

    /// Remove what this start created, then build the error.
    fn stop(&mut self, stop: Stop, owner_lock_created: bool) -> Error {
        let (make, text, incomplete): (fn(String) -> Error, String, bool) = match stop {
            Stop::Refuse(make, text) => (make, text, false),
            Stop::Incomplete(text) => (|s| Error::Startup(s.into()), text, true),
            #[cfg(test)]
            Stop::Crash => {
                // A killed process undoes nothing; its locks go with it.
                self.db.take();
                self.cache.take();
                return Error::Startup("injected crash".into());
            }
        };

        let mut left = Vec::new();
        for guard in [self.db.take(), self.cache.take()].into_iter().flatten() {
            left.extend(guard.undo_creations());
        }

        let data_dir = &self.data_dir;
        let mut msg = text;
        if incomplete {
            msg.push_str(". The consent move is incomplete and nothing was deleted");
        } else {
            msg.push_str(". No data was changed");
        }
        if owner_lock_created {
            msg.push_str(&format!(
                "; this start created {data_dir}/hiqlite-owner.lock, which stays (every later \
                 start takes the same lock)"
            ));
        } else {
            msg.push_str(&format!("; {data_dir}/hiqlite-owner.lock already existed"));
        }
        if left.is_empty() {
            msg.push_str(", and every WAL lock file or directory this start created was removed");
        } else {
            msg.push_str(&format!(
                ", and these files or directories this start created could not be removed: {}",
                left.join(", ")
            ));
        }
        make(msg)
    }

    /// The lock the SQLite raft's log store adopts.
    #[cfg(feature = "sqlite")]
    pub(crate) fn db_lock(&self) -> Option<&LockFile> {
        self.db.as_ref().map(|g| &g.lock)
    }

    /// The lock the cache raft's log store adopts.
    #[cfg(feature = "cache")]
    pub(crate) fn cache_lock(&self) -> Option<&LockFile> {
        self.cache.as_ref().map(|g| &g.lock)
    }

    /// After something that removes or relocates WAL directories wholesale (the restore's
    /// quarantine, `HQL_DANGER_RAFT_STATE_RESET`), take the lock again on the file now at each
    /// path, so the log stores adopt a lock that protects their path. The lock on the old inode
    /// is released only after the new one is held.
    ///
    /// Not continuous for a 0.14 contender: between the removal and this call, nothing holds a
    /// lock at the path (`035` KD-4). Both operations are operator-initiated and destructive.
    pub(crate) fn relock_moved(&mut self) -> Result<(), Error> {
        let moved_to = self.moved_to.clone();
        for guard in [&mut self.db, &mut self.cache].into_iter().flatten() {
            if guard.lock.is_linked_at(&guard.dir).map_err(io_startup)? {
                continue;
            }
            match WalGuard::acquire(guard.dir.clone()).map_err(io_startup)? {
                Ok(new) => {
                    let old = std::mem::replace(guard, new);
                    drop(old);
                }
                Err(_) => {
                    return Err(Error::StorageInUse(
                        format!(
                            "{}/lock.hql was taken by another process after this start moved or \
                             removed that directory; this start stops{}",
                            guard.dir,
                            match &moved_to {
                                Some(to) => format!(
                                    ". Its consent move had completed: the legacy cache is in \
                                     {to}/"
                                ),
                                None => String::new(),
                            }
                        )
                        .into(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// After a clean stop of every writer: unlink each WAL lock file while it is held, then
    /// release it (`035` B-2).
    pub(crate) fn release_clean(mut self) {
        for guard in [self.db.take(), self.cache.take()].into_iter().flatten() {
            if let Err(err) = guard.lock.unlink_while_held(&guard.dir) {
                warn!("Could not remove the WAL lock file in {}: {err}", guard.dir);
            }
        }
    }

    /// `027` B-10 under the locks of step 2. `self.cache` is held.
    fn cache_log_format(&mut self, consent: bool) -> Result<(), Stop> {
        let data_dir = self.data_dir.clone();
        let dir_logs = format!("{data_dir}/logs_cache");
        let marker = format!("{dir_logs}/{CACHE_LOG_FORMAT_FILE}");
        let io = |what: &str, err: io::Error| {
            Stop::Refuse(
                |s| Error::Startup(s.into()),
                format!("cannot check the cache raft log format, {what}: {err}"),
            )
        };

        let partials = list_prefixed(&data_dir, |n| {
            n.starts_with(PRE_UPGRADE_DIR_PREFIX) && n.ends_with(PARTIAL_SUFFIX)
        })
        .map_err(|e| io("listing the data directory", e))?;
        if partials.len() > 1 {
            return Err(Stop::Refuse(
                |s| Error::Startup(s.into()),
                format!(
                    "{data_dir} holds more than one incomplete consent move ({}); this build \
                     resumes exactly one and will not choose. Move all but one aside by hand",
                    partials.join(", ")
                ),
            ));
        }
        let partial = partials.into_iter().next();

        let staged = format!("{data_dir}/{STAGED_CACHE_LOG_DIR}");
        if partial.is_none() && Path::new(&staged).exists() {
            return Err(Stop::Refuse(
                |s| Error::Startup(s.into()),
                format!(
                    "{staged} exists without an incomplete consent move beside it; this build did \
                     not leave it. Inspect it and remove it by hand"
                ),
            ));
        }

        match fs::read_to_string(&marker) {
            Ok(found) if found.trim() == CACHE_LOG_FORMAT => {
                return match partial {
                    None => {
                        if consent {
                            info!(
                                "{} is set and the cache raft log is already in this build's \
                                 format; nothing to move",
                                CONSENT_ENV
                            );
                        }
                        Ok(())
                    }
                    Some(partial) => self.resume(consent, &partial),
                };
            }
            Ok(found) => {
                return Err(Stop::Refuse(
                    |s| Error::Startup(s.into()),
                    format!(
                        "the cache raft log in {dir_logs} was written in format {}, and this \
                         build reads only format {CACHE_LOG_FORMAT}",
                        found.trim()
                    ),
                ));
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(io("reading the format marker", err)),
        }

        if let Some(partial) = partial {
            return self.resume(consent, &partial);
        }

        // The WAL files are the evidence, and only they (`027` B-10).
        let legacy = holds_wal_files(&dir_logs).map_err(|e| io("reading logs_cache", e))?;
        if !legacy {
            // The published 0.15.0-patched.1 moved `logs_cache` first. A crash between its two
            // renames left a 0.14 cache snapshot at the original path, which it would have
            // restored (F-130). Refuse or finish that move instead.
            if let Some(dest) = self.published_interrupted_move()? {
                if !consent {
                    return Err(Stop::Refuse(
                        |s| Error::Startup(s.into()),
                        format!(
                            "{data_dir}/{dest}/ holds a legacy cache raft log without its \
                             snapshots, and {data_dir}/state_machine_cache is still in place: an \
                             earlier consent move was interrupted between its two renames. Start \
                             once with {}=true to finish it; the snapshots then move into \
                             {data_dir}/{dest}/",
                            CONSENT_ENV
                        ),
                    ));
                }
                let from = format!("{data_dir}/state_machine_cache");
                let to = format!("{data_dir}/{dest}/state_machine_cache");
                rename_synced(&from, &to, &data_dir, &format!("{data_dir}/{dest}")).map_err(
                    |e| {
                        Stop::Incomplete(format!(
                            "cannot finish the interrupted consent move in {data_dir}/{dest}/, \
                             moving {from}: {e}"
                        ))
                    },
                )?;
                warn!("Finished the interrupted consent move: moved {from} to {to}");
                self.moved_to = Some(format!("{data_dir}/{dest}"));
            }
            return self.write_marker_in_place(&dir_logs).map_err(|e| io("writing the marker", e));
        }

        if !consent {
            return Err(Stop::Refuse(
                |s| Error::Startup(s.into()),
                format!(
                    "the cache raft's log or snapshots in {dir_logs} and {data_dir}/\
                     state_machine_cache were written by a hiqlite version whose replicated \
                     cache format this build cannot read safely (hiqlite 0.14.x, or a \
                     pre-release build without a format marker). The cache is not carried \
                     across this upgrade; the SQLite database is. Either start once with {}=true, \
                     which moves both directories into {data_dir}/{PRE_UPGRADE_DIR_PREFIX}<unix \
                     seconds>/, or stop the node and move them aside yourself. The cache raft \
                     then starts empty, as an in-memory cache does after every restart",
                    CONSENT_ENV
                ),
            ));
        }

        // A new operation. Its identity is the `.partial` directory, created before anything
        // moves and synced into `data_dir`.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let name = format!("{PRE_UPGRADE_DIR_PREFIX}{secs}");
        let final_dir = format!("{data_dir}/{name}");
        let partial = format!("{name}{PARTIAL_SUFFIX}");
        let partial_dir = format!("{data_dir}/{partial}");
        if Path::new(&final_dir).exists() || Path::new(&partial_dir).exists() {
            return Err(Stop::Refuse(
                |s| Error::Startup(s.into()),
                format!(
                    "{final_dir} already exists; this build never merges two moves into one \
                     directory. Start again in a second"
                ),
            ));
        }
        fs::create_dir(&partial_dir)
            .and_then(|()| sync_dir(&data_dir))
            .map_err(|e| io(&format!("creating {partial_dir}"), e))?;
        fault("after-partial-created")?;
        self.resume(true, &partial)
    }

    /// Resume, or with a fresh `.partial`, perform, the consent move into `partial`.
    fn resume(&mut self, consent: bool, partial: &str) -> Result<(), Stop> {
        let data_dir = self.data_dir.clone();
        let partial_dir = format!("{data_dir}/{partial}");
        let env = CONSENT_ENV;
        if !consent {
            return Err(Stop::Refuse(
                |s| Error::Startup(s.into()),
                format!(
                    "{partial_dir}/ is an incomplete consent move of a legacy cache raft log. \
                     Start once with {env}=true to finish it; it resumes into that same directory"
                ),
            ));
        }
        let incomplete = |what: String| {
            Stop::Incomplete(format!(
                "{what}. The move's state is kept in {partial_dir}/, and the next start with \
                 {env}=true resumes it. Do not start hiqlite 0.14 on this directory"
            ))
        };
        // Keeps an injected crash a crash; words any other stop as `incomplete` does.
        let reword = |stop: Stop| match stop {
            Stop::Incomplete(what) | Stop::Refuse(_, what) => incomplete(what),
            #[cfg(test)]
            Stop::Crash => Stop::Crash,
        };

        // 1. The snapshots, first, so no interruption leaves a 0.14 snapshot at the original
        //    path without the log that makes the next start refuse it (F-130).
        let smc_from = format!("{data_dir}/state_machine_cache");
        let smc_to = format!("{partial_dir}/state_machine_cache");
        match (Path::new(&smc_from).exists(), Path::new(&smc_to).exists()) {
            (true, true) => {
                return Err(incomplete(format!(
                    "both {smc_from} and {smc_to} exist; this build will not choose between them"
                )));
            }
            (true, false) => {
                rename_synced(&smc_from, &smc_to, &data_dir, &partial_dir)
                    .map_err(|e| incomplete(format!("cannot move {smc_from}: {e}")))?;
                warn!("Moved the legacy cache snapshots {smc_from} to {smc_to}");
            }
            (false, _) => {}
        }
        fault("after-snapshots-moved")?;

        // 2. The log. The replacement is built and locked first, then the legacy directory
        //    moves (its lock goes with it), then the replacement takes its name.
        let dir_logs = format!("{data_dir}/logs_cache");
        let log_to = format!("{partial_dir}/logs_cache");
        let log_moved = Path::new(&log_to).exists();
        let marked = fs::read_to_string(format!("{dir_logs}/{CACHE_LOG_FORMAT_FILE}"))
            .is_ok_and(|found| found.trim() == CACHE_LOG_FORMAT);
        let legacy_here = !marked
            && holds_wal_files(&dir_logs)
                .map_err(|e| incomplete(format!("cannot read {dir_logs}: {e}")))?;
        if log_moved && legacy_here {
            return Err(incomplete(format!(
                "{log_to} exists and {dir_logs} holds a legacy log again; this build will not \
                 choose between them"
            )));
        }

        self.remove_stale_staging().map_err(reword)?;
        if legacy_here {
            self.swap_in_new_cache_log(&dir_logs, &log_to, &partial_dir)
                .map_err(reword)?;
        } else {
            // Already moved (or never there): the directory at the path is the one this start
            // created and locked at step 2. Mark it.
            self.write_marker_in_place(&dir_logs)
                .map_err(|e| incomplete(format!("cannot write the format marker: {e}")))?;
        }
        fault("after-log-moved")?;

        // 3. Complete: the directory takes its final name.
        let final_dir = partial_dir.trim_end_matches(PARTIAL_SUFFIX).to_string();
        if Path::new(&final_dir).exists() {
            return Err(incomplete(format!(
                "{final_dir} exists already; this build never merges two moves"
            )));
        }
        fs::rename(&partial_dir, &final_dir)
            .and_then(|()| sync_dir(&data_dir))
            .map_err(|e| incomplete(format!("cannot rename {partial_dir}: {e}")))?;
        warn!(
            "The legacy cache raft log and snapshots are in {final_dir}/: the cache raft starts \
             empty"
        );
        self.moved_to = Some(final_dir);
        Ok(())
    }

    /// Build `logs_cache.hiqlite-next/` with its lock held and the marker in it, move the
    /// legacy `logs_cache/` into the operation directory, and give the new one its name.
    fn swap_in_new_cache_log(
        &mut self,
        dir_logs: &str,
        log_to: &str,
        partial_dir: &str,
    ) -> Result<(), Stop> {
        let data_dir = self.data_dir.clone();
        let staged = format!("{data_dir}/{STAGED_CACHE_LOG_DIR}");
        let fail = Stop::Incomplete;

        create_dir_private(&staged).map_err(|e| fail(format!("cannot create {staged}: {e}")))?;
        let new_lock = match LockFile::try_acquire(&staged)
            .map_err(|e| fail(format!("cannot lock {staged}/lock.hql: {e}")))?
        {
            TryAcquire::Acquired(lock) => lock,
            TryAcquire::Held { .. } => {
                return Err(fail(format!("{staged}/lock.hql is locked by another process")));
            }
        };
        write_marker(&staged)
            .map_err(|e| fail(format!("cannot write the marker in {staged}: {e}")))?;
        sync_dir(&data_dir).map_err(|e| fail(format!("cannot sync {data_dir}: {e}")))?;
        fault("after-staged")?;

        // The legacy lock is held across the rename and follows the inode.
        fs::rename(dir_logs, log_to).map_err(|e| fail(format!("cannot move {dir_logs}: {e}")))?;
        fault("after-legacy-log-moved")?;
        // A contender that created `logs_cache` in the instant since would make this fail
        // (a non-empty directory), or would find the staged lock held (an empty one replaced).
        fs::rename(&staged, dir_logs).map_err(|e| {
            fail(format!(
                "the legacy log moved to {log_to}, but the replacement could not take its name \
                 ({e}); another process may have created {dir_logs} in between"
            ))
        })?;
        sync_dir(&data_dir)
            .and_then(|()| sync_dir(partial_dir))
            .map_err(|e| fail(format!("cannot sync the moved directories: {e}")))?;

        // The legacy directory keeps exactly its original entries: a lock file this start
        // created in it is unlinked from its new place, while still held.
        let old = self.cache.take().expect("the cache lock is held at step 2");
        if !old.lock.existed_before() {
            if let Err(err) = old.lock.unlink_while_held(log_to) {
                warn!("Could not remove {log_to}/lock.hql, which this start created: {err}");
            }
        } else {
            drop(old.lock);
        }
        self.cache = Some(WalGuard {
            dir: dir_logs.to_string(),
            lock: new_lock,
            created_dir: false,
        });
        warn!("Moved the legacy cache raft log {dir_logs} to {log_to}");
        Ok(())
    }

    /// A staging directory left by an interrupted start: it holds a lock file and the marker at
    /// most. Removed only if nobody holds its lock.
    fn remove_stale_staging(&self) -> Result<(), Stop> {
        let staged = format!("{}/{STAGED_CACHE_LOG_DIR}", self.data_dir);
        if !Path::new(&staged).exists() {
            return Ok(());
        }
        let entries = list_prefixed(&staged, |_| true)
            .map_err(|e| Stop::Incomplete(format!("cannot read {staged}: {e}")))?;
        let expected = |n: &str| {
            n == "lock.hql"
                || n == CACHE_LOG_FORMAT_FILE
                || n == format!("{CACHE_LOG_FORMAT_FILE}.tmp")
        };
        if let Some(other) = entries.iter().find(|n| !expected(n)) {
            return Err(Stop::Incomplete(format!(
                "{staged} holds {other}, which this build never writes there; inspect it"
            )));
        }
        let lock = match LockFile::try_acquire(&staged).map_err(|e| Stop::Incomplete(e.to_string()))? {
            TryAcquire::Acquired(lock) => lock,
            TryAcquire::Held { .. } => {
                return Err(Stop::Incomplete(format!(
                    "{staged}/lock.hql is locked by another process"
                )));
            }
        };
        for name in entries.iter().filter(|n| *n != "lock.hql") {
            fs::remove_file(format!("{staged}/{name}"))
                .map_err(|e| Stop::Incomplete(format!("cannot remove {staged}/{name}: {e}")))?;
        }
        lock.unlink_while_held(&staged)
            .map_err(|e| Stop::Incomplete(format!("cannot remove {staged}/lock.hql: {e}")))?;
        fs::remove_dir(&staged)
            .map_err(|e| Stop::Incomplete(format!("cannot remove {staged}: {e}")))?;
        sync_dir(&self.data_dir).map_err(|e| Stop::Incomplete(e.to_string()))
    }

    /// The published build's interrupted move (F-130): `state_machine_cache` still in place,
    /// and exactly one completed `pre-upgrade-<secs>/` holding `logs_cache` without
    /// `state_machine_cache`.
    fn published_interrupted_move(&self) -> Result<Option<String>, Stop> {
        let data_dir = &self.data_dir;
        if !Path::new(&format!("{data_dir}/state_machine_cache")).exists() {
            return Ok(None);
        }
        let candidates = list_prefixed(data_dir, |n| {
            n.starts_with(PRE_UPGRADE_DIR_PREFIX) && !n.ends_with(PARTIAL_SUFFIX)
        })
        .map_err(|e| {
            Stop::Refuse(
                |s| Error::Startup(s.into()),
                format!("cannot list {data_dir}: {e}"),
            )
        })?
        .into_iter()
        .filter(|n| {
            Path::new(&format!("{data_dir}/{n}/logs_cache")).exists()
                && !Path::new(&format!("{data_dir}/{n}/state_machine_cache")).exists()
        })
        .collect::<Vec<_>>();
        match candidates.len() {
            0 => Ok(None),
            1 => Ok(candidates.into_iter().next()),
            _ => Err(Stop::Refuse(
                |s| Error::Startup(s.into()),
                format!(
                    "{data_dir}/state_machine_cache is in place and more than one earlier consent \
                     move ({}) holds a cache log without snapshots; this build will not choose",
                    candidates.join(", ")
                ),
            )),
        }
    }

    fn write_marker_in_place(&self, dir_logs: &str) -> io::Result<()> {
        write_marker(dir_logs)
    }

    /// After `HQL_DANGER_RAFT_STATE_RESET` removed `logs_cache`, marker and all: the cache
    /// raft would otherwise write WAL files into a directory the next start refuses as legacy.
    /// A marked directory is left alone.
    #[cfg(feature = "cache")]
    pub(crate) fn mark_cache_log_if_unmarked(&self) -> Result<(), Error> {
        let Some(guard) = &self.cache else {
            return Ok(());
        };
        let marker = format!("{}/{CACHE_LOG_FORMAT_FILE}", guard.dir);
        if Path::new(&marker).exists() {
            return Ok(());
        }
        if holds_wal_files(&guard.dir).map_err(io_startup)? {
            return Err(Error::Startup(
                format!("{} holds WAL files and no format marker", guard.dir).into(),
            ));
        }
        write_marker(&guard.dir).map_err(io_startup)
    }
}

fn io_startup(err: io::Error) -> Error {
    Error::Startup(format!("cannot check the WAL lock: {err}").into())
}

fn holds_wal_files(dir: &str) -> io::Result<bool> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    for entry in entries {
        if entry?.file_name().to_string_lossy().ends_with(".wal") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn list_prefixed(dir: &str, keep: impl Fn(&str) -> bool) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if keep(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Create `dir` owner-only if it is absent. Returns whether this call created it.
fn create_dir_private(dir: &str) -> io::Result<bool> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(dir) {
        Ok(()) => {
            if let Some(parent) = Path::new(dir).parent() {
                sync_dir(&parent.to_string_lossy())?;
            }
            Ok(true)
        }
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err),
    }
}

fn write_marker(dir: &str) -> io::Result<()> {
    let marker = format!("{dir}/{CACHE_LOG_FORMAT_FILE}");
    let tmp = format!("{marker}.tmp");
    fs::write(&tmp, CACHE_LOG_FORMAT)?;
    fs::File::open(&tmp)?.sync_all()?;
    fs::rename(&tmp, &marker)?;
    sync_dir(dir)
}

fn rename_synced(from: &str, to: &str, from_parent: &str, to_parent: &str) -> io::Result<()> {
    fs::rename(from, to)?;
    sync_dir(from_parent)?;
    sync_dir(to_parent)
}

fn sync_dir(dir: &str) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

#[cfg(all(test, feature = "sqlite", feature = "cache"))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    type Tree = BTreeMap<String, (bool, u64, Vec<u8>)>;

    fn fresh(case: &str) -> String {
        let dir = format!("../target/test_data/upgrade_exclusion_unit/{case}");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Every entry: kind, inode, bytes.
    fn tree(dir: &str) -> Tree {
        fn walk(root: &Path, dir: &Path, out: &mut Tree) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                let ino = std::os::unix::fs::MetadataExt::ino(&meta);
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                if meta.is_dir() {
                    out.insert(rel, (true, ino, Vec::new()));
                    walk(root, &path, out);
                } else {
                    out.insert(rel, (false, ino, fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = Tree::new();
        walk(Path::new(dir), Path::new(dir), &mut out);
        out
    }

    fn legacy(dir: &str) {
        fs::create_dir_all(format!("{dir}/logs_cache")).unwrap();
        fs::write(format!("{dir}/logs_cache/00000000000000000001.wal"), b"legacy wal").unwrap();
        fs::create_dir_all(format!("{dir}/state_machine_cache/snapshots")).unwrap();
        fs::write(format!("{dir}/state_machine_cache/snapshots/s1"), b"legacy snap").unwrap();
    }

    fn acquire(dir: &str, consent: bool) -> Result<crate::storage_lock::StorageOwnership, Error> {
        acquire_storage(dir, true, true, consent)
    }

    /// A lock held through another open file description, as another process holds it.
    fn hold(dir: &str) -> LockFile {
        match LockFile::try_acquire(dir).unwrap() {
            TryAcquire::Acquired(lock) => lock,
            TryAcquire::Held { .. } => panic!("{dir} already held"),
        }
    }

    fn is_held(dir: &str) -> bool {
        matches!(LockFile::try_acquire(dir).unwrap(), TryAcquire::Held { .. })
    }

    fn without(mut tree: Tree, name: &str) -> Tree {
        tree.remove(name);
        tree
    }

    fn with_fault<T>(point: &'static str, f: impl FnOnce() -> T) -> T {
        FAULT_POINT.with(|p| *p.borrow_mut() = Some(point));
        let out = f();
        FAULT_POINT.with(|p| *p.borrow_mut() = None);
        out
    }

    /// `035` U-1: with a lock held on each of the three lock paths in turn, a start with consent
    /// refuses before any rename; entries, inodes and bytes are unchanged apart from the owner
    /// lock it may create.
    #[test]
    fn u1_a_held_lock_refuses_before_any_rename() {
        for held in ["hiqlite-owner.lock", "logs", "logs_cache"] {
            let dir = fresh(&format!("u1-{held}"));
            legacy(&dir);
            fs::create_dir_all(format!("{dir}/logs")).unwrap();
            let _holder = if held == "hiqlite-owner.lock" {
                Err(crate::storage_lock::StorageOwnership::acquire_without_note(&dir).unwrap())
            } else {
                Ok(hold(&format!("{dir}/{held}")))
            };
            let before = tree(&dir);

            let err = acquire(&dir, true).expect_err(held);
            assert!(matches!(err, Error::StorageInUse(_)), "{held}: {err}");
            let after = tree(&dir);
            if held == "hiqlite-owner.lock" {
                assert_eq!(before, after, "{held}");
            } else {
                assert_eq!(before, without(after, "hiqlite-owner.lock"), "{held}");
            }
        }
    }

    /// `035` U-2.
    #[cfg(not(feature = "auto-heal"))]
    #[test]
    fn u2_the_unclean_marker_refuses_before_any_rename() {
        let dir = fresh("u2");
        legacy(&dir);
        fs::create_dir_all(format!("{dir}/logs")).unwrap();
        fs::write(format!("{dir}/logs/meta.hql"), b"db raft metadata").unwrap();
        fs::create_dir_all(format!("{dir}/state_machine")).unwrap();
        fs::write(format!("{dir}/state_machine/lock"), b"").unwrap();
        let before = tree(&dir);

        let err = acquire(&dir, true).expect_err("refused");
        assert!(matches!(err, Error::Startup(_)), "{err}");
        assert!(err.to_string().contains("Nothing was moved"), "{err}");
        assert_eq!(before, without(tree(&dir), "hiqlite-owner.lock"));
    }

    /// `035` U-3: the log store adopts the held descriptor; taking the lock again, as a second
    /// descriptor would, fails in this process; the unclean-start signal survives.
    #[tokio::test]
    async fn u3_the_log_store_adopts_the_held_lock() {
        let dir = fresh("u3");
        fs::create_dir_all(format!("{dir}/logs")).unwrap();
        fs::write(format!("{dir}/logs/lock.hql"), b"").unwrap();

        let ownership = acquire(&dir, false).unwrap();
        let exclusion = ownership.wal_exclusion().unwrap();
        let lock = exclusion.db_lock().unwrap();
        assert!(lock.existed_before(), "a pre-existing lock file is the unclean-start signal");
        assert!(!exclusion.cache_lock().unwrap().existed_before());

        // Release-and-reacquire is not an option: the second descriptor is refused.
        let again = hiqlite_wal::LogStore::<crate::store::state_machine::sqlite::TypeConfigSqlite>::start(
            format!("{dir}/logs"),
            hiqlite_wal::LogSync::Immediate,
            64 * 1024,
        )
        .await;
        assert!(again.is_err(), "a second descriptor must not get the lock");

        let store = hiqlite_wal::LogStore::<crate::store::state_machine::sqlite::TypeConfigSqlite>::start_with_lock(
            format!("{dir}/logs"),
            lock,
            hiqlite_wal::LogSync::Immediate,
            64 * 1024,
        )
        .await
        .expect("the held lock is adopted");
        store.stop().await.unwrap();

        // A lock whose file is no longer at the path is refused.
        let moved = format!("{dir}/logs-moved");
        fs::rename(format!("{dir}/logs"), &moved).unwrap();
        fs::create_dir_all(format!("{dir}/logs")).unwrap();
        let err = hiqlite_wal::LogStore::<crate::store::state_machine::sqlite::TypeConfigSqlite>::start_with_lock(
            format!("{dir}/logs"),
            ownership.wal_exclusion().unwrap().db_lock().unwrap(),
            hiqlite_wal::LogSync::Immediate,
            64 * 1024,
        )
        .await
        .expect_err("a moved lock protects nothing at the path");
        assert!(err.to_string().contains("no longer the file"), "{err}");
    }

    /// `035` U-4: at every step, the file at `logs_cache/lock.hql`, when there is one, is held
    /// by this start; between the two renames there is none and the staged lock is held; and
    /// a contender that creates `logs_cache` in that instant makes the move stop, incomplete,
    /// with the legacy log kept.
    #[test]
    fn u4_the_cache_log_path_stays_locked_across_the_move() {
        let dir = fresh("u4");
        legacy(&dir);
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let (d, s) = (dir.clone(), seen.clone());
        PROBE.with(|p| {
            *p.borrow_mut() = Some(Box::new(move |point| {
                let at_path = Path::new(&format!("{d}/logs_cache/lock.hql")).exists();
                let staged = Path::new(&format!("{d}/{STAGED_CACHE_LOG_DIR}/lock.hql")).exists();
                let held = if at_path {
                    Some(is_held(&format!("{d}/logs_cache")))
                } else if staged {
                    Some(is_held(&format!("{d}/{STAGED_CACHE_LOG_DIR}")))
                } else {
                    None
                };
                s.borrow_mut().push((point, at_path, held));
            }))
        });
        let ownership = acquire(&dir, true).expect("the move completes");
        PROBE.with(|p| *p.borrow_mut() = None);
        assert_eq!(seen.borrow().len(), 7, "{:?}", seen.borrow());
        for (point, at_path, held) in seen.borrow().iter() {
            match *point {
                // Before this start took the cache lock, `logs_cache` was not locked by it.
                "after-db-lock" => continue,
                // The one instant with no file at the path; the staged lock is held.
                "after-legacy-log-moved" => assert_eq!(held, &Some(true), "{point}"),
                _ => {
                    assert!(at_path, "{point}: no lock file at the path");
                    assert_eq!(held, &Some(true), "{point}");
                }
            }
        }
        assert!(is_held(&format!("{dir}/logs_cache")));
        ownership.release_clean();

        // The contender in the instant between the renames.
        let dir = fresh("u4-contender");
        legacy(&dir);
        let d = dir.clone();
        let contender = std::rc::Rc::new(std::cell::RefCell::new(None));
        let c = contender.clone();
        PROBE.with(|p| {
            *p.borrow_mut() = Some(Box::new(move |point| {
                if point == "after-legacy-log-moved" {
                    fs::create_dir_all(format!("{d}/logs_cache")).unwrap();
                    *c.borrow_mut() = Some(hold(&format!("{d}/logs_cache")));
                }
            }))
        });
        let err = acquire(&dir, true).expect_err("the contender stops the move");
        PROBE.with(|p| *p.borrow_mut() = None);
        assert!(matches!(err, Error::Startup(_)), "{err}");
        assert!(err.to_string().contains("incomplete"), "{err}");
        let partial = list_prefixed(&dir, |n| n.ends_with(PARTIAL_SUFFIX)).unwrap();
        assert_eq!(partial.len(), 1);
        assert_eq!(
            fs::read(format!("{dir}/{}/logs_cache/00000000000000000001.wal", partial[0])).unwrap(),
            b"legacy wal"
        );
        let held = contender.borrow_mut().take().unwrap();
        assert!(held.is_linked_at(&format!("{dir}/logs_cache")).unwrap(), "left alone");
    }

    /// `035` U-5: each refusal names exactly what it created, against the listing.
    #[test]
    fn u5_a_refusal_names_what_it_created() {
        let dir = fresh("u5");
        legacy(&dir);
        let before = tree(&dir);
        let err = acquire(&dir, false).expect_err("legacy without consent");
        let msg = err.to_string();
        assert!(msg.contains("No data was changed"), "{msg}");
        assert!(msg.contains("this start created"), "{msg}");
        assert!(msg.contains("hiqlite-owner.lock"), "{msg}");
        assert!(msg.contains("was removed"), "{msg}");
        // `logs/` was absent: created for the lock, removed again. `logs_cache/lock.hql` too.
        assert_eq!(before, without(tree(&dir), "hiqlite-owner.lock"));

        let err = acquire(&dir, false).expect_err("again");
        assert!(err.to_string().contains("already existed"), "{err}");
        assert_eq!(before, without(tree(&dir), "hiqlite-owner.lock"));
    }

    /// `035` U-6 and F-133 at the node's level: the WAL lock outlives the log store's writer and
    /// is released, unlinked while held, only by the clean release after every writer stopped.
    #[tokio::test]
    async fn u6_the_wal_lock_outlives_the_writer_until_the_clean_release() {
        let dir = fresh("u6");
        let ownership = acquire(&dir, false).unwrap();
        let store = hiqlite_wal::LogStore::<crate::store::state_machine::sqlite::TypeConfigSqlite>::start_with_lock(
            format!("{dir}/logs"),
            ownership.wal_exclusion().unwrap().db_lock().unwrap(),
            hiqlite_wal::LogSync::Immediate,
            64 * 1024,
        )
        .await
        .unwrap();
        store.stop().await.unwrap();
        assert!(is_held(&format!("{dir}/logs")), "the writer stopped, the lock is still held");

        ownership.release_clean();
        assert!(!Path::new(&format!("{dir}/logs/lock.hql")).exists());
        assert!(!Path::new(&format!("{dir}/logs_cache/lock.hql")).exists());
        assert!(Path::new(&format!("{dir}/hiqlite-owner.lock")).exists(), "never unlinked");
    }

    /// `035` U-7: a crash after each step; the next start without consent refuses, and with
    /// consent completes, and no state leaves a legacy log or snapshot where the cache raft
    /// opens it.
    #[test]
    fn u7_every_interruption_has_a_defined_next_start() {
        for point in [
            "after-db-lock",
            "after-cache-lock",
            "after-partial-created",
            "after-snapshots-moved",
            "after-staged",
            "after-legacy-log-moved",
            "after-log-moved",
        ] {
            let dir = fresh(&format!("u7-{point}"));
            legacy(&dir);
            let crashed = with_fault(point, || acquire(&dir, true));
            assert!(crashed.is_err(), "{point}: the injected crash stops the start");

            let err = acquire(&dir, false).expect_err(point);
            assert!(matches!(err, Error::Startup(_)), "{point}: {err}");
            assert!(err.to_string().contains(CONSENT_ENV), "{point}: {err}");

            let ownership = acquire(&dir, true).unwrap_or_else(|e| panic!("{point}: {e}"));
            let moved = list_prefixed(&dir, |n| n.starts_with(PRE_UPGRADE_DIR_PREFIX)).unwrap();
            assert_eq!(moved.len(), 1, "{point}: one operation, one directory: {moved:?}");
            let moved = &moved[0];
            assert!(!moved.ends_with(PARTIAL_SUFFIX), "{point}: completed");
            assert_eq!(
                fs::read(format!("{dir}/{moved}/logs_cache/00000000000000000001.wal")).unwrap(),
                b"legacy wal",
                "{point}"
            );
            assert_eq!(
                fs::read(format!("{dir}/{moved}/state_machine_cache/snapshots/s1")).unwrap(),
                b"legacy snap",
                "{point}"
            );
            assert!(!Path::new(&format!("{dir}/state_machine_cache")).exists(), "{point}");
            assert!(!holds_wal_files(&format!("{dir}/logs_cache")).unwrap(), "{point}");
            assert_eq!(
                fs::read_to_string(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}")).unwrap(),
                CACHE_LOG_FORMAT,
                "{point}"
            );
            assert!(!Path::new(&format!("{dir}/{STAGED_CACHE_LOG_DIR}")).exists(), "{point}");
            ownership.release_clean();

            // And the start after that is ordinary.
            acquire(&dir, false).unwrap_or_else(|e| panic!("{point}: {e}")).release_clean();
        }
    }

    /// `027` B-10's unit cases, carried over: a fresh directory is marked, a foreign marker is
    /// refused, and snapshots without a cache log are not a legacy cache.
    #[test]
    fn fresh_foreign_and_memory_snapshot_directories() {
        let dir = fresh("fresh");
        acquire(&dir, false).unwrap().release_clean();
        assert_eq!(
            fs::read_to_string(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}")).unwrap(),
            CACHE_LOG_FORMAT
        );
        fs::write(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}"), b"9").unwrap();
        let err = acquire(&dir, true).expect_err("format 9 is not ours");
        assert!(err.to_string().contains("format 9"), "{err}");

        let dir = fresh("memory-snap");
        fs::create_dir_all(format!("{dir}/state_machine_cache/snapshots")).unwrap();
        fs::write(format!("{dir}/state_machine_cache/snapshots/s"), b"x").unwrap();
        acquire(&dir, false)
            .expect("snapshots without a cache log are not a legacy cache")
            .release_clean();
    }
}
