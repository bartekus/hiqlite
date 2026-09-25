//! Command line.

use crate::fault::FaultSpec;
use crate::topology::{FeatureSet, Layout};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// The qualification/n3 directory, resolved at compile time from this crate's manifest.
pub fn n3_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("harness crate lives inside qualification/n3")
        .to_path_buf()
}

#[derive(Debug, Parser)]
#[command(
    name = "n3-harness",
    about = "The real-node N=3 harness of spec 033 B-2. Harness runs are not qualification."
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// List the scenario registry.
    List,
    /// Run one scenario a fixed number of times, stopping at the first failure.
    Run(Box<RunArgs>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InjectFailure {
    /// SIGKILL the first pod's nodes after the writes, while the scenario still expects
    /// them alive. The harness must detect the exit and fail the run.
    KillNode,
    /// After the restart, assert that a key that was never written is present.
    ImpossibleAssert,
}

impl InjectFailure {
    pub fn name(&self) -> &'static str {
        match self {
            InjectFailure::KillNode => "kill-node",
            InjectFailure::ImpossibleAssert => "impossible-assert",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    /// Scenario name, see `list`.
    #[arg(long)]
    pub scenario: String,
    #[arg(long, value_enum, default_value_t = Layout::Split)]
    pub layout: Layout,
    /// Number of clusters (each three voters). 033 D-4: two, one per consumer StatefulSet.
    #[arg(long, default_value_t = 2)]
    pub clusters: usize,
    /// Feature set per cluster, comma-separated; one value applies to every cluster.
    #[arg(long, value_enum, value_delimiter = ',', default_value = "rahi")]
    pub feature_set: Vec<FeatureSet>,
    /// `immediate`, `immediate_async` or `interval_<ms>` (hiqlite `LogSync`).
    #[arg(long, default_value = "immediate")]
    pub log_sync: String,
    #[arg(long, default_value_t = 29100)]
    pub base_port: u16,
    #[arg(long, default_value_t = 1)]
    pub seed: u64,
    /// Fault spec: `none`, or comma-separated steps `sigkill:<pod>`, `sigterm:<pod>`,
    /// `isolate:<pod>`, `link-down:<pod>:<pod>`, `heal`. A scenario states which it accepts.
    #[arg(long, default_value = "none")]
    pub fault: FaultSpec,
    /// Fixed run count. The invocation stops at the first failed run.
    #[arg(long, default_value_t = 1)]
    pub runs: u32,
    /// Wall-clock bound of one run.
    #[arg(long, default_value_t = 300)]
    pub run_bound_secs: u64,
    /// Wall-clock bound of the whole invocation. Default: runs x run bound + 30 s.
    #[arg(long)]
    pub max_total_secs: Option<u64>,
    /// Bound of each wait for cluster formation or convergence.
    #[arg(long, default_value_t = 120)]
    pub ready_bound_secs: u64,
    /// Bound of each control request (a write or a read through a node).
    #[arg(long, default_value_t = 30)]
    pub op_bound_secs: u64,
    /// After SIGTERM, how long the harness waits before SIGKILL (a pod's termination grace).
    #[arg(long, default_value_t = 30)]
    pub term_grace_secs: u64,
    /// Poll interval inside a bounded wait.
    #[arg(long, default_value_t = 100)]
    pub poll_ms: u64,
    /// Status-file interval of every node.
    #[arg(long, default_value_t = 250)]
    pub status_interval_ms: u64,
    /// Root under which each invocation gets a private directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Delete a passing run's directory after its report is written. A failing run's
    /// directory is always kept.
    #[arg(long, default_value_t = false)]
    pub discard_passing: bool,
    #[arg(long, value_enum)]
    pub inject_failure: Option<InjectFailure>,
    #[arg(long)]
    pub node_bin_rahi: Option<PathBuf>,
    #[arg(long)]
    pub node_bin_rauthy: Option<PathBuf>,
    /// `RUST_LOG` for every node.
    #[arg(long, default_value = "info")]
    pub node_log: String,
}

impl RunArgs {
    pub fn node_bin(&self, fs: FeatureSet) -> PathBuf {
        let explicit = match fs {
            FeatureSet::Rahi => &self.node_bin_rahi,
            FeatureSet::Rauthy => &self.node_bin_rauthy,
        };
        explicit.clone().unwrap_or_else(|| {
            n3_dir()
                .join("target")
                .join(fs.to_string())
                .join("release")
                .join("n3-node")
        })
    }

    pub fn validate_log_sync(&self) -> Result<(), String> {
        match self.log_sync.as_str() {
            "immediate" | "immediate_async" => Ok(()),
            v => match v.strip_prefix("interval_").map(|ms| ms.parse::<u64>()) {
                Some(Ok(ms)) if ms > 0 => Ok(()),
                _ => Err(format!(
                    "--log-sync `{v}`: expected immediate, immediate_async or interval_<ms>"
                )),
            },
        }
    }
}
