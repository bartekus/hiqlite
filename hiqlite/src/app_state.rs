use crate::NodeId;
use chrono::Utc;
use serde::Deserialize;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::Mutex;

#[cfg(any(feature = "backup", feature = "dashboard"))]
use crate::client::stream::ClientStreamReq;
#[cfg(feature = "dashboard")]
use crate::dashboard::DashboardState;
#[cfg(feature = "s3")]
use crate::s3::S3Config;
#[cfg(feature = "dlock")]
use crate::store::state_machine::memory::dlock_handler::LockRequest;
#[cfg(feature = "listen_notify")]
use crate::store::state_machine::memory::notify_handler::NotifyRequest;
#[cfg(feature = "cache")]
use crate::store::state_machine::memory::state_machine::CacheIncompatibility;
#[cfg(feature = "cache")]
use crate::store::state_machine::memory::{TypeConfigKV, kv_handler::CacheRequestHandler};
#[cfg(feature = "cache")]
use std::sync::OnceLock;
#[cfg(feature = "sqlite")]
use crate::store::state_machine::sqlite::{
    TypeConfigSqlite, state_machine::SqlitePool, writer::WriterRequest,
};
#[cfg(any(feature = "backup", feature = "dashboard"))]
use std::sync::atomic::{AtomicUsize, Ordering};

/// Which raft group a request is about.
///
/// `Unknown` is a **valid value of this type** and it is reachable from outside: the routes are
/// `/{raft_type}` and this enum derives `Deserialize`, so `unknown` in a path deserializes to
/// it. Six helpers used to answer it with `panic!("neither `sqlite` nor `cache` feature
/// enabled")`, a message about a build configuration, for a value that arrives over the
/// network. F-069.
///
/// It is rejected at the boundary instead: [`Self::selected`] turns it into a `BadRequest`, and
/// every handler that takes one calls that before doing anything else. The `Unknown` variant
/// stays, because the type also represents "this build has neither feature", which is a real
/// state and is what the remaining arms describe.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RaftType {
    #[cfg(feature = "sqlite")]
    Sqlite,
    #[cfg(feature = "cache")]
    Cache,
    Unknown,
}

impl RaftType {
    pub fn as_str(&self) -> &str {
        match self {
            #[cfg(feature = "sqlite")]
            RaftType::Sqlite => "sqlite",
            #[cfg(feature = "cache")]
            RaftType::Cache => "cache",
            RaftType::Unknown => "unknown",
        }
    }

    /// `Err` unless this names a raft group this build actually has.
    ///
    /// The message names the values that exist in this build rather than the features that
    /// were not enabled, because the caller is a client and not the person who compiled it.
    pub fn selected(&self) -> Result<&Self, crate::Error> {
        match self {
            RaftType::Unknown => {
                let mut available: Vec<&str> = Vec::new();
                #[cfg(feature = "sqlite")]
                available.push("sqlite");
                #[cfg(feature = "cache")]
                available.push("cache");
                Err(crate::Error::BadRequest(
                    if available.is_empty() {
                        "this node serves no raft group: it was built without both the `sqlite` \
                         and the `cache` feature"
                            .to_string()
                    } else {
                        format!(
                            "unknown raft type; this node serves: {}",
                            available.join(", ")
                        )
                    }
                    .into(),
                ))
            }
            other => Ok(other),
        }
    }
}

// Representation of an application state. This struct can be shared around to share
// instances of raft, store and more.
pub(crate) struct AppState {
    /// Terminal-failure record for this node.
    ///
    /// Set by whatever failed first: the WAL writer thread ending, a listener that stopped
    /// serving, the cache state machine refusing committed work. Once set, readiness is gone
    /// and operations are refused with an account of why. Nothing clears it and nothing
    /// restarts the failed component.
    pub(crate) lifecycle: crate::lifecycle::NodeLifecycle,
    /// Exclusive ownership of `data_dir`.
    ///
    /// `None` only for a node that keeps nothing on disk. Held here rather than in a local so
    /// its lifetime is the node's, and released explicitly by `release_storage_ownership` at
    /// the end of shutdown, once every task and handle that can touch storage has stopped. If
    /// that never runs, dropping this state releases it anyway: there is no cleanup step that
    /// has to succeed.
    pub(crate) storage_ownership: std::sync::Mutex<Option<crate::storage_lock::StorageOwnership>>,
    pub app_start: chrono::DateTime<Utc>,
    pub is_shutting_down: AtomicBool,
    #[cfg(feature = "backup")]
    pub backups_dir: String,
    pub id: NodeId,
    #[cfg(feature = "cache")]
    pub nodes: Vec<crate::Node>,
    pub addr_api: String,
    #[cfg(feature = "sqlite")]
    pub raft_db: StateRaftDB,
    #[cfg(feature = "cache")]
    pub raft_cache: StateRaftCache,
    pub raft_lock: Arc<Mutex<()>>,
    #[cfg(feature = "s3")]
    pub s3_config: Option<Arc<S3Config>>,
    pub secret_raft: String,
    pub secret_api: String,
    #[cfg(feature = "dashboard")]
    pub dashboard: DashboardState,
    #[cfg(any(feature = "backup", feature = "dashboard"))]
    pub client_request_id: AtomicUsize,
    #[cfg(any(feature = "backup", feature = "dashboard"))]
    pub tx_client_stream: flume::Sender<ClientStreamReq>,
    pub health_check_delay_secs: u32,
    pub learner_only: bool,
}

#[cfg(any(feature = "backup", feature = "dashboard"))]
impl AppState {
    #[inline(always)]
    pub fn new_request_id(&self) -> usize {
        self.client_request_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl AppState {
    /// Give up exclusive ownership of the data directory.
    ///
    /// Called at the end of shutdown, **after** the raft groups, the WAL writer and the SQLite
    /// writer have all stopped, which is the point at which nothing in this process can still
    /// touch the storage. Doing it earlier would let a second node take the directory while
    /// this one was still writing to it; doing it only on drop would keep a restarted node in
    /// the same process out until the last `Arc` happened to go away.
    pub(crate) fn release_storage_ownership(&self) {
        let taken = self
            .storage_ownership
            .lock()
            .map(|mut guard| guard.take())
            .unwrap_or(None);
        if taken.is_some() {
            tracing::info!("Exclusive storage ownership released");
        }
    }
}

#[cfg(feature = "sqlite")]
pub struct StateRaftDB {
    pub raft: openraft::Raft<TypeConfigSqlite>,
    pub shutdown_handle: hiqlite_wal::ShutdownHandle,
    pub sql_writer: flume::Sender<WriterRequest>,
    pub read_pool: SqlitePool,
    pub log_statements: bool,
    pub is_raft_stopped: Arc<AtomicBool>,
    pub is_startup_finished: Arc<AtomicBool>,
}

#[cfg(feature = "cache")]
pub struct StateRaftCache {
    pub raft: openraft::Raft<TypeConfigKV>,
    pub tx_caches: Vec<flume::Sender<CacheRequestHandler>>,
    /// Set once, and never cleared, when a replicated cache command could not be applied on
    /// this node. While it is set the cache Raft group has stopped applying committed work, so
    /// every cache read and write on this node refuses instead of answering from state that is
    /// known to be behind.
    pub cache_incompatible: Arc<OnceLock<CacheIncompatibility>>,
    #[cfg(feature = "listen_notify")]
    pub tx_notify: flume::Sender<NotifyRequest>,
    #[cfg(feature = "listen_notify_local")]
    pub rx_notify: flume::Receiver<(i64, Vec<u8>)>,
    #[cfg(feature = "dlock")]
    pub tx_dlock: flume::Sender<LockRequest>,
    pub is_raft_stopped: Arc<AtomicBool>,
    pub is_startup_finished: Arc<AtomicBool>,
    pub shutdown_handle: Option<hiqlite_wal::ShutdownHandle>,
    #[cfg(feature = "cache")]
    pub cache_storage_disk: bool,
}

#[cfg(feature = "cache")]
impl StateRaftCache {
    /// Refuse every cache operation once a replicated cache command could not be applied.
    ///
    /// This is the read half of the `apply` guard. Stopping application is what keeps this
    /// node from executing work it does not understand; without this check the node would go
    /// on answering cache reads from a state machine that stopped advancing, which is a
    /// misleading read rather than a visible failure. It is terminal on purpose: the offending
    /// entry is committed, so a restart replays it.
    pub fn ensure_cache_compatible(&self) -> Result<(), crate::Error> {
        match self.cache_incompatible.get() {
            None => Ok(()),
            Some(incompat) => Err(crate::Error::CacheIncompatible(incompat.message().into())),
        }
    }
}
