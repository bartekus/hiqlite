use crate::error::Error;
use fs4::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io;

/// The advisory lock on a WAL directory's `lock.hql`, held for as long as this value lives.
///
/// Every hiqlite version takes this lock on the WAL directories it opens, including 0.14,
/// which knows no other lock. Holding it is what keeps a process of any version out of the
/// directory; a lock-then-release probe keeps nobody out (`035` B-2).
///
/// The lock belongs to the open file description, so it follows the file's inode: a renamed
/// directory takes it along, and an unlinked file leaves the path free for a new file that a
/// contender can lock. [`Self::is_linked_at`] tells the two apart.
#[derive(Debug)]
pub struct LockFile {
    file: File,
    /// `lock.hql` existed before this value opened it: the previous user of the directory did
    /// not stop cleanly, or a start that refused left it. Today's unclean-start signal.
    existed: bool,
    /// A second descriptor on the caller's lock ([`Self::share`]): dropping it never unlinks
    /// the file, and never releases the lock while the caller's descriptor is open.
    shared: bool,
}

/// For each unlink, whether another descriptor saw the lock held at that instant (`035` U-6).
#[cfg(test)]
pub(crate) static UNLINK_OBSERVED: std::sync::Mutex<Vec<(String, bool)>> =
    std::sync::Mutex::new(Vec::new());

/// The outcome of [`LockFile::try_acquire`].
#[derive(Debug)]
pub enum TryAcquire {
    /// The lock is held by the returned value.
    Acquired(LockFile),
    /// Another open file description holds it: another live process, or another descriptor in
    /// this one. Nothing was created or changed, except `lock.hql` if it did not exist (`035`
    /// B-3), which the caller learns from `created`.
    Held { created: bool },
}

impl LockFile {
    /// Open `{base_path}/lock.hql` without truncating it, creating it if absent, and take a
    /// non-blocking exclusive lock on that descriptor.
    ///
    /// The directory must exist. The bytes of an existing file are never changed.
    pub fn try_acquire(base_path: &str) -> Result<TryAcquire, Error> {
        let path = Self::path(base_path);
        let (file, existed) = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => (file, false),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(|err| {
                        Error::Internal(format!("Cannot open WAL lock file {path}: {err}").into())
                    })?;
                (file, true)
            }
            Err(err) => {
                return Err(Error::Internal(
                    format!("Cannot create WAL lock file {path}: {err}").into(),
                ));
            }
        };

        match FileExt::try_lock(&file) {
            Ok(()) => Ok(TryAcquire::Acquired(Self {
                file,
                existed,
                shared: false,
            })),
            Err(fs4::TryLockError::WouldBlock) => Ok(TryAcquire::Held { created: !existed }),
            Err(fs4::TryLockError::Error(err)) => Err(Error::Internal(
                format!("Error locking WAL lock file {path}: {err}").into(),
            )),
        }
    }

    /// A duplicate descriptor of the same open file description, for a writer that must keep
    /// the lock held until its own last write even if the caller's value is dropped first
    /// (`035` B-2: a start that fails after its log store opened). An advisory `flock` belongs
    /// to the open file description, so it stays held until every duplicate is closed.
    /// Dropping the share, or passing it to [`Self::unlink_while_held`], never unlinks.
    pub fn share(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
            existed: self.existed,
            shared: true,
        })
    }

    /// Whether `lock.hql` existed before this value opened it.
    #[inline]
    pub fn existed_before(&self) -> bool {
        self.existed
    }

    /// Whether the file this value holds is the one linked at `{base_path}/lock.hql` now.
    ///
    /// False after the file was unlinked or its directory renamed, when the held lock protects
    /// an inode nobody reaches through `base_path` any more.
    pub fn is_linked_at(&self, base_path: &str) -> io::Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = self.file.metadata()?;
            match fs::metadata(Self::path(base_path)) {
                Ok(linked) => Ok(held.dev() == linked.dev() && held.ino() == linked.ino()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(err) => Err(err),
            }
        }
        #[cfg(not(unix))]
        {
            // No portable inode identity. Existence is the best this platform offers.
            fs::exists(Self::path(base_path))
        }
    }

    /// Unlink `{base_path}/lock.hql` while the lock is still held, then release it.
    ///
    /// The order is the point (`035` B-2, F-133): released first, a contender could lock the
    /// still-linked file between the release and the unlink, and would then hold a lock on a
    /// file nobody else can reach. Unlinked first, a contender can only create a new file, and
    /// by then the caller has finished writing. A file that is no longer the one held (a
    /// contender's, after a rename) is left alone.
    pub fn unlink_while_held(self, base_path: &str) -> io::Result<()> {
        if !self.shared && self.is_linked_at(base_path)? {
            #[cfg(test)]
            UNLINK_OBSERVED.lock().unwrap().push((
                base_path.to_string(),
                Self::is_locked(base_path).unwrap_or(false),
            ));
            fs::remove_file(Self::path(base_path))?;
        }
        drop(self);
        Ok(())
    }

    /// Whether `{base_path}/lock.hql` exists.
    #[inline]
    #[cfg(test)]
    pub(crate) fn exists(base_path: &str) -> Result<bool, Error> {
        Ok(fs::exists(Self::path(base_path))?)
    }

    /// Whether another open file description holds the lock. A momentary probe: it proves
    /// nothing about the next instant, and is only used by tests.
    #[cfg(test)]
    pub(crate) fn is_locked(base_path: &str) -> Result<bool, Error> {
        let file = File::open(Self::path(base_path))?;
        match FileExt::try_lock(&file) {
            Ok(()) => Ok(false),
            Err(fs4::TryLockError::WouldBlock) => Ok(true),
            Err(fs4::TryLockError::Error(err)) => Err(err.into()),
        }
    }

    #[inline]
    fn path(base_path: &str) -> String {
        format!("{base_path}/lock.hql")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = "test_data";

    fn fresh(name: &str) -> String {
        let base_path = format!("{PATH}/lockfile/{name}");
        let _ = fs::remove_dir_all(&base_path);
        fs::create_dir_all(&base_path).unwrap();
        base_path
    }

    fn acquired(base_path: &str) -> LockFile {
        match LockFile::try_acquire(base_path).unwrap() {
            TryAcquire::Acquired(lock) => lock,
            TryAcquire::Held { .. } => panic!("{base_path} was expected to be free"),
        }
    }

    #[test]
    fn lockfile() {
        let base_path = fresh("basic");

        assert!(!LockFile::exists(&base_path).unwrap());
        let lock = acquired(&base_path);
        assert!(!lock.existed_before());
        assert!(LockFile::exists(&base_path).unwrap());
        assert!(LockFile::is_locked(&base_path).unwrap());
        assert!(lock.is_linked_at(&base_path).unwrap());

        // A second descriptor, even in this process, does not get it.
        assert!(matches!(
            LockFile::try_acquire(&base_path).unwrap(),
            TryAcquire::Held { created: false }
        ));

        lock.unlink_while_held(&base_path).unwrap();
        assert!(!LockFile::exists(&base_path).unwrap());

        // A new file at the same path is free.
        let lock = acquired(&base_path);
        assert!(!lock.existed_before());
    }

    /// An existing file keeps its bytes, and reports that it existed.
    #[test]
    fn an_existing_lock_file_is_neither_truncated_nor_new() {
        let base_path = fresh("existing");
        fs::write(LockFile::path(&base_path), b"left by an earlier run").unwrap();

        let lock = acquired(&base_path);
        assert!(
            lock.existed_before(),
            "the unclean-start signal must survive"
        );
        assert_eq!(
            fs::read(LockFile::path(&base_path)).unwrap(),
            b"left by an earlier run"
        );
        drop(lock);
        // Released without unlinking: the file stays, and so does the signal.
        assert!(LockFile::exists(&base_path).unwrap());
    }

    /// A held lock follows a renamed directory; the original path is then free, and
    /// `is_linked_at` says so. Unlinking leaves a contender's file alone.
    #[test]
    fn a_held_lock_protects_an_inode_not_a_path() {
        let root = fresh("renamed");
        let dir = format!("{root}/logs_cache");
        let moved = format!("{root}/moved");
        fs::create_dir_all(&dir).unwrap();

        let lock = acquired(&dir);
        fs::rename(&dir, &moved).unwrap();
        assert!(!lock.is_linked_at(&dir).unwrap());
        assert!(lock.is_linked_at(&moved).unwrap());

        fs::create_dir_all(&dir).unwrap();
        let contender = acquired(&dir);
        assert!(!lock.is_linked_at(&dir).unwrap());

        // Not ours at `dir`: nothing is unlinked there.
        lock.unlink_while_held(&dir).unwrap();
        assert!(LockFile::exists(&dir).unwrap());
        assert!(contender.is_linked_at(&dir).unwrap());
        assert!(LockFile::exists(&moved).unwrap());
    }
}
