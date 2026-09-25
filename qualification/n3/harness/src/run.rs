//! One run: its private directory, its node processes, its proxy, its bounded waits and its
//! stop records.

use crate::cli::{InjectFailure, RunArgs};
use crate::process::NodeProc;
use crate::proxy::{EndpointKind, EndpointSpec, LsofResolver, Proxy};
use crate::report::{Event, NodeReport, NodeStop, PodStop, StopEvent, StopOutcome};
use crate::rng::Rng;
use crate::topology::Topology;
use n3_proto::{CtlRequest, CtlResponse, GroupStatus, Launch, NodeStatus, Peer, Phase};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long the harness waits for the kernel to reap a process after `SIGKILL`.
const REAP_BOUND: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSignal {
    Term,
    Kill,
}

/// What one poll of a bounded wait observed.
pub enum Probe<T> {
    Ready(T),
    NotYet(String),
}

pub struct RunCtx {
    pub args: RunArgs,
    pub topo: Topology,
    pub nodes: Vec<NodeProc>,
    pub proxy: Proxy,
    pub rng: Rng,
    pub started: Instant,
    pub events: Vec<Event>,
    pub stops: Vec<StopEvent>,
    /// The nodes the scenario expects to be running. Every bounded wait fails as soon as one
    /// of them is not.
    pub expected_alive: BTreeSet<usize>,
    pub inject: Option<InjectFailure>,
}

impl RunCtx {
    pub async fn new(
        args: &RunArgs,
        topo: Topology,
        run_dir: PathBuf,
        seed: u64,
    ) -> Result<Self, String> {
        let mut rng = Rng::new(seed);
        std::fs::create_dir_all(&run_dir).map_err(|e| format!("{run_dir:?}: {e}"))?;

        // Per-cluster secrets and key, derived from the seed.
        let secrets: Vec<(String, String, String)> = topo
            .clusters
            .iter()
            .map(|_| (rng.hex(16), rng.hex(16), rng.hex(32)))
            .collect();

        let mut endpoints = Vec::new();
        let mut nodes = Vec::new();
        for n in &topo.nodes {
            let dir = run_dir.join(n.name());
            std::fs::create_dir_all(dir.join("data")).map_err(|e| format!("{dir:?}: {e}"))?;
            let peers: Vec<Peer> = topo
                .cluster_nodes(n.cluster)
                .into_iter()
                .map(|k| {
                    let p = &topo.nodes[k];
                    Peer {
                        id: p.node_id,
                        addr_raft: format!("127.0.0.1:{}", p.raft_port),
                        addr_api: format!("127.0.0.1:{}", p.api_port),
                    }
                })
                .collect();
            let (secret_raft, secret_api, enc_key_hex) = secrets[n.cluster].clone();
            let fs = topo.clusters[n.cluster].feature_set;
            let launch = Launch {
                cluster: n.cluster_name.clone(),
                node_id: n.node_id,
                nodes: peers,
                listen_addr: "[::1]".into(),
                data_dir: dir.join("data").to_string_lossy().into_owned(),
                log_sync: args.log_sync.clone(),
                cache_storage_disk: true,
                secret_raft,
                secret_api,
                enc_key_hex,
                ctl_addr: format!("127.0.0.1:{}", n.ctl_port),
                status_path: dir
                    .join(n3_proto::STATUS_FILE)
                    .to_string_lossy()
                    .into_owned(),
                shutdown_path: dir
                    .join(n3_proto::SHUTDOWN_FILE)
                    .to_string_lossy()
                    .into_owned(),
                status_interval_ms: args.status_interval_ms,
                feature_set: fs.to_string(),
            };
            let launch_path = dir.join(n3_proto::LAUNCH_FILE);
            std::fs::write(
                &launch_path,
                serde_json::to_vec_pretty(&launch).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("{launch_path:?}: {e}"))?;

            for (kind, port) in [
                (EndpointKind::Api, n.api_port),
                (EndpointKind::Raft, n.raft_port),
            ] {
                endpoints.push(EndpointSpec {
                    node: n.key,
                    kind,
                    listen: format!("127.0.0.1:{port}").parse().unwrap(),
                    upstream: format!("[::1]:{port}").parse().unwrap(),
                });
            }
            nodes.push(NodeProc {
                key: n.key,
                name: n.name(),
                pod: n.pod,
                dir,
                bin: args.node_bin(fs),
                rust_log: args.node_log.clone(),
                ctl_addr: format!("127.0.0.1:{}", n.ctl_port),
                child: None,
                pid: None,
                incarnation: 0,
                last_exit: None,
            });
        }
        let proxy = Proxy::start(endpoints, Box::new(LsofResolver)).await?;

        Ok(Self {
            args: args.clone(),
            topo,
            nodes,
            proxy,
            rng,
            started: Instant::now(),
            events: Vec::new(),
            stops: Vec::new(),
            expected_alive: BTreeSet::new(),
            inject: args.inject_failure,
        })
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn event(&mut self, what: String) {
        let at_ms = self.elapsed_ms();
        println!("[{:>7.1}s] {what}", at_ms as f64 / 1000.0);
        self.events.push(Event { at_ms, what });
    }

    pub fn pod_name(&self, pod: usize) -> String {
        self.topo.pods[pod].0.clone()
    }

    pub fn all_keys(&self) -> Vec<usize> {
        self.topo.nodes.iter().map(|n| n.key).collect()
    }

    /// Spawns the nodes in the given order, back to back, and registers each process with the
    /// proxy for link attribution.
    pub fn spawn(&mut self, keys: &[usize]) -> Result<(), String> {
        for &k in keys {
            let pid = self.nodes[k].spawn()?;
            self.proxy.register_pid(pid, k);
            self.expected_alive.insert(k);
            let name = self.nodes[k].name.clone();
            let inc = self.nodes[k].incarnation;
            self.event(format!("spawned {name} (pid {pid}, incarnation {inc})"));
        }
        Ok(())
    }

    pub fn check_expected_alive(&mut self) -> Result<(), String> {
        let keys: Vec<usize> = self.expected_alive.iter().copied().collect();
        for k in keys {
            self.nodes[k].check_alive()?;
        }
        Ok(())
    }

    /// Polls `probe` until it is ready, failing when the bound elapses or when a node the run
    /// expects alive is not. The poll interval only paces the observation; it conceals nothing,
    /// because the wait ends on the observed state or on the bound.
    pub async fn wait_until<T>(
        &mut self,
        what: &str,
        bound: Duration,
        mut probe: impl FnMut(&RunCtx) -> Result<Probe<T>, String>,
    ) -> Result<T, String> {
        let t0 = Instant::now();
        let poll = Duration::from_millis(self.args.poll_ms);
        loop {
            self.check_expected_alive()
                .map_err(|e| format!("{what}: {e}"))?;
            match probe(self)? {
                Probe::Ready(v) => {
                    let ms = t0.elapsed().as_millis();
                    self.event(format!("{what}: observed after {ms} ms"));
                    return Ok(v);
                }
                Probe::NotYet(why) => {
                    if t0.elapsed() >= bound {
                        return Err(format!(
                            "{what}: not observed within {bound:?}; last observation: {why}"
                        ));
                    }
                }
            }
            tokio::time::sleep(poll).await;
        }
    }

    pub async fn ctl(&mut self, key: usize, req: CtlRequest) -> Result<CtlResponse, String> {
        self.nodes[key].check_alive()?;
        let bound = Duration::from_secs(self.args.op_bound_secs);
        self.nodes[key].ctl(&req, bound).await
    }

    /// Sends `sig` to every listed node at once, then observes every exit. After `SIGTERM`,
    /// a node still running when the grace ends is sent `SIGKILL` and recorded as a forced
    /// exit. The nodes are no longer expected alive.
    pub async fn stop(&mut self, keys: &[usize], sig: StopSignal, label: &str) -> StopEvent {
        let at_ms = self.elapsed_ms();
        for k in keys {
            self.expected_alive.remove(k);
        }
        let (signame, signum) = match sig {
            StopSignal::Term => ("SIGTERM", libc::SIGTERM),
            StopSignal::Kill => ("SIGKILL", libc::SIGKILL),
        };
        let mut pending: Vec<(usize, Instant)> = Vec::new();
        let mut results: Vec<NodeStop> = Vec::new();
        for &k in keys {
            let node = &mut self.nodes[k];
            // A node that already exited is recorded as it is found.
            if let Some(exit) = node.poll_exit() {
                results.push(NodeStop {
                    node: node.name.clone(),
                    pod: String::new(),
                    signal: signame,
                    duration_ms: None,
                    outcome: StopOutcome::ExitedWithoutRecord,
                    exit: Some(exit),
                    record: node.shutdown_record(),
                });
                continue;
            }
            if !node.is_running() {
                continue;
            }
            match node.signal(signum) {
                Ok(()) => pending.push((k, Instant::now())),
                Err(e) => eprintln!("{e}"),
            }
        }
        self.event(format!("{label}: {signame} to {} node(s)", pending.len()));

        let grace = match sig {
            StopSignal::Term => Duration::from_secs(self.args.term_grace_secs),
            StopSignal::Kill => REAP_BOUND,
        };
        let poll = Duration::from_millis(self.args.poll_ms.min(50));
        let t0 = Instant::now();
        let mut forced: Vec<(usize, Instant)> = Vec::new();
        while !pending.is_empty() {
            pending.retain(|(k, sent)| {
                let node = &mut self.nodes[*k];
                match node.poll_exit() {
                    Some(exit) => {
                        let record = node.shutdown_record();
                        let outcome = match sig {
                            StopSignal::Kill => StopOutcome::Killed,
                            StopSignal::Term => match &record {
                                Some(r) => match r.result {
                                    n3_proto::ShutdownResult::Ok => StopOutcome::ConfirmedOk,
                                    n3_proto::ShutdownResult::Timeout => {
                                        StopOutcome::UnconfirmedTimeout
                                    }
                                    n3_proto::ShutdownResult::Error => StopOutcome::ShutdownError,
                                },
                                None => StopOutcome::ExitedWithoutRecord,
                            },
                        };
                        results.push(NodeStop {
                            node: node.name.clone(),
                            pod: String::new(),
                            signal: signame,
                            duration_ms: Some(sent.elapsed().as_millis() as u64),
                            outcome,
                            exit: Some(exit),
                            record,
                        });
                        false
                    }
                    None => true,
                }
            });
            if pending.is_empty() {
                break;
            }
            if t0.elapsed() >= grace {
                if sig == StopSignal::Term {
                    for (k, sent) in pending.drain(..) {
                        let _ = self.nodes[k].signal(libc::SIGKILL);
                        forced.push((k, sent));
                    }
                } else {
                    for (k, _) in pending.drain(..) {
                        results.push(NodeStop {
                            node: self.nodes[k].name.clone(),
                            pod: String::new(),
                            signal: signame,
                            duration_ms: None,
                            outcome: StopOutcome::NotReaped,
                            exit: None,
                            record: None,
                        });
                    }
                }
                break;
            }
            tokio::time::sleep(poll).await;
        }

        // Grace elapsed: reap the forced ones, bounded.
        let t1 = Instant::now();
        while !forced.is_empty() {
            forced.retain(|(k, sent)| {
                let node = &mut self.nodes[*k];
                match node.poll_exit() {
                    Some(exit) => {
                        results.push(NodeStop {
                            node: node.name.clone(),
                            pod: String::new(),
                            signal: "SIGTERM",
                            duration_ms: Some(sent.elapsed().as_millis() as u64),
                            outcome: StopOutcome::ForcedKill,
                            exit: Some(exit),
                            record: node.shutdown_record(),
                        });
                        false
                    }
                    None => true,
                }
            });
            if t1.elapsed() >= REAP_BOUND {
                for (k, _) in forced.drain(..) {
                    results.push(NodeStop {
                        node: self.nodes[k].name.clone(),
                        pod: String::new(),
                        signal: "SIGTERM",
                        duration_ms: None,
                        outcome: StopOutcome::NotReaped,
                        exit: None,
                        record: None,
                    });
                }
                break;
            }
            tokio::time::sleep(poll).await;
        }

        // Pod names, and the per-pod aggregate A-9 reports.
        for r in results.iter_mut() {
            let k = self.nodes.iter().position(|n| n.name == r.node).unwrap();
            r.pod = self.pod_name(self.nodes[k].pod);
        }
        let mut pods: Vec<PodStop> = Vec::new();
        for (pname, _) in &self.topo.pods {
            let mine: Vec<&NodeStop> = results.iter().filter(|r| &r.pod == pname).collect();
            if mine.is_empty() {
                continue;
            }
            pods.push(PodStop {
                pod: pname.clone(),
                duration_ms: if mine.iter().all(|r| r.duration_ms.is_some()) {
                    mine.iter().filter_map(|r| r.duration_ms).max()
                } else {
                    None
                },
                outcome: mine.iter().map(|r| r.outcome).max().unwrap(),
            });
        }
        for r in &results {
            let d = r
                .duration_ms
                .map(|d| format!("{d} ms"))
                .unwrap_or_else(|| "n/a".into());
            let exit = r
                .exit
                .as_ref()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "none".into());
            self.event(format!(
                "{label}: {} ({}) {:?} after {d}, {exit}",
                r.node, r.pod, r.outcome
            ));
        }
        let ev = StopEvent {
            label: label.to_string(),
            at_ms,
            nodes: results,
            pods,
        };
        self.stops.push(ev.clone());
        ev
    }

    /// Kills every node this run started that is still running, for the end of a run and for
    /// a failed or bounded-out one. Recorded as a stop event like any other.
    pub async fn cleanup(&mut self) {
        let running: Vec<usize> = self
            .nodes
            .iter()
            .filter(|n| n.is_running())
            .map(|n| n.key)
            .collect();
        if !running.is_empty() {
            self.stop(&running, StopSignal::Kill, "cleanup").await;
        }
        self.proxy.stop();
    }

    pub fn node_reports(&self) -> Vec<NodeReport> {
        self.nodes
            .iter()
            .map(|n| {
                let spec = &self.topo.nodes[n.key];
                NodeReport {
                    name: n.name.clone(),
                    cluster: spec.cluster_name.clone(),
                    node_id: spec.node_id,
                    pod: self.pod_name(n.pod),
                    incarnations: n.incarnation,
                    running_at_end: n.is_running(),
                    last_exit: n.last_exit.clone(),
                    last_status: n.status_any(),
                    shutdown_record: n.shutdown_record(),
                    dir: n.dir.to_string_lossy().into_owned(),
                }
            })
            .collect()
    }

    /// Kills the nodes of `pod` with `SIGKILL` while the run keeps expecting them alive: the
    /// injected failure the harness must detect.
    pub fn inject_kill(&mut self, pod: usize) -> Result<(), String> {
        let keys = self.topo.pods[pod].1.clone();
        for k in keys {
            self.nodes[k].signal(libc::SIGKILL)?;
            let name = self.nodes[k].name.clone();
            self.event(format!(
                "INJECTED FAILURE: SIGKILL to {name}, still expected alive"
            ));
        }
        Ok(())
    }
}

/// Which raft group of a node's status.
pub fn group_of(s: &NodeStatus, db: bool) -> Option<&GroupStatus> {
    if db { s.db.as_ref() } else { s.cache.as_ref() }
}

/// A cluster's formation as observed: per group, the membership log id every member agrees on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Formation {
    pub cluster: String,
    pub group: &'static str,
    pub leader: u64,
    pub membership_log_id: String,
    pub voters: Vec<u64>,
}

/// Every node of every cluster running, and each group of each cluster with voters `{1,2,3}`,
/// no learners, one agreed leader and one agreed membership log id.
pub fn probe_formed(ctx: &RunCtx) -> Result<Probe<Vec<Formation>>, String> {
    let mut out = Vec::new();
    for (c, cluster) in ctx.topo.clusters.iter().enumerate() {
        let keys = ctx.topo.cluster_nodes(c);
        for (db, gname) in [(true, "db"), (false, "cache")] {
            let mut leader: Option<u64> = None;
            let mut mlog: Option<String> = None;
            for &k in &keys {
                let node = &ctx.nodes[k];
                let Some(s) = node.status() else {
                    return Ok(Probe::NotYet(format!("{}: no status yet", node.name)));
                };
                if s.phase != Phase::Running {
                    return Ok(Probe::NotYet(format!("{}: phase {:?}", node.name, s.phase)));
                }
                let Some(g) = group_of(&s, db) else {
                    return Ok(Probe::NotYet(format!("{}: no {gname} metrics", node.name)));
                };
                let mut voters = g.voters.clone();
                voters.sort();
                if voters != [1, 2, 3] || !g.learners.is_empty() {
                    return Ok(Probe::NotYet(format!(
                        "{} {gname}: voters {:?} learners {:?}",
                        node.name, g.voters, g.learners
                    )));
                }
                let Some(l) = g.current_leader else {
                    return Ok(Probe::NotYet(format!("{} {gname}: no leader", node.name)));
                };
                if leader.is_some_and(|x| x != l) {
                    return Ok(Probe::NotYet(format!("{gname}: leaders disagree")));
                }
                leader = Some(l);
                let Some(m) = &g.membership_log_id else {
                    return Ok(Probe::NotYet(format!(
                        "{} {gname}: no membership log id",
                        node.name
                    )));
                };
                let m = format!("{}-{}", m.leader, m.index);
                if mlog.as_ref().is_some_and(|x| x != &m) {
                    return Ok(Probe::NotYet(format!(
                        "{gname}: membership log ids disagree"
                    )));
                }
                mlog = Some(m);
            }
            out.push(Formation {
                cluster: cluster.name.clone(),
                group: gname,
                leader: leader.unwrap(),
                membership_log_id: mlog.unwrap(),
                voters: vec![1, 2, 3],
            });
        }
    }
    Ok(Probe::Ready(out))
}

/// Every running member of every cluster has applied up to its leader's last log index, for
/// both groups.
pub fn probe_converged(ctx: &RunCtx) -> Result<Probe<()>, String> {
    for (c, _) in ctx.topo.clusters.iter().enumerate() {
        let keys = ctx.topo.cluster_nodes(c);
        for (db, gname) in [(true, "db"), (false, "cache")] {
            let statuses: Vec<(String, NodeStatus)> = keys
                .iter()
                .filter_map(|&k| {
                    ctx.nodes[k]
                        .status()
                        .map(|s| (ctx.nodes[k].name.clone(), s))
                })
                .collect();
            if statuses.len() != keys.len() {
                return Ok(Probe::NotYet("a status is missing".into()));
            }
            let Some(target) = statuses.iter().find_map(|(_, s)| {
                let g = group_of(s, db)?;
                (g.server_state == "Leader")
                    .then_some(g.last_log_index)
                    .flatten()
            }) else {
                return Ok(Probe::NotYet(format!("{gname}: no leader reported")));
            };
            for (name, s) in &statuses {
                let applied = group_of(s, db)
                    .and_then(|g| g.last_applied.as_ref())
                    .map(|l| l.index);
                if applied != Some(target) {
                    return Ok(Probe::NotYet(format!(
                        "{name} {gname}: applied {applied:?}, leader's last log {target}"
                    )));
                }
            }
        }
    }
    Ok(Probe::Ready(()))
}
