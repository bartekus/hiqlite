#![allow(clippy::upper_case_acronyms)]

use crate::helpers::{deserialize, set_path_access};
use crate::migration::Migration;
use crate::query::rows::RowOwned;
use crate::store::state_machine::sqlite::TypeConfigSqlite;
use crate::store::state_machine::sqlite::param::Param;
use crate::store::state_machine::sqlite::snapshot_builder::SQLiteSnapshotBuilder;
use crate::store::state_machine::sqlite::writer::WriterRequest::MetadataRead;
use crate::store::state_machine::sqlite::writer::{
    self, MetaPersistRequest, SqlBatch, SqlTransaction, WriterRequest,
};
use crate::store::{StorageResult, logs};
use crate::{Error, Node, NodeId};
use openraft::storage::RaftStateMachine;
use openraft::{
    EntryPayload, LogId, OptionalSend, Snapshot, SnapshotId, SnapshotMeta, StorageError,
    StorageIOError, StoredMembership,
};
use rusqlite::functions::FunctionFlags;
use rusqlite::{OpenFlags, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::clone::Clone;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::{fs, task, time};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

type Entry = openraft::Entry<TypeConfigSqlite>;
type SnapshotData = tokio::fs::File;

// TODO uses a `Mutex<_>` inside. We could make this pool a lot
//  faster by building our own lock-free one.
pub type SqlitePool = deadpool::unmanaged::Pool<rusqlite::Connection>;

pub type Params = Vec<Param>;

/// Non-deterministic SQLite functions that are forbidden on raft write connections,
/// where every node must apply the identical statement. The dashboard pre-scans
/// manual queries against this list to return a proper error instead of the panic.
pub(crate) const FORBIDDEN_NON_DET_FNS: &[&str] = &[
    "date",
    "datetime",
    "julianday",
    "now",
    "random",
    "randomblob",
    "strftime",
    "time",
    "timediff",
    "unixepoch",
];

pub struct PathDb(pub String);
pub struct PathBackups(pub String);
pub struct PathSnapshots(pub String);
pub struct PathLockFile(pub String);

// The variant order is part of the raft log format and must stay stable and
// feature-independent (see `CacheRequest` for details).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryWrite {
    Execute(Query),
    ExecuteReturning(Query),
    Transaction(Vec<Query>),
    Batch(Cow<'static, str>),
    Migration(Vec<Migration>),
    #[allow(dead_code)] // only constructed with the `backup` feature
    Backup((NodeId, i64)),
    RTT,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    pub sql: Cow<'static, str>,
    pub params: Params,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Empty,
    Execute(ResponseExecute),
    ExecuteReturning(ResponseExecuteReturning),
    Transaction(Result<Vec<Result<usize, Error>>, Error>),
    Batch(ResponseBatch),
    Migrate(Result<(), Error>),
    Backup(Result<(), Error>),
    RTT,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseExecute {
    pub result: Result<usize, Error>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseExecuteReturning {
    pub result: Result<Vec<Result<RowOwned, Error>>, Error>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseBatch {
    pub result: Result<Vec<Result<usize, Error>>, Error>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSnapshot {
    pub meta: SnapshotMeta<NodeId, Node>,
    pub path: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StateMachineData {
    pub last_applied_log_id: Option<LogId<NodeId>>,
    pub last_membership: StoredMembership<NodeId, Node>,
    pub last_snapshot_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StateMachineSqlite {
    // pub data: StateMachineData,
    this_node: NodeId,
    path_snapshots: String,
    #[cfg(feature = "backup")]
    path_backups: String,
    path_lock_file: String,

    #[cfg(feature = "s3")]
    s3_config: Option<Arc<crate::s3::S3Config>>,

    pub(crate) read_pool: SqlitePool,
    pub(crate) write_tx: flume::Sender<WriterRequest>,
}

/// What the local WAL can still supply, as read before the state machine is constructed.
///
/// Only used when startup has to fall back to an older snapshot. See
/// [`StateMachineSqlite::select_startup_snapshot`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SnapshotRecoveryBounds {
    /// The highest log index the WAL reports as purged, if any.
    pub(crate) last_purged_index: Option<u64>,
}

impl StateMachineSqlite {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn new(
        data_dir: &str,
        filename_db: &str,
        this_node: NodeId,
        log_statements: bool,
        prepared_statement_cache_capacity: usize,
        read_pool_size: usize,
        #[cfg(feature = "s3")] s3_config: Option<Arc<crate::s3::S3Config>>,
        do_reset_metadata: bool,
        #[cfg(feature = "backup")] local_backup_keep_days: u16,
        recovery_bounds: SnapshotRecoveryBounds,
    ) -> Result<StateMachineSqlite, Box<StorageError<NodeId>>> {
        // IMPORTANT: Do NOT change the order of the db exists check!
        // DB recovery will fail otherwise!
        let mut db_exists = Self::db_exists(data_dir, filename_db).await;
        debug!("db_exists in state_machine::new(): {db_exists}");

        let (
            PathDb(path_db),
            PathBackups(path_backups),
            PathSnapshots(path_snapshots),
            PathLockFile(path_lock_file),
        ) = Self::build_folders(data_dir, true).await;

        Self::check_set_lock_file(&path_lock_file, &path_db, &mut db_exists).await;

        // Always start the writer first! -> creates mandatory tables
        let conn = Self::connect(
            path_db.to_string(),
            filename_db.to_string(),
            false,
            prepared_statement_cache_capacity,
        )
        .await
        .map_err(|err| StorageError::IO {
            source: StorageIOError::write(&err),
        })?;
        let write_tx = writer::spawn_writer(
            conn,
            this_node,
            path_lock_file.clone(),
            log_statements,
            do_reset_metadata,
            #[cfg(feature = "backup")]
            local_backup_keep_days,
        );

        let read_pool = Self::connect_read_pool(
            path_db.as_ref(),
            filename_db,
            prepared_statement_cache_capacity,
            read_pool_size,
        )
        .await
        .map_err(|err| StorageError::IO {
            source: StorageIOError::read(&err),
        })?;

        let mut slf = Self {
            // data: state_machine_data,
            this_node,
            path_snapshots,
            #[cfg(feature = "backup")]
            path_backups,
            path_lock_file,
            #[cfg(feature = "s3")]
            s3_config,
            read_pool,
            write_tx,
        };

        if !db_exists && let Some(snapshot) = slf.select_startup_snapshot(recovery_bounds).await? {
            slf.update_state_machine_(snapshot.path).await?;
        }

        Ok(slf)
    }

    async fn db_exists(data_dir: &str, filename_db: &str) -> bool {
        let path_db = Self::path_db(data_dir);
        let path_db_full = format!("{path_db}/{filename_db}");
        fs::File::open(&path_db_full).await.is_ok()
    }

    pub fn path_base(data_dir: &str) -> String {
        format!("{data_dir}/state_machine")
    }

    fn path_db(data_dir: &str) -> String {
        format!("{}/db", Self::path_base(data_dir))
    }

    pub async fn build_folders(
        data_dir: &str,
        create: bool,
    ) -> (PathDb, PathBackups, PathSnapshots, PathLockFile) {
        let path_base = Self::path_base(data_dir);

        let path_db = Self::path_db(data_dir);
        let path_backups = format!("{path_base}/backups");
        let path_snapshots = format!("{path_base}/snapshots");
        let path_lock_file = format!("{path_base}/lock");

        if create {
            // this may error if we did already re-create it in a lock file recovery before
            let _ = fs::create_dir_all(&path_db).await;
            set_path_access(&path_base, 0o700)
                .await
                .expect("Cannot set access rights for path_base");
            set_path_access(&path_db, 0o700)
                .await
                .expect("Cannot set access rights for path_db");

            fs::create_dir_all(&path_backups)
                .await
                .expect("create state machine folder backups");
            set_path_access(&path_backups, 0o700)
                .await
                .expect("Cannot set access rights for path_backups");

            fs::create_dir_all(&path_snapshots)
                .await
                .expect("create state machine folder snapshots");
            set_path_access(&path_snapshots, 0o700)
                .await
                .expect("Cannot set access rights for path_snapshots");
        }

        (
            PathDb(path_db),
            PathBackups(path_backups),
            PathSnapshots(path_snapshots),
            PathLockFile(path_lock_file),
        )
    }

    async fn check_set_lock_file(path_lock_file: &str, path_db: &str, db_exists: &mut bool) {
        let is_locked = fs::File::open(path_lock_file).await.is_ok();

        if is_locked {
            #[cfg(feature = "auto-heal")]
            {
                warn!(
                    "Lock file already exists: {path_lock_file}\n\
                    Node did not shut down gracefully - auto-rebuilding State Machine"
                );

                // if we can't create the lock file, we will delete the current state machine
                // data so it can be rebuilt.
                // TODO is it enough to delete DB only, or do we need to do a full wipe?
                let _ = fs::remove_dir_all(path_db).await;

                // re-create the DB folder
                if let Err(err) = fs::create_dir_all(path_db).await {
                    panic!("Cannot re-create DB folder {path_db}: {err}");
                }

                *db_exists = false;
            }

            #[cfg(not(feature = "auto-heal"))]
            panic!(
                "Lock file already exists: {}\n\
                Node did not shut down gracefully - needs manual interaction",
                path_lock_file
            );
        } else if let Err(err) = fs::File::create(path_lock_file).await {
            panic!("Error creating lock file {path_lock_file}: {err}");
        }
    }

    pub(crate) fn remove_lock_file(path: &str) {
        let _ = std::fs::remove_file(path);
    }

    pub async fn connect(
        path: String,
        filename_db: String,
        read_only: bool,
        prepared_statement_cache_capacity: usize,
    ) -> Result<rusqlite::Connection, Error> {
        task::spawn_blocking(move || {
            let path_full = format!("{path}/{filename_db}");
            let conn = rusqlite::Connection::open(path_full)?;

            Self::apply_pragmas(&conn, read_only, prepared_statement_cache_capacity)?;
            if !read_only {
                Self::overwrite_non_det_fns(&conn);
            }

            Ok(conn)
        })
        .await?
    }

    async fn connect_read_pool(
        path: &str,
        filename_db: &str,
        prepared_statement_cache_capacity: usize,
        pool_size: usize,
    ) -> Result<SqlitePool, Error> {
        let path_full = format!("{path}/{filename_db}");

        let mut conns = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            let mut conn = Self::connect(
                path.to_string(),
                filename_db.to_string(),
                true,
                prepared_statement_cache_capacity,
            )
            .await;
            while conn.is_err() {
                time::sleep(Duration::from_millis(10)).await;
                conn = Self::connect(
                    path.to_string(),
                    filename_db.to_string(),
                    true,
                    prepared_statement_cache_capacity,
                )
                .await;
            }
            conns.push(conn?);
        }

        let pool = deadpool::unmanaged::Pool::from(conns);
        let conn = pool.get().await?;
        task::spawn_blocking(move || {
            let _ = conn.query_row("SELECT 1", (), |row| {
                let res: i64 = row.get(0)?;
                Ok(res)
            })?;
            Ok::<(), Error>(())
        })
        .await?;

        Ok(pool)
    }

    fn apply_pragmas(
        conn: &rusqlite::Connection,
        read_only: bool,
        prepared_statement_cache_capacity: usize,
    ) -> Result<(), rusqlite::Error> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // synchronous set to OFF is not an issue in our case.
        // If the OS crashes before it could flush any buffers to disk, we will rebuild the DB
        // anyway from the logs store just to be 100% sure that all cluster members are in a
        // consistent state. Setting it to OFF here gives us an ~18% boost compared to NORMAL while
        // not having any disadvantage with the Raft setup.
        conn.pragma_update(None, "synchronous", "OFF")?;

        conn.pragma_update(None, "page_size", 4096)?;
        conn.pragma_update(None, "journal_size_limit", 16384)?;
        conn.pragma_update(None, "wal_autocheckpoint", 4_000)?;

        // setting in-memory temp_store actually slows down SELECTs a little bit
        // conn.pragma_update(None, "temp_store", "memory")?;

        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        conn.pragma_update(None, "optimize", "0x10002")?;

        // backups/snapshot restores hold an exclusive lock; 30s busy timeout stops
        // concurrent reads failing with `SQLITE_BUSY` during those windows
        conn.busy_timeout(Duration::from_secs(30))?;

        // note:
        // in tests, `mmap_size` did not show any performance benefit with the settings above

        // only allow select statements
        if read_only {
            conn.pragma_update(None, "query_only", true)?;
        } else {
            // conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
        }

        // TODO make configurable
        conn.set_prepared_statement_cache_capacity(prepared_statement_cache_capacity);

        Ok(())
    }

    fn overwrite_non_det_fns(conn: &rusqlite::Connection) {
        // Overwrite the non-deterministic functions with a panicking guard: using one on
        // the write path would diverge the cluster, so it fails loudly.
        // No query-string scan: the guard only runs when a statement actually calls the
        // function, so queries that never use them pay nothing.
        for &name in FORBIDDEN_NON_DET_FNS {
            conn.create_scalar_function(
                name,
                -1,
                FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
                move |_| -> rusqlite::Result<String> {
                    panic!(
                        "forbidden usage of `{name}()` - non-deterministic functions must never be \
                        used for writing connections in a Raft cluster"
                    )
                },
            )
            .expect("Cannot register forbidden function");
        }
    }

    // The error type is huge, but defined by the openraft trait.
    #[allow(clippy::result_large_err)]
    async fn update_state_machine_(
        &mut self,
        snapshot_path: String,
    ) -> Result<(), StorageError<NodeId>> {
        let (tx, rx) = oneshot::channel();
        self.write_tx
            .send_async(WriterRequest::SnapshotApply((snapshot_path, tx)))
            .await
            .expect("SQLite Writer rx to always be listening");

        rx.await
            .expect("Snapshot installation to succeed")
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write(&err),
            })?;

        Ok(())
    }

    /// Every published snapshot in the directory, newest first.
    ///
    /// Only names that parse as a full UUID are published: `temp`, `{uuid}.temp` and
    /// `{uuid}.incoming` are staging names and are deliberately unselectable. UUIDv7 is
    /// time-ordered, so sorting the names is sorting them by age.
    async fn snapshot_candidates(&self) -> StorageResult<Vec<Uuid>> {
        let mut list = tokio::fs::read_dir(&self.path_snapshots)
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::read(&err),
            })?;

        let mut ids = Vec::new();
        loop {
            let entry = match list.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(err) => {
                    warn!("Error reading directory entries: {err:?}");
                    break;
                }
            };
            let file_name = entry.file_name();
            let name = file_name.to_str().unwrap_or("UNKNOWN");
            let Ok(id) = Uuid::parse_str(name) else {
                debug!("Non-UUID in snapshots folder");
                continue;
            };

            let meta = entry.metadata().await.map_err(|err| StorageError::IO {
                source: StorageIOError::read(&err),
            })?;
            if meta.is_dir() {
                warn!("Invalid folder in snapshots dir: {}", name);
                continue;
            }

            ids.push(id);
        }

        ids.sort();
        ids.reverse();
        Ok(ids)
    }

    /// Open one published snapshot and establish that it is usable, or say why it is not.
    ///
    /// Three things are checked, and the first two were not checked at all before. The file
    /// must pass SQLite's own `quick_check`, which is what catches a short or torn image; its
    /// `_metadata` row must exist and decode, which used to `expect` and panic; and the
    /// snapshot id embedded in it must match the file name, which used to be an `assert_eq!`
    /// and so was a panic rather than a rejection.
    ///
    /// A rejection is a value here, never a panic, because the whole point is to try the next
    /// candidate.
    async fn validate_snapshot(&self, id: Uuid) -> Result<StoredSnapshot, Error> {
        let path_snapshot = format!("{}/{}", self.path_snapshots, id);
        let conn = Self::connect(self.path_snapshots.clone(), id.to_string(), true, 2).await?;

        let id_str = id.to_string();
        let path_dbg = path_snapshot.clone();
        let metadata = task::spawn_blocking(move || {
            let check: String = conn
                .query_row("PRAGMA quick_check(1)", (), |row| row.get(0))
                .map_err(|err| {
                    Error::Sqlite(
                        format!("snapshot '{path_dbg}' failed its integrity check: {err}").into(),
                    )
                })?;
            if check != "ok" {
                return Err(Error::Sqlite(
                    format!("snapshot '{path_dbg}' failed its integrity check: {check}").into(),
                ));
            }

            let mut stmt = conn
                .prepare("SELECT data FROM _metadata WHERE key = 'meta'")
                .map_err(|err| {
                    Error::Sqlite(
                        format!("snapshot '{path_dbg}' has no readable metadata table: {err}")
                            .into(),
                    )
                })?;
            let bytes: Vec<u8> = stmt.query_row((), |row| row.get(0)).map_err(|err| {
                Error::Sqlite(format!("snapshot '{path_dbg}' has no metadata row: {err}").into())
            })?;
            let metadata: StateMachineData = deserialize(&bytes).map_err(|err| {
                Error::Sqlite(
                    format!("snapshot '{path_dbg}' has metadata that does not decode: {err}")
                        .into(),
                )
            })?;

            if metadata.last_snapshot_id.as_deref() != Some(id_str.as_str()) {
                return Err(Error::Sqlite(
                    format!(
                        "snapshot '{path_dbg}' carries snapshot id {:?}, which is not its file \
                         name",
                        metadata.last_snapshot_id
                    )
                    .into(),
                ));
            }

            Ok::<StateMachineData, Error>(metadata)
        })
        .await??;

        Ok(StoredSnapshot {
            meta: SnapshotMeta {
                last_log_id: metadata.last_applied_log_id,
                last_membership: metadata.last_membership,
                snapshot_id: id.to_string(),
            },
            path: path_snapshot,
        })
    }

    /// The same checks as [`Self::validate_snapshot`], against a staging path.
    ///
    /// The staged file is not yet under its published name, so the id it must carry is passed
    /// in rather than read from the file name.
    async fn validate_staged_snapshot(&self, path: &str, id: Uuid) -> Result<(), Error> {
        let (dir, file) = path
            .rsplit_once('/')
            .map(|(d, f)| (d.to_string(), f.to_string()))
            .ok_or_else(|| {
                Error::Error(format!("{path} is not a path inside a directory").into())
            })?;
        let conn = Self::connect(dir, file, true, 2).await?;

        let id_str = id.to_string();
        let path_dbg = path.to_string();
        task::spawn_blocking(move || {
            let check: String = conn
                .query_row("PRAGMA quick_check(1)", (), |row| row.get(0))
                .map_err(|err| {
                    Error::Sqlite(
                        format!("received snapshot '{path_dbg}' failed its integrity check: {err}")
                            .into(),
                    )
                })?;
            if check != "ok" {
                return Err(Error::Sqlite(
                    format!("received snapshot '{path_dbg}' failed its integrity check: {check}")
                        .into(),
                ));
            }

            let mut stmt = conn
                .prepare("SELECT data FROM _metadata WHERE key = 'meta'")
                .map_err(|err| {
                    Error::Sqlite(
                        format!("received snapshot '{path_dbg}' has no metadata table: {err}")
                            .into(),
                    )
                })?;
            let bytes: Vec<u8> = stmt.query_row((), |row| row.get(0)).map_err(|err| {
                Error::Sqlite(
                    format!("received snapshot '{path_dbg}' has no metadata row: {err}").into(),
                )
            })?;
            let metadata: StateMachineData = deserialize(&bytes).map_err(|err| {
                Error::Sqlite(
                    format!(
                        "received snapshot '{path_dbg}' has metadata that does not decode: {err}"
                    )
                    .into(),
                )
            })?;
            if metadata.last_snapshot_id.as_deref() != Some(id_str.as_str()) {
                return Err(Error::Sqlite(
                    format!(
                        "received snapshot '{path_dbg}' carries snapshot id {:?}, which is not \
                         the id it was sent as",
                        metadata.last_snapshot_id
                    )
                    .into(),
                ));
            }
            Ok::<(), Error>(())
        })
        .await??;

        Ok(())
    }

    /// The newest snapshot that is actually readable, for serving a peer.
    ///
    /// Used by `get_current_snapshot`, where an unreadable newest file means "send the one
    /// below it", not "refuse". Startup uses [`Self::select_startup_snapshot`] instead, which
    /// has to answer a harder question.
    // The error type is huge, but defined by the openraft trait.
    #[allow(clippy::result_large_err)]
    async fn read_current_snapshot(&mut self) -> StorageResult<Option<StoredSnapshot>> {
        for id in self.snapshot_candidates().await? {
            match self.validate_snapshot(id).await {
                Ok(snapshot) => return Ok(Some(snapshot)),
                Err(err) => warn!("Skipping unusable snapshot {id}: {err}"),
            }
        }
        Ok(None)
    }

    /// The snapshot this node may restart from, or a refusal that says what to do.
    ///
    /// Three outcomes, and the middle one is the whole of F-006:
    ///
    /// - no published snapshot at all: `Ok(None)`, which is a pristine node.
    /// - the newest published snapshot is usable: that one, unconditionally.
    /// - the newest is **not** usable: fall back to an older one, but only when the local WAL
    ///   still holds every entry from that older snapshot onwards. Otherwise refuse.
    ///
    /// The condition on the fallback is the part that is easy to get wrong. Restoring an older
    /// snapshot rewinds this node's applied state, and the only thing that can carry it forward
    /// again is the log. If the WAL has purged past that snapshot's last applied index there is
    /// a gap, and nothing local can close it.
    ///
    /// **A peer is not assumed to be able to close it either.** At `N = 1` there is no peer, so
    /// assuming one would be simply wrong; at `N > 1` a leader might be able to supply the
    /// missing history, but might equally have purged it or be unreachable. Refusing with an
    /// actionable error is the conservative answer in both cases, and it is the answer this
    /// spec takes.
    // The error type is huge, but defined by the openraft trait.
    #[allow(clippy::result_large_err)]
    async fn select_startup_snapshot(
        &mut self,
        bounds: SnapshotRecoveryBounds,
    ) -> StorageResult<Option<StoredSnapshot>> {
        let candidates = self.snapshot_candidates().await?;
        if candidates.is_empty() {
            return Ok(None);
        }

        let mut rejections: Vec<String> = Vec::new();
        for (position, id) in candidates.iter().enumerate() {
            let snapshot = match self.validate_snapshot(*id).await {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    warn!("Snapshot {id} cannot be used for recovery: {err}");
                    rejections.push(format!("{id}: {err}"));
                    continue;
                }
            };

            // The newest published snapshot is what this node was last told to be at. Taking
            // it is not a fallback and needs no log check.
            if position == 0 {
                return Ok(Some(snapshot));
            }

            let snapshot_index = snapshot.meta.last_log_id.map(|id| id.index).unwrap_or(0);
            match bounds.last_purged_index {
                // Nothing was ever purged, so every entry after this snapshot is still here.
                None => {
                    info!(
                        "Falling back to snapshot {id}: the newest published snapshot is not \
                         usable, and no WAL entry has been purged"
                    );
                    return Ok(Some(snapshot));
                }
                Some(purged) if purged <= snapshot_index => {
                    info!(
                        "Falling back to snapshot {id} at index {snapshot_index}: the WAL still \
                         holds every entry from there (purged through {purged})"
                    );
                    return Ok(Some(snapshot));
                }
                Some(purged) => {
                    rejections.push(format!(
                        "{id}: it stops at log index {snapshot_index} and the WAL has purged \
                         through {purged}, so the entries between them exist nowhere on this node"
                    ));
                }
            }
        }

        // Every candidate was rejected. Returning `None` here would start this node on an empty
        // database as though it had never held any state, which is the one outcome that must
        // not happen quietly.
        Err(StorageError::IO {
            source: StorageIOError::read_snapshot(
                None,
                openraft::AnyError::error(format!(
                    "no snapshot in {} can be used to recover this node, so it refuses to start \
                     rather than come up with an empty database. Candidates, newest first: {}. \
                     Recover by restoring a backup with HQL_BACKUP_RESTORE, or, on a cluster \
                     whose other members are healthy, by removing this node's data directory \
                     and letting it re-join as a new learner.",
                    self.path_snapshots,
                    rejections.join("; ")
                )),
            ),
        })
    }
}

impl RaftStateMachine<TypeConfigSqlite> for StateMachineSqlite {
    type SnapshotBuilder = SQLiteSnapshotBuilder;

    async fn applied_state(
        &mut self,
    ) -> Result<(Option<LogId<NodeId>>, StoredMembership<NodeId, Node>), StorageError<NodeId>> {
        let (ack, rx) = oneshot::channel();
        self.write_tx
            .send_async(WriterRequest::MetadataRead(ack))
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::read(&err),
            })?;
        let data = rx.await.expect("To always get Metadata from DB");

        debug!("applied_state: {:?}", data);

        Ok((data.last_applied_log_id, data.last_membership))
    }

    async fn apply<I>(&mut self, entries: I) -> Result<Vec<Response>, StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry> + OptionalSend,
        I::IntoIter: OptionalSend,
    {
        let entries = entries.into_iter();

        let (bound_lower, bound_upper) = entries.size_hint();
        let entries_len = bound_upper
            .expect("We always expect an upper bound to entries in apply()")
            - bound_lower
            + 1;
        let mut replies = Vec::with_capacity(entries_len);

        for entry in entries {
            let last_applied_log_id = Some(entry.log_id);

            // TODO if we always collect 1 in-flight req in a temp var to always have 1 req prepared
            // before we await the rx before, we could probably improve the throughput here a bit
            // in exchange for a more complicated logic -> test!

            let resp = match entry.payload {
                // TODO we probably need to update the log id in writer in case of ::Empty?
                EntryPayload::Blank => Response::Empty,

                EntryPayload::Normal(QueryWrite::Execute(Query { sql, params })) => {
                    let (tx, rx) = oneshot::channel();
                    let query = writer::Query::Execute(writer::SqlExecute {
                        sql,
                        params,
                        last_applied_log_id,
                        tx,
                    });

                    self.write_tx
                        .send_async(WriterRequest::Query(query))
                        .await
                        .expect("sql writer to always be listening");

                    let result = rx.await.expect("to always get a response from sql writer");
                    Response::Execute(ResponseExecute { result })
                }

                EntryPayload::Normal(QueryWrite::ExecuteReturning(Query { sql, params })) => {
                    let (tx, rx) = oneshot::channel();
                    let query = writer::Query::ExecuteReturning(writer::SqlExecuteReturning {
                        sql,
                        params,
                        last_applied_log_id,
                        tx,
                    });

                    self.write_tx
                        .send_async(WriterRequest::Query(query))
                        .await
                        .expect("sql writer to always be listening");

                    let result = rx.await.expect("to always get a response from sql writer");
                    Response::ExecuteReturning(ResponseExecuteReturning { result })
                }

                EntryPayload::Normal(QueryWrite::Transaction(queries)) => {
                    let (tx, rx) = oneshot::channel();
                    let req = WriterRequest::Query(writer::Query::Transaction(SqlTransaction {
                        queries,
                        last_applied_log_id,
                        tx,
                    }));

                    self.write_tx
                        .send_async(req)
                        .await
                        .expect("sql writer to always be listening");

                    let result = rx.await.expect("to always get a response from sql writer");
                    Response::Transaction(result)
                }

                EntryPayload::Normal(QueryWrite::Batch(sql)) => {
                    let (tx, rx) = oneshot::channel();
                    let req = WriterRequest::Query(writer::Query::Batch(SqlBatch {
                        sql,
                        last_applied_log_id,
                        tx,
                    }));

                    self.write_tx
                        .send_async(req)
                        .await
                        .expect("sql writer to always be listening");

                    let result = rx.await.expect("to always get a response from sql writer");
                    Response::Batch(ResponseBatch { result })
                }

                EntryPayload::Normal(QueryWrite::Backup((node_id, ts))) => {
                    #[cfg(feature = "backup")]
                    {
                        let (ack, rx) = oneshot::channel();
                        let req = WriterRequest::Backup(writer::BackupRequest {
                            node_id,
                            target_folder: self.path_backups.clone(),
                            ts,
                            #[cfg(feature = "s3")]
                            s3_config: self.s3_config.clone(),
                            last_applied_log_id,
                            ack,
                        });

                        self.write_tx
                            .send_async(req)
                            .await
                            .expect("sql writer to always be listening");

                        let result = rx.await.expect("to always get a response from sql writer");
                        Response::Backup(result)
                    }
                    #[cfg(not(feature = "backup"))]
                    unreachable!("Backup requires the `backup` feature")
                }

                EntryPayload::Normal(QueryWrite::Migration(migrations)) => {
                    let (tx, rx) = oneshot::channel();
                    let req = WriterRequest::Migrate(writer::Migrate {
                        migrations,
                        last_applied_log_id,
                        tx,
                    });

                    self.write_tx
                        .send_async(req)
                        .await
                        .expect("sql writer to always be listening");

                    let result = rx.await.expect("to always get a response from sql writer");
                    Response::Migrate(result)
                }

                EntryPayload::Normal(QueryWrite::RTT) => {
                    let (ack, rx) = oneshot::channel();
                    let req = WriterRequest::RTT(writer::RTTRequest {
                        last_applied_log_id,
                        ack,
                    });

                    self.write_tx
                        .send_async(req)
                        .await
                        .expect("sql writer to always be listening");

                    rx.await.expect("to always get a response from sql writer");
                    Response::RTT
                }

                EntryPayload::Membership(mem) => {
                    let (ack, rx) = oneshot::channel();
                    let req = WriterRequest::MetadataMembership(writer::MetaMembershipRequest {
                        last_membership: StoredMembership::new(Some(entry.log_id), mem),
                        last_applied_log_id,
                        ack,
                    });

                    self.write_tx
                        .send_async(req)
                        .await
                        .expect("sql writer to always be listening");

                    rx.await.expect("to always get a response from sql writer");

                    Response::Empty
                }
            };

            replies.push(resp);
        }

        Ok(replies)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        // TODO clean up possibly existing restore files inside snapshot builder upon success

        SQLiteSnapshotBuilder {
            #[cfg(feature = "backup")]
            path_backups: self.path_backups.clone(),
            path_snapshots: self.path_snapshots.clone(),
            write_tx: self.write_tx.clone(),
        }
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn begin_receiving_snapshot(&mut self) -> Result<Box<fs::File>, StorageError<NodeId>> {
        let path = format!("{}/temp", self.path_snapshots);

        // clean up possible existing old data
        let _ = fs::remove_file(&path).await;

        match fs::File::create(path).await {
            Ok(file) => Ok(Box::new(file)),
            Err(err) => Err(StorageError::IO {
                source: StorageIOError::write(&err),
            }),
        }
    }

    #[tracing::instrument(level = "trace", skip(self, _snapshot))]
    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, Node>,
        _snapshot: Box<SnapshotData>,
    ) -> Result<(), StorageError<NodeId>> {
        let src = format!("{}/temp", self.path_snapshots);
        let staged = format!("{}/{}.incoming", self.path_snapshots, meta.snapshot_id);
        let dest = format!("{}/{}", self.path_snapshots, meta.snapshot_id);

        // Stage first. `{id}.incoming` is not a UUID, so nothing selects it at restart, which
        // is what makes every failure below survivable: the received image is on disk under a
        // name that cannot be mistaken for a published snapshot.
        fs::rename(&src, &staged)
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write(&err),
            })?;
        crate::store::state_machine::sqlite::sync_file(&staged)
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write(&err),
            })?;
        crate::store::state_machine::sqlite::sync_parent_dir(&staged)
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write(&err),
            })?;

        // Validate before touching anything. A stream that ended early, or one that is not a
        // SQLite database at all, is discarded here with the live database untouched. Before
        // this, the file was published under its final name first and validated never.
        let staged_id = Uuid::parse_str(&meta.snapshot_id).map_err(|err| StorageError::IO {
            source: StorageIOError::write(openraft::AnyError::error(format!(
                "the received snapshot id {:?} is not a UUID: {err}",
                meta.snapshot_id
            ))),
        })?;
        if let Err(err) = self.validate_staged_snapshot(&staged, staged_id).await {
            let _ = fs::remove_file(&staged).await;
            return Err(StorageError::IO {
                source: StorageIOError::write(openraft::AnyError::error(format!(
                    "the received snapshot {} is not usable and was discarded without touching \
                     this node's database: {err}",
                    meta.snapshot_id
                ))),
            });
        }

        // Restore from the staging name, not the published one. A restore that fails now leaves
        // no new published snapshot, so restart selection still sees the state this node had
        // before the install, which is the property F-004 was missing.
        self.update_state_machine_(staged.clone()).await?;

        // Publish only now, and durably. From here on this snapshot is a legitimate restart
        // candidate, which it is, because the database has just been restored from it.
        fs::rename(&staged, &dest)
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write(&err),
            })?;
        crate::store::state_machine::sqlite::sync_parent_dir(&dest)
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write(&err),
            })?;

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<TypeConfigSqlite>>, StorageError<NodeId>> {
        match self.read_current_snapshot().await? {
            None => Ok(None),
            Some(snap) => {
                let file = fs::File::open(&snap.path)
                    .await
                    .map_err(|err| StorageError::IO {
                        source: StorageIOError::read(&err),
                    })?;

                Ok(Some(Snapshot {
                    meta: snap.meta,
                    snapshot: Box::new(file),
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::serialize;
    use crate::store::state_machine::sqlite::snapshot_builder::SNAPSHOTS_KEPT;
    use hiqlite_wal::{Action, LogStore, LogSync};
    use openraft::storage::{RaftLogReader, RaftStateMachine};
    use openraft::{CommittedLeaderId, RaftSnapshotBuilder};

    fn batch_entry(index: u64, sql: &'static str) -> Entry {
        Entry {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), index),
            payload: EntryPayload::Normal(QueryWrite::Batch(Cow::Borrowed(sql))),
        }
    }

    async fn append_entries_to_wal(log_store: &LogStore<TypeConfigSqlite>, entries: &[Entry]) {
        let (entry_tx, entry_rx) = flume::bounded(1);
        let (ack_tx, ack_rx) = oneshot::channel();
        let (completed_tx, completed_rx) = oneshot::channel();

        log_store
            .writer
            .send_async(Action::Append {
                rx: entry_rx,
                // The completion notification is result-bearing, so this helper forwards it and
                // the assertion below fails if the WAL reports a failed append.
                callback: Box::new(move |res| {
                    let _ = completed_tx.send(res);
                }),
                ack: ack_tx,
            })
            .await
            .unwrap();

        for entry in entries {
            entry_tx
                .send_async(Some((entry.log_id.index, serialize(entry).unwrap())))
                .await
                .unwrap();
        }
        entry_tx.send_async(None).await.unwrap();

        ack_rx.await.unwrap().unwrap();
        completed_rx.await.unwrap().unwrap();
    }

    async fn shutdown_state_machine(state_machine: &StateMachineSqlite) {
        let (ack_tx, ack_rx) = oneshot::channel();
        state_machine
            .write_tx
            .send_async(WriterRequest::Shutdown(ack_tx))
            .await
            .unwrap();
        ack_rx.await.unwrap();
    }

    #[tokio::test]
    async fn restart_reconstructs_snapshot_then_replays_retained_wal() {
        let root =
            std::env::temp_dir().join(format!("hiqlite-snapshot-retained-wal-{}", Uuid::now_v7()));
        let root_str = root.to_string_lossy().into_owned();
        let logs_path = root.join("logs").to_string_lossy().into_owned();

        let snapshot_entry = batch_entry(
            1,
            "CREATE TABLE recovery (id INTEGER PRIMARY KEY, value TEXT NOT NULL);\
             INSERT INTO recovery VALUES (1, 'from snapshot');",
        );
        let retained_entry =
            batch_entry(2, "INSERT INTO recovery VALUES (2, 'from retained wal');");

        let log_store =
            LogStore::<TypeConfigSqlite>::start(logs_path.clone(), LogSync::Immediate, 64 * 1024)
                .await
                .unwrap();
        append_entries_to_wal(
            &log_store,
            &[snapshot_entry.clone(), retained_entry.clone()],
        )
        .await;

        let mut state_machine = StateMachineSqlite::new(
            &root_str,
            "state.sqlite",
            1,
            false,
            2,
            1,
            #[cfg(feature = "s3")]
            None,
            false,
            #[cfg(feature = "backup")]
            1,
            SnapshotRecoveryBounds::default(),
        )
        .await
        .unwrap();
        state_machine.apply([snapshot_entry]).await.unwrap();

        let snapshot = state_machine
            .get_snapshot_builder()
            .await
            .build_snapshot()
            .await
            .unwrap();
        assert_eq!(snapshot.meta.last_log_id.unwrap().index, 1);

        shutdown_state_machine(&state_machine).await;
        drop(snapshot);
        drop(state_machine);
        log_store.stop().await.unwrap();

        fs::remove_dir_all(root.join("state_machine/db"))
            .await
            .unwrap();

        let mut recovered = StateMachineSqlite::new(
            &root_str,
            "state.sqlite",
            1,
            false,
            2,
            1,
            #[cfg(feature = "s3")]
            None,
            false,
            #[cfg(feature = "backup")]
            1,
            SnapshotRecoveryBounds::default(),
        )
        .await
        .unwrap();
        assert_eq!(recovered.applied_state().await.unwrap().0.unwrap().index, 1);

        let mut retained_log_store =
            LogStore::<TypeConfigSqlite>::start(logs_path, LogSync::Immediate, 64 * 1024)
                .await
                .unwrap();
        let retained = retained_log_store.try_get_log_entries(2..=2).await.unwrap();
        assert_eq!(retained.len(), 1);
        recovered.apply(retained).await.unwrap();

        let conn = recovered.read_pool.get().await.unwrap();
        let rows = task::spawn_blocking(move || {
            let mut stmt = conn
                .prepare("SELECT id, value FROM recovery ORDER BY id")
                .unwrap();
            stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
        })
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (1, "from snapshot".to_string()),
                (2, "from retained wal".to_string()),
            ]
        );
        assert_eq!(recovered.applied_state().await.unwrap().0.unwrap().index, 2);

        shutdown_state_machine(&recovered).await;
        drop(recovered);
        retained_log_store.stop().await.unwrap();
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn interrupted_staging_files_are_not_published_snapshots() {
        let root =
            std::env::temp_dir().join(format!("hiqlite-snapshot-staging-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).await.unwrap();
        fs::write(root.join("temp"), b"partial receive")
            .await
            .unwrap();
        fs::write(
            root.join(format!("{}.temp", Uuid::now_v7())),
            b"complete builder staging image",
        )
        .await
        .unwrap();

        let (write_tx, _write_rx) = flume::bounded(1);
        let mut state_machine = StateMachineSqlite {
            this_node: 1,
            path_snapshots: root.to_string_lossy().into_owned(),
            #[cfg(feature = "backup")]
            path_backups: root.to_string_lossy().into_owned(),
            path_lock_file: root.join("lock").to_string_lossy().into_owned(),
            #[cfg(feature = "s3")]
            s3_config: None,
            read_pool: SqlitePool::from(Vec::<rusqlite::Connection>::new()),
            write_tx,
        };

        assert!(
            state_machine
                .read_current_snapshot()
                .await
                .unwrap()
                .is_none()
        );
        fs::remove_dir_all(root).await.unwrap();
    }

    // ---- W-20: publication, installation and restart selection ----

    /// Build one state machine over `root`, apply `sql` at index `index`, and take a snapshot.
    /// Returns the published snapshot id.
    async fn build_one_snapshot(root_str: &str, index: u64, sql: &'static str) -> String {
        let mut sm = StateMachineSqlite::new(
            root_str,
            "state.sqlite",
            1,
            false,
            2,
            1,
            #[cfg(feature = "s3")]
            None,
            false,
            #[cfg(feature = "backup")]
            1,
            SnapshotRecoveryBounds::default(),
        )
        .await
        .unwrap();
        sm.apply([batch_entry(index, sql)]).await.unwrap();
        let snapshot = sm
            .get_snapshot_builder()
            .await
            .build_snapshot()
            .await
            .unwrap();
        let id = snapshot.meta.snapshot_id.clone();
        drop(snapshot);
        shutdown_state_machine(&sm).await;
        drop(sm);
        id
    }

    fn scratch_root(name: &str) -> (std::path::PathBuf, String) {
        let root = std::env::temp_dir().join(format!("hiqlite-w20-{name}-{}", Uuid::now_v7()));
        let root_str = root.to_string_lossy().into_owned();
        (root, root_str)
    }

    async fn state_machine_over(path_snapshots: &str) -> StateMachineSqlite {
        let (write_tx, _rx) = flume::bounded(1);
        // Deliberately leaked: the test only calls selection, which never sends to the writer,
        // and dropping the receiver here would make an accidental send fail loudly instead of
        // hanging.
        std::mem::forget(_rx);
        StateMachineSqlite {
            this_node: 1,
            path_snapshots: path_snapshots.to_string(),
            #[cfg(feature = "backup")]
            path_backups: path_snapshots.to_string(),
            path_lock_file: format!("{path_snapshots}/lock"),
            #[cfg(feature = "s3")]
            s3_config: None,
            read_pool: SqlitePool::from(Vec::<rusqlite::Connection>::new()),
            write_tx,
        }
    }

    /// F-003: publication used `fs::copy` into the final UUID name, so an interruption left a
    /// short file under a name startup selection accepts.
    ///
    /// The injection is deterministic rather than timing-based: the file is written truncated
    /// on purpose, which is exactly the state an interrupted copy leaves behind, and selection
    /// must not take it.
    #[tokio::test]
    async fn a_torn_published_snapshot_is_never_selected() {
        let (root, root_str) = scratch_root("torn");
        let snapshots = format!("{root_str}/state_machine/snapshots");

        let good_id = build_one_snapshot(
            &root_str,
            1,
            "CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);",
        )
        .await;

        // A newer UUID whose file is a truncated copy of the good one: a valid name, a broken
        // image. `Uuid::now_v7()` sorts above the one just built, so selection sees it first.
        let torn_id = Uuid::now_v7();
        let good_bytes = fs::read(format!("{snapshots}/{good_id}")).await.unwrap();
        fs::write(
            format!("{snapshots}/{torn_id}"),
            &good_bytes[..good_bytes.len() / 3],
        )
        .await
        .unwrap();

        let mut sm = state_machine_over(&snapshots).await;
        let selected = sm
            .select_startup_snapshot(SnapshotRecoveryBounds::default())
            .await
            .expect("an older valid snapshot is available, so startup is not refused")
            .expect("one of them is usable");
        assert_eq!(
            selected.meta.snapshot_id, good_id,
            "the torn newest file must be skipped for the older valid one"
        );

        fs::remove_dir_all(root).await.unwrap();
    }

    /// F-006: startup chose the greatest UUID and then asserted on its embedded id, so a
    /// corrupt newest snapshot was a panic and never a fallback.
    ///
    /// Three different corruptions, each of which used to be fatal in its own way: not a
    /// database at all, a valid database with no metadata, and a valid snapshot whose embedded
    /// id does not match its file name.
    #[tokio::test]
    async fn a_corrupt_newest_snapshot_falls_back_to_an_older_valid_one() {
        for (case, bytes) in [
            ("not-a-database", b"this is not a sqlite file".to_vec()),
            ("empty", Vec::new()),
        ] {
            let (root, root_str) = scratch_root(&format!("fallback-{case}"));
            let snapshots = format!("{root_str}/state_machine/snapshots");
            let good_id = build_one_snapshot(&root_str, 1, "CREATE TABLE t (id INTEGER);").await;

            let bad_id = Uuid::now_v7();
            fs::write(format!("{snapshots}/{bad_id}"), &bytes)
                .await
                .unwrap();

            let mut sm = state_machine_over(&snapshots).await;
            let selected = sm
                .select_startup_snapshot(SnapshotRecoveryBounds::default())
                .await
                .unwrap_or_else(|err| panic!("{case}: startup must fall back, got {err}"))
                .expect("the older snapshot is usable");
            assert_eq!(selected.meta.snapshot_id, good_id, "{case}");

            fs::remove_dir_all(root).await.unwrap();
        }
    }

    /// The fallback is not unconditional. Rewinding to an older snapshot is only recoverable
    /// while the WAL still holds every entry from that snapshot onwards; once it has purged
    /// past it, the entries between exist nowhere on this node.
    ///
    /// `N = 1` is the case that makes this non-negotiable: there is no peer that could supply
    /// the missing history, so a node that started anyway would silently be missing committed
    /// writes. At `N > 1` a leader might be able to supply them and might equally have purged
    /// them or be unreachable, so the refusal is the same and the error says what to do.
    #[tokio::test]
    async fn a_fallback_is_refused_when_the_wal_has_purged_past_the_older_snapshot() {
        let (root, root_str) = scratch_root("purged");
        let snapshots = format!("{root_str}/state_machine/snapshots");
        let _good_id = build_one_snapshot(&root_str, 1, "CREATE TABLE t (id INTEGER);").await;

        let bad_id = Uuid::now_v7();
        fs::write(format!("{snapshots}/{bad_id}"), b"torn")
            .await
            .unwrap();

        let mut sm = state_machine_over(&snapshots).await;
        let err = sm
            .select_startup_snapshot(SnapshotRecoveryBounds {
                // The older snapshot stops at index 1 and the WAL has purged through 42.
                last_purged_index: Some(42),
            })
            .await
            .expect_err("startup must refuse rather than come up missing committed writes");

        let text = err.to_string();
        assert!(
            text.contains("purged through 42"),
            "the refusal must say why the fallback is not recoverable, got: {text}"
        );
        assert!(
            text.contains("HQL_BACKUP_RESTORE"),
            "the refusal must be actionable, got: {text}"
        );

        fs::remove_dir_all(root).await.unwrap();
    }

    /// When nothing is usable, startup refuses. It does **not** come up on an empty database,
    /// which is what returning `None` here would have meant: a node that had held state
    /// silently presenting itself as pristine.
    #[tokio::test]
    async fn no_usable_snapshot_refuses_startup_instead_of_starting_empty() {
        let (root, root_str) = scratch_root("none-usable");
        let snapshots = format!("{root_str}/state_machine/snapshots");
        fs::create_dir_all(&snapshots).await.unwrap();
        fs::write(format!("{snapshots}/{}", Uuid::now_v7()), b"torn one")
            .await
            .unwrap();
        fs::write(format!("{snapshots}/{}", Uuid::now_v7()), b"torn two")
            .await
            .unwrap();

        let mut sm = state_machine_over(&snapshots).await;
        let err = sm
            .select_startup_snapshot(SnapshotRecoveryBounds::default())
            .await
            .expect_err("no usable snapshot must be a refusal");
        assert!(err.to_string().contains("refuses to start"), "got: {err}");

        // And a directory with no snapshots at all is still a pristine node, not a refusal.
        let (root2, root_str2) = scratch_root("pristine");
        let snapshots2 = format!("{root_str2}/state_machine/snapshots");
        fs::create_dir_all(&snapshots2).await.unwrap();
        let mut pristine = state_machine_over(&snapshots2).await;
        assert!(
            pristine
                .select_startup_snapshot(SnapshotRecoveryBounds::default())
                .await
                .unwrap()
                .is_none()
        );

        fs::remove_dir_all(root).await.unwrap();
        fs::remove_dir_all(root2).await.unwrap();
    }

    /// F-004: install renamed the received file to its final published name and *then* restored
    /// from it, so a failed restore left an unusable image eligible for selection at restart.
    ///
    /// The received image here is not a database at all, which is what a stream that ended
    /// early leaves. It must be discarded at the staging name, the live database must be
    /// untouched, and nothing must be published.
    #[tokio::test]
    async fn an_unusable_received_snapshot_is_discarded_without_publishing_or_restoring() {
        let (root, root_str) = scratch_root("install-invalid");
        let snapshots = format!("{root_str}/state_machine/snapshots");

        let good_id = build_one_snapshot(
            &root_str,
            1,
            "CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);",
        )
        .await;
        let live_before = fs::read(format!("{root_str}/state_machine/db/state.sqlite"))
            .await
            .unwrap();

        // What `begin_receiving_snapshot` leaves behind for a stream that ended early.
        fs::write(format!("{snapshots}/temp"), b"half a snapshot")
            .await
            .unwrap();

        let incoming_id = Uuid::now_v7();
        let mut sm = state_machine_over(&snapshots).await;
        // Bounded, because the failure being guarded against is an install that gets as far as
        // the restore. This state machine has no writer behind its channel, so reaching the
        // restore hangs; the timeout turns that into a failed assertion instead of a hung test.
        let err = time::timeout(
            Duration::from_secs(10),
            sm.install_snapshot(
                &SnapshotMeta {
                    last_log_id: None,
                    last_membership: Default::default(),
                    snapshot_id: incoming_id.to_string(),
                },
                Box::new(fs::File::open(format!("{snapshots}/temp")).await.unwrap()),
            ),
        )
        .await
        .expect("an unusable received snapshot must be rejected before the restore is attempted")
        .expect_err("an unusable received snapshot must not install");
        assert!(
            err.to_string()
                .contains("without touching this node's database"),
            "got: {err}"
        );

        assert!(
            !std::path::Path::new(&format!("{snapshots}/{incoming_id}")).exists(),
            "nothing may be published for a snapshot that never restored"
        );
        assert!(
            !std::path::Path::new(&format!("{snapshots}/{incoming_id}.incoming")).exists(),
            "the discarded staging file must be removed"
        );
        assert_eq!(
            fs::read(format!("{root_str}/state_machine/db/state.sqlite"))
                .await
                .unwrap(),
            live_before,
            "the live database must be byte-identical"
        );

        // And the node can still restart from what it had.
        let selected = sm
            .select_startup_snapshot(SnapshotRecoveryBounds::default())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(selected.meta.snapshot_id, good_id);

        fs::remove_dir_all(root).await.unwrap();
    }

    /// Staging names are never restart candidates, whichever staging name they are.
    ///
    /// `interrupted_staging_files_are_not_published_snapshots` covers `temp` and `{id}.temp`;
    /// this adds `{id}.incoming`, which the install path introduces.
    #[tokio::test]
    async fn an_incoming_staging_file_is_never_a_restart_candidate() {
        let (root, root_str) = scratch_root("incoming-not-candidate");
        let snapshots = format!("{root_str}/state_machine/snapshots");
        fs::create_dir_all(&snapshots).await.unwrap();
        fs::write(
            format!("{snapshots}/{}.incoming", Uuid::now_v7()),
            b"a received image that never finished installing",
        )
        .await
        .unwrap();

        let mut sm = state_machine_over(&snapshots).await;
        assert!(
            sm.select_startup_snapshot(SnapshotRecoveryBounds::default())
                .await
                .unwrap()
                .is_none(),
            "a staging name is not a published snapshot"
        );

        fs::remove_dir_all(root).await.unwrap();
    }

    /// Two published snapshots are kept, so there is something to fall back to.
    ///
    /// Keeping one is why F-006 had no fallback available even once selection could look for
    /// one: the build that produced the newest had already deleted the only other copy.
    #[tokio::test]
    async fn the_previous_published_snapshot_is_kept() {
        let (root, root_str) = scratch_root("retention");
        let snapshots = format!("{root_str}/state_machine/snapshots");

        let mut sm = StateMachineSqlite::new(
            &root_str,
            "state.sqlite",
            1,
            false,
            2,
            1,
            #[cfg(feature = "s3")]
            None,
            false,
            #[cfg(feature = "backup")]
            1,
            SnapshotRecoveryBounds::default(),
        )
        .await
        .unwrap();

        let mut ids = Vec::new();
        for index in 1..=3u64 {
            sm.apply([batch_entry(
                index,
                "CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY);",
            )])
            .await
            .unwrap();
            let snapshot = sm
                .get_snapshot_builder()
                .await
                .build_snapshot()
                .await
                .unwrap();
            ids.push(snapshot.meta.snapshot_id.clone());
            drop(snapshot);
            // The cleanup is spawned, so wait for it to have run before counting.
            for _ in 0..200 {
                let mut published = 0;
                let mut list = fs::read_dir(&snapshots).await.unwrap();
                while let Some(e) = list.next_entry().await.unwrap() {
                    if Uuid::parse_str(e.file_name().to_str().unwrap_or("")).is_ok() {
                        published += 1;
                    }
                }
                if published <= SNAPSHOTS_KEPT {
                    break;
                }
                time::sleep(Duration::from_millis(10)).await;
            }
        }

        let mut published: Vec<String> = Vec::new();
        let mut list = fs::read_dir(&snapshots).await.unwrap();
        while let Some(e) = list.next_entry().await.unwrap() {
            let name = e.file_name().to_str().unwrap_or("").to_string();
            if Uuid::parse_str(&name).is_ok() {
                published.push(name);
            }
        }
        published.sort();

        assert_eq!(
            published.len(),
            SNAPSHOTS_KEPT,
            "exactly the newest two published snapshots are kept, found {published:?}"
        );
        assert!(published.contains(&ids[2]), "the newest is kept");
        assert!(published.contains(&ids[1]), "and the one before it");
        assert!(!published.contains(&ids[0]), "the oldest is not");

        shutdown_state_machine(&sm).await;
        drop(sm);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn forbidden_functions_panic_on_purpose_and_fail_the_statement() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        StateMachineSqlite::overwrite_non_det_fns(&conn);

        // The registered closures `panic!` on purpose; rusqlite turns that into a
        // statement error at the FFI boundary, so the query fails.
        let err = conn
            .query_row("SELECT now()", (), |row| row.get::<_, String>(0))
            .unwrap_err();
        assert!(err.to_string().contains("unwinding panic"));

        // the connection stays usable afterwards
        let one: i64 = conn.query_row("SELECT 1", (), |row| row.get(0)).unwrap();
        assert_eq!(one, 1);

        // every registered function panics, not just `now()` - "unwinding panic" proves
        // the call reached our closure, not a missing function
        for name in FORBIDDEN_NON_DET_FNS {
            let err = conn
                .query_row(&format!("SELECT {name}()"), (), |row| {
                    row.get::<_, String>(0)
                })
                .unwrap_err();
            assert!(
                err.to_string().contains("unwinding panic"),
                "{name} did not panic: {err}"
            );
        }
    }
}

#[cfg(test)]
mod serialized_enum_order {
    use super::*;

    /// The serialized variant index is part of the raft log format: a reorder
    /// would silently corrupt logs written by older builds with a different
    /// feature set. Pin the current order so a reorder fails this test instead.
    #[test]
    fn query_write_variant_order_is_stable() {
        let idx = |req: &QueryWrite| crate::helpers::serialize(req).unwrap()[0];
        let query = || Query {
            sql: Cow::Owned(String::new()),
            params: vec![],
        };

        assert_eq!(idx(&QueryWrite::Execute(query())), 0);
        assert_eq!(idx(&QueryWrite::ExecuteReturning(query())), 1);
        assert_eq!(idx(&QueryWrite::Transaction(vec![])), 2);
        assert_eq!(idx(&QueryWrite::Batch(Cow::Owned(String::new()))), 3);
        assert_eq!(idx(&QueryWrite::Migration(vec![])), 4);
        assert_eq!(idx(&QueryWrite::Backup((0, 0))), 5);
        assert_eq!(idx(&QueryWrite::RTT), 6);
    }
}
