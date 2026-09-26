//! Exclusive ownership of a hiqlite data directory.
//!
//! Before this existed, the only thing standing between two processes and the same storage was
//! a marker file at `{data_dir}/state_machine/lock`, whose presence was tested with
//! `File::open` and then created with `File::create`: a check and a create with a race between
//! them, no OS lock, empty content, removed on orderly shutdown and by a restore. It recorded
//! that a node had *started*, not that one was *running*, and F-005 is the finding that says
//! so. Under `auto-heal` the second process additionally wiped the first one's live database.
//!
//! This module is the actual exclusion: one OS advisory lock, taken before anything touches
//! storage and held until every task and handle that could touch it has stopped.
//!
//! **What it is not.** It is not a distributed lease and it is not object-store writer fencing.
//! It says one thing: on one machine, on a filesystem whose advisory locks work, two live
//! processes cannot own the same data directory.

use crate::Error;
use fs4::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// The owner lock lives at the data directory root, not inside `state_machine/`.
///
/// Deliberate: `state_machine/` is removed wholesale by a restore, and a lock file that a
/// restore can unlink is not a lock. Unlinking it while ownership is active would leave the
/// holder locking an inode nobody can reach, so the next process would create a *different*
/// file and both would believe they own the storage.
const OWNER_LOCK_FILE: &str = "hiqlite-owner.lock";

/// Exclusive ownership of one data directory, for as long as this value is alive.
///
/// The lock is released when the `File` is dropped, which includes the process ending for any
/// reason: an orderly exit, a panic, an abort, or a kill. There is no cleanup step that has to
/// run, which is the property a marker file could never have.
#[derive(Debug)]
pub(crate) struct StorageOwnership {
    /// Holding this open holds the lock. Never unlinked: see `OWNER_LOCK_FILE`.
    file: File,
    /// Diagnostics, and the one path the restore sweep has to skip.
    #[cfg_attr(not(any(test, feature = "backup")), allow(dead_code))]
    path: PathBuf,
    /// This start created the lock file, rather than finding it. A refusal says which (`035`
    /// B-3): the file is the only entry a refused start leaves behind.
    created: bool,
    /// The WAL directories' locks, held from before the first mutation until every writer of
    /// this node has stopped (`035` B-2). Released before the owner lock, which is last.
    wal_exclusion: Option<crate::upgrade_exclusion::UpgradeExclusion>,
}

impl StorageOwnership {
    /// Take exclusive ownership of `data_dir`, or refuse.
    ///
    /// Refusing does not touch anything else in the directory. That matters: the caller is
    /// about to run a restore, a migration, or a state-machine rebuild, and a contender that
    /// mutated storage before discovering it had lost would defeat the point.
    #[cfg_attr(not(any(test, feature = "__abort-probe")), allow(dead_code))]
    pub(crate) fn acquire(data_dir: &str) -> Result<Self, Error> {
        let mut slf = Self::acquire_without_note(data_dir)?;
        slf.record_owner_note(data_dir);
        Ok(slf)
    }

    /// [`Self::acquire`] without writing the owner note, so a start refused by a later check
    /// leaves the previous note as it was (`035` B-3). [`Self::record_owner_note`] writes it
    /// once the start has passed those checks.
    pub(crate) fn acquire_without_note(data_dir: &str) -> Result<Self, Error> {
        std::fs::create_dir_all(data_dir).map_err(|err| {
            Error::Config(format!("cannot create the data directory {data_dir}: {err}").into())
        })?;

        let path = Path::new(data_dir).join(OWNER_LOCK_FILE);

        // `truncate(false)` on purpose. Truncating would discard the previous owner's
        // diagnostics before we know whether we are allowed to have the lock at all, and it
        // would do it to a file another process may be holding.
        let open_err = |err: std::io::Error| {
            Error::Config(
                format!(
                    "cannot open the storage owner lock {}: {err}",
                    path.display()
                )
                .into(),
            )
        };
        let (mut file, created) = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => (file, true),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => (
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .truncate(false)
                    .open(&path)
                    .map_err(open_err)?,
                false,
            ),
            Err(err) => return Err(open_err(err)),
        };

        match FileExt::try_lock(&file) {
            Ok(()) => {}
            Err(fs4::TryLockError::WouldBlock) => {
                // Diagnostics only. Whoever wrote this may be gone, may have been killed, or
                // may never have written it; the refusal stands on the lock, not on this text.
                let mut held_by = String::new();
                let _ = file.read_to_string(&mut held_by);
                let held_by = held_by.trim();
                return Err(Error::StorageInUse(
                    format!(
                        "the data directory {data_dir} is owned by another live process, so this \
                         node refuses to start and {}. Last recorded owner, for diagnosis only (it \
                         may be an earlier process that has since stopped): {}",
                        if created {
                            "changed no data; it created hiqlite-owner.lock, which stays"
                        } else {
                            "has changed nothing"
                        },
                        if held_by.is_empty() {
                            "(the lock file carries no owner record)"
                        } else {
                            held_by
                        }
                    )
                    .into(),
                ));
            }
            Err(fs4::TryLockError::Error(err)) => {
                // A filesystem that cannot do this is an unsupported storage arrangement, and
                // it is refused explicitly rather than treated as "probably fine".
                return Err(Error::StorageInUse(
                    format!(
                        "the filesystem holding {data_dir} did not accept an exclusive advisory \
                         lock on {}: {err}. hiqlite cannot establish exclusive ownership of this \
                         storage and refuses to start on it",
                        path.display()
                    )
                    .into(),
                ));
            }
        }

        info!("Exclusive storage ownership acquired for {data_dir}");
        Ok(Self {
            file,
            path,
            created,
            wal_exclusion: None,
        })
    }

    /// Whether this start created `hiqlite-owner.lock`.
    pub(crate) fn created(&self) -> bool {
        self.created
    }

    /// Keep the WAL locks with the owner lock, so both are released at the end of shutdown,
    /// WAL locks first.
    pub(crate) fn attach_wal_exclusion(
        &mut self,
        exclusion: crate::upgrade_exclusion::UpgradeExclusion,
    ) {
        self.wal_exclusion = Some(exclusion);
    }

    /// The WAL locks, for the log stores that adopt them.
    pub(crate) fn wal_exclusion(&self) -> Option<&crate::upgrade_exclusion::UpgradeExclusion> {
        self.wal_exclusion.as_ref()
    }

    pub(crate) fn wal_exclusion_mut(
        &mut self,
    ) -> Option<&mut crate::upgrade_exclusion::UpgradeExclusion> {
        self.wal_exclusion.as_mut()
    }

    /// Release after a clean stop: the WAL lock files are unlinked while held and released,
    /// then the owner lock is released. Dropping without this keeps the WAL lock files, which
    /// is how the next start learns the stop was not clean.
    pub(crate) fn release_clean(mut self) {
        if let Some(exclusion) = self.wal_exclusion.take() {
            exclusion.release_clean();
        }
    }

    /// Record who holds the lock, for diagnosis. Written only after the start's checks passed.
    pub(crate) fn record_owner_note(&mut self, data_dir: &str) {
        let path = &self.path;
        let file = &mut self.file;
        // Diagnostics for an operator reading the file after a crash. Never read back as proof
        // of anything: a process id can be reused and a hostname can be a container's.
        let record = format!(
            "pid={} host={} since={} path={}\n",
            std::process::id(),
            hostname_or_unknown(),
            chrono::Utc::now().to_rfc3339(),
            std::fs::canonicalize(data_dir)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| data_dir.to_string()),
        );
        if let Err(err) = file
            .set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)))
            .and_then(|_| file.write_all(record.as_bytes()))
            .and_then(|()| file.sync_all())
        {
            // The lock is held; only the note failed. Say so and carry on rather than giving
            // up ownership we already have.
            warn!(
                "Could not record the storage owner note in {}: {err}",
                path.display()
            );
        }
    }

    /// The lock file path, for a caller that must make sure it does not delete it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// True when `path` is this ownership's lock file.
    ///
    /// Used by the restore path, which deletes the data directory's contents and must leave
    /// this one file alone.
    #[cfg_attr(not(any(test, feature = "backup")), allow(dead_code))]
    pub(crate) fn is_owner_lock_file(path: &Path) -> bool {
        path.file_name().is_some_and(|n| n == OWNER_LOCK_FILE)
    }
}

impl Drop for StorageOwnership {
    /// The WAL locks go before the owner lock on every drop, clean or not, so the owner lock is
    /// always the last one released (fields drop in declaration order, which is the reverse;
    /// found in review).
    fn drop(&mut self) {
        drop(self.wal_exclusion.take());
    }
}

fn hostname_or_unknown() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn scratch(name: &str) -> String {
        let dir = format!("../target/test_data/storage_lock/{name}");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The child half of the two-process tests. A no-op unless the parent asked for it, so a
    /// normal `cargo test` run does nothing here.
    ///
    /// It is a test rather than a separate binary so the parent can re-run its own executable,
    /// which is the only way to get a second *real* process without adding a build target.
    #[test]
    fn owner_lock_child_process() {
        let Ok(dir) = std::env::var("HQL_TEST_OWNER_DIR") else {
            return;
        };
        let mode = std::env::var("HQL_TEST_OWNER_MODE").unwrap_or_default();

        match StorageOwnership::acquire(&dir) {
            Ok(guard) => {
                println!("CHILD_ACQUIRED");
                let _ = std::io::stdout().flush();
                match mode.as_str() {
                    "hold" => {
                        // Held until the parent kills this process. The parent never waits this
                        // long; it is a ceiling so a failed test cannot leave a process behind.
                        std::thread::sleep(Duration::from_secs(60));
                        drop(guard);
                    }
                    "crash" => {
                        println!("CHILD_CRASHING");
                        let _ = std::io::stdout().flush();
                        // No unwinding, no destructors, no cleanup step. The OS releases it.
                        std::process::abort();
                    }
                    _ => {
                        drop(guard);
                        println!("CHILD_RELEASED");
                        let _ = std::io::stdout().flush();
                    }
                }
            }
            Err(err) => {
                println!("CHILD_REFUSED: {err}");
                let _ = std::io::stdout().flush();
            }
        }
    }

    fn spawn_child(dir: &str, mode: &str) -> std::process::Child {
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage_lock::tests::owner_lock_child_process",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HQL_TEST_OWNER_DIR", dir)
            .env("HQL_TEST_OWNER_MODE", mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the test binary must be re-runnable as a child process")
    }

    /// Run a child to completion and return its exit status together with everything it
    /// printed.
    ///
    /// Read to EOF rather than stopping at a marker: closing the pipe early makes the child's
    /// own test harness fail on a broken pipe, which is an artefact of the harness and not
    /// anything this module does.
    fn run_child(dir: &str, mode: &str) -> (std::process::ExitStatus, String) {
        let out = spawn_child(dir, mode)
            .wait_with_output()
            .expect("the child must run to completion");
        (out.status, String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Stream a child's stdout until `marker` appears, leaving it running.
    fn wait_for_marker(child: &mut std::process::Child, marker: &str) -> String {
        let stdout = child.stdout.take().expect("piped stdout");
        let reader = std::io::BufReader::new(stdout);
        let deadline = Instant::now() + Duration::from_secs(30);
        for line in reader.lines() {
            let line = line.unwrap_or_default();
            if line.contains(marker) {
                return line;
            }
            if Instant::now() > deadline {
                break;
            }
        }
        panic!("the child never printed {marker}");
    }

    /// A second process is refused, and **changes nothing** while being refused.
    ///
    /// The sentinel is the point: F-005's `auto-heal` path had the second process delete the
    /// first one's live database directory before anything established which of them owned the
    /// storage.
    #[test]
    fn a_second_process_is_refused_without_touching_the_data() {
        let dir = scratch("second_process_refused");
        let sentinel = format!("{dir}/sentinel");
        std::fs::write(&sentinel, b"owned by the first process").unwrap();

        let _owner = StorageOwnership::acquire(&dir).expect("the first owner takes it");

        let (_, output) = run_child(&dir, "once");

        assert!(
            output.contains("CHILD_REFUSED"),
            "a second process must be refused, got: {output}"
        );
        assert!(
            !output.contains("CHILD_ACQUIRED"),
            "a second process must not have acquired anything, got: {output}"
        );
        assert!(
            output.contains("changed nothing"),
            "the refusal must say that nothing was changed, got: {output}"
        );
        assert_eq!(
            std::fs::read(&sentinel).unwrap(),
            b"owned by the first process",
            "a refused contender must not have touched the data"
        );
    }

    /// An orderly shutdown releases ownership, and the next process gets it.
    #[test]
    fn an_orderly_shutdown_releases_ownership() {
        let dir = scratch("orderly_shutdown");

        let (status, output) = run_child(&dir, "once");
        assert!(output.contains("CHILD_ACQUIRED"), "got: {output}");
        assert!(output.contains("CHILD_RELEASED"), "got: {output}");
        assert!(
            status.success(),
            "the child must exit cleanly, printed: {output}"
        );

        let owner = StorageOwnership::acquire(&dir)
            .expect("ownership must be available after an orderly shutdown");
        assert!(owner.path().exists(), "the lock file is never unlinked");
    }

    /// A crash releases ownership too, and that is the property a marker file never had: there
    /// is no cleanup step that has to run.
    #[test]
    fn a_crash_releases_ownership() {
        let dir = scratch("crash_release");

        let (status, output) = run_child(&dir, "crash");
        assert!(output.contains("CHILD_ACQUIRED"), "got: {output}");
        assert!(output.contains("CHILD_CRASHING"), "got: {output}");
        assert!(
            !status.success(),
            "the child aborted, so it must not have exited successfully"
        );

        StorageOwnership::acquire(&dir)
            .expect("ownership must be available after the owner aborted");
    }

    /// A restore, a migration, or any other storage mutation started while another process
    /// owns the directory is refused at the door.
    ///
    /// This is the same refusal as the first test seen from the other side: the contender is
    /// the one holding a `HQL_BACKUP_RESTORE` request, and the point is that it never reaches
    /// the code that deletes anything.
    #[test]
    fn a_contender_is_refused_while_ownership_is_held_even_to_restore() {
        let dir = scratch("restore_while_held");
        std::fs::create_dir_all(format!("{dir}/state_machine/db")).unwrap();
        std::fs::write(format!("{dir}/state_machine/db/hiqlite.db"), b"live").unwrap();

        let _owner = StorageOwnership::acquire(&dir).expect("the live node owns it");

        let err = StorageOwnership::acquire(&dir)
            .expect_err("a restore must not be able to take storage that is in use");
        assert!(matches!(err, Error::StorageInUse(_)), "got: {err}");

        assert_eq!(
            std::fs::read(format!("{dir}/state_machine/db/hiqlite.db")).unwrap(),
            b"live",
            "the refused restore must not have removed the live database"
        );
    }

    /// The other direction: the **child** owns the storage and this process is refused.
    ///
    /// The first test proves a contender is refused; this one proves the holder can be a
    /// different process from the one doing the asking, which is what makes the lock an OS
    /// fact rather than an in-process convention.
    #[test]
    fn this_process_is_refused_while_another_process_holds_it() {
        let dir = scratch("child_holds");

        let mut child = spawn_child(&dir, "hold");
        let line = wait_for_marker(&mut child, "CHILD_ACQUIRED");
        assert!(line.contains("CHILD_ACQUIRED"), "got: {line}");

        let err =
            StorageOwnership::acquire(&dir).expect_err("another live process owns this directory");
        assert!(matches!(err, Error::StorageInUse(_)), "got: {err}");
        assert!(
            err.to_string().contains("pid="),
            "the refusal carries the owner note as a diagnostic, got: {err}"
        );

        let _ = child.kill();
        let _ = child.wait();

        // Killed, not shut down: ownership is still released, because there is no cleanup step.
        let mut acquired = None;
        for _ in 0..100 {
            if let Ok(guard) = StorageOwnership::acquire(&dir) {
                acquired = Some(guard);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            acquired.is_some(),
            "a killed owner must release the storage"
        );
    }

    /// Two nodes created inside **one** process are the same hazard as two processes, and an
    /// advisory lock on a separate open file description catches it.
    ///
    /// Worth its own test because a lock implementation keyed on a process id, a canonical
    /// path, or a global registry would pass the two-process tests and fail this one.
    #[test]
    fn a_second_node_in_the_same_process_is_refused() {
        let dir = scratch("same_process_duplicate");

        let _first = StorageOwnership::acquire(&dir).expect("the first node takes it");
        let err = StorageOwnership::acquire(&dir)
            .expect_err("a second node in the same process must be refused");
        assert!(matches!(err, Error::StorageInUse(_)), "got: {err}");
    }

    /// Two different paths naming the same directory are the same storage.
    ///
    /// A canonical-path comparison would get this right and a naive string comparison would
    /// not, which is why the exclusion is not built on either: the lock is on the inode, so a
    /// symlink, a bind mount, or a relative path all land on the same lock without anything
    /// having to reason about paths at all.
    #[test]
    #[cfg(unix)]
    fn an_aliased_path_to_the_same_directory_is_refused() {
        let dir = scratch("aliased_path");
        let alias = format!("../target/test_data/storage_lock/aliased_path_link");
        let _ = std::fs::remove_file(&alias);
        std::os::unix::fs::symlink(std::fs::canonicalize(&dir).unwrap(), &alias).unwrap();

        let _first = StorageOwnership::acquire(&dir).expect("the first owner takes it");
        let err = StorageOwnership::acquire(&alias)
            .expect_err("the same directory under another name must be refused");
        assert!(matches!(err, Error::StorageInUse(_)), "got: {err}");

        let _ = std::fs::remove_file(&alias);
    }

    /// Ownership is released when the guard is dropped, in the same process.
    #[test]
    fn dropping_the_guard_releases_ownership() {
        let dir = scratch("drop_releases");

        let first = StorageOwnership::acquire(&dir).unwrap();
        drop(first);
        StorageOwnership::acquire(&dir).expect("a dropped guard must have released the lock");
    }

    /// The lock file is a file the rest of the tree has to leave alone.
    #[test]
    fn the_owner_lock_file_is_recognisable() {
        let dir = scratch("recognisable");
        let owner = StorageOwnership::acquire(&dir).unwrap();
        assert!(StorageOwnership::is_owner_lock_file(owner.path()));
        assert!(!StorageOwnership::is_owner_lock_file(Path::new(
            "/tmp/state_machine/lock"
        )));
    }
}
