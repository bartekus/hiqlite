//! The real-node N=3 harness (spec 033 B-2). See README.md.
//!
//! Exit codes: `0` every run passed; `1` a run failed (its directory is kept); `2` the
//! invocation was refused before any run (usage, an unimplemented scenario, a missing binary,
//! a port in use); `3` the invocation bound was reached before the run count, which is not a
//! pass; `130` interrupted.

mod cli;
// The fault primitives are parsed for every run and applied by fault-accepting scenarios; the
// only implemented scenario, `smoke`, accepts none, so `apply` has no caller yet.
#[allow(dead_code)]
mod fault;
mod process;
mod proxy;
mod report;
mod rng;
mod run;
mod scenario;
mod topology;

use clap::Parser;
use cli::{Cli, Cmd, RunArgs};
use report::{InvocationReport, RunReport, RunSummary};
use run::RunCtx;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use topology::{ClusterSpec, Topology};

fn main() {
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::List => {
            for s in scenario::REGISTRY {
                let state = if s.implemented {
                    "implemented"
                } else {
                    "not implemented (refuses to run)"
                };
                println!("{:<40} {state}\n    {}", s.name, s.summary);
            }
            0
        }
        Cmd::Run(args) => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(invoke(*args))
        }
    };
    std::process::exit(code);
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn git(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(cli::n3_dir())
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn write_json<T: serde::Serialize>(p: &Path, v: &T) {
    match serde_json::to_vec_pretty(v) {
        Ok(b) => {
            if let Err(e) = std::fs::write(p, b) {
                eprintln!("writing {p:?}: {e}");
            }
        }
        Err(e) => eprintln!("serializing {p:?}: {e}"),
    }
}

fn refuse(msg: String) -> i32 {
    eprintln!("refused: {msg}");
    2
}

async fn invoke(args: RunArgs) -> i32 {
    let t0 = Instant::now();
    let started_unix_ms = unix_ms();

    // Refusals before anything is created.
    let Some(def) = scenario::find(&args.scenario) else {
        return refuse(format!("unknown scenario `{}`; see `list`", args.scenario));
    };
    if !def.implemented {
        return refuse(format!(
            "scenario `{}` is registered but not implemented. Harness construction (lane D) \
             implements no A-scenario, and no harness run counts as qualification.",
            def.name
        ));
    }
    if !def.accepts_fault && !args.fault.is_none() {
        return refuse(format!(
            "scenario `{}` applies no fault spec; got `{}`",
            def.name, args.fault
        ));
    }
    if let Err(e) = args.validate_log_sync() {
        return refuse(e);
    }
    if args.runs == 0 {
        return refuse("--runs must be at least 1".into());
    }
    if args.clusters == 0 {
        return refuse("--clusters must be at least 1".into());
    }
    let clusters: Vec<ClusterSpec> = (0..args.clusters)
        .map(|c| ClusterSpec {
            name: format!("c{}", c + 1),
            feature_set: if args.feature_set.len() == 1 {
                args.feature_set[0]
            } else {
                args.feature_set
                    .get(c)
                    .copied()
                    .unwrap_or(args.feature_set[0])
            },
        })
        .collect();
    if args.feature_set.len() > 1 && args.feature_set.len() != args.clusters {
        return refuse(format!(
            "--feature-set names {} sets for {} clusters",
            args.feature_set.len(),
            args.clusters
        ));
    }
    let mut bins: Vec<String> = Vec::new();
    for c in &clusters {
        let bin = args.node_bin(c.feature_set);
        if !bin.is_file() {
            return refuse(format!(
                "node binary for `{}` not found at {bin:?}; build it (README.md)",
                c.feature_set
            ));
        }
        let s = bin.to_string_lossy().into_owned();
        if !bins.contains(&s) {
            bins.push(s);
        }
    }
    let topo = match Topology::build(args.layout, &clusters, args.base_port) {
        Ok(t) => t,
        Err(e) => return refuse(e),
    };
    if let Err(e) = topology::check_ports_free(&topo.all_ports()) {
        return refuse(e);
    }

    let run_bound = Duration::from_secs(args.run_bound_secs);
    let total_bound = Duration::from_secs(
        args.max_total_secs
            .unwrap_or(args.run_bound_secs * args.runs as u64 + 30),
    );
    let root = args
        .root
        .clone()
        .unwrap_or_else(|| cli::n3_dir().join("runs"));
    let inv_dir: PathBuf = root.join(format!(
        "{started_unix_ms}-{}-{}-s{}",
        args.scenario, args.layout, args.seed
    ));
    if let Err(e) = std::fs::create_dir_all(&inv_dir) {
        return refuse(format!("{inv_dir:?}: {e}"));
    }
    println!(
        "n3-harness: scenario {} layout {} clusters {:?} log_sync {} seed {} runs {} \
         (run bound {:?}, invocation bound {:?})\n  dir {}\n  NOTE: harness runs are not \
         qualification and count toward no A-scenario.",
        args.scenario,
        args.layout,
        clusters
            .iter()
            .map(|c| format!("{}={}", c.name, c.feature_set))
            .collect::<Vec<_>>(),
        args.log_sync,
        args.seed,
        args.runs,
        run_bound,
        total_bound,
        inv_dir.display()
    );

    let mut inv = InvocationReport {
        harness: "qualification/n3 n3-harness",
        counts_as_qualification: false,
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        git_head: git(&["rev-parse", "HEAD"]),
        git_dirty: git(&["status", "--porcelain"]).map(|s| !s.is_empty()),
        started_unix_ms,
        scenario: args.scenario.clone(),
        layout: args.layout,
        clusters: clusters.clone(),
        log_sync: args.log_sync.clone(),
        base_port: args.base_port,
        seed: args.seed,
        fault: args.fault.to_string(),
        inject_failure: args.inject_failure.map(|i| i.name().to_string()),
        runs_requested: args.runs,
        run_bound_ms: run_bound.as_millis() as u64,
        invocation_bound_ms: total_bound.as_millis() as u64,
        node_binaries: bins,
        runs: Vec::new(),
        result: "passed".into(),
        exit_code: 0,
        wall_ms: 0,
        dir: inv_dir.to_string_lossy().into_owned(),
    };

    for index in 1..=args.runs {
        let remaining = total_bound.saturating_sub(t0.elapsed());
        if remaining.is_zero() {
            inv.result = "bound_reached".into();
            inv.exit_code = 3;
            break;
        }
        let bound = run_bound.min(remaining);
        let run_dir = inv_dir.join(format!("run-{index}"));
        // Each run's seed is derived from the invocation seed and its index.
        let seed = args.seed.wrapping_mul(1_000_003).wrapping_add(index as u64);
        println!("--- run {index}/{} (bound {bound:?}) ---", args.runs);
        let (report, interrupted) =
            one_run(&args, topo.clone(), run_dir.clone(), index, seed, bound).await;
        let report_path = run_dir.join("report.json");
        write_json(&report_path, &report);
        let passed = report.passed;
        println!(
            "--- run {index}: {} in {} ms{} ---",
            if passed { "PASS" } else { "FAIL" },
            report.wall_ms,
            report
                .failure
                .as_ref()
                .map(|f| format!(": {f}"))
                .unwrap_or_default()
        );
        inv.runs.push(RunSummary {
            index,
            passed,
            wall_ms: report.wall_ms,
            failure: report.failure.clone(),
            report: report_path.to_string_lossy().into_owned(),
        });
        if interrupted {
            inv.result = "interrupted".into();
            inv.exit_code = 130;
            break;
        }
        if !passed {
            inv.result = "failed".into();
            inv.exit_code = 1;
            println!("stopping at the first failure; kept {}", run_dir.display());
            break;
        }
        if args.discard_passing {
            let _ = std::fs::remove_dir_all(&run_dir);
        }
    }
    if inv.exit_code == 0 && (inv.runs.len() as u32) < args.runs {
        inv.result = "bound_reached".into();
        inv.exit_code = 3;
    }
    inv.wall_ms = t0.elapsed().as_millis() as u64;
    let inv_report = inv_dir.join("report.json");
    write_json(&inv_report, &inv);
    println!(
        "n3-harness: {} ({} of {} runs) in {} ms; report {}",
        inv.result,
        inv.runs.iter().filter(|r| r.passed).count(),
        args.runs,
        inv.wall_ms,
        inv_report.display()
    );
    inv.exit_code
}

async fn one_run(
    args: &RunArgs,
    topo: Topology,
    run_dir: PathBuf,
    index: u32,
    seed: u64,
    bound: Duration,
) -> (RunReport, bool) {
    let t0 = Instant::now();
    let mut interrupted = false;
    let mut ctx = match RunCtx::new(args, topo.clone(), run_dir.clone(), seed).await {
        Ok(c) => c,
        Err(e) => {
            return (
                RunReport {
                    scenario: args.scenario.clone(),
                    run_index: index,
                    run_dir: run_dir.to_string_lossy().into_owned(),
                    passed: false,
                    failure: Some(format!("setup: {e}")),
                    wall_ms: t0.elapsed().as_millis() as u64,
                    bound_ms: bound.as_millis() as u64,
                    layout: args.layout,
                    clusters: topo.clusters.clone(),
                    log_sync: args.log_sync.clone(),
                    base_port: args.base_port,
                    seed,
                    fault: args.fault.to_string(),
                    inject_failure: args.inject_failure.map(|i| i.name().to_string()),
                    events: vec![],
                    stops: vec![],
                    nodes: vec![],
                    proxy: vec![],
                    proxy_rules_at_end: Default::default(),
                },
                false,
            );
        }
    };

    let result = tokio::select! {
        r = tokio::time::timeout(bound, scenario::run(&args.scenario, &mut ctx)) => match r {
            Ok(r) => r,
            Err(_) => Err(format!("the run did not finish within its bound of {bound:?}")),
        },
        _ = tokio::signal::ctrl_c() => {
            interrupted = true;
            Err("interrupted".to_string())
        }
    };
    if let Err(e) = &result {
        ctx.event(format!("FAILED: {e}"));
    }
    let rules_at_end = ctx.proxy.rules();
    ctx.cleanup().await;

    let report = RunReport {
        scenario: args.scenario.clone(),
        run_index: index,
        run_dir: run_dir.to_string_lossy().into_owned(),
        passed: result.is_ok(),
        failure: result.err(),
        wall_ms: t0.elapsed().as_millis() as u64,
        bound_ms: bound.as_millis() as u64,
        layout: args.layout,
        clusters: topo.clusters.clone(),
        log_sync: args.log_sync.clone(),
        base_port: args.base_port,
        seed,
        fault: args.fault.to_string(),
        inject_failure: args.inject_failure.map(|i| i.name().to_string()),
        events: ctx.events.clone(),
        stops: ctx.stops.clone(),
        nodes: ctx.node_reports(),
        proxy: ctx.proxy.report(),
        proxy_rules_at_end: rules_at_end,
    };
    (report, interrupted)
}
