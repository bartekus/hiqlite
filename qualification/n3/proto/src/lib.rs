//! The formats the N=3 harness and its node binary exchange.
//!
//! Three channels, each chosen for being the simplest one that survives the process it
//! describes:
//!
//! - **Launch file** (`launch.json`, harness to node): everything a node needs to start. The
//!   node reads it once.
//! - **Status file** (`status.json`, node to harness): rewritten atomically (write to a
//!   temporary name, then `rename`) at a fixed interval while the node lives. It carries the
//!   raft metrics of both groups, so the harness synchronizes on observed state and the last
//!   state a node reported survives the node.
//! - **Shutdown record** (`shutdown.json`, node to harness): written once, after the node's
//!   `Client::shutdown()` returned, before the process exits. Its absence after an exit is
//!   itself an outcome (the process ended before the shutdown returned).
//! - **Control socket** (TCP, line-delimited JSON on `127.0.0.1`, not proxied): the harness's
//!   writes and reads through a node's own `Client`.

use serde::{Deserialize, Serialize};

pub const LAUNCH_FILE: &str = "launch.json";
pub const STATUS_FILE: &str = "status.json";
pub const SHUTDOWN_FILE: &str = "shutdown.json";

/// The SQL table the harness writes through the durable group.
pub const KV_TABLE: &str = "n3_kv";

/// One peer as every member of a cluster sees it: the advertised addresses, which are the
/// harness proxy's listeners.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Peer {
    pub id: u64,
    pub addr_raft: String,
    pub addr_api: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Launch {
    /// The cluster name, for logs and reports only.
    pub cluster: String,
    pub node_id: u64,
    pub nodes: Vec<Peer>,
    /// The host both listeners bind to. The port comes from this node's own advertised
    /// address (hiqlite's `build_listen_addr`), so the harness proxy listens on a different
    /// loopback address with the same port.
    pub listen_addr: String,
    pub data_dir: String,
    /// `immediate`, `immediate_async` or `interval_<ms>`, parsed by `hiqlite::LogSync`.
    pub log_sync: String,
    pub cache_storage_disk: bool,
    pub secret_raft: String,
    pub secret_api: String,
    /// 32 bytes, hex, used for the `s3` and `dashboard` features' mandatory encryption key.
    pub enc_key_hex: String,
    /// `127.0.0.1:<port>`, not proxied.
    pub ctl_addr: String,
    pub status_path: String,
    pub shutdown_path: String,
    pub status_interval_ms: u64,
    /// The feature set the harness expects this binary to have been built with. A node built
    /// with another refuses to start, so a wrong binary is an error and not a silent swap.
    pub feature_set: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Running,
    ShuttingDown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogIdView {
    /// openraft's `Display` of the leader id the log id was committed under.
    pub leader: String,
    pub index: u64,
}

/// One raft group's metrics, as the node's own `RaftMetrics` reported them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupStatus {
    pub server_state: String,
    pub current_term: u64,
    pub current_leader: Option<u64>,
    pub last_log_index: Option<u64>,
    pub last_applied: Option<LogIdView>,
    pub membership_log_id: Option<LogIdView>,
    pub voters: Vec<u64>,
    pub learners: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    /// Increments with every write of the status file.
    pub seq: u64,
    pub pid: u32,
    pub unix_ms: u64,
    pub cluster: String,
    pub node_id: u64,
    pub feature_set: String,
    pub phase: Phase,
    /// `None` while the node has not started, or when the metrics read failed.
    pub db: Option<GroupStatus>,
    pub cache: Option<GroupStatus>,
    /// `Client::node_failure()`, when the node reports a terminal failure.
    pub node_failure: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownResult {
    /// `Client::shutdown()` returned `Ok(())`: confirmed graceful completion.
    Ok,
    /// `Client::shutdown()` returned `Err(Error::Timeout)`: the harness's wait ended, the
    /// sequence possibly still running. Unconfirmed.
    Timeout,
    /// Any other error from `Client::shutdown()`.
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShutdownRecord {
    pub result: ShutdownResult,
    pub detail: Option<String>,
    /// From the node's receipt of `SIGTERM` to `shutdown()` returning, measured by the node.
    pub node_measured_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Db,
    Cache,
}

/// One control request, one line of JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CtlRequest {
    /// Creates the harness table through the durable group. Idempotent.
    Schema,
    Write {
        group: Group,
        key: String,
        value: String,
    },
    /// A durable-group read is `query_consistent` (leader, quorum); a cache read is the
    /// node's local `get`.
    Read { group: Group, key: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtlResponse {
    pub ok: bool,
    pub value: Option<String>,
    pub error: Option<String>,
}

impl CtlResponse {
    pub fn ok(value: Option<String>) -> Self {
        Self {
            ok: true,
            value,
            error: None,
        }
    }

    pub fn err(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            value: None,
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctl_request_round_trips_as_one_line() {
        let req = CtlRequest::Write {
            group: Group::Cache,
            key: "k".into(),
            value: "v".into(),
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(!line.contains('\n'));
        assert_eq!(
            line,
            r#"{"op":"write","group":"cache","key":"k","value":"v"}"#
        );
        let back: CtlRequest = serde_json::from_str(&line).unwrap();
        assert!(matches!(
            back,
            CtlRequest::Write {
                group: Group::Cache,
                ..
            }
        ));
    }

    #[test]
    fn shutdown_result_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&ShutdownResult::Timeout).unwrap(),
            "\"timeout\""
        );
    }
}
