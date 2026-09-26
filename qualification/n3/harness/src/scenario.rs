//! The scenario registry.
//!
//! `smoke` is the only implemented scenario. It validates the harness; it is not an
//! A-scenario and a pass counts toward nothing in 033 B-6. The A-scenario names are registered
//! so their slots exist and refuse to run: implementing them, and running them as
//! qualification, is lane E's work under its own authorization.

use crate::cli::InjectFailure;
use crate::report::StopOutcome;
use crate::run::{Formation, RunCtx, StopSignal, probe_converged, probe_formed};
use n3_proto::{CtlRequest, Group};
use std::time::Duration;

pub struct ScenarioDef {
    pub name: &'static str,
    pub summary: &'static str,
    pub implemented: bool,
    /// Whether the scenario applies a `--fault` spec. A scenario that does not refuses any
    /// spec other than `none`.
    pub accepts_fault: bool,
}

pub const REGISTRY: &[ScenarioDef] = &[
    ScenarioDef {
        name: "smoke",
        summary: "Harness validation only, not an A-scenario: start every cluster, write through \
                  each group via each node, verify, SIGTERM every node at once, restart on the \
                  same data, verify membership and data, SIGTERM again.",
        implemented: true,
        accepts_fault: false,
    },
    ScenarioDef {
        name: "a1-bootstrap",
        summary: "033 B-6 A-1: bootstrap in every start order; negative case for node 1 (B-3).",
        implemented: false,
        accepts_fault: true,
    },
    ScenarioDef {
        name: "a2-immediate-restart",
        summary: "033 B-6 A-2: each node restarted and written to the moment start_node returns.",
        implemented: false,
        accepts_fault: true,
    },
    ScenarioDef {
        name: "a3-leader-loss",
        summary: "033 B-6 A-3: SIGKILL of each group's leader under write load.",
        implemented: false,
        accepts_fault: true,
    },
    ScenarioDef {
        name: "a4-partition-rejoin",
        summary: "033 B-6 A-4: leader isolated by the proxy; minority refuses, majority serves, \
                  heal converges.",
        implemented: false,
        accepts_fault: true,
    },
    ScenarioDef {
        name: "a5-lagging-rejoin",
        summary: "033 B-6 A-5: follower partitioned past the snapshot threshold rejoins by \
                  snapshot.",
        implemented: false,
        accepts_fault: true,
    },
    ScenarioDef {
        name: "a6-replacement-same-id",
        summary: "033 B-6 A-6: data directory emptied, node restarted with the same id.",
        implemented: false,
        accepts_fault: true,
    },
    ScenarioDef {
        name: "a7-membership-shutdown-interleavings",
        summary: "033 B-6 A-7: SIGTERM interleaved with membership changes.",
        implemented: false,
        accepts_fault: true,
    },
    ScenarioDef {
        name: "a8-rolling-restart-under-load",
        summary: "033 B-6 A-8: each node restarted in turn while writes continue.",
        implemented: false,
        accepts_fault: true,
    },
    ScenarioDef {
        name: "a9-shutdown-budget",
        summary: "033 B-6 A-9: recorded from every SIGTERM of A-1 to A-8, not run on its own.",
        implemented: false,
        accepts_fault: false,
    },
    ScenarioDef {
        name: "a10-full-stop-start",
        summary: "033 B-6 A-10: all nodes stopped, then all started; the cluster reforms.",
        implemented: false,
        accepts_fault: false,
    },
];

pub fn find(name: &str) -> Option<&'static ScenarioDef> {
    REGISTRY.iter().find(|s| s.name == name)
}

pub async fn run(name: &str, ctx: &mut RunCtx) -> Result<(), String> {
    match name {
        "smoke" => smoke(ctx).await,
        other => Err(format!("scenario `{other}` is not implemented")),
    }
}

struct Acked {
    cluster: usize,
    group: Group,
    key: String,
    value: String,
}

/// Outcomes that fail the smoke: the process did not end through its own shutdown path.
/// `UnconfirmedTimeout` and `ShutdownError` are recorded and reported, and do not fail the
/// smoke, which measures nothing (A-9 is where they would matter).
fn stop_acceptable(o: StopOutcome) -> bool {
    matches!(
        o,
        StopOutcome::ConfirmedOk | StopOutcome::UnconfirmedTimeout | StopOutcome::ShutdownError
    )
}

async fn smoke(ctx: &mut RunCtx) -> Result<(), String> {
    let ready = Duration::from_secs(ctx.args.ready_bound_secs);
    let mut order = ctx.all_keys();
    ctx.rng.shuffle(&mut order);
    let names: Vec<String> = order.iter().map(|k| ctx.nodes[*k].name.clone()).collect();
    ctx.event(format!("smoke: start order {names:?}"));

    // 1. Start and form.
    ctx.spawn(&order)?;
    let formed = ctx
        .wait_until("initial formation", ready, probe_formed)
        .await?;
    ctx.event(format!("formation: {}", describe(&formed)));

    // 2. Schema, then a write through each group via each node.
    for c in 0..ctx.topo.clusters.len() {
        let first = ctx.topo.cluster_nodes(c)[0];
        let r = ctx.ctl(first, CtlRequest::Schema).await?;
        if !r.ok {
            return Err(format!(
                "schema on {}: {:?}",
                ctx.nodes[first].name, r.error
            ));
        }
    }
    let mut acked: Vec<Acked> = Vec::new();
    for c in 0..ctx.topo.clusters.len() {
        let mut keys = ctx.topo.cluster_nodes(c);
        ctx.rng.shuffle(&mut keys);
        for k in keys {
            for group in [Group::Db, Group::Cache] {
                let key = format!("smoke/{}/{:?}", ctx.nodes[k].name, group).to_lowercase();
                let value = ctx.rng.hex(8);
                let r = ctx
                    .ctl(
                        k,
                        CtlRequest::Write {
                            group,
                            key: key.clone(),
                            value: value.clone(),
                        },
                    )
                    .await?;
                if !r.ok {
                    return Err(format!(
                        "write {key} via {}: {:?}",
                        ctx.nodes[k].name, r.error
                    ));
                }
                acked.push(Acked {
                    cluster: c,
                    group,
                    key,
                    value,
                });
            }
        }
    }
    ctx.event(format!("{} writes acknowledged", acked.len()));

    if ctx.inject == Some(InjectFailure::KillNode) {
        ctx.inject_kill(0)?;
    }

    // 3. Converge and read back through every node.
    ctx.wait_until("convergence after writes", ready, probe_converged)
        .await?;
    verify(ctx, &acked, "before stop").await?;

    // 4. Full stop, every node at once.
    let all = ctx.all_keys();
    let ev = ctx.stop(&all, StopSignal::Term, "full-stop-1").await;
    check_stop(&ev)?;

    // 5. Restart on the same data; the same membership and the data.
    ctx.spawn(&order)?;
    let reformed = ctx
        .wait_until("formation after restart", ready, probe_formed)
        .await?;
    ctx.event(format!("formation after restart: {}", describe(&reformed)));
    for (a, b) in formed.iter().zip(reformed.iter()) {
        if a.membership_log_id != b.membership_log_id {
            // Recorded, not asserted: the smoke does not claim membership is untouched by a
            // restart, only that the voters are the same three.
            ctx.event(format!(
                "note: {} {} membership log id changed across the restart: {} -> {}",
                a.cluster, a.group, a.membership_log_id, b.membership_log_id
            ));
        }
    }
    ctx.wait_until("convergence after restart", ready, probe_converged)
        .await?;
    verify(ctx, &acked, "after restart").await?;

    if ctx.inject == Some(InjectFailure::ImpossibleAssert) {
        ctx.event("INJECTED FAILURE: asserting a key that was never written".into());
        let never = Acked {
            cluster: 0,
            group: Group::Db,
            key: "smoke/never-written".into(),
            value: "x".into(),
        };
        verify(ctx, &[never], "injected assertion").await?;
    }

    // 6. Final stop.
    let ev = ctx.stop(&all, StopSignal::Term, "full-stop-2").await;
    check_stop(&ev)?;
    Ok(())
}

fn describe(f: &[Formation]) -> String {
    f.iter()
        .map(|x| {
            format!(
                "{}/{} leader {} membership {}",
                x.cluster, x.group, x.leader, x.membership_log_id
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn check_stop(ev: &crate::report::StopEvent) -> Result<(), String> {
    let bad: Vec<String> = ev
        .nodes
        .iter()
        .filter(|n| !stop_acceptable(n.outcome))
        .map(|n| format!("{} {:?}", n.node, n.outcome))
        .collect();
    if bad.is_empty() {
        Ok(())
    } else {
        Err(format!("{}: {}", ev.label, bad.join(", ")))
    }
}

async fn verify(ctx: &mut RunCtx, acked: &[Acked], when: &str) -> Result<(), String> {
    let mut reads = 0;
    for a in acked {
        for k in ctx.topo.cluster_nodes(a.cluster) {
            let r = ctx
                .ctl(
                    k,
                    CtlRequest::Read {
                        group: a.group,
                        key: a.key.clone(),
                    },
                )
                .await?;
            if !r.ok {
                return Err(format!(
                    "{when}: read {} via {}: {:?}",
                    a.key, ctx.nodes[k].name, r.error
                ));
            }
            if r.value.as_deref() != Some(a.value.as_str()) {
                return Err(format!(
                    "{when}: {} via {} read {:?}, acknowledged {:?}",
                    a.key, ctx.nodes[k].name, r.value, a.value
                ));
            }
            reads += 1;
        }
    }
    ctx.event(format!(
        "{when}: {reads} reads matched every acknowledged write"
    ));
    Ok(())
}
