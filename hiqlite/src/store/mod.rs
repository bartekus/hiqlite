#![allow(unused)]

use crate::app_state::{AppState, RaftType};
use crate::network::NetworkStreaming;
use crate::{CacheVariants, Error, NodeConfig, NodeId, RaftConfig, init};
use hiqlite_wal::LogSync;
use openraft::storage::RaftLogStorage;
use openraft::{Raft, StorageError};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cmp::PartialEq;
use std::fmt::Debug;
use std::mem;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::time;
use tracing::info;

#[cfg(feature = "cache")]
use crate::{
    app_state::StateRaftCache,
    store::state_machine::memory::{TypeConfigKV, state_machine::StateMachineMemory},
};
#[cfg(feature = "sqlite")]
use crate::{
    app_state::StateRaftDB,
    store::state_machine::sqlite::{
        TypeConfigSqlite,
        state_machine::{SqlitePool, StateMachineSqlite},
        writer::WriterRequest,
    },
};

pub mod logs;
pub mod state_machine;

pub type StorageResult<T> = Result<T, StorageError<NodeId>>;

/// Start a WAL log store on the lock `035` B-1 took for it, or take the lock itself when this
/// node took none (no data directory to exclude on).
async fn start_log_store<T: openraft::RaftTypeConfig>(
    dir: String,
    node_config: &NodeConfig,
    wal_lock: Option<&hiqlite_wal::LockFile>,
) -> Result<hiqlite_wal::LogStore<T>, Error> {
    Ok(match wal_lock {
        Some(lock) => {
            hiqlite_wal::LogStore::start_with_lock(
                dir,
                lock,
                node_config.wal_sync.clone(),
                node_config.wal_size,
            )
            .await?
        }
        None => {
            hiqlite_wal::LogStore::start(dir, node_config.wal_sync.clone(), node_config.wal_size)
                .await?
        }
    })
}

#[cfg(feature = "sqlite")]
pub(crate) async fn start_raft_db(
    node_config: &NodeConfig,
    raft_config: Arc<RaftConfig>,
    do_reset_metadata: bool,
    lifecycle: crate::lifecycle::NodeLifecycle,
    wal_lock: Option<&hiqlite_wal::LockFile>,
) -> Result<StateRaftDB, Error> {
    // We always want to start stopped and set to `false` as soon as we found out,
    // that we are not pristine node and need cleanup.
    let is_raft_stopped = Arc::new(AtomicBool::new(true));

    let mut log_store = start_log_store::<TypeConfigSqlite>(
        logs::logs_dir_db(&node_config.data_dir),
        node_config,
        wal_lock,
    )
    .await?;

    // Read before the state machine is built, because the state machine needs it to decide
    // whether falling back to an older snapshot is recoverable at all. The log store is started
    // first anyway, so this costs one read and no reordering.
    let recovery_bounds = {
        let log_state = log_store.get_log_state().await.map_err(|err| {
            Error::Error(format!("cannot read the WAL log state at startup: {err}").into())
        })?;
        (
            crate::store::state_machine::sqlite::state_machine::SnapshotRecoveryBounds {
                last_purged_index: log_state.last_purged_log_id.map(|id| id.index),
            },
            log_state.last_log_id.map(|id| id.index),
        )
    };
    let (recovery_bounds, recovery_target) = recovery_bounds;

    // `037`: the unclean-stop marker is read before the state machine's constructor acts on it
    // (under `auto-heal` it deletes the database), so the recovery can say why it is rebuilding.
    let unclean_stop = tokio::fs::try_exists(format!(
        "{}/lock",
        StateMachineSqlite::path_base(&node_config.data_dir)
    ))
    .await
    .unwrap_or(false);
    let recovery = crate::recovery::StartupRecovery::new("db", recovery_target, unclean_stop);

    let state_machine_store = StateMachineSqlite::new(
        &node_config.data_dir,
        &node_config.filename_db,
        node_config.node_id,
        node_config.log_statements,
        node_config.prepared_statement_cache_capacity,
        node_config.read_pool_size,
        #[cfg(feature = "s3")]
        node_config.s3_config.clone(),
        do_reset_metadata,
        #[cfg(feature = "backup")]
        node_config.backup_keep_days_local,
        recovery_bounds,
    )
    .await
    .map_err(|err| Error::Startup(format!("cannot open the sqlite state machine: {err}").into()))?;

    let is_startup_finished = Arc::new(AtomicBool::new(false));
    let sql_writer = state_machine_store.write_tx.clone();
    let read_pool = state_machine_store.read_pool.clone();

    let network = NetworkStreaming {
        node_id: node_config.node_id,
        tls_config: node_config.tls_raft.as_ref().map(|tls| tls.client_config()),
        secret_raft: node_config.secret_raft.as_bytes().to_vec(),
        raft_type: RaftType::Sqlite,
        heartbeat_interval: node_config.raft_config.heartbeat_interval,
        is_raft_stopped: is_raft_stopped.clone(),
        is_startup_finished: is_startup_finished.clone(),
    };

    let shutdown_handle = log_store.shutdown_handle();
    // `008` KD-3's missing consumer. A writer thread that ends, for any reason including a
    // panic inside it, now takes this node out of service instead of leaving it answering as
    // though its log storage were healthy.
    crate::lifecycle::watch_wal_writer(
        lifecycle.clone(),
        log_store.writer_failure(),
        "the sqlite raft log",
    );

    let raft = openraft::Raft::new(
        node_config.node_id,
        raft_config.clone(),
        network,
        log_store,
        state_machine_store,
    )
    .await
    .map_err(|err| {
        // A start that failed is not a component that failed: the log store dropped here
        // ends its writer, and without this the watch recorded that as a WAL writer failure.
        lifecycle.begin_shutdown();
        Error::Startup(format!("cannot create the sqlite raft: {err}").into())
    })?;

    if let Err(err) = init::init_pristine_node_1_db(
        &raft,
        node_config.node_id,
        &node_config.nodes,
        &node_config.secret_api,
        node_config.tls_api.is_some(),
        node_config
            .tls_api
            .as_ref()
            .map(|c| c.danger_tls_no_verify())
            .unwrap_or(false),
    )
    .await
    {
        // The raft, its WAL writer and the SQLite writer are all running by now. Returning
        // without stopping them leaked the WAL writer's lock past a failed start, and the next
        // start in the same process panicked on it (found in review).
        lifecycle.begin_shutdown();
        let _ = raft.shutdown().await;
        let _ = shutdown_handle.shutdown().await;
        let (tx, rx) = tokio::sync::oneshot::channel();
        if sql_writer
            .send_async(state_machine::sqlite::writer::WriterRequest::Shutdown(tx))
            .await
            .is_ok()
        {
            let _ = rx.await;
        }
        return Err(err);
    }

    recovery.watch(raft.metrics());

    Ok(StateRaftDB {
        raft,
        shutdown_handle,
        sql_writer,
        read_pool,
        log_statements: node_config.log_statements,
        is_raft_stopped,
        is_startup_finished,
        recovery,
    })
}

#[cfg(feature = "cache")]
pub(crate) async fn start_raft_cache<C>(
    node_config: &NodeConfig,
    raft_config: Arc<RaftConfig>,
    lifecycle: crate::lifecycle::NodeLifecycle,
    wal_lock: Option<&hiqlite_wal::LockFile>,
) -> Result<StateRaftCache, Error>
where
    C: Debug + CacheVariants,
{
    // TODO add a check here and only start the cache layer, if the given Idx Enum is NOT empty

    // We always want to start stopped and set to `false` as soon as we found out,
    // that we are not pristine node and need cleanup.
    let is_raft_stopped = Arc::new(AtomicBool::new(true));
    let is_startup_finished = Arc::new(AtomicBool::new(false));

    let state_machine_store = Arc::new(
        StateMachineMemory::new::<C>(&node_config.data_dir, !node_config.cache_storage_disk)
            .await?,
    );
    let network = NetworkStreaming {
        node_id: node_config.node_id,
        tls_config: node_config.tls_raft.as_ref().map(|tls| tls.client_config()),
        secret_raft: node_config.secret_raft.as_bytes().to_vec(),
        raft_type: RaftType::Cache,
        heartbeat_interval: node_config.raft_config.heartbeat_interval,
        is_startup_finished: is_startup_finished.clone(),
        is_raft_stopped: is_raft_stopped.clone(),
    };

    let tx_caches = state_machine_store.tx_caches.clone();
    // Shared with the state machine so the local read and write paths can refuse once a
    // replicated cache command could not be applied. Cloned before the store is moved into
    // `Raft::new`.
    let cache_incompatible = state_machine_store.incompatible.clone();
    #[cfg(feature = "listen_notify")]
    let tx_notify = state_machine_store.tx_notify.clone();
    #[cfg(feature = "listen_notify_local")]
    let rx_notify = state_machine_store.rx_notify.clone();

    #[cfg(feature = "dlock")]
    let tx_dlock = state_machine_store.tx_dlock.clone();

    // `037`: a disk-backed cache keeps its state machine in memory, so every start replays the
    // log it holds. An in-memory log starts empty and has nothing of its own to recover.
    let mut recovery_target = None;

    let (raft, shutdown_handle) = if node_config.cache_storage_disk {
        let mut log_store = start_log_store::<TypeConfigKV>(
            logs::logs_dir_cache(&node_config.data_dir),
            node_config,
            wal_lock,
        )
        .await?;
        recovery_target = log_store
            .get_log_state()
            .await
            .map_err(|err| {
                Error::Error(
                    format!("cannot read the cache WAL log state at startup: {err}").into(),
                )
            })?
            .last_log_id
            .map(|id| id.index);
        let shutdown_handle = log_store.shutdown_handle();
        crate::lifecycle::watch_wal_writer(
            lifecycle.clone(),
            log_store.writer_failure(),
            "the cache raft log",
        );

        let raft = openraft::Raft::new(
            node_config.node_id,
            raft_config.clone(),
            network,
            log_store,
            state_machine_store,
        )
        .await
        .map_err(|err| {
            // A start that failed is not a component that failed: the log store dropped here
            // ends its writer, and without this the watch recorded that as a WAL writer failure.
            lifecycle.begin_shutdown();
            Error::Startup(format!("cannot create the cache raft: {err}").into())
        })?;

        (raft, Some(shutdown_handle))
    } else {
        let raft = openraft::Raft::new(
            node_config.node_id,
            raft_config.clone(),
            network,
            logs::memory::LogStoreMemory::new(),
            state_machine_store,
        )
        .await
        .map_err(|err| {
            // A start that failed is not a component that failed: the log store dropped here
            // ends its writer, and without this the watch recorded that as a WAL writer failure.
            lifecycle.begin_shutdown();
            Error::Startup(format!("cannot create the cache raft: {err}").into())
        })?;

        (raft, None)
    };

    if let Err(err) = init::init_pristine_node_1_cache(
        &raft,
        node_config.cache_storage_disk,
        node_config.node_id,
        &node_config.nodes,
        &node_config.secret_api,
        node_config.tls_api.is_some(),
        node_config
            .tls_api
            .as_ref()
            .map(|c| c.danger_tls_no_verify())
            .unwrap_or(false),
    )
    .await
    {
        // As for the SQLite group: nothing started here may outlive a failed start.
        lifecycle.begin_shutdown();
        let _ = raft.shutdown().await;
        if let Some(handle) = &shutdown_handle {
            let _ = handle.shutdown().await;
        }
        return Err(err);
    }

    let recovery = crate::recovery::StartupRecovery::new("cache", recovery_target, false);
    recovery.watch(raft.metrics());

    Ok(StateRaftCache {
        recovery,
        raft,
        tx_caches,
        cache_incompatible,
        #[cfg(feature = "listen_notify")]
        tx_notify,
        #[cfg(feature = "listen_notify_local")]
        rx_notify,
        #[cfg(feature = "dlock")]
        tx_dlock,
        is_raft_stopped,
        is_startup_finished,
        shutdown_handle,
        cache_storage_disk: node_config.cache_storage_disk,
    })
}
