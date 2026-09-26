use crate::store::state_machine::sqlite::TypeConfigSqlite;
use crate::store::state_machine::sqlite::state_machine::StateMachineSqlite;
use crate::store::state_machine::sqlite::writer::{SnapshotRequest, WriterRequest};
use crate::{Node, NodeId};
use openraft::{
    RaftSnapshotBuilder, Snapshot, SnapshotMeta, StorageError, StorageIOError, StoredMembership,
};
use tokio::sync::oneshot;
use tokio::{fs, task};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// How many published snapshots are kept.
///
/// Two, so a startup that finds the newest unreadable has something to fall back to. One was
/// the old value and is why F-006 had no fallback available even after selection could look
/// for one.
pub(crate) const SNAPSHOTS_KEPT: usize = 2;

#[derive(Debug, Clone)]
pub struct SQLiteSnapshotBuilder {
    // pub last_applied_log_id: Option<LogId<NodeId>>,
    // pub last_membership: StoredMembership<NodeId, Node>,
    #[cfg(feature = "backup")]
    pub path_backups: String,
    pub path_snapshots: String,
    pub write_tx: flume::Sender<WriterRequest>,
}

impl RaftSnapshotBuilder<TypeConfigSqlite> for SQLiteSnapshotBuilder {
    #[tracing::instrument(level = "trace", skip(self))]
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfigSqlite>, StorageError<NodeId>> {
        // - build new snapshot id
        // - make sure target path exists
        // - send snapshot request to db writer
        // - await vaccuum response
        // - open db snapshot file
        // - return snapshot handle

        let snapshot_id = Uuid::now_v7();

        let path = format!("{}/{}", self.path_snapshots, snapshot_id);
        let path_temp = format!("{path}.temp");
        let (ack, rx) = oneshot::channel();
        let req = WriterRequest::Snapshot(SnapshotRequest {
            snapshot_id,
            // last_membership: self.last_membership.clone(),
            path: path_temp.clone(),
            ack,
        });
        self.write_tx
            .send_async(req)
            .await
            .expect("Sender to always be listening");

        let resp = rx.await.expect("to always receive a snapshot response")?;

        // Publication is a same-directory rename, not a copy.
        //
        // `fs::copy` wrote the final UUID name byte by byte, so an interrupted copy left a
        // short file under a name that startup selection accepts, which is F-003. The writer
        // has already produced a complete, synced image at the staging name, and `.temp` is
        // not a UUID so nothing selects it; renaming publishes it in one step that either
        // happened or did not.
        fs::rename(&path_temp, &path)
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write_state_machine(&err),
            })?;
        // And the rename itself is made durable, so a crash cannot leave the new name in a
        // directory that was never written back.
        crate::store::state_machine::sqlite::sync_parent_dir(&path)
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write_state_machine(&err),
            })?;

        let snapshot = fs::File::open(&path)
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::read_state_machine(&err),
            })?;

        let path_snapshots = self.path_snapshots.clone();
        #[cfg(feature = "backup")]
        let path_backups = self.path_backups.clone();
        // cleanup can easily happen in the background
        task::spawn(snapshots_cleanup(
            path_snapshots,
            #[cfg(feature = "backup")]
            path_backups,
            snapshot_id,
        ));

        let snapshot = Snapshot {
            meta: SnapshotMeta {
                last_log_id: resp.meta.last_applied_log_id,
                last_membership: resp.meta.last_membership,
                snapshot_id: snapshot_id.to_string(),
            },
            snapshot: Box::new(snapshot),
        };

        Ok(snapshot)
    }
}

// The error type is huge, but defined by the openraft trait.
#[allow(clippy::result_large_err)]
async fn snapshots_cleanup(
    path_snapshots: String,
    #[cfg(feature = "backup")] path_backups: String,
    keep_id: Uuid,
) -> Result<(), StorageError<NodeId>> {
    let mut list = tokio::fs::read_dir(&path_snapshots)
        .await
        .map_err(|err| StorageError::IO {
            source: StorageIOError::read(&err),
        })?;

    let keep_id = keep_id.to_string();
    let mut candidates = Vec::new();
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

        let meta = entry.metadata().await.map_err(|err| StorageError::IO {
            source: StorageIOError::read(&err),
        })?;

        // we only expect sub-dirs in the snapshot dir
        if meta.is_dir() {
            warn!("Invalid folder in snapshots dir: {name}");
            continue;
        }

        candidates.push(name.to_string());
    }

    // Keep the newest two published snapshots, not one.
    //
    // Deleting everything but the newest is what left F-006 with nothing to fall back to: the
    // moment the newest file turned out to be unreadable, the only other copy had already been
    // removed by the build that produced it. UUIDv7 is time-ordered, so a lexicographic sort
    // over the published names is a chronological one.
    let mut published: Vec<&String> = candidates
        .iter()
        .filter(|n| Uuid::parse_str(n).is_ok())
        .collect();
    published.sort();
    published.reverse();
    let keep: Vec<String> = published
        .iter()
        .take(SNAPSHOTS_KEPT)
        .map(|n| (*n).clone())
        .collect();
    debug_assert!(keep.iter().any(|k| k == &keep_id) || keep.is_empty());
    let oldest_kept = keep.last().cloned();

    let mut deletes = Vec::new();
    for name in &candidates {
        if Uuid::parse_str(name).is_ok() {
            if !keep.contains(name) {
                deletes.push(name.clone());
            }
            continue;
        }

        // Staging names. `temp` is the install stream's and may be in flight, so it is never
        // touched here. A `{uuid}.temp` or `{uuid}.incoming` whose id sorts below the oldest
        // snapshot being kept belongs to a build or an install that is long finished.
        let Some(oldest_kept) = &oldest_kept else {
            continue;
        };
        for suffix in [".temp", ".incoming"] {
            if let Some(stem) = name.strip_suffix(suffix)
                && Uuid::parse_str(stem).is_ok()
                && stem < oldest_kept.as_str()
            {
                deletes.push(name.clone());
            }
        }
    }

    #[cfg(feature = "backup")]
    {
        debug!("Cleaning up possibly existing old backup restore files");
        let restore_path = format!("{}/{}", path_backups, crate::backup::BACKUP_DB_NAME);
        let _ = fs::remove_file(&restore_path).await;
    }

    for file_name in deletes {
        let path = format!("{path_snapshots}/{file_name}");
        if let Err(err) = fs::remove_file(path).await {
            error!("Error removing old snapshot {file_name}: {err}");
        }
    }

    Ok(())
}
