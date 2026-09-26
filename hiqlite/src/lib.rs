// Copyright 2026 Sebastian Dobe <sebastiandobe@mailbox.org>

#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![cfg_attr(doc, feature(doc_cfg))]

#[cfg(all(feature = "cast_ints", feature = "cast_ints_unchecked"))]
compile_error!("features `cast_ints` and `cast_ints_unchecked` are mutually exclusive!");

#[cfg(all(
    feature = "jemalloc",
    not(target_env = "msvc"),
    not(feature = "__profiling")
))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(any(feature = "sqlite", feature = "cache"))]
pub use hiqlite_wal::LogSync;
#[cfg(any(feature = "sqlite", feature = "cache"))]
pub use openraft::SnapshotPolicy;
#[cfg(any(feature = "sqlite", feature = "cache"))]
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "sqlite", feature = "cache"))]
use std::fmt::{Debug, Display};

#[cfg(feature = "sqlite")]
use crate::store::state_machine::sqlite::state_machine::Response;
#[cfg(any(feature = "sqlite", feature = "cache"))]
pub use crate::{client::Client, error::Error};
#[cfg(any(feature = "sqlite", feature = "cache"))]
pub use config::{NodeConfig, RaftConfig, RateLimitConfig};
#[cfg(feature = "sqlite")]
pub use query::cust_types::VecText;

#[cfg(feature = "sqlite")]
pub use crate::query::rows::Row;
#[cfg(feature = "sqlite")]
pub use crate::store::state_machine::sqlite::{
    param::Param,
    state_machine::Params,
    transaction_variable::{StmtColumn, StmtIndex},
};
#[cfg(feature = "dlock")]
pub use client::dlock::Lock;
#[cfg(feature = "sqlite")]
pub use migration::AppliedMigration;

/// Re-export of the exact `rusqlite` version Hiqlite is built with.
///
/// Use this instead of adding a separate `rusqlite` dependency to avoid
/// version conflicts, e.g. when implementing a
/// [`DeterministicSqliteOperation`](external_state_machine::DeterministicSqliteOperation)
/// against the [`Transaction`](rusqlite::Transaction) type.
#[cfg(any(feature = "sqlite", feature = "external-state-machine"))]
pub use rusqlite;

/// SQLite state-machine machinery for applications that already own consensus.
///
/// This module does not start a Hiqlite Raft group or network service.
#[cfg(feature = "external-state-machine")]
pub mod external_state_machine;

#[cfg(any(feature = "sqlite", feature = "cache"))]
mod app_state;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod client;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod config;
#[cfg(all(any(feature = "sqlite", feature = "cache"), feature = "toml"))]
mod config_toml;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod error;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod helpers;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod init;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod network;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod start;

#[cfg(any(feature = "sqlite", feature = "cache"))]
pub mod lifecycle;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod membership_gate;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod recovery;
#[cfg(any(feature = "sqlite", feature = "cache"))]
pub use recovery::{RecoveryProgress, RecoveryState};

/// Entry points for the abort-profile probe binary.
///
/// Not part of the supported API: it exists so `hiqlite-abort-probe` can call the expected
/// failure paths from outside the crate, under a profile no test can run in.
#[cfg(feature = "__abort-probe")]
#[doc(hidden)]
pub mod probe {
    pub fn split_brain_interval(raw: &str) -> Result<std::time::Duration, crate::Error> {
        crate::split_brain_check::split_brain_interval_from(Some(raw))
    }

    #[cfg(feature = "s3")]
    pub fn s3_config_from(
        pairs: &[(&str, &str)],
    ) -> Result<Option<std::sync::Arc<crate::s3::S3Config>>, crate::Error> {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        crate::s3::S3Config::from_lookup(&move |name| map.get(name).cloned())
    }

    pub async fn bind(addr: &str) -> Result<std::net::TcpListener, crate::Error> {
        crate::start::bind_listener(addr, "the probe endpoint").await
    }

    pub fn take_storage_ownership(dir: &str) -> Result<impl Sized, crate::Error> {
        crate::storage_lock::StorageOwnership::acquire(dir)
    }
}

#[cfg(any(feature = "sqlite", feature = "cache"))]
mod storage_lock;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod store;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod upgrade_exclusion;
/// The environment variable that moves a hiqlite 0.14.x cache raft log aside on the one start
/// that upgrades a data directory. See the consumer handoff.
#[cfg(feature = "cache")]
pub use store::logs::CACHE_LEGACY_MOVE_ASIDE_ENV;

#[cfg(feature = "backup")]
mod backup;
#[cfg(feature = "dashboard")]
mod dashboard;
#[cfg(feature = "sqlite")]
mod migration;
#[cfg(feature = "sqlite")]
mod query;
#[cfg(any(feature = "sqlite", feature = "cache"))]
mod split_brain_check;

#[cfg(feature = "macros")]
pub mod macros;

/// Exports and types to set up a connection to an S3 storage bucket.
/// Needs the feature `s3` enabled.
#[cfg(feature = "s3")]
pub mod s3;

/// Contains everything to start the server binary.
/// Changes inside this module are not considered breaking changes.
/// They should only be used internally to compile the standalone binary.
#[cfg(feature = "server")]
pub mod server;

#[cfg(any(feature = "sqlite", feature = "cache"))]
mod http_client;
#[cfg(any(feature = "sqlite", feature = "cache"))]
pub mod tls;

#[cfg(any(feature = "sqlite", feature = "cache"))]
type NodeId = u64;

#[cfg(any(feature = "sqlite", feature = "cache"))]
pub trait CacheVariants {
    /// Returns the Enum Variants index, strictly matching the output of `hiqlite_cache_variants()`.
    fn hiqlite_cache_index(&self) -> usize;

    /// Returns the Enum Variants as `(idx, name)` in strictly ascending order, starting at `0`.
    fn hiqlite_cache_variants() -> &'static [(usize, &'static str)];
}

/// A Raft / Hiqlite node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg(any(feature = "sqlite", feature = "cache"))]
pub struct Node {
    /// Each Raft config must include one Node with `id == 1`.
    /// Node `1` will care about init and setup if the Raft does not exit yet or
    /// if other Nodes need to join.
    pub id: NodeId,
    /// The Raft internal address. This is separated from the API address and runs on
    /// a different server and port to make it possible to boost security and split
    /// network bandwidth. The internal Raft API should never be exposed to the public.
    pub addr_raft: String,
    /// The public API address. To this address, the `Client`s will connect to talk
    /// to other Raft Leader nodes over the network if necessary.
    pub addr_api: String,
}

#[cfg(any(feature = "sqlite", feature = "cache"))]
impl Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Node {{ id: {}, rpc_addr: {}, api_addr: {} }}",
            self.id, self.addr_raft, self.addr_api
        )
    }
}

#[cfg(feature = "sqlite")]
mod empty {
    use crate::CacheVariants;

    #[derive(Debug)]
    pub enum Empty {}

    impl CacheVariants for Empty {
        fn hiqlite_cache_index(&self) -> usize {
            unreachable!()
        }

        fn hiqlite_cache_variants() -> &'static [(usize, &'static str)] {
            &[]
        }
    }
}

/// The main entry function to start a Raft / Hiqlite node.
/// # Panics
/// If an incorrect `node_config` was given.
#[cfg(feature = "sqlite")]
pub async fn start_node(node_config: NodeConfig) -> Result<Client, Error> {
    start::start_node_inner::<empty::Empty>(Box::new(node_config)).await
}

/// The main entry function to start a Raft / Hiqlite node.
/// With the `cache` feature enabled, you need to provide the generic enum which
/// will function as the Cache Index value to decide between multiple caches.
/// # Panics
/// If an incorrect `node_config` was given.
#[cfg(feature = "cache")]
pub async fn start_node_with_cache<C>(node_config: NodeConfig) -> Result<Client, Error>
where
    C: Debug + CacheVariants,
{
    start::start_node_inner::<C>(Box::new(node_config)).await
}

/// The root for a lib test's scratch directories, one per test process.
///
/// Fixed paths under `../target/test_data` were shared by every process running these tests
/// from the same checkout, so two concurrent runs (a suite and an acceptance block, say)
/// deleted and locked each other's directories (F-139, F-140). A child process a test spawns
/// is handed its directory explicitly, so it does not need to derive the same root.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn test_scratch_root() -> String {
    format!("../target/test_data/{}", std::process::id())
}
