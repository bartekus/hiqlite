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

/// Set to `true` for the one start that upgrades a data directory from hiqlite 0.14.x: the
/// legacy cache raft log and snapshots are moved aside instead of refused.
#[cfg(feature = "cache")]
pub const CACHE_LEGACY_MOVE_ASIDE_ENV: &str = "HQL_CACHE_LEGACY_MOVE_ASIDE";

/// The consent of `027` B-10, read from the environment. The check and the move it allows run
/// in `upgrade_exclusion`, under the WAL locks of every live node of either version (`035`).
#[cfg(feature = "cache")]
pub(crate) fn legacy_move_consent() -> Result<bool, crate::Error> {
    parse_consent(std::env::var(CACHE_LEGACY_MOVE_ASIDE_ENV).ok().as_deref())
}

/// `legacy_move_consent` with the value passed in, so it can be tested without touching the
/// process-wide environment (`009` D-3).
#[cfg(feature = "cache")]
fn parse_consent(value: Option<&str>) -> Result<bool, crate::Error> {
    match value {
        None => Ok(false),
        Some(v) if v.eq_ignore_ascii_case("true") => Ok(true),
        Some(v) if v.eq_ignore_ascii_case("false") => Ok(false),
        Some(v) => Err(crate::Error::Startup(
            format!("{CACHE_LEGACY_MOVE_ASIDE_ENV} must be 'true' or 'false', got '{v}'").into(),
        )),
    }
}

#[cfg(all(test, feature = "cache"))]
mod tests {
    //! `027` B-10's cases, whose names `027`'s acceptance runs. Since `035` they drive the whole
    //! start sequence of `upgrade_exclusion`, under the locks the check now runs beneath.

    use super::*;
    use crate::upgrade_exclusion::{
        CACHE_LOG_FORMAT, CACHE_LOG_FORMAT_FILE, PRE_UPGRADE_DIR_PREFIX, acquire_storage,
    };
    use std::fs;

    fn fresh(case: &str) -> String {
        let dir = format!("../target/test_data/cache_format/{case}");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn check(dir: &str, consent: bool) -> Result<(), crate::Error> {
        acquire_storage(dir, cfg!(feature = "sqlite"), true, consent).map(|o| o.release_clean())
    }

    /// A directory hiqlite 0.14.x left behind: WAL files and no marker. It is refused without
    /// the opt-in, with an error naming the opt-in and the manual procedure, and nothing moves.
    #[test]
    fn a_legacy_cache_log_is_refused_and_left_untouched() {
        let dir = fresh("legacy");
        fs::create_dir_all(format!("{dir}/logs_cache")).unwrap();
        fs::write(format!("{dir}/logs_cache/0000000000000001.wal"), b"legacy").unwrap();

        let err = check(&dir, false).expect_err("a legacy cache log must be refused");
        let msg = err.to_string();
        assert!(msg.contains(CACHE_LEGACY_MOVE_ASIDE_ENV), "got: {msg}");
        assert!(msg.contains("state_machine_cache"), "got: {msg}");
        // `035` B-3: what is true, and what this start created.
        assert!(msg.contains("No data was changed"), "got: {msg}");
        assert!(msg.contains("hiqlite-owner.lock"), "got: {msg}");
        assert!(fs::exists(format!("{dir}/logs_cache/0000000000000001.wal")).unwrap());
        assert!(!fs::exists(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}")).unwrap());
        assert!(!fs::exists(format!("{dir}/logs_cache/lock.hql")).unwrap());

        // Snapshots with no log are what a memory-only run of this build leaves; switching the
        // cache to disk must not be refused as an upgrade from 0.14.
        let dir2 = fresh("memory-snap");
        fs::create_dir_all(format!("{dir2}/state_machine_cache/snapshots")).unwrap();
        fs::write(format!("{dir2}/state_machine_cache/snapshots/s"), b"x").unwrap();
        check(&dir2, false).expect("snapshots without a cache log are not a legacy cache");
    }

    /// With the opt-in, both directories move into `pre-upgrade-<secs>/` intact, the marker is
    /// written, and the next start needs no opt-in.
    #[test]
    fn the_opt_in_moves_the_legacy_cache_aside_and_marks_the_new_one() {
        let dir = fresh("move");
        fs::create_dir_all(format!("{dir}/logs_cache")).unwrap();
        fs::write(format!("{dir}/logs_cache/0000000000000001.wal"), b"legacy").unwrap();
        fs::create_dir_all(format!("{dir}/state_machine_cache/snapshots")).unwrap();

        check(&dir, true).expect("the opt-in moves it aside");

        let moved = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .find(|n| n.starts_with(PRE_UPGRADE_DIR_PREFIX))
            .expect("a pre-upgrade directory");
        assert_eq!(
            fs::read(format!("{dir}/{moved}/logs_cache/0000000000000001.wal")).unwrap(),
            b"legacy",
            "moved, not deleted"
        );
        assert!(fs::exists(format!("{dir}/{moved}/state_machine_cache")).unwrap());
        assert_eq!(
            fs::read_to_string(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}")).unwrap(),
            CACHE_LOG_FORMAT
        );
        check(&dir, false).expect("a marked directory needs no opt-in");
    }

    /// A fresh directory is marked, and a marker from another format is refused.
    #[test]
    fn a_fresh_directory_is_marked_and_a_foreign_marker_is_refused() {
        let dir = fresh("fresh");
        check(&dir, false).unwrap();
        assert!(fs::exists(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}")).unwrap());

        fs::write(format!("{dir}/logs_cache/{CACHE_LOG_FORMAT_FILE}"), b"9").unwrap();
        let err = check(&dir, true).expect_err("format 9 is not ours");
        assert!(err.to_string().contains("format 9"), "got: {err}");
    }

    #[test]
    fn the_consent_is_true_false_or_an_error() {
        assert!(!parse_consent(None).unwrap());
        assert!(parse_consent(Some("TRUE")).unwrap());
        assert!(!parse_consent(Some("false")).unwrap());
        assert!(parse_consent(Some("yes")).is_err());
    }
}
