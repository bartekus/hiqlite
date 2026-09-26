//! The machine-readable reports. One `report.json` per run directory and one per invocation.

use crate::process::ExitInfo;
use crate::proxy::{EndpointReport, Rules};
use crate::topology::{ClusterSpec, Layout};
use n3_proto::{NodeStatus, ShutdownRecord};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopOutcome {
    /// `Client::shutdown()` returned `Ok(())` and the process exited within the grace.
    ConfirmedOk,
    /// `Client::shutdown()` returned `Err(Timeout)`: the process exited with the sequence
    /// possibly still running. Unconfirmed.
    UnconfirmedTimeout,
    /// `Client::shutdown()` returned another error.
    ShutdownError,
    /// A deliberate `SIGKILL` by the harness (a fault, an injection, or cleanup).
    Killed,
    /// The process exited within the grace without writing its shutdown record: it ended
    /// before `shutdown()` returned (a crash, an abort, or an exit elsewhere).
    ExitedWithoutRecord,
    /// The grace elapsed and the harness sent `SIGKILL`: a confirmed forced exit before
    /// completion.
    ForcedKill,
    /// Not reaped even after `SIGKILL` within the reap bound.
    NotReaped,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeStop {
    pub node: String,
    pub pod: String,
    pub signal: &'static str,
    /// From the harness's signal to the harness observing the exit (poll resolution is the
    /// harness's poll interval).
    pub duration_ms: Option<u64>,
    pub outcome: StopOutcome,
    pub exit: Option<ExitInfo>,
    pub record: Option<ShutdownRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PodStop {
    pub pod: String,
    /// The longest of the pod's nodes: a pod is down when its last node is.
    pub duration_ms: Option<u64>,
    /// The worst of the pod's nodes' outcomes.
    pub outcome: StopOutcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct StopEvent {
    pub label: String,
    pub at_ms: u64,
    pub nodes: Vec<NodeStop>,
    pub pods: Vec<PodStop>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub at_ms: u64,
    pub what: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeReport {
    pub name: String,
    pub cluster: String,
    pub node_id: u64,
    pub pod: String,
    pub incarnations: u32,
    pub running_at_end: bool,
    pub last_exit: Option<ExitInfo>,
    /// The last status file, whichever incarnation wrote it: membership (voters, learners,
    /// membership log id), applied and last log positions for both groups.
    pub last_status: Option<NodeStatus>,
    pub shutdown_record: Option<ShutdownRecord>,
    pub dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub scenario: String,
    pub run_index: u32,
    pub run_dir: String,
    pub passed: bool,
    pub failure: Option<String>,
    pub wall_ms: u64,
    pub bound_ms: u64,
    pub layout: Layout,
    pub clusters: Vec<ClusterSpec>,
    pub log_sync: String,
    pub base_port: u16,
    pub seed: u64,
    pub fault: String,
    pub inject_failure: Option<String>,
    pub events: Vec<Event>,
    pub stops: Vec<StopEvent>,
    pub nodes: Vec<NodeReport>,
    pub proxy: Vec<EndpointReport>,
    pub proxy_rules_at_end: Rules,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub index: u32,
    pub passed: bool,
    pub wall_ms: u64,
    pub failure: Option<String>,
    pub report: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InvocationReport {
    pub harness: &'static str,
    /// Always false: a smoke or harness run is not an A-scenario pass and not a support claim.
    pub counts_as_qualification: bool,
    pub platform: String,
    pub git_head: Option<String>,
    pub git_dirty: Option<bool>,
    pub started_unix_ms: u64,
    pub scenario: String,
    pub layout: Layout,
    pub clusters: Vec<ClusterSpec>,
    pub log_sync: String,
    pub base_port: u16,
    pub seed: u64,
    pub fault: String,
    pub inject_failure: Option<String>,
    pub runs_requested: u32,
    pub run_bound_ms: u64,
    pub invocation_bound_ms: u64,
    pub node_binaries: Vec<String>,
    pub runs: Vec<RunSummary>,
    /// `passed`, `failed`, `bound_reached` or `interrupted`.
    pub result: String,
    pub exit_code: i32,
    pub wall_ms: u64,
    pub dir: String,
}
