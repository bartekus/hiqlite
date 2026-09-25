//! Clusters, simulated pods, nodes and their ports.
//!
//! Every node has three ports, allocated from the base port in node order:
//! `api = base + 3g`, `raft = base + 3g + 1`, `ctl = base + 3g + 2`, where `g` is the node's
//! global index (cluster-major). The node binds `api` and `raft` on `[::1]`; the harness proxy
//! binds the same port numbers on `127.0.0.1`, and every node advertises the `127.0.0.1`
//! addresses. hiqlite takes a listener's port from the node's advertised address, so a proxy in
//! front of the same port number needs a second loopback address, and `::1` is the one every
//! host has without configuration. `ctl` is bound on `127.0.0.1` by the node and not proxied.

use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    /// One node per simulated pod: the N=3 layout of 033 D-4 (proposal D-14).
    Split,
    /// One node of every cluster per simulated pod: the N=1 profile's shape. A pod-level
    /// signal applies to a voter of each cluster.
    CoLocated,
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Layout::Split => "split",
            Layout::CoLocated => "co-located",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum FeatureSet {
    /// sqlite, cache, counters, dlock, listen_notify_local, backup, s3
    Rahi,
    /// hiqlite defaults plus cache, cast_ints, counters, dashboard, listen_notify_local, macros
    Rauthy,
}

impl fmt::Display for FeatureSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FeatureSet::Rahi => "rahi",
            FeatureSet::Rauthy => "rauthy",
        })
    }
}

pub const NODES_PER_CLUSTER: u64 = 3;

/// Ports this harness must never use: the N=1 probes' fixed ports and every fixed port the
/// repository's own tests and examples bind.
pub const FORBIDDEN_PORTS: &[u16] = &[
    8080, 8100, 8101, 8102, 8103, 8200, 8201, 8202, 8203, 18411, 18412, 31001, 31002, 31003, 32001,
    32002, 32003, 35001, 35002, 35003, 36001, 36002, 36003,
];

#[derive(Debug, Clone, Serialize)]
pub struct ClusterSpec {
    pub name: String,
    pub feature_set: FeatureSet,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeSpec {
    /// Global index, cluster-major.
    pub key: usize,
    pub cluster: usize,
    pub cluster_name: String,
    pub node_id: u64,
    pub pod: usize,
    pub api_port: u16,
    pub raft_port: u16,
    pub ctl_port: u16,
}

impl NodeSpec {
    pub fn name(&self) -> String {
        format!("{}-n{}", self.cluster_name, self.node_id)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Topology {
    pub layout: Layout,
    pub clusters: Vec<ClusterSpec>,
    pub nodes: Vec<NodeSpec>,
    /// Pod name and the node keys it holds.
    pub pods: Vec<(String, Vec<usize>)>,
}

impl Topology {
    pub fn build(layout: Layout, clusters: &[ClusterSpec], base_port: u16) -> Result<Self, String> {
        let total = clusters.len() as u64 * NODES_PER_CLUSTER;
        let last = base_port as u64 + total * 3;
        if last > u16::MAX as u64 {
            return Err(format!(
                "base port {base_port} leaves no room for {total} nodes of three ports each"
            ));
        }
        let mut nodes = Vec::new();
        for (c, cluster) in clusters.iter().enumerate() {
            for id in 1..=NODES_PER_CLUSTER {
                let key = nodes.len();
                let port = base_port + (key as u16) * 3;
                let pod = match layout {
                    Layout::Split => key,
                    Layout::CoLocated => (id - 1) as usize,
                };
                nodes.push(NodeSpec {
                    key,
                    cluster: c,
                    cluster_name: cluster.name.clone(),
                    node_id: id,
                    pod,
                    api_port: port,
                    raft_port: port + 1,
                    ctl_port: port + 2,
                });
            }
        }
        for n in &nodes {
            for p in [n.api_port, n.raft_port, n.ctl_port] {
                if FORBIDDEN_PORTS.contains(&p) {
                    return Err(format!(
                        "port {p} is reserved for another harness or test; choose another \
                         --base-port"
                    ));
                }
            }
        }
        let pods = match layout {
            Layout::Split => nodes
                .iter()
                .map(|n| (format!("{}-{}", n.cluster_name, n.node_id - 1), vec![n.key]))
                .collect(),
            Layout::CoLocated => (0..NODES_PER_CLUSTER as usize)
                .map(|p| {
                    (
                        format!("pod-{p}"),
                        nodes.iter().filter(|n| n.pod == p).map(|n| n.key).collect(),
                    )
                })
                .collect(),
        };
        Ok(Self {
            layout,
            clusters: clusters.to_vec(),
            nodes,
            pods,
        })
    }

    pub fn cluster_nodes(&self, cluster: usize) -> Vec<usize> {
        self.nodes
            .iter()
            .filter(|n| n.cluster == cluster)
            .map(|n| n.key)
            .collect()
    }

    /// Every port this topology binds, for the pre-run availability check.
    pub fn all_ports(&self) -> Vec<(u16, bool)> {
        let mut v = Vec::new();
        for n in &self.nodes {
            // (port, bound on both loopback addresses)
            v.push((n.api_port, true));
            v.push((n.raft_port, true));
            v.push((n.ctl_port, false));
        }
        v
    }
}

/// Refuses a run when any port it needs is already bound by anything, on either loopback
/// address. This harness never stops a process it did not start.
pub fn check_ports_free(ports: &[(u16, bool)]) -> Result<(), String> {
    for &(port, both) in ports {
        let v4 = std::net::TcpListener::bind(("127.0.0.1", port));
        if let Err(err) = v4 {
            return Err(format!("port 127.0.0.1:{port} is not available: {err}"));
        }
        if both && let Err(err) = std::net::TcpListener::bind(("::1", port)) {
            return Err(format!("port [::1]:{port} is not available: {err}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Vec<ClusterSpec> {
        vec![
            ClusterSpec {
                name: "c1".into(),
                feature_set: FeatureSet::Rahi,
            },
            ClusterSpec {
                name: "c2".into(),
                feature_set: FeatureSet::Rahi,
            },
        ]
    }

    #[test]
    fn split_has_one_node_per_pod() {
        let t = Topology::build(Layout::Split, &two(), 29100).unwrap();
        assert_eq!(t.nodes.len(), 6);
        assert_eq!(t.pods.len(), 6);
        assert!(t.pods.iter().all(|(_, n)| n.len() == 1));
        assert_eq!(t.pods[3].0, "c2-0");
        assert_eq!(t.nodes[5].api_port, 29115);
        assert_eq!(t.nodes[5].ctl_port, 29117);
    }

    #[test]
    fn co_located_puts_one_voter_of_each_cluster_in_a_pod() {
        let t = Topology::build(Layout::CoLocated, &two(), 29100).unwrap();
        assert_eq!(t.pods.len(), 3);
        for (p, (_, keys)) in t.pods.iter().enumerate() {
            assert_eq!(keys.len(), 2);
            let clusters: Vec<_> = keys.iter().map(|k| t.nodes[*k].cluster).collect();
            assert_eq!(clusters, vec![0, 1]);
            assert!(keys.iter().all(|k| t.nodes[*k].node_id == p as u64 + 1));
        }
    }

    #[test]
    fn forbidden_ports_are_refused() {
        assert!(Topology::build(Layout::Split, &two(), 18400).is_err());
        assert!(Topology::build(Layout::Split, &two(), 31000).is_err());
        assert!(Topology::build(Layout::Split, &two(), 65530).is_err());
    }
}
