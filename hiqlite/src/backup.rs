use crate::app_state::AppState;
use crate::helpers::{deserialize, set_path_access};
use crate::store::logs;
use crate::store::state_machine::sqlite::state_machine::{
    PathBackups, PathDb, PathLockFile, PathSnapshots, QueryWrite, StateMachineData,
    StateMachineSqlite,
};
use crate::{Client, Error, NodeConfig};
use chrono::{DateTime, Utc};
use std::env;
use std::ops::Sub;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tokio::{fs, task, time};
use tracing::{debug, error, info, warn};

#[cfg(feature = "s3")]
use crate::s3::S3Config;

pub const BACKUP_DB_NAME: &str = "restore.sqlite";

#[derive(Debug, Clone)]
pub struct BackupConfig {
    cron_schedule: cron::Schedule,
    keep_days: u16,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            cron_schedule: cron::Schedule::from_str("0 30 2 * * * *").unwrap(),
            keep_days: 30,
        }
    }
}

impl BackupConfig {
    pub fn new(cron_schedule: &str, keep_days: u16) -> Result<Self, Error> {
        Ok(Self {
            cron_schedule: cron::Schedule::from_str(cron_schedule)
                .map_err(|_| Error::Config("Invalid syntax for cron_schedule".into()))?,
            keep_days,
        })
    }

    pub fn from_env() -> Self {
        let cron_str = env::var("HQL_BACKUP_CRON").unwrap_or_else(|_| "0 30 2 * * * *".to_string());
        let cron_schedule =
            cron::Schedule::from_str(&cron_str).expect("Invalid syntax for HQL_BACKUP_CRON");

        let keep_days = env::var("HQL_BACKUP_KEEP_DAYS")
            .unwrap_or_else(|_| "30".to_string())
            .parse::<u16>()
            .expect("Cannot parse HQL_BACKUP_KEEP_DAYS to u16");

        Self {
            cron_schedule,
            keep_days,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum BackupSource {
    S3(String),
    File(String),
}

impl BackupSource {
    fn from_env() -> Option<Self> {
        let var = env::var("HQL_BACKUP_RESTORE").ok()?;

        if let Some(obj) = var.strip_prefix("s3:") {
            return Some(Self::S3(obj.to_string()));
        }

        if let Some(file) = var.strip_prefix("file:") {
            return Some(Self::File(file.to_string()));
        }

        error!(
            "HQL_BACKUP_RESTORE must start with either 's3:' or 'file:'. \
            Cannot restore from backup - unknown prefix: {}",
            var
        );
        None
    }
}

pub fn start_cron(
    client: Client,
    backup_config: BackupConfig,
    #[cfg(feature = "s3")] s3_config: Option<Arc<S3Config>>,
) {
    task::spawn(Box::pin(async move {
        info!("Backup cron task started");

        loop {
            let dur = {
                let now = chrono::Local::now();
                let Some(next) = backup_config.cron_schedule.upcoming(chrono::Local).next() else {
                    // e.g. a cron with an impossible date like Feb 30: no upcoming event.
                    // Keep the task alive instead of panicking and silently losing backups.
                    warn!("Cron schedule has no upcoming event - retrying in 1 hour");
                    time::sleep(Duration::from_secs(3600)).await;
                    continue;
                };
                if next <= now {
                    // don't set it to 0 to not go crazy in case of a bad config or timing
                    Duration::from_secs(1)
                } else {
                    Duration::from_secs((next.timestamp() - now.timestamp()) as u64)
                }
            };
            time::sleep(dur).await;

            info!("Executing backup now");
            let mut success = false;
            // The bound is on attempts, and the message below now reports how many were
            // actually made. It used to print this constant whatever happened, so a single
            // failed attempt was reported as five.
            let max_attempts = 5;
            let mut attempts = 0;
            let mut last_err: Option<String> = None;

            for _ in 0..max_attempts {
                attempts += 1;
                match backup_cron_job(
                    &client,
                    backup_config.keep_days,
                    #[cfg(feature = "s3")]
                    &s3_config,
                )
                .await
                {
                    Ok(_) => {
                        info!("Backup task finished successfully");
                        success = true;
                        break;
                    }
                    Err(err) => {
                        last_err = Some(err.to_string());
                        if err.is_forward_to_leader().is_some() {
                            debug!(
                                "Raft currently has no leader - retrying in 10 seconds\n{:?}",
                                err
                            );
                            time::sleep(Duration::from_secs(10)).await;
                        } else {
                            error!("Error during backup task execution: {}", err);
                            break;
                        }
                    }
                }
            }

            if !success {
                warn!(
                    "Backup task failed after {attempts} attempt(s) of at most {max_attempts}: {}",
                    last_err.as_deref().unwrap_or("no error was recorded")
                );
            }
        }
    }));
}

async fn backup_cron_job(
    client: &Client,
    keep_days: u16,
    #[cfg(feature = "s3")] s3_config: &Option<Arc<S3Config>>,
) -> Result<(), Error> {
    client.backup().await?;

    #[cfg(feature = "s3")]
    {
        if let Some(s3_config) = s3_config {
            // the backup task will be async in the background, but we can start cleaning up already
            let threshold = Utc::now().sub(chrono::Duration::days(keep_days as i64));

            let list = s3_config.bucket.list("", None).await?;
            let mut backups = Vec::new();
            for bucket in list.iter() {
                if bucket.name != s3_config.bucket.name {
                    info!("Found non-configured bucket {} - skipping", bucket.name);
                    continue;
                }
                for object in bucket.contents.iter() {
                    // The same floor the local sweep applies: nothing that claims to predate
                    // hiqlite is treated as a backup (found by the AI review of `b5039d2`).
                    if let Some(dt) = dt_from_backup_name(&object.key)
                        && dt.timestamp() > TS_MIN
                    {
                        backups.push((object.key.as_str(), dt));
                    }
                }
            }
            // The upload of the backup just taken runs in the background and may not have
            // landed, or may never land. The newest remote copy is therefore never deleted,
            // so uploads that keep failing cannot age every remote copy out.
            for key in expired_backups(&backups, threshold) {
                info!("Deleting expired backup: {}", key);
                s3_config.bucket.delete(key.to_string()).await?;
            }
        }
    }

    Ok(())
}

/// The retention floor: no backup can legitimately predate hiqlite's first release.
///
/// `1704063600` was annotated `2024/01/01 00:00:00` and is that midnight in CET, an hour
/// earlier than the UTC one, while every value it is compared against comes from `Utc::now()`.
/// Nothing observable followed from the hour, and the constant guards a deletion, so it is
/// stated in the zone it is compared in.
const TS_MIN: i64 = 1_704_067_200; // 2024-01-01T00:00:00Z

pub(crate) async fn backup_local_cleanup(backup_path: String, keep_days: u16) -> Result<(), Error> {
    let ts_min = TS_MIN;

    let ts_threshold = Utc::now()
        .sub(chrono::Duration::days(keep_days as i64))
        .timestamp();

    let path = Path::new(&backup_path);
    let mut dir_entries = tokio::fs::read_dir(path).await?;
    let mut names = Vec::new();

    loop {
        let entry = match dir_entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => {
                warn!("Error reading directory entries: {err:?}");
                break;
            }
        };
        if entry.metadata().await?.is_dir() {
            continue;
        }
        if let Some(s) = entry.file_name().to_str() {
            names.push(s.to_string());
        }
    }

    // One predicate for what a backup file is, shared with the S3 sweep and with
    // `Client::backup_list_local`. The guard here used to be `!starts_with(..) &&
    // !ends_with(..)`, which skips a file only when it matches **neither** half where the
    // intent is to skip unless it matches both, so any `.sqlite` file whose trailing token
    // parses as a plausible timestamp reached the deletion branch.
    let backups = names
        .iter()
        .filter_map(|s| dt_from_backup_name(s).map(|dt| (s.as_str(), dt)))
        .filter(|(_, dt)| dt.timestamp() > ts_min)
        .collect::<Vec<_>>();
    let threshold = DateTime::from_timestamp(ts_threshold, 0).unwrap_or_default();

    // Never the newest: with a short `keep_days` the sweep runs right after the backup it
    // follows, and that backup may still be the source of an S3 upload.
    for s in expired_backups(&backups, threshold) {
        let p = format!("{backup_path}/{s}");
        info!("Cleaning up local backup {s} ({p})");
        if let Err(err) = tokio::fs::remove_file(p).await {
            error!(?err, "Error removing local backup");
        }
    }

    Ok(())
}

/// The single definition of what a hiqlite backup file is called.
///
/// `backup_node_{node_id}_{unix_seconds}.sqlite`. Every caller that has to decide "is this a
/// backup" goes through this, so a name that does not parse is never a deletion candidate, on
/// disk or in a bucket.
/// Retention: the backups older than `threshold`, never including the newest one.
///
/// A floor of one copy means a backup that keeps failing, locally or on its way to S3, can never
/// leave nothing behind, whatever `keep_days` says.
fn expired_backups<'a>(
    backups: &[(&'a str, DateTime<Utc>)],
    threshold: DateTime<Utc>,
) -> Vec<&'a str> {
    let newest = backups
        .iter()
        .max_by_key(|(name, dt)| (*dt, *name))
        .map(|(name, _)| *name);
    backups
        .iter()
        .filter(|(name, dt)| *dt < threshold && Some(*name) != newest)
        .map(|(name, _)| *name)
        .collect()
}

pub(crate) fn dt_from_backup_name(name: &str) -> Option<DateTime<Utc>> {
    let backup = name.strip_prefix("backup_node_")?;
    // `{node_id}_{ts}.sqlite`
    let (node_id, rest) = backup.split_once('_')?;
    if node_id.is_empty() || node_id.parse::<u64>().is_err() {
        debug!("Not a backup filename, node id does not parse: {name}");
        return None;
    }
    let ts = rest.strip_suffix(".sqlite")?;
    match ts.parse::<i64>() {
        Ok(ts) => DateTime::from_timestamp(ts, 0),
        Err(_) => {
            debug!("Not a backup filename, timestamp does not parse: {name}");
            None
        }
    }
}

/// The prefix of a quarantine directory left by a follower preparing for a restore join.
#[cfg(feature = "sqlite")]
pub(crate) const PRE_RESTORE_DIR_PREFIX: &str = "pre-restore-";

/// Move everything in the data directory aside, keeping the storage owner lock in place.
///
/// Nothing is deleted. The entries land in `{data_dir}/pre-restore-{unix_seconds}/`, which no
/// hiqlite path reads, so the node starts as though the directory were empty while an operator
/// can still get the previous state back if the restore this was preparing for never happens.
///
/// Failures are returned rather than discarded: a node that could not move its own directory
/// aside must not proceed into a cluster join with a half-moved one.
#[cfg(feature = "sqlite")]
async fn quarantine_data_dir_contents(data_dir: &str) -> Result<(), Error> {
    let quarantine = format!(
        "{data_dir}/{PRE_RESTORE_DIR_PREFIX}{}",
        Utc::now().timestamp()
    );
    let mut created = false;
    let mut entries = match fs::read_dir(data_dir).await {
        Ok(entries) => entries,
        // Nothing to clean up.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(Error::Error(
                format!("cannot read the data directory {data_dir}: {err}").into(),
            ));
        }
    };

    while let Some(entry) = entries.next_entry().await.map_err(|err| {
        Error::Error(format!("cannot list the data directory {data_dir}: {err}").into())
    })? {
        let path = entry.path();
        if crate::storage_lock::StorageOwnership::is_owner_lock_file(&path) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy().to_string();
        // An earlier quarantine is left where it is rather than nested inside a new one.
        if name.starts_with(PRE_RESTORE_DIR_PREFIX) {
            continue;
        }

        if !created {
            fs::create_dir_all(&quarantine)
                .await
                .map_err(|err| Error::Error(format!("cannot create {quarantine}: {err}").into()))?;
            created = true;
        }

        fs::rename(&path, format!("{quarantine}/{name}"))
            .await
            .map_err(|err| {
                Error::Error(
                    format!(
                        "cannot move {} aside while preparing a restore: {err}",
                        path.display()
                    )
                    .into(),
                )
            })?;
    }

    if created {
        warn!(
            "Previous state moved to {quarantine}. It is not read by anything and is not \
             cleaned up automatically; remove it once this node has rejoined successfully."
        );
    }

    Ok(())
}

/// Check if the env var `HQL_BACKUP_RESTORE` is set and restores the given backup if so.
/// Returns `Ok(true)` if backup has been applied.
/// This will only run if the current node ID is `1`.
pub(crate) async fn restore_backup_start(node_config: &NodeConfig) -> Result<bool, Error> {
    // A restore that was interrupted after its image was staged is finished before anything
    // else, whether or not the environment still asks for one: the previous database may
    // already be missing its write-ahead log, and starting on it is not an option.
    if finish_staged_restore(node_config).await? {
        warn!("Completed a database restore that had been interrupted");
        return Ok(true);
    }

    if let Some(src) = BackupSource::from_env() {
        info!("Found backup restore request {:?}", src);

        if node_config.node_id == 1 {
            restore_backup(node_config, src).await?;
            return Ok(true);
        } else {
            warn!("Moving existing files aside and starting the restore cluster join");
            // Two reasons this is not `remove_dir_all(data_dir)`.
            //
            // The storage owner lock lives at the data directory root, and unlinking it while
            // this process holds it would leave the holder locking an inode nobody can reach,
            // so the next process would create a different file and both would believe they
            // owned the storage.
            //
            // And this branch runs on an environment variable alone, before node 1 has pulled,
            // validated or copied anything. A restore that then fails on node 1, because the
            // `file:` path does not exist or the object is not in the bucket, used to have
            // already destroyed every other node's state. Moving it aside instead keeps that
            // recoverable; the two are still not coordinated, which is why `N > 1` restore is
            // not a supported topology for this release.
            quarantine_data_dir_contents(node_config.data_dir.as_ref()).await?;
        }
    }

    Ok(false)
}

/// Apply the given backup from S3 storage.
///
/// **CAUTION: This function MUST BE CALLED when the Raft is not running!**
///
/// You only need to invoke this manually if you want to apply a backup in another
/// way than with the `HQL_BACKUP_RESTORE` env var, which is being done automatically.
pub async fn restore_backup(node_config: &NodeConfig, src: BackupSource) -> Result<(), Error> {
    info!("Starting database restore from backup {:?}", src);

    if let BackupSource::S3(_) = &src
        && node_config.s3_config.is_none()
    {
        return Err(Error::S3(
            "No `S3Config` given, cannot restore backup".to_string(),
        ));
    }

    let (PathDb(path_db), PathBackups(path_backups), _, _) =
        StateMachineSqlite::build_folders(&node_config.data_dir, false).await;

    fs::create_dir_all(&path_backups).await?;
    set_path_access(&path_backups, 0o700).await?;

    let (path_backup, remove_src) = match src {
        BackupSource::S3(s3_obj) => {
            let s3_config = match &node_config.s3_config {
                None => {
                    return Err(Error::S3(
                        "No `S3Config` given, cannot restore backup".to_string(),
                    ));
                }
                Some(c) => c,
            };
            // Prove the credentials before pulling. A wrong key used to surface as a failed
            // transfer partway through a restore that had already been announced.
            s3_config.verify_access().await?;

            let path_backup = format!("{path_backups}/{BACKUP_DB_NAME}");
            s3_config.pull(&s3_obj, &path_backup).await?;
            (path_backup, true)
        }
        BackupSource::File(path_src) => {
            let (path, filename) = path_src.rsplit_once('/').unwrap_or(("", &path_src));
            debug!("Given backup path full: '{path_src}', after parsing: '{path}' / '{filename}'");
            let path_backup = format!("{path_backups}/{filename}");

            fs::copy(path_src, &path_backup).await?;
            (path_backup, false)
        }
    };

    is_metadata_ok(path_backup.clone()).await?;
    debug!("Database backup metadata is ok");

    // Stage first, destroy last.
    //
    // This used to remove the database, the snapshots, the lock marker and the logs and *then*
    // copy the backup into place, so a failure of the copy, of the `create_dir_all`, or of the
    // access-rights call between them left the node with neither its previous state nor the
    // backup, and there was no rollback. The copy now lands beside the database under a
    // staging name, is synced, and is renamed into place; nothing is removed until it is
    // there.
    fs::create_dir_all(&path_db).await?;
    set_path_access(&path_db, 0o700).await?;

    // A restore committed earlier and interrupted is finished before this one replaces its
    // staged image. Deleting that image first would leave the node on a database that may
    // already have lost its write-ahead log if this restore then failed. Found in review.
    if finish_staged_restore(node_config).await? {
        warn!("Completed an interrupted earlier restore before starting this one");
    }
    let (_, path_db_staged) = restore_paths(node_config, &path_db);
    let path_db_staging = format!("{path_db_staged}.tmp");
    let _ = fs::remove_file(&path_db_staging).await;
    let _ = fs::remove_file(&path_db_staged).await;

    info!(
        "Given backup check ok - staging it next to the database: {} -> {}",
        path_backup, path_db_staged
    );
    fs::copy(&path_backup, &path_db_staging).await?;
    set_path_access(&path_db_staging, 0o700).await?;
    // The copied image is what the node will start on, so it is made durable before anything
    // is removed. It is then renamed to the staged name, and that rename is the commit point
    // of the restore: from here a crash is rolled forward on the next start, by
    // `finish_staged_restore`, and never rolled back onto a database that may already have lost
    // its write-ahead log.
    sync_file(&path_db_staging).await?;
    fs::rename(&path_db_staging, &path_db_staged).await?;
    sync_parent_dir(&path_db).await?;

    if !finish_staged_restore(node_config).await? {
        return Err(Error::Error(
            format!("the staged restore image {path_db_staged} disappeared before it was applied")
                .into(),
        ));
    }
    if remove_src {
        info!("Cleaning up S3 backup from {}", path_backup);
        fs::remove_file(path_backup).await?;
    }

    Ok(())
}

/// The database's final path and the staged restore image's path beside it.
fn restore_paths(node_config: &NodeConfig, path_db: &str) -> (String, String) {
    let path_db_full = format!("{}/{}", path_db, node_config.filename_db);
    let path_db_staged = format!("{path_db_full}.restoring");
    (path_db_full, path_db_staged)
}

/// Apply a staged restore image, if there is one: the destructive half of a restore.
///
/// Idempotent, so it is both the second half of `restore_backup` and the roll-forward of one
/// that was interrupted. `Ok(false)` means nothing was staged. A leftover `.restoring.tmp` is an
/// image whose staging never completed, before anything was removed, and is discarded.
async fn finish_staged_restore(node_config: &NodeConfig) -> Result<bool, Error> {
    let (PathDb(path_db), _, PathSnapshots(path_snapshots), PathLockFile(path_lock_file)) =
        StateMachineSqlite::build_folders(&node_config.data_dir, false).await;
    let path_logs = logs::logs_dir_db(&node_config.data_dir);
    let (path_db_full, path_db_staged) = restore_paths(node_config, &path_db);

    let _ = fs::remove_file(format!("{path_db_staged}.tmp")).await;
    if !fs::try_exists(&path_db_staged).await.unwrap_or(false) {
        return Ok(false);
    }

    debug!("Removing old data");
    // The database is published last, and by rename rather than by deletion, so a crash
    // anywhere in here leaves the staged image in place and this function runs again.
    remove_dir_all_reported(&path_snapshots).await?;
    remove_file_reported(&path_lock_file).await?;
    remove_dir_all_reported(&path_logs).await?;

    // F-100: the previous database's write-ahead log and shared-memory file are removed too.
    // Replacing `hiqlite.db` alone leaves `hiqlite.db-wal` beside it, and that WAL belongs to
    // the database that was just discarded: it carries the pre-restore `_metadata`, including
    // the membership. A node restored that way came up believing it was already a member of
    // the cluster it was supposed to rejoin, so it never initialized itself, could not reach a
    // quorum on its own, and never served the endpoint its peers needed in order to rejoin it.
    //
    // They are removed before the rename, so the window in which the final name holds a
    // database with a foreign WAL is never entered.
    for sidecar in [format!("{path_db_full}-wal"), format!("{path_db_full}-shm")] {
        if fs::try_exists(&sidecar).await.unwrap_or(false) {
            remove_file_reported(&sidecar).await?;
        }
    }

    // The removals above are directory entries in other directories. Made durable before the
    // rename publishes the restored database: otherwise a power loss could keep the rename and
    // bring the old `logs/` back beside a database whose metadata says nothing was applied, and
    // the node would replay the discarded cluster's log onto the backup. Found in review.
    let mut parents = [&path_snapshots, &path_lock_file, &path_logs, &path_db_full]
        .iter()
        .filter_map(|p| Path::new(p.as_str()).parent())
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    parents.sort();
    parents.dedup();
    for parent in &parents {
        if fs::try_exists(parent).await.unwrap_or(false) {
            sync_parent_dir(parent).await?;
        }
    }

    fs::rename(&path_db_staged, &path_db_full).await?;
    sync_parent_dir(&path_db).await?;
    Ok(true)
}

/// Establish that a candidate backup is a hiqlite database this node can start on.
///
/// Three checks, where there used to be one. SQLite's own `quick_check` catches a truncated or
/// structurally broken image, which a validation that only read one row did not; the schema
/// check confirms the table the state machine needs is there; and the metadata blob is
/// **returned as an error** when it does not decode rather than `unwrap`ed, which used to panic
/// the blocking task so the caller saw a join failure instead of "this backup is not valid".
async fn is_metadata_ok(path_db: String) -> Result<(), Error> {
    // Parsed case-insensitively, and loudly. The old comparison was against the exact string
    // `"true"`, so `TRUE` silently validated; that direction is fail-closed, but an operator
    // who meant to skip validation and did not is worth a warning either way.
    if env::var("HQL_BACKUP_SKIP_VALIDATION")
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        warn!(
            "HQL_BACKUP_SKIP_VALIDATION is set: restoring {path_db} without checking that it is \
             a valid hiqlite database"
        );
        return Ok(());
    }

    let path_dbg = path_db.clone();
    task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open(&path_db).map_err(|err| {
            Error::Sqlite(format!("backup '{path_db}' cannot be opened: {err}").into())
        })?;

        let check: String = conn
            .query_row("PRAGMA quick_check(1)", (), |row| row.get(0))
            .map_err(|err| {
                Error::Sqlite(
                    format!("backup '{path_db}' failed its integrity check: {err}").into(),
                )
            })?;
        if check != "ok" {
            return Err(Error::Sqlite(
                format!("backup '{path_db}' failed its integrity check: {check}").into(),
            ));
        }

        let mut stmt = conn
            .prepare_cached("SELECT data FROM _metadata WHERE key = 'meta'")
            .map_err(|err| {
                Error::Sqlite(
                    format!("backup '{path_db}' has no readable _metadata table: {err}").into(),
                )
            })?;
        let bytes: Vec<u8> = stmt.query_row((), |row| row.get(0)).map_err(|err| {
            Error::Sqlite(format!("backup '{path_db}' has no metadata row: {err}").into())
        })?;
        let _meta: StateMachineData = deserialize(&bytes).map_err(|err| {
            Error::Sqlite(
                format!("backup '{path_db}' has metadata that is not a StateMachineData: {err}")
                    .into(),
            )
        })?;

        Ok::<(), Error>(())
    })
    .await
    .map_err(|err| {
        Error::Error(format!("backup validation task for '{path_dbg}' failed: {err}").into())
    })??;
    Ok(())
}

/// `remove_dir_all` whose failure is reported rather than discarded.
///
/// A restore that could not remove the old snapshots or logs has left the node with a new
/// database and stale state beside it, which is not a state to start on.
async fn remove_dir_all_reported(path: &str) -> Result<(), Error> {
    match fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::Error(
            format!("cannot remove {path} during a restore: {err}").into(),
        )),
    }
}

async fn remove_file_reported(path: &str) -> Result<(), Error> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::Error(
            format!("cannot remove {path} during a restore: {err}").into(),
        )),
    }
}

async fn sync_file(path: &str) -> Result<(), Error> {
    let path = path.to_string();
    task::spawn_blocking(move || {
        std::fs::File::open(&path)
            .and_then(|f| f.sync_all())
            .map_err(|err| Error::Error(format!("cannot sync {path}: {err}").into()))
    })
    .await?
}

async fn sync_parent_dir(dir: &str) -> Result<(), Error> {
    #[cfg(unix)]
    {
        let dir = dir.to_string();
        task::spawn_blocking(move || {
            std::fs::File::open(&dir)
                .and_then(|d| d.sync_all())
                .map_err(|err| {
                    Error::Error(format!("cannot sync the directory {dir}: {err}").into())
                })
        })
        .await?
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

// pub fn restore_backup_finish(state: Arc<AppState>, nodes_count: usize) {
//     task::spawn(restore_backup_cleanup_task(state, nodes_count));
// }

#[tracing::instrument(level = "debug", skip_all)]
#[cfg(feature = "backup")]
pub async fn restore_backup_finish(state: &Arc<AppState>) {
    loop {
        match state.raft_db.raft.is_initialized().await {
            Ok(res) => {
                if res {
                    break;
                }
            }
            Err(err) => {
                error!("{}", err);
            }
        }
        debug!("Waiting for Raft init");
        time::sleep(Duration::from_millis(50)).await;
    }

    while state.raft_db.raft.current_leader().await.is_none() {
        time::sleep(Duration::from_millis(50)).await;
    }

    debug_assert!(
        state.raft_db.raft.current_leader().await == Some(state.id),
        "It should never happen that node 1 is not the raft leader during backup restore"
    );

    let reqs = 10;
    for _ in 0..reqs {
        let start = Instant::now();
        match state.raft_db.raft.client_write(QueryWrite::RTT).await {
            Ok(_) => {
                info!("Raft RTT: {} micros", start.elapsed().as_micros());
            }
            Err(err) => {
                error!("Raft RTT request error: {}", err);
            }
        }
    }

    let last_log;
    loop {
        let metrics = state.raft_db.raft.metrics().borrow().clone();
        if let Some(last_applied) = metrics.last_applied
            && last_applied.index >= reqs
        {
            debug!("Found high enough last_applied log id");
            last_log = last_applied.index;
            break;
        }
        time::sleep(Duration::from_millis(50)).await;
    }

    debug!("Taking snapshot now");
    if let Err(err) = state.raft_db.raft.trigger().snapshot().await {
        // e.g. the raft is shutting down: do not panic the task, just bail out - the
        // wait-for-snapshot loop below would never finish anyway
        error!("Error triggering snapshot: {err}");
        return;
    }

    // while let Err(_err) = state.raft_db.raft.trigger().snapshot().await {
    //     debug_assert!("")
    //     time::sleep(Duration::from_millis(500)).await;
    // }

    // wait until snapshot has been built
    while state.raft_db.raft.metrics().borrow().snapshot.is_none() {
        info!("Waiting for snapshot build to finish");
        time::sleep(Duration::from_millis(100)).await;
    }

    debug!("Purging logs");
    // Bounded, and it bails out for the same reason the snapshot trigger twelve lines above
    // does: a purge that keeps failing means the raft is going away, and the old
    // `while let Err(..)` had no sleep, no attempt bound and no exit, so it spun the task at
    // full CPU emitting an error line per iteration.
    for attempt in 1..=10u32 {
        match state.raft_db.raft.trigger().purge_log(last_log).await {
            Ok(()) => break,
            Err(err) => {
                error!("Error during logs purge (attempt {attempt} of 10): {err}");
                if attempt == 10 {
                    error!(
                        "Giving up on the post-restore log purge; the logs stay until the next one"
                    );
                    return;
                }
                time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    info!("restore_backup_finish task successful");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Characterizes `BackupConfig::new`: the cron string is the only validated
    /// input, and `keep_days` is accepted as given.
    #[test]
    fn backup_config_validates_only_the_cron_expression() {
        let ok = BackupConfig::new("0 30 2 * * * *", 30).unwrap();
        assert_eq!(ok.keep_days, 30);

        assert!(BackupConfig::new("not a cron", 30).is_err());

        // keep_days is never range-checked, not even against zero
        assert_eq!(BackupConfig::new("0 30 2 * * * *", 0).unwrap().keep_days, 0);
    }

    /// Characterizes the S3 retention filter. Every rejection is silent and
    /// returns `None`; only the exact documented shape is recognised.
    #[test]
    fn the_remote_retention_filter_accepts_only_the_documented_name_shape() {
        assert_eq!(
            dt_from_backup_name("backup_node_1_1704067200.sqlite"),
            DateTime::from_timestamp(1704067200, 0)
        );

        // wrong prefix
        assert!(dt_from_backup_name("node_1_1704067200.sqlite").is_none());
        // no second underscore after the prefix
        assert!(dt_from_backup_name("backup_node_1.sqlite").is_none());
        // missing suffix
        assert!(dt_from_backup_name("backup_node_1_1704067200").is_none());
        // unparsable timestamp
        assert!(dt_from_backup_name("backup_node_1_never.sqlite").is_none());
    }

    /// Replaces `local_cleanup_deletes_files_that_are_not_backups`, which pinned F-056 as
    /// expected behavior: the guard was `!prefix && !suffix`, which skips a file only when it
    /// matches **neither** half where the intent is to skip unless it matches both, so a
    /// `.sqlite` file with a parsable trailing token was deleted by the retention sweep.
    ///
    /// The sweep now uses the same predicate as the S3 filter and as
    /// `Client::backup_list_local`: `dt_from_backup_name`. Three predicates over one naming
    /// convention became one.
    #[tokio::test]
    async fn local_cleanup_only_deletes_files_that_are_actually_backups() {
        let dir = std::env::temp_dir().join(format!(
            "hiqlite-backup-cleanup-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&dir).await.unwrap();
        let base = dir.to_str().unwrap().to_string();

        // 2024-01-02, older than the retention window below
        let old_ts = 1704153600i64;
        let recent_ts = Utc::now().timestamp();

        let expired_backup = format!("backup_node_1_{old_ts}.sqlite");
        let fresh_backup = format!("backup_node_1_{recent_ts}.sqlite");
        // The F-056 shape: a `.sqlite` file whose trailing token parses as a timestamp.
        let lookalike_suffix = format!("someone_elses_{old_ts}.sqlite");
        // The other half of the same defect: the prefix without the suffix.
        let lookalike_prefix = format!("backup_node_1_{old_ts}.sqlite.bak");
        // A node id that is not a number is not this naming convention either.
        let lookalike_node = format!("backup_node_x_{old_ts}.sqlite");
        let untouched = "notes.txt".to_string();

        for name in [
            &expired_backup,
            &fresh_backup,
            &lookalike_suffix,
            &lookalike_prefix,
            &lookalike_node,
            &untouched,
        ] {
            fs::write(format!("{base}/{name}"), b"x").await.unwrap();
        }

        backup_local_cleanup(base.clone(), 1).await.unwrap();

        let exists = |name: &str| Path::new(&format!("{base}/{name}")).exists();

        assert!(!exists(&expired_backup), "an expired backup is deleted");
        assert!(exists(&fresh_backup), "a backup inside the window stays");
        assert!(
            exists(&lookalike_suffix),
            "a non-backup ending in .sqlite must survive the sweep"
        );
        assert!(
            exists(&lookalike_prefix),
            "a file with the backup prefix but not the suffix must survive the sweep"
        );
        assert!(
            exists(&lookalike_node),
            "a file whose node id does not parse is not a backup"
        );
        assert!(exists(&untouched), "an unrelated file is never touched");

        fs::remove_dir_all(&dir).await.unwrap();
    }

    /// Retention never deletes the newest backup, whatever `keep_days` says.
    ///
    /// `keep_days = 0` is accepted, and the local sweep runs right after the backup it follows,
    /// so it used to delete that backup, possibly while it was still being uploaded. On S3 the
    /// sweep deleted expired copies whether or not the new upload had landed, so uploads that
    /// kept failing for longer than `keep_days` aged every remote copy out.
    #[test]
    fn retention_never_deletes_the_newest_backup() {
        let t = |secs: i64| DateTime::from_timestamp(secs, 0).unwrap();
        let backups = [
            ("a", t(1_800_000_000)),
            ("b", t(1_800_000_100)),
            ("c", t(1_800_000_200)),
        ];

        // Everything is past the threshold: all but the newest go.
        assert_eq!(expired_backups(&backups, t(1_900_000_000)), vec!["a", "b"]);
        // A threshold in the middle: only what is older than it, and never the newest.
        assert_eq!(expired_backups(&backups, t(1_800_000_150)), vec!["a", "b"]);
        assert_eq!(expired_backups(&backups, t(1_800_000_050)), vec!["a"]);
        // A single backup, however old, is kept.
        assert!(expired_backups(&backups[..1], t(1_900_000_000)).is_empty());
        assert!(expired_backups(&[], t(1_900_000_000)).is_empty());
    }

    /// The same floor through the local sweep, with `keep_days = 0`.
    #[tokio::test]
    async fn local_cleanup_with_zero_keep_days_keeps_the_backup_it_follows() {
        let dir = std::env::temp_dir().join(format!(
            "hiqlite-backup-floor-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&dir).await.unwrap();
        let base = dir.to_str().unwrap().to_string();

        let older = format!("backup_node_1_{}.sqlite", 1_704_153_600i64);
        let newest = format!("backup_node_1_{}.sqlite", Utc::now().timestamp() - 5);
        for name in [&older, &newest] {
            fs::write(format!("{base}/{name}"), b"x").await.unwrap();
        }

        backup_local_cleanup(base.clone(), 0).await.unwrap();

        let exists = |name: &str| Path::new(&format!("{base}/{name}")).exists();
        assert!(!exists(&older), "an expired backup is deleted");
        assert!(exists(&newest), "the newest backup is never deleted");
        fs::remove_dir_all(&dir).await.unwrap();
    }

    /// The retention floor is stated in the zone it is compared in.
    ///
    /// Every timestamp it guards comes from `Utc::now()`, and the constant was CET midnight
    /// annotated as if it were UTC midnight. Nothing observable followed from the hour, and the
    /// constant guards a deletion, which is why it is pinned rather than left to a comment.
    #[test]
    fn the_retention_floor_is_utc_midnight_on_2024_01_01() {
        assert_eq!(
            DateTime::from_timestamp(TS_MIN, 0).unwrap().to_rfc3339(),
            "2024-01-01T00:00:00+00:00"
        );
    }

    /// A corrupt backup is an invalid backup, not a panicked task.
    ///
    /// `deserialize(..).unwrap()` inside `spawn_blocking` meant the caller saw a join error
    /// rather than a validation failure, and nothing checked the image's integrity at all.
    #[tokio::test]
    async fn an_invalid_backup_fails_validation_instead_of_panicking() {
        let dir = std::env::temp_dir().join(format!(
            "hiqlite-backup-validate-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(&dir).await.unwrap();

        // Not a database at all.
        let not_a_db = dir.join("not-a-db.sqlite").to_string_lossy().into_owned();
        fs::write(&not_a_db, b"certainly not sqlite").await.unwrap();
        let err = is_metadata_ok(not_a_db.clone())
            .await
            .expect_err("a file that is not a database is not a valid backup");
        assert!(
            err.to_string().contains("integrity check")
                || err.to_string().contains("cannot be opened"),
            "got: {err}"
        );

        // A real database with a `_metadata` row that does not decode.
        let bad_meta = dir.join("bad-meta.sqlite").to_string_lossy().into_owned();
        let path = bad_meta.clone();
        task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(path).unwrap();
            conn.execute(
                "CREATE TABLE _metadata (key TEXT PRIMARY KEY, data BLOB)",
                (),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO _metadata VALUES ('meta', ?1)",
                [b"not a StateMachineData".to_vec()],
            )
            .unwrap();
        })
        .await
        .unwrap();

        let err = is_metadata_ok(bad_meta)
            .await
            .expect_err("metadata that does not decode is not a valid backup");
        assert!(
            err.to_string().contains("not a StateMachineData"),
            "the failure must say what is wrong, got: {err}"
        );

        fs::remove_dir_all(&dir).await.unwrap();
    }

    /// A follower preparing for a restore join moves its state aside instead of deleting it,
    /// and leaves the storage owner lock where it is.
    ///
    /// The deletion was unconditional and ran on an environment variable alone, before node 1
    /// had pulled, validated or copied anything, so a restore that failed on node 1 had already
    /// destroyed every other node's state with no way back.
    #[tokio::test]
    async fn a_follower_moves_its_state_aside_instead_of_deleting_it() {
        let dir = std::env::temp_dir().join(format!(
            "hiqlite-backup-quarantine-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let base = dir.to_string_lossy().into_owned();
        fs::create_dir_all(dir.join("state_machine/db"))
            .await
            .unwrap();
        fs::create_dir_all(dir.join("logs")).await.unwrap();
        fs::write(
            dir.join("state_machine/db/hiqlite.db"),
            b"the previous state",
        )
        .await
        .unwrap();
        fs::write(dir.join("hiqlite-owner.lock"), b"pid=1")
            .await
            .unwrap();

        quarantine_data_dir_contents(&base).await.unwrap();

        assert!(
            dir.join("hiqlite-owner.lock").exists(),
            "the storage owner lock must never be moved or removed"
        );
        assert!(
            !dir.join("state_machine").exists(),
            "the node starts as though the directory were empty"
        );

        let mut quarantined = None;
        let mut entries = fs::read_dir(&dir).await.unwrap();
        while let Some(e) = entries.next_entry().await.unwrap() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(PRE_RESTORE_DIR_PREFIX) {
                quarantined = Some(e.path());
            }
        }
        let quarantined = quarantined.expect("the previous state is kept, not deleted");
        assert_eq!(
            fs::read(quarantined.join("state_machine/db/hiqlite.db"))
                .await
                .unwrap(),
            b"the previous state",
            "and it is recoverable byte for byte"
        );
        assert!(quarantined.join("logs").exists());

        // Running it again does not nest the quarantine inside itself.
        quarantine_data_dir_contents(&base).await.unwrap();
        assert!(
            quarantined.join("state_machine/db/hiqlite.db").exists(),
            "an earlier quarantine is left where it is"
        );

        fs::remove_dir_all(&dir).await.unwrap();
    }

    /// F-100: a restore removes the previous database's write-ahead log and shared-memory file
    /// along with the database itself.
    ///
    /// Replacing `hiqlite.db` alone left `hiqlite.db-wal` beside it, and that WAL belongs to the
    /// database that was just discarded. The restored node opened the backup with the old WAL
    /// attached, read the pre-restore `_metadata` out of it, and therefore believed it was
    /// already a member of the cluster it had just been told to rejoin: it never initialized
    /// itself, could not reach a quorum alone, and never served the endpoint its peers needed
    /// in order to rejoin it. Observed as a three-node deadlock in the cluster test's restore
    /// phase.
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn a_restore_removes_the_previous_write_ahead_log_and_not_only_the_database() {
        let dir = std::env::temp_dir().join(format!(
            "hiqlite-restore-sidecars-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let base = dir.to_string_lossy().into_owned();
        let db_dir = dir.join("state_machine/db");
        fs::create_dir_all(&db_dir).await.unwrap();

        // The state the restore is replacing, sidecars and all.
        fs::write(db_dir.join("hiqlite.db"), b"the previous database")
            .await
            .unwrap();
        fs::write(
            db_dir.join("hiqlite.db-wal"),
            b"the previous write-ahead log",
        )
        .await
        .unwrap();
        fs::write(db_dir.join("hiqlite.db-shm"), b"the previous shared memory")
            .await
            .unwrap();

        // A backup that passes validation on its own merits, so nothing here depends on
        // `HQL_BACKUP_SKIP_VALIDATION`, which is process-wide.
        let backup = dir
            .join("valid-backup.sqlite")
            .to_string_lossy()
            .into_owned();
        let path = backup.clone();
        task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(path).unwrap();
            conn.execute(
                "CREATE TABLE _metadata (key TEXT PRIMARY KEY, data BLOB)",
                (),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO _metadata VALUES ('meta', ?1)",
                [crate::helpers::serialize(&StateMachineData::default()).unwrap()],
            )
            .unwrap();
        })
        .await
        .unwrap();

        let node_config = NodeConfig {
            data_dir: base.clone().into(),
            filename_db: "hiqlite.db".into(),
            ..Default::default()
        };

        restore_backup(&node_config, BackupSource::File(backup))
            .await
            .expect("a valid backup restores");

        assert!(
            db_dir.join("hiqlite.db").exists(),
            "the restored database is in place"
        );
        assert!(
            !db_dir.join("hiqlite.db-wal").exists(),
            "the previous write-ahead log must not survive the restore"
        );
        assert!(
            !db_dir.join("hiqlite.db-shm").exists(),
            "the previous shared-memory file must not survive the restore"
        );
        assert!(
            !db_dir.join("hiqlite.db.restoring").exists(),
            "the staging name is consumed by the rename"
        );

        fs::remove_dir_all(&dir).await.unwrap();
    }

    /// A restore interrupted after its image was staged is rolled forward on the next start.
    ///
    /// The state below is the one a crash leaves after the previous write-ahead log, logs and
    /// snapshots have been removed and before the rename: the old database without its WAL. A
    /// normal start used to open it. Now the staged image is the commit record, and the start
    /// finishes the restore. A staging copy that never completed is discarded instead, and
    /// nothing else is touched.
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn an_interrupted_restore_is_rolled_forward_on_the_next_start() {
        let dir = std::env::temp_dir().join(format!(
            "hiqlite-restore-rollforward-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let db_dir = dir.join("state_machine/db");
        fs::create_dir_all(&db_dir).await.unwrap();
        let node_config = NodeConfig {
            data_dir: dir.to_string_lossy().into_owned().into(),
            filename_db: "hiqlite.db".into(),
            ..Default::default()
        };

        // An incomplete staging copy: discarded, and the database is left alone.
        fs::write(db_dir.join("hiqlite.db"), b"the previous database")
            .await
            .unwrap();
        fs::write(db_dir.join("hiqlite.db-wal"), b"its write-ahead log")
            .await
            .unwrap();
        fs::write(db_dir.join("hiqlite.db.restoring.tmp"), b"half a copy")
            .await
            .unwrap();
        assert!(!finish_staged_restore(&node_config).await.unwrap());
        assert!(!db_dir.join("hiqlite.db.restoring.tmp").exists());
        assert!(
            db_dir.join("hiqlite.db-wal").exists(),
            "nothing was destroyed"
        );

        // The crash state: staged image committed, the old WAL already gone.
        fs::remove_file(db_dir.join("hiqlite.db-wal"))
            .await
            .unwrap();
        fs::write(
            db_dir.join("hiqlite.db.restoring"),
            b"the restored database",
        )
        .await
        .unwrap();
        assert!(
            restore_backup_start(&node_config).await.unwrap(),
            "the start reports the restore as applied"
        );
        assert_eq!(
            fs::read(db_dir.join("hiqlite.db")).await.unwrap(),
            b"the restored database"
        );
        assert!(!db_dir.join("hiqlite.db.restoring").exists());
        assert!(
            !restore_backup_start(&node_config).await.unwrap(),
            "and it is not applied twice"
        );

        fs::remove_dir_all(&dir).await.unwrap();
    }

    /// F-061: reading `HQL_S3_URL` successfully committed the configuration to five further
    /// `expect`s, a `parse().expect` and an `unwrap`, on the authored assumption that all
    /// values exist together. A single missing or misspelled variable ended the process at
    /// configuration time. Each one is now named.
    ///
    /// Driven through `from_lookup` rather than through the environment, because the
    /// environment is process-wide and `009` D-3 records that as the reason no environment
    /// route in this corpus has a test.
    #[cfg(feature = "s3")]
    #[test]
    fn missing_s3_variables_are_named_rather_than_panicking() {
        use crate::s3::S3Config;
        use std::collections::HashMap;

        let cfg = |pairs: &[(&str, &str)]| {
            let map: HashMap<String, String> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            S3Config::from_lookup(&move |name| map.get(name).cloned())
        };

        // No URL at all is not a configuration error, it is no S3 configuration.
        assert!(cfg(&[]).unwrap().is_none());

        let err = cfg(&[("HQL_S3_URL", "https://s3.example.com")])
            .expect_err("a URL with nothing else is an incomplete configuration");
        assert!(err.to_string().contains("HQL_S3_BUCKET"), "got: {err}");

        let err = cfg(&[
            ("HQL_S3_URL", "https://s3.example.com"),
            ("HQL_S3_BUCKET", "b"),
            ("HQL_S3_REGION", "r"),
            ("HQL_S3_KEY", "k"),
        ])
        .expect_err("the secret is still missing");
        assert!(err.to_string().contains("HQL_S3_SECRET"), "got: {err}");

        let err = cfg(&[("HQL_S3_URL", "not a url")])
            .expect_err("an unparsable URL is a configuration error, not a panic");
        assert!(err.to_string().contains("HQL_S3_URL"), "got: {err}");

        // `HQL_S3_PATH_STYLE` is the one variable with an obvious default and is optional.
        let complete = [
            ("HQL_S3_URL", "https://s3.example.com"),
            ("HQL_S3_BUCKET", "b"),
            ("HQL_S3_REGION", "r"),
            ("HQL_S3_KEY", "k"),
            ("HQL_S3_SECRET", "s"),
        ];
        assert!(
            cfg(&complete).unwrap().is_some(),
            "a complete configuration without HQL_S3_PATH_STYLE is valid"
        );

        let mut with_bad_style = complete.to_vec();
        with_bad_style.push(("HQL_S3_PATH_STYLE", "neither"));
        let err =
            cfg(&with_bad_style).expect_err("an unparsable path style is a configuration error");
        assert!(err.to_string().contains("HQL_S3_PATH_STYLE"), "got: {err}");
    }
}
