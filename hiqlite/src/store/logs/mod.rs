#[cfg(feature = "cache")]
pub mod memory;

#[cfg(feature = "sqlite")]
pub fn logs_dir_db(data_dir: &str) -> String {
    format!("{data_dir}/logs")
}

#[cfg(feature = "cache")]
pub fn logs_dir_cache(data_dir: &str) -> String {
    format!("{data_dir}/logs_cache")
}

/// The file in the cache raft's log directory that says which `CacheRequest` layout wrote it.
#[cfg(feature = "cache")]
pub(crate) const CACHE_LOG_FORMAT_FILE: &str = "hiqlite-cache-log-format";

/// The `CacheRequest` layout this build reads and writes: upstream PR #362's, in which every
/// variant exists whatever the features and `GetRemove` and `Replace` sit at indices 2 and 3.
#[cfg(feature = "cache")]
const CACHE_LOG_FORMAT: &str = "2";

/// Set to `true` for the one start that upgrades a data directory from hiqlite 0.14.x: the
/// legacy cache raft log and snapshots are moved aside instead of refused.
#[cfg(feature = "cache")]
pub const CACHE_LEGACY_MOVE_ASIDE_ENV: &str = "HQL_CACHE_LEGACY_MOVE_ASIDE";

#[cfg(feature = "cache")]
const PRE_UPGRADE_DIR_PREFIX: &str = "pre-upgrade-";

/// Move the cache raft's log and snapshot directories into `pre-upgrade-<unix seconds>/`.
///
/// Moved, not deleted, like `026` B-4's pre-restore quarantine, so an operator can still look
/// at what was there. The storage owner lock at the data directory root is not touched.
#[cfg(feature = "cache")]
async fn move_legacy_cache_aside(data_dir: &str) -> Result<(), crate::Error> {
    use tokio::fs;

    let target = format!(
        "{data_dir}/{PRE_UPGRADE_DIR_PREFIX}{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default()
    );
    let err = |what: &str, err: std::io::Error| {
        crate::Error::Startup(format!("cannot move the legacy cache aside, {what}: {err}").into())
    };
    fs::create_dir_all(&target)
        .await
        .map_err(|e| err(&format!("creating {target}"), e))?;
    for name in ["logs_cache", "state_machine_cache"] {
        let from = format!("{data_dir}/{name}");
        match fs::rename(&from, format!("{target}/{name}")).await {
            Ok(()) => tracing::warn!(
                "Moved the legacy cache directory {from} to {target}/{name}: the cache raft \
                 starts empty"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(err(&format!("moving {from}"), e)),
        }
    }
    #[cfg(unix)]
    fs::File::open(data_dir)
        .await
        .map_err(|e| err("opening the data directory", e))?
        .sync_all()
        .await
        .map_err(|e| err("syncing the data directory", e))?;
    Ok(())
}

/// Refuse a disk-backed cache raft whose log or snapshots this build cannot read safely.
///
/// hiqlite 0.14.0 wrote `CacheRequest` in a different, feature-dependent layout. Upstream PR
/// #362 inserted two variants and made every variant unconditional, so a 0.14.0 cache log does
/// not merely fail to decode here: some entries decode **as a different command**, which is
/// silent divergence. Decoding is therefore never attempted on a log that does not carry this
/// build's marker. A fresh directory gets the marker; one with data and no marker is a startup
/// error that says what to do. The SQLite raft is unaffected.
#[cfg(feature = "cache")]
pub(crate) async fn ensure_cache_log_format(data_dir: &str) -> Result<(), crate::Error> {
    let move_aside = match std::env::var(CACHE_LEGACY_MOVE_ASIDE_ENV) {
        Err(_) => false,
        Ok(v) if v.eq_ignore_ascii_case("true") => true,
        Ok(v) if v.eq_ignore_ascii_case("false") => false,
        Ok(v) => {
            return Err(crate::Error::Startup(
                format!("{CACHE_LEGACY_MOVE_ASIDE_ENV} must be 'true' or 'false', got '{v}'")
                    .into(),
            ));
        }
    };
    check_cache_log_format(data_dir, move_aside).await
}

/// `ensure_cache_log_format` with the opt-in passed in, so it can be tested without touching
/// the process-wide environment (`009` D-3).
#[cfg(feature = "cache")]
async fn check_cache_log_format(data_dir: &str, move_aside: bool) -> Result<(), crate::Error> {
    use tokio::fs;

    let dir_logs = logs_dir_cache(data_dir);
    let dir_snapshots = format!("{data_dir}/state_machine_cache/snapshots");
    let marker = format!("{dir_logs}/{CACHE_LOG_FORMAT_FILE}");

    async fn holds_files(dir: &str, only_wal: bool) -> std::io::Result<bool> {
        let mut entries = match fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(err),
        };
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !only_wal || name.ends_with(".wal") {
                return Ok(true);
            }
        }
        Ok(false)
    }

    let io = |err: std::io::Error| {
        crate::Error::Startup(format!("cannot check the cache raft log format: {err}").into())
    };

    match fs::read_to_string(&marker).await {
        Ok(found) if found.trim() == CACHE_LOG_FORMAT => return Ok(()),
        Ok(found) => {
            return Err(crate::Error::Startup(
                format!(
                    "the cache raft log in {dir_logs} was written in format {}, and this build \
                     reads only format {CACHE_LOG_FORMAT}",
                    found.trim()
                )
                .into(),
            ));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(io(err)),
    }

    if holds_files(&dir_logs, true).await.map_err(io)?
        || holds_files(&dir_snapshots, false).await.map_err(io)?
    {
        if !move_aside {
            return Err(crate::Error::Startup(
                format!(
                    "the cache raft's log or snapshots in {dir_logs} and {data_dir}/\
                     state_machine_cache were written by a hiqlite version whose replicated \
                     cache format this build cannot read safely (hiqlite 0.14.x, or a \
                     pre-release build without a format marker). The cache is not carried \
                     across this upgrade; the SQLite database is. Either start once with \
                     {CACHE_LEGACY_MOVE_ASIDE_ENV}=true, which moves both directories into \
                     {data_dir}/{PRE_UPGRADE_DIR_PREFIX}<unix seconds>/, or stop the node and \
                     move them aside yourself. The cache raft then starts empty, as an \
                     in-memory cache does after every restart. Nothing was changed."
                )
                .into(),
            ));
        }
        move_legacy_cache_aside(data_dir).await?;
    }

    fs::create_dir_all(&dir_logs).await.map_err(io)?;
    let tmp = format!("{marker}.tmp");
    fs::write(&tmp, CACHE_LOG_FORMAT).await.map_err(io)?;
    fs::File::open(&tmp).await.map_err(io)?.sync_all().await.map_err(io)?;
    fs::rename(&tmp, &marker).await.map_err(io)?;
    #[cfg(unix)]
    fs::File::open(&dir_logs).await.map_err(io)?.sync_all().await.map_err(io)?;
    Ok(())
}

#[cfg(all(test, feature = "cache"))]
mod tests {
    use super::*;
    use tokio::fs;

    async fn fresh(case: &str) -> String {
        let dir = std::env::temp_dir()
            .join(format!(
                "hiqlite-cache-format-{case}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .to_string_lossy()
            .into_owned();
        fs::create_dir_all(&dir).await.unwrap();
        dir
    }

    /// A directory hiqlite 0.14.x left behind: WAL files and no marker. It is refused without
    /// the opt-in, with an error naming the opt-in and the manual procedure, and nothing moves.
    #[tokio::test]
    async fn a_legacy_cache_log_is_refused_and_left_untouched() {
        let dir = fresh("legacy").await;
        fs::create_dir_all(format!("{dir}/logs_cache")).await.unwrap();
        fs::write(format!("{dir}/logs_cache/0000000000000001.wal"), b"legacy")
            .await
            .unwrap();

        let err = check_cache_log_format(&dir, false)
            .await
            .expect_err("a legacy cache log must be refused");
        let msg = err.to_string();
        assert!(msg.contains(CACHE_LEGACY_MOVE_ASIDE_ENV), "got: {msg}");
        assert!(msg.contains("state_machine_cache"), "got: {msg}");
        assert!(msg.contains("Nothing was changed"), "got: {msg}");
        assert!(fs::try_exists(format!("{dir}/logs_cache/0000000000000001.wal")).await.unwrap());
        assert!(!fs::try_exists(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}")).await.unwrap());

        // Legacy snapshots alone are refused too.
        let dir2 = fresh("legacy-snap").await;
        fs::create_dir_all(format!("{dir2}/state_machine_cache/snapshots")).await.unwrap();
        fs::write(format!("{dir2}/state_machine_cache/snapshots/s"), b"x").await.unwrap();
        assert!(check_cache_log_format(&dir2, false).await.is_err());

        let _ = fs::remove_dir_all(&dir).await;
        let _ = fs::remove_dir_all(&dir2).await;
    }

    /// With the opt-in, both directories move into `pre-upgrade-<secs>/` intact, the marker is
    /// written, and the next start needs no opt-in.
    #[tokio::test]
    async fn the_opt_in_moves_the_legacy_cache_aside_and_marks_the_new_one() {
        let dir = fresh("move").await;
        fs::create_dir_all(format!("{dir}/logs_cache")).await.unwrap();
        fs::write(format!("{dir}/logs_cache/0000000000000001.wal"), b"legacy")
            .await
            .unwrap();
        fs::create_dir_all(format!("{dir}/state_machine_cache/snapshots")).await.unwrap();

        check_cache_log_format(&dir, true).await.expect("the opt-in moves it aside");

        let mut moved = None;
        let mut entries = fs::read_dir(&dir).await.unwrap();
        while let Some(e) = entries.next_entry().await.unwrap() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(PRE_UPGRADE_DIR_PREFIX) {
                moved = Some(name);
            }
        }
        let moved = moved.expect("a pre-upgrade directory");
        assert_eq!(
            fs::read(format!("{dir}/{moved}/logs_cache/0000000000000001.wal")).await.unwrap(),
            b"legacy",
            "moved, not deleted"
        );
        assert!(fs::try_exists(format!("{dir}/{moved}/state_machine_cache")).await.unwrap());
        assert_eq!(
            fs::read_to_string(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}")).await.unwrap(),
            CACHE_LOG_FORMAT
        );
        check_cache_log_format(&dir, false)
            .await
            .expect("a marked directory needs no opt-in");
        let _ = fs::remove_dir_all(&dir).await;
    }

    /// A fresh directory is marked, and a marker from another format is refused.
    #[tokio::test]
    async fn a_fresh_directory_is_marked_and_a_foreign_marker_is_refused() {
        let dir = fresh("fresh").await;
        check_cache_log_format(&dir, false).await.unwrap();
        assert!(fs::try_exists(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}")).await.unwrap());

        fs::write(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}"), b"9").await.unwrap();
        let err = check_cache_log_format(&dir, true).await.expect_err("format 9 is not ours");
        assert!(err.to_string().contains("format 9"), "got: {err}");
        let _ = fs::remove_dir_all(&dir).await;
    }
}
