//! The harness-owned TCP proxy in front of every raft and API listener.
//!
//! A partition is a state of this proxy, never a firewall rule or a timing accident:
//!
//! - **Endpoint down**: every connection to the endpoint is refused (accepted and closed at
//!   once), and every open one is severed when the rule is set.
//! - **Link down `(src, dst)`**: connections *from* node `src` to either endpoint of node `dst`
//!   are refused and severed. Isolating a node is `down` on both its endpoints plus
//!   `link down (node, k)` for every other node `k`.
//!
//! Attribution. Every node dials `127.0.0.1`, so the proxy cannot tell sources apart by
//! address. When, and only when, a link rule names an endpoint's node, a new connection to that
//! endpoint is attributed by mapping its peer port to the owning process with `lsof`, and the
//! process id to a node through the table the harness keeps of the processes it started. A
//! connection that cannot be attributed while such a rule applies is refused: the rule fails
//! closed, and the refusal is counted as `unattributed`. When a rule changes, every open
//! connection whose admission the new rules would decide differently is severed; a connection
//! accepted without attribution is severed as soon as any link rule names its endpoint's node,
//! and its reconnection is attributed.
//!
//! Without rules, the proxy only copies bytes: no attribution, no added latency beyond one
//! loopback hop.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

const UPSTREAM_CONNECT_BOUND: Duration = Duration::from_secs(5);
const ATTRIBUTION_BOUND: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    Api,
    Raft,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointSpec {
    /// The node key the endpoint belongs to.
    pub node: usize,
    pub kind: EndpointKind,
    pub listen: SocketAddr,
    pub upstream: SocketAddr,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Rules {
    pub down_endpoints: HashSet<usize>,
    /// `(src node, dst node)`
    pub blocked_links: HashSet<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Not looked up, because no rule needed it when the connection was accepted.
    NotAttributed,
    /// Looked up and not found.
    Unknown,
    Node(usize),
}

impl Rules {
    pub fn needs_attribution(&self, dst_node: usize) -> bool {
        self.blocked_links.iter().any(|(_, d)| *d == dst_node)
    }

    /// Whether a connection from `src` to endpoint `ep` (of node `dst_node`) is admitted.
    pub fn admits(&self, ep: usize, dst_node: usize, src: Source) -> bool {
        if self.down_endpoints.contains(&ep) {
            return false;
        }
        if !self.needs_attribution(dst_node) {
            return true;
        }
        match src {
            Source::Node(s) => !self.blocked_links.contains(&(s, dst_node)),
            Source::NotAttributed | Source::Unknown => false,
        }
    }
}

#[derive(Debug, Default)]
pub struct Counters {
    pub accepted: AtomicU64,
    pub refused: AtomicU64,
    pub severed: AtomicU64,
    pub unattributed: AtomicU64,
    pub upstream_failed: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointReport {
    pub node: usize,
    pub kind: EndpointKind,
    pub listen: String,
    pub upstream: String,
    pub accepted: u64,
    pub refused: u64,
    pub severed: u64,
    pub unattributed: u64,
    pub upstream_failed: u64,
}

/// Maps a peer's local port to the process that owns it.
pub trait SourceResolver: Send + Sync + 'static {
    fn owner_pid(&self, peer_port: u16, listen_port: u16) -> Option<u32>;
}

/// `lsof`, available by default on macOS and on most Linux hosts.
pub struct LsofResolver;

impl SourceResolver for LsofResolver {
    fn owner_pid(&self, peer_port: u16, listen_port: u16) -> Option<u32> {
        let out = std::process::Command::new("lsof")
            .args([
                "-nP",
                &format!("-iTCP:{peer_port}"),
                "-sTCP:ESTABLISHED",
                "-Fpn",
            ])
            .output()
            .ok()?;
        parse_lsof(
            &String::from_utf8_lossy(&out.stdout),
            peer_port,
            listen_port,
            std::process::id(),
        )
    }
}

/// Finds the process whose socket has local port `peer_port` and remote port `listen_port`.
/// `lsof -F pn` prints a `p<pid>` line per process followed by `n<local>-><remote>` lines.
pub fn parse_lsof(out: &str, peer_port: u16, listen_port: u16, own_pid: u32) -> Option<u32> {
    let mut pid: Option<u32> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().ok();
        } else if let Some(n) = line.strip_prefix('n') {
            let Some((local, remote)) = n.split_once("->") else {
                continue;
            };
            let port_of = |s: &str| s.rsplit(':').next().and_then(|p| p.parse::<u16>().ok());
            if port_of(local) == Some(peer_port)
                && port_of(remote) == Some(listen_port)
                && let Some(p) = pid
                && p != own_pid
            {
                return Some(p);
            }
        }
    }
    None
}

struct Shared {
    rules: Mutex<Rules>,
    generation: watch::Sender<u64>,
    pids: Mutex<HashMap<u32, usize>>,
    resolver: Box<dyn SourceResolver>,
}

pub struct Proxy {
    shared: Arc<Shared>,
    endpoints: Vec<(EndpointSpec, Arc<Counters>)>,
    tasks: Vec<JoinHandle<()>>,
}

impl Proxy {
    /// Binds every endpoint's listener before returning, so a port that is taken is an error
    /// here and not a silent gap later.
    pub async fn start(
        specs: Vec<EndpointSpec>,
        resolver: Box<dyn SourceResolver>,
    ) -> Result<Self, String> {
        let (generation, _) = watch::channel(0u64);
        let shared = Arc::new(Shared {
            rules: Mutex::new(Rules::default()),
            generation,
            pids: Mutex::new(HashMap::new()),
            resolver,
        });
        let mut endpoints = Vec::new();
        let mut tasks = Vec::new();
        for (idx, spec) in specs.into_iter().enumerate() {
            let listener = TcpListener::bind(spec.listen)
                .await
                .map_err(|e| format!("proxy could not bind {}: {e}", spec.listen))?;
            let counters = Arc::new(Counters::default());
            tasks.push(tokio::spawn(accept_loop(
                idx,
                spec.clone(),
                listener,
                shared.clone(),
                counters.clone(),
            )));
            endpoints.push((spec, counters));
        }
        Ok(Self {
            shared,
            endpoints,
            tasks,
        })
    }

    pub fn register_pid(&self, pid: u32, node: usize) {
        self.shared.pids.lock().unwrap().insert(pid, node);
    }

    pub fn rules(&self) -> Rules {
        self.shared.rules.lock().unwrap().clone()
    }

    /// Replaces the rules and re-evaluates every open connection.
    pub fn set_rules(&self, rules: Rules) {
        *self.shared.rules.lock().unwrap() = rules;
        self.shared.generation.send_modify(|g| *g += 1);
    }

    pub fn endpoints_of(&self, node: usize) -> Vec<usize> {
        self.endpoints
            .iter()
            .enumerate()
            .filter(|(_, (s, _))| s.node == node)
            .map(|(i, _)| i)
            .collect()
    }

    pub fn report(&self) -> Vec<EndpointReport> {
        self.endpoints
            .iter()
            .map(|(s, c)| EndpointReport {
                node: s.node,
                kind: s.kind,
                listen: s.listen.to_string(),
                upstream: s.upstream.to_string(),
                accepted: c.accepted.load(Ordering::Relaxed),
                refused: c.refused.load(Ordering::Relaxed),
                severed: c.severed.load(Ordering::Relaxed),
                unattributed: c.unattributed.load(Ordering::Relaxed),
                upstream_failed: c.upstream_failed.load(Ordering::Relaxed),
            })
            .collect()
    }

    pub fn stop(&mut self) {
        for t in self.tasks.drain(..) {
            t.abort();
        }
        // Severs every connection task: they all watch the generation.
        self.shared.rules.lock().unwrap().down_endpoints = (0..self.endpoints.len()).collect();
        self.shared.generation.send_modify(|g| *g += 1);
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn accept_loop(
    ep: usize,
    spec: EndpointSpec,
    listener: TcpListener,
    shared: Arc<Shared>,
    counters: Arc<Counters>,
) {
    loop {
        let Ok((inbound, peer)) = listener.accept().await else {
            continue;
        };
        let (spec, shared, counters) = (spec.clone(), shared.clone(), counters.clone());
        tokio::spawn(async move {
            connection(ep, spec, inbound, peer, shared, counters).await;
        });
    }
}

async fn attribute(shared: &Arc<Shared>, peer: SocketAddr, listen_port: u16) -> Source {
    let sh = shared.clone();
    let pid = tokio::time::timeout(
        ATTRIBUTION_BOUND,
        tokio::task::spawn_blocking(move || sh.resolver.owner_pid(peer.port(), listen_port)),
    )
    .await
    .ok()
    .and_then(|r| r.ok())
    .flatten();
    match pid.and_then(|p| shared.pids.lock().unwrap().get(&p).copied()) {
        Some(node) => Source::Node(node),
        None => Source::Unknown,
    }
}

async fn connection(
    ep: usize,
    spec: EndpointSpec,
    mut inbound: TcpStream,
    peer: SocketAddr,
    shared: Arc<Shared>,
    counters: Arc<Counters>,
) {
    let mut gen_rx = shared.generation.subscribe();
    let needs = shared.rules.lock().unwrap().needs_attribution(spec.node);
    let src = if needs {
        attribute(&shared, peer, spec.listen.port()).await
    } else {
        Source::NotAttributed
    };
    if src == Source::Unknown {
        counters.unattributed.fetch_add(1, Ordering::Relaxed);
    }
    if !shared.rules.lock().unwrap().admits(ep, spec.node, src) {
        counters.refused.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let mut outbound =
        match tokio::time::timeout(UPSTREAM_CONNECT_BOUND, TcpStream::connect(spec.upstream)).await
        {
            Ok(Ok(s)) => s,
            _ => {
                counters.upstream_failed.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
    counters.accepted.fetch_add(1, Ordering::Relaxed);
    let _ = inbound.set_nodelay(true);
    let _ = outbound.set_nodelay(true);

    let sever = async {
        loop {
            if gen_rx.changed().await.is_err() {
                return;
            }
            if !shared.rules.lock().unwrap().admits(ep, spec.node, src) {
                return;
            }
        }
    };
    tokio::select! {
        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => {}
        _ = sever => {
            counters.severed.fetch_add(1, Ordering::Relaxed);
        }
    }
    // Both streams drop here, closing both sides.
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Fixed(Option<u32>);
    impl SourceResolver for Fixed {
        fn owner_pid(&self, _: u16, _: u16) -> Option<u32> {
            self.0
        }
    }

    async fn echo_server() -> SocketAddr {
        let l = TcpListener::bind("[::1]:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = l.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = [0u8; 64];
                    loop {
                        match s.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => {
                                if s.write_all(&buf[..n]).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                });
            }
        });
        addr
    }

    async fn free_v4() -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap()
    }

    async fn roundtrip(addr: SocketAddr) -> Result<(), String> {
        let mut s = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
        s.write_all(b"ping").await.map_err(|e| e.to_string())?;
        let mut buf = [0u8; 4];
        match tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut buf)).await {
            Ok(Ok(_)) if &buf == b"ping" => Ok(()),
            Ok(Ok(_)) => Err("wrong bytes".into()),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err("timeout".into()),
        }
    }

    async fn proxy(resolver: Option<u32>) -> (Proxy, SocketAddr) {
        let upstream = echo_server().await;
        let listen = free_v4().await;
        let p = Proxy::start(
            vec![EndpointSpec {
                node: 1,
                kind: EndpointKind::Raft,
                listen,
                upstream,
            }],
            Box::new(Fixed(resolver)),
        )
        .await
        .unwrap();
        (p, listen)
    }

    #[tokio::test]
    async fn forwards_without_rules() {
        let (p, listen) = proxy(None).await;
        roundtrip(listen).await.unwrap();
        assert_eq!(p.report()[0].accepted, 1);
    }

    #[tokio::test]
    async fn endpoint_down_severs_open_connections_and_refuses_new_ones() {
        let (p, listen) = proxy(None).await;
        let mut s = TcpStream::connect(listen).await.unwrap();
        s.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        s.read_exact(&mut buf).await.unwrap();

        let mut rules = Rules::default();
        rules.down_endpoints.insert(0);
        p.set_rules(rules);
        // The severed connection reads EOF or a reset, within a bound.
        let r = tokio::time::timeout(Duration::from_secs(2), s.read(&mut buf))
            .await
            .expect("sever within the bound");
        assert!(matches!(r, Ok(0) | Err(_)));
        assert!(roundtrip(listen).await.is_err());
        let rep = &p.report()[0];
        assert_eq!(rep.severed, 1);
        assert!(rep.refused >= 1);

        p.set_rules(Rules::default());
        roundtrip(listen).await.unwrap();
    }

    #[tokio::test]
    async fn link_rule_blocks_the_attributed_source_only() {
        // Source attributed as node 7.
        let (p, listen) = proxy(Some(4242)).await;
        p.register_pid(4242, 7);
        let mut rules = Rules::default();
        rules.blocked_links.insert((7, 1));
        p.set_rules(rules.clone());
        assert!(roundtrip(listen).await.is_err());

        // Another source is admitted under the same rule.
        let mut rules2 = Rules::default();
        rules2.blocked_links.insert((8, 1));
        p.set_rules(rules2);
        roundtrip(listen).await.unwrap();
    }

    #[tokio::test]
    async fn unattributed_connections_fail_closed_under_a_link_rule() {
        let (p, listen) = proxy(None).await;
        // Accepted without rules, so not attributed.
        let mut s = TcpStream::connect(listen).await.unwrap();
        s.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        s.read_exact(&mut buf).await.unwrap();

        let mut rules = Rules::default();
        rules.blocked_links.insert((9, 1));
        p.set_rules(rules);
        let r = tokio::time::timeout(Duration::from_secs(2), s.read(&mut buf))
            .await
            .expect("sever within the bound");
        assert!(matches!(r, Ok(0) | Err(_)));
        // A new connection cannot be attributed (the resolver knows nothing): refused.
        assert!(roundtrip(listen).await.is_err());
        assert!(p.report()[0].unattributed >= 1);
    }

    #[test]
    fn parses_lsof_output() {
        let out =
            "p100\nn127.0.0.1:29100->127.0.0.1:50000\np200\nn127.0.0.1:50000->127.0.0.1:29100\n";
        assert_eq!(parse_lsof(out, 50000, 29100, 100), Some(200));
        // The harness's own side is never the answer.
        assert_eq!(parse_lsof(out, 50000, 29100, 200), None);
        assert_eq!(parse_lsof(out, 50001, 29100, 100), None);
    }

    #[tokio::test]
    async fn lsof_attributes_a_real_connection_to_its_process() {
        if std::process::Command::new("lsof")
            .arg("-v")
            .output()
            .is_err()
        {
            eprintln!("lsof not installed; skipped");
            return;
        }
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let listen_port = l.local_addr().unwrap().port();
        // The client side lives in this same process, so parse with a foreign "own pid".
        let c = std::net::TcpStream::connect(("127.0.0.1", listen_port)).unwrap();
        let _server_side = l.accept().unwrap();
        let peer_port = c.local_addr().unwrap().port();
        let out = std::process::Command::new("lsof")
            .args([
                "-nP",
                &format!("-iTCP:{peer_port}"),
                "-sTCP:ESTABLISHED",
                "-Fpn",
            ])
            .output()
            .unwrap();
        let got = parse_lsof(
            &String::from_utf8_lossy(&out.stdout),
            peer_port,
            listen_port,
            0,
        );
        assert_eq!(got, Some(std::process::id()));
    }
}
