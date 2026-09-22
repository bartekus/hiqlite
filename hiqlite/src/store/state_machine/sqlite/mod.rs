use crate::store::state_machine::sqlite::state_machine::QueryWrite;
use crate::Node;
use crate::Response;

pub mod param;
pub mod snapshot_builder;
pub mod state_machine;
pub mod transaction_variable;
pub mod writer;

mod transaction_env;

openraft::declare_raft_types!(
    pub TypeConfigSqlite:
        D = QueryWrite,
        R = Response,
        Node = Node,
        SnapshotData = tokio::fs::File,
);

/// Flush one file's contents to the storage device.
///
/// A rename is atomic with respect to a reader, and by itself orders nothing with respect to a
/// crash: the directory entry can be present while the file's contents are not yet written
/// back. Publication therefore syncs the bytes, renames, and then syncs the directory that
/// holds the new name.
pub(crate) fn sync_file_blocking(path: &str) -> Result<(), crate::Error> {
    std::fs::File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|err| crate::Error::Error(format!("cannot sync {path}: {err}").into()))
}

/// Flush the directory entry that names `path`.
///
/// Unix only, deliberately: on Windows a directory cannot be opened as a file and the rename
/// carries its own ordering. Best effort is not good enough for the file, and is what is
/// available for the directory, so a failure here is reported and not swallowed.
pub(crate) fn sync_parent_dir_blocking(path: &str) -> Result<(), crate::Error> {
    #[cfg(unix)]
    {
        let parent = std::path::Path::new(path)
            .parent()
            .ok_or_else(|| crate::Error::Error(format!("{path} has no parent directory").into()))?;
        std::fs::File::open(parent)
            .and_then(|d| d.sync_all())
            .map_err(|err| {
                crate::Error::Error(
                    format!("cannot sync the directory {}: {err}", parent.display()).into(),
                )
            })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// The async wrappers, for the paths that are not already on a blocking thread.
pub(crate) async fn sync_file(path: &str) -> Result<(), crate::Error> {
    let path = path.to_string();
    tokio::task::spawn_blocking(move || sync_file_blocking(&path)).await?
}

pub(crate) async fn sync_parent_dir(path: &str) -> Result<(), crate::Error> {
    let path = path.to_string();
    tokio::task::spawn_blocking(move || sync_parent_dir_blocking(&path)).await?
}
