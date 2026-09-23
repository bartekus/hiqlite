use crate::error::Error;
use crate::lockfile::{LockFile, TryAcquire};
use crate::metadata::Metadata;
use crate::wal::WalFileSet;
use crate::{LogSync, ShutdownHandle, reader, writer};
use openraft::RaftTypeConfig;
use std::fs;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};
use tokio::sync::oneshot;
use tokio::task;
use tracing::warn;

/// `T::NodeId` MUST be a `u64` for the `LogStore` to work correctly.
#[derive(Debug)]
pub struct LogStore<T>
where
    T: RaftTypeConfig,
{
    meta: Arc<RwLock<Metadata>>,
    wal: Arc<RwLock<WalFileSet>>,
    pub writer: flume::Sender<writer::Action>,
    /// Observes the writer thread. See [`Self::writer_failure`].
    writer_failure: tokio::sync::watch::Receiver<Option<String>>,
    pub reader: flume::Sender<reader::Action>,
    _marker: PhantomData<T>,
}

impl<T> LogStore<T>
where
    T: RaftTypeConfig,
{
    /// Start the LogStore, taking and holding the directory's WAL lock itself.
    ///
    /// The lock is held by the writer thread and, at a clean stop, `lock.hql` is unlinked while
    /// still held and then released (`035` B-2). A lock held elsewhere is an error, never a
    /// panic, and the file's bytes are not truncated.
    pub async fn start(base_path: String, sync: LogSync, wal_size: u32) -> Result<Self, Error> {
        task::spawn_blocking(move || {
            prepare_dir(&base_path)?;
            let lockfile = match LockFile::try_acquire(&base_path)? {
                TryAcquire::Acquired(lock) => lock,
                TryAcquire::Held { .. } => {
                    return Err(Error::Locked(
                        "the WAL lock file is locked and in use by another process",
                    ));
                }
            };
            let lock_existed = lockfile.existed_before();
            if lock_existed {
                warn!("LockFile {base_path} exists already - this is not a clean start!");
            }
            Self::spawn_parts(base_path, Some(lockfile), lock_existed, sync, wal_size)
        })
        .await?
    }

    /// Start the LogStore in a directory whose WAL lock the caller already holds, and keeps
    /// holding until everything that writes to the directory set it belongs to has stopped.
    ///
    /// A second descriptor in the same process cannot take a lock the process holds, so a
    /// caller that excluded contenders before starting hands its lock over here instead of
    /// releasing and re-acquiring it (`035` B-2). The log store does not release or unlink
    /// that lock; the caller does, after its last write. `lock.existed_before()` carries the
    /// unclean-start signal, so a directory prepared by the caller is still told apart from
    /// one a previous run left locked.
    ///
    /// Refused when `lock` is no longer the file linked at `{base_path}/lock.hql`: a renamed
    /// or unlinked lock protects nothing at this path.
    pub async fn start_with_lock(
        base_path: String,
        lock: &LockFile,
        sync: LogSync,
        wal_size: u32,
    ) -> Result<Self, Error> {
        if !lock.is_linked_at(&base_path)? {
            return Err(Error::Locked(
                "the WAL lock handed to the log store is no longer the file linked at its path",
            ));
        }
        let lock_existed = lock.existed_before();
        task::spawn_blocking(move || {
            prepare_dir(&base_path)?;
            if lock_existed {
                warn!("LockFile {base_path} exists already - this is not a clean start!");
            }
            Self::spawn_parts(base_path, None, lock_existed, sync, wal_size)
        })
        .await?
    }

    fn spawn_parts(
        base_path: String,
        lockfile: Option<LockFile>,
        lock_existed: bool,
        sync: LogSync,
        wal_size: u32,
    ) -> Result<Self, Error> {
        let meta = Metadata::read_or_create(&base_path)?;
        let meta = Arc::new(RwLock::new(meta));

        let (writer, wal, writer_failure) = writer::spawn(
            base_path,
            lockfile,
            sync,
            wal_size,
            lock_existed,
            meta.clone(),
        )?;
        let reader = reader::spawn(meta.clone(), wal.clone())?;

        Ok(Self {
            meta,
            wal,
            writer,
            writer_failure,
            reader,
            _marker: Default::default(),
        })
    }

    /// Gives you a raw handle to the writer channel to perform manual migrations. Does not start
    /// a log store and does not do anything on its own.
    #[cfg(feature = "migration")]
    pub async fn start_writer_migration(
        base_path: String,
        wal_size: u32,
    ) -> Result<flume::Sender<writer::Action>, Error> {
        let lockfile = match LockFile::try_acquire(&base_path)? {
            TryAcquire::Acquired(lock) => lock,
            TryAcquire::Held { .. } => {
                return Err(Error::Locked(
                    "the WAL lock file is locked and in use by another process",
                ));
            }
        };
        let lock_exists = lockfile.existed_before();
        if lock_exists {
            warn!("LockFile in {base_path} exists already - this is not a clean start!");
        }

        task::spawn_blocking(move || {
            let meta = Metadata::read_or_create(&base_path)?;
            let meta = Arc::new(RwLock::new(meta));
            let (writer, _, _) = writer::spawn(
                base_path,
                Some(lockfile),
                LogSync::ImmediateAsync,
                wal_size,
                lock_exists,
                meta,
            )?;
            Ok(writer)
        })
        .await?
    }

    /// Watch the writer thread for termination.
    ///
    /// `None` while it is running; `Some(reason)` once it has ended and said why. The channel
    /// **closing** means the thread ended without saying why, which is what a panic looks like
    /// from outside it. All three are a statement about the writer and none of them is a
    /// statement about what the caller should do, which is the caller's policy.
    ///
    /// `008` KD-3 recorded that the writer's termination report had no consumer. This is the
    /// half that lets there be one.
    pub fn writer_failure(&self) -> tokio::sync::watch::Receiver<Option<String>> {
        self.writer_failure.clone()
    }

    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle::new(self.writer.clone(), self.reader.clone())
    }

    pub async fn stop(self) -> Result<(), Error> {
        let (tx_ack, ack) = oneshot::channel();
        self.writer
            .send_async(writer::Action::Shutdown(tx_ack))
            .await?;
        ack.await?;

        let _ = self.reader.send_async(reader::Action::Shutdown).await;

        Ok(())
    }

    pub(crate) fn spawn_reader(&self) -> Result<LogStoreReader<T>, Error> {
        let tx = reader::spawn(self.meta.clone(), self.wal.clone())?;

        Ok(LogStoreReader {
            tx,
            _marker: self._marker,
        })
    }
}

#[derive(Debug)]
pub struct LogStoreReader<T>
where
    T: RaftTypeConfig,
{
    pub tx: flume::Sender<reader::Action>,
    _marker: PhantomData<T>,
}

/// Create the WAL directory (owner-only on Linux), as every start did before taking its lock.
fn prepare_dir(base_path: &str) -> Result<(), Error> {
    fs::create_dir_all(base_path)?;
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(base_path)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(base_path, perms)?;
    }
    Ok(())
}
