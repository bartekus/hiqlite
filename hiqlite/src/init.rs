use crate::app_state::{AppState, RaftType};
use crate::helpers::{deserialize, serialize};
use crate::network::HEADER_NAME_SECRET;
use crate::network::management::{ClusterLeaveReq, LearnerReq};
use crate::{Error, Node, NodeId, helpers};
use openraft::{Membership, RaftMetrics};
use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::{fs, time};
use tracing::{debug, error, warn};

#[cfg(feature = "sqlite")]
use crate::store::state_machine::sqlite::TypeConfigSqlite;

#[cfg(feature = "cache")]
use crate::store::state_machine::memory::TypeConfigKV;

use crate::http_client::build_http_client;
#[cfg(any(feature = "cache", feature = "sqlite"))]
use std::collections::BTreeMap;
use std::env;
use std::sync::atomic::Ordering;
#[cfg(any(feature = "cache", feature = "sqlite"))]
use tracing::info;

/// Checks if a Raft Logs / Metadata reset should be performed and deletes all logs if true.
pub async fn check_execute_reset(base_path: &str) -> Result<bool, Error> {
    let do_reset = env::var("HQL_DANGER_RAFT_STATE_RESET")
        .as_deref()
        .unwrap_or("false")
        .parse::<bool>()
        .expect("Cannot parse HQL_DANGER_RAFT_STATE_RESET as u64");
    if !do_reset {
        return Ok(false);
    }

    warn!(
        r#"

    !!! CAUTION !!!

    Performing a full Raft State reset! If used incorrectly, this option
    can destroy your cluster and end up with an inconsistent state!

    Base target directory: {base_path}

    Continuing in 10 seconds ...

    "#
    );
    time::sleep(Duration::from_secs(10)).await;

    if let Err(err) = fs::remove_dir_all(format!("{base_path}/logs")).await {
        error!("Error during Raft Logs cleanup: {err:?}");
    }
    if let Err(err) = fs::remove_dir_all(format!("{base_path}/logs_cache")).await {
        error!("Error during Raft Logs cleanup: {err:?}");
    }
    if let Err(err) = fs::remove_dir_all(format!("{base_path}/state_machine/snapshots")).await {
        error!("Error during Raft Logs cleanup: {err:?}");
    }
    if let Err(err) = fs::remove_dir_all(format!("{base_path}/state_machine_cache/snapshots")).await
    {
        error!("Error during Raft Logs cleanup: {err:?}");
    }

    Ok(true)
}

/// Initializes a fresh node 1, if it has not been set up yet.
#[cfg(feature = "sqlite")]
pub async fn init_pristine_node_1_db(
    raft: &openraft::Raft<TypeConfigSqlite>,
    node_id: u64,
    nodes: &[Node],
    secret_api: &str,
    tls: bool,
    tls_no_verify: bool,
    peer_wait: Duration,
) -> Result<(), Error> {
    if node_id == 1 {
        let this_node = get_this_node(node_id, nodes);

        if is_initialized_timeout_sqlite(node_id, raft).await? {
            info!("node 1 raft is already initialized");
            return Ok(());
        }

        if should_node_1_skip_init(
            &RaftType::Sqlite,
            nodes,
            secret_api,
            tls,
            tls_no_verify,
            peer_wait,
        )
        .await?
        {
            info!("node 1 (DB) should skip its own init - found existing cluster on remotes");
            return Ok(());
        }

        info!("initializing pristine node 1 raft");
        let mut nodes_set = BTreeMap::new();
        nodes_set.insert(this_node.id, this_node);
        raft.initialize(nodes_set).await?;
    }

    Ok(())
}

// TODO this duplication is not pretty but getting the types correct is pretty hard
/// Initializes a fresh node 1, if it has not been set up yet.
#[cfg(feature = "cache")]
#[allow(clippy::too_many_arguments)]
pub async fn init_pristine_node_1_cache(
    raft: &openraft::Raft<TypeConfigKV>,
    wal_on_disk: bool,
    node_id: u64,
    nodes: &[Node],
    secret_api: &str,
    tls: bool,
    tls_no_verify: bool,
    peer_wait: Duration,
) -> Result<(), Error> {
    if node_id == 1 {
        let this_node = get_this_node(node_id, nodes);

        if wal_on_disk && is_initialized_timeout_cache(node_id, raft).await? {
            info!("node 1 raft is already initialized");
            return Ok(());
        }

        // `033` B-3: the cache group takes the same decision as the SQLite group.
        if should_node_1_skip_init(
            &RaftType::Cache,
            nodes,
            secret_api,
            tls,
            tls_no_verify,
            peer_wait,
        )
        .await?
        {
            info!("node 1 (cache) should skip its own init - found existing cluster on remotes");
            return Ok(());
        }

        info!("initializing pristine node 1 raft");
        let mut nodes_set = BTreeMap::new();
        nodes_set.insert(this_node.id, this_node);
        raft.initialize(nodes_set).await?;
    }

    Ok(())
}

fn get_this_node(this_node: u64, nodes: &[Node]) -> Node {
    let filtered = nodes
        .iter()
        .filter(|node| node.id == this_node)
        .collect::<Vec<&Node>>();
    let node = filtered
        .first()
        .cloned()
        .expect("this node to always exist in all nodes");
    (*node).clone()
}

/// What a pristine node 1 learned from its peers about one raft group (`033` B-3).
#[derive(Debug, PartialEq)]
pub(crate) enum PeerInitEvidence {
    /// A peer answered, authenticated, with a non-empty membership: the group exists.
    Initialized { peer: NodeId },
    /// At least `N / 2` (rounded down) distinct peers answered, authenticated, that their own
    /// group is not initialized, so together with this node a majority has positively said so.
    Fresh,
}

/// Ask the peers whether this raft group already exists (`033` B-3, repairs F-118).
///
/// Only an **explicit** answer counts: an authenticated `200` whose body is a membership. An
/// empty one is a peer saying "my group is not initialized"; a non-empty one is an existing
/// group. A connection error, a timeout, a `401`, any other status and a body that does not
/// decode count for nothing. In particular the `400` a peer returns while its raft is stopped
/// is not read as "not initialized": an initialized peer answers that while it starts or shuts
/// down.
///
/// Node 1 never counts itself. The unrepaired decision seeded its own id, which at N=3
/// satisfied the quorum of one before any peer was asked, so a single unreachable peer made a
/// pristine node 1 initialize a second cluster beside a live majority.
///
/// Bounded by `wait`: on expiry this returns `Error::Startup` naming every peer that did not
/// give an explicit answer and why. It never initializes by default.
#[tracing::instrument(skip(nodes, secret_api, tls, tls_no_verify))]
pub(crate) async fn peer_init_evidence(
    raft_type: &RaftType,
    this_node: NodeId,
    nodes: &[Node],
    secret_api: &str,
    tls: bool,
    tls_no_verify: bool,
    wait: Duration,
) -> Result<PeerInitEvidence, Error> {
    // Distinct peers, and never this node.
    let mut peers: BTreeMap<NodeId, &Node> = BTreeMap::new();
    for node in nodes.iter().filter(|n| n.id != this_node) {
        peers.entry(node.id).or_insert(node);
    }
    let quorum = nodes.len() / 2;
    if quorum == 0 {
        return Ok(PeerInitEvidence::Fresh);
    }

    let client = build_http_client(tls_no_verify);
    let scheme = if tls { "https" } else { "http" };

    let deadline = time::Instant::now() + wait;
    let mut fresh = std::collections::BTreeSet::new();
    let mut silent: BTreeMap<NodeId, String> = peers
        .keys()
        .map(|id| (*id, "not asked yet".to_string()))
        .collect();

    loop {
        // One round: every peer without an explicit answer yet is asked **concurrently**, each
        // request capped at `PEER_REQUEST_CAP` and at what is left of the wait. Asked one after
        // another, a peer that accepted the connection and never answered took the whole wait
        // and left the peers after it a 100 ms floor (review finding 7).
        let left = deadline
            .saturating_duration_since(time::Instant::now())
            .max(Duration::from_millis(100));
        let cap = left.min(PEER_REQUEST_CAP);
        let mut round = tokio::task::JoinSet::new();
        for (id, node) in &peers {
            if fresh.contains(id) {
                continue;
            }
            let url = format!(
                "{}://{}/cluster/membership/{}",
                scheme,
                node.addr_api,
                raft_type.as_str()
            );
            debug!("checking membership via {}", url);
            let req = client
                .get(url)
                .header(HEADER_NAME_SECRET, secret_api)
                .send();
            let id = *id;
            round.spawn(async move { (id, ask_peer(req, cap).await) });
        }

        let mut initialized = None;
        while let Some(joined) = round.join_next().await {
            let Ok((id, answer)) = joined else {
                continue;
            };
            match answer {
                PeerAnswer::Initialized => {
                    initialized.get_or_insert(id);
                }
                PeerAnswer::NotInitialized => {
                    fresh.insert(id);
                    silent.remove(&id);
                }
                PeerAnswer::Silent(reason) => {
                    debug!("node {id} did not answer explicitly: {reason}");
                    silent.insert(id, reason);
                }
            }
        }
        // An initialized group ends the decision whatever else the round heard, so an
        // initialized peer that answers is never outvoted by fresh ones in the same round.
        if let Some(peer) = initialized {
            info!(
                "node {peer} answered with an initialized {} group",
                raft_type.as_str()
            );
            return Ok(PeerInitEvidence::Initialized { peer });
        }

        if fresh.len() >= quorum {
            info!(
                "{} of {} peers answered that their {} group is not initialized: a fresh cluster",
                fresh.len(),
                peers.len(),
                raft_type.as_str()
            );
            return Ok(PeerInitEvidence::Fresh);
        }

        let now = time::Instant::now();
        if now >= deadline {
            let mut names = String::new();
            for (id, reason) in &silent {
                let addr = peers.get(id).map(|n| n.addr_api.as_str()).unwrap_or("?");
                let _ = write!(names, "; node {id} ({addr}): {reason}");
            }
            return Err(Error::Startup(
                format!(
                    "node {this_node} cannot tell whether the {} raft group already exists: it \
                     needs {quorum} peer(s) to answer, authenticated, that they are not \
                     initialized, and {} did within {wait:?}. It does not initialize without \
                     that evidence (033 B-3). Peers without an explicit answer{names}",
                    raft_type.as_str(),
                    fresh.len(),
                )
                .into(),
            ));
        }
        info!(
            "Waiting for {} more peer(s) to answer whether the {} group exists",
            quorum - fresh.len(),
            raft_type.as_str()
        );
        time::sleep(Duration::from_secs(1).min(deadline - now)).await;
    }
}

/// The most a single peer is waited for in one round of `peer_init_evidence`.
const PEER_REQUEST_CAP: Duration = Duration::from_secs(5);

/// One peer's answer to "is your group initialized?".
enum PeerAnswer {
    Initialized,
    NotInitialized,
    /// Not an explicit answer, and why.
    Silent(String),
}

async fn ask_peer(
    req: impl std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
    cap: Duration,
) -> PeerAnswer {
    let started = time::Instant::now();
    let resp = match time::timeout(cap, req).await {
        Err(_) => return PeerAnswer::Silent(format!("no answer within {cap:?}")),
        Ok(Err(err)) => return PeerAnswer::Silent(format!("unreachable: {err}")),
        Ok(Ok(resp)) => resp,
    };
    let rest = cap
        .saturating_sub(started.elapsed())
        .max(Duration::from_millis(100));
    let status = resp.status();
    let body = match time::timeout(rest, resp.bytes()).await {
        Ok(Ok(body)) => body,
        Ok(Err(err)) => return PeerAnswer::Silent(format!("answered {status}, body failed: {err}")),
        Err(_) => return PeerAnswer::Silent(format!("answered {status}, no body within {rest:?}")),
    };
    if !status.is_success() {
        let text = String::from_utf8_lossy(&body)
            .chars()
            .take(200)
            .collect::<String>();
        return PeerAnswer::Silent(format!(
            "answered {status}, which is not an explicit answer: {text}"
        ));
    }
    match deserialize::<Membership<NodeId, Node>>(&body) {
        Ok(m) if m.nodes().count() > 0 => PeerAnswer::Initialized,
        Ok(_) => PeerAnswer::NotInitialized,
        Err(err) => {
            PeerAnswer::Silent(format!("answered 200 with a body that is not a membership: {err}"))
        }
    }
}

/// `true` if a pristine node 1 must not initialize this group because a peer already holds it.
async fn should_node_1_skip_init(
    raft_type: &RaftType,
    nodes: &[Node],
    secret_api: &str,
    tls: bool,
    tls_no_verify: bool,
    wait: Duration,
) -> Result<bool, Error> {
    if nodes.len() < 2 {
        return Ok(false);
    }
    match peer_init_evidence(raft_type, 1, nodes, secret_api, tls, tls_no_verify, wait).await? {
        PeerInitEvidence::Initialized { .. } => Ok(true),
        PeerInitEvidence::Fresh => Ok(false),
    }
}

#[derive(Debug, PartialEq)]
enum SkipBecome {
    Yes,
    No,
}

/// If this node is not a cluster member, it will try to become a learner and
/// a voting member afterward.
#[tracing::instrument(skip(state, nodes, tls, tls_no_verify))]
#[allow(clippy::too_many_arguments)]
pub async fn become_cluster_member(
    state: Arc<AppState>,
    raft_type: &RaftType,
    this_node: u64,
    nodes: &[Node],
    tls: bool,
    tls_no_verify: bool,
) -> Result<(), Error> {
    if helpers::is_raft_initialized(&state, raft_type).await? {
        info!(
            "Node {}: {} Raft is already initialized - skipping become_cluster_member()",
            state.id,
            raft_type.as_str(),
        );
        set_raft_running(&state, raft_type);

        // Wait until we have a leader before returning to the main application.
        // Only makes sense for actual HA deployments.
        if nodes.len() > 1 {
            time::sleep(Duration::from_secs(1)).await;
            let mut metrics = helpers::get_raft_metrics(&state, raft_type).await;
            info!("Waiting for Raft Leader");
            for _ in 0..5 {
                // Make sure that this node is not the current leader,
                // which can happen after too quick restarts.
                if let Some(id) = metrics.current_leader
                    && id != this_node
                {
                    info!("Current Raft Leader: {id}");
                    break;
                }
                time::sleep(Duration::from_millis(1000)).await;
                metrics = helpers::get_raft_metrics(&state, raft_type).await;
            }
        }

        return Ok(());
    }

    let client = build_http_client(tls_no_verify);
    let scheme = if tls { "https" } else { "http" };

    // It is possible that this node is un-initialized while still being a member on a remote
    // cluster. This can happen, if e.g. the volume got lost and the node was a member before
    // already.
    // We will only get here, if a Raft membership state does not exist! Therefore, we want to
    // make sure we start clean again and that the remote cluster is clean as well.
    if is_remote_cluster_member(&state, raft_type, &client, scheme, this_node, nodes).await {
        leave_remote_cluster(
            &state, raft_type, &client, scheme, this_node, nodes, 10, false,
        )
        .await
        .expect("Cannot leave remote cluster");
    }
    set_raft_running(&state, raft_type);

    let this_node = get_this_node(this_node, nodes);
    let payload = serialize(&LearnerReq {
        node_id: this_node.id,
        addr_api: this_node.addr_api,
        addr_raft: this_node.addr_raft,
    })?;

    info!(
        "Node {}: Trying to become {} raft learner",
        state.id,
        raft_type.as_str()
    );
    let skip = try_become(
        &state,
        raft_type,
        &client,
        scheme,
        "add_learner",
        &payload,
        this_node.id,
        nodes,
        true,
    )
    .await?;
    // TODO check if this is still possible with the addition of "leave before proceed"
    if skip == SkipBecome::Yes {
        info!(
            "Node {}: Became a {:?} Raft member in the meantime - skipping further init",
            state.id, raft_type,
        );
        return Ok(());
    }
    info!(
        "Node {}: Successfully became {} raft learner",
        state.id,
        raft_type.as_str()
    );

    // If we try to become a member too fast and the request arrives at remote directly in between
    // closing and re-opening the socket to us again, and it then also badly overlaps with the raft
    // membership modification, we can get into a deadlock situation on the leader.
    // We want to wait until we are a commited Raft learner.
    {
        let mut metrics = helpers::get_raft_metrics(&state, raft_type).await;

        let mut are_we_learner = metrics
            .membership_config
            .nodes()
            .any(|(id, _)| *id == state.id);
        while !are_we_learner {
            info!("Waiting until we are a replicated Raft Learner ...",);
            time::sleep(Duration::from_secs(1)).await;
            metrics = helpers::get_raft_metrics(&state, raft_type).await;
            are_we_learner = metrics
                .membership_config
                .nodes()
                .any(|(id, _)| *id == state.id);
        }
        info!(
            "Node {}: Successfully became replicated {} raft learner",
            state.id,
            raft_type.as_str(),
        );
    }

    if state.learner_only {
        info!(
            "Node {}: learner_only=true - skipping {} raft voter promotion",
            state.id,
            raft_type.as_str(),
        );
        return Ok(());
    }

    info!(
        "Node {}: Trying to become {:?} raft member",
        state.id, raft_type
    );
    try_become(
        &state,
        raft_type,
        &client,
        scheme,
        "become_member",
        &payload,
        this_node.id,
        nodes,
        false,
    )
    .await?;
    info!(
        "Node {}: Successfully became {} raft member",
        state.id,
        raft_type.as_str()
    );

    {
        let mut metrics = helpers::get_raft_metrics(&state, raft_type).await;

        // To smooth out startups, wait until this node has replicated its
        // own voter state logs.
        let mut are_we_voter = metrics
            .membership_config
            .voter_ids()
            .any(|id| id == state.id);
        while !are_we_voter {
            info!("Waiting until we are a replicated Raft Voter ...",);
            time::sleep(Duration::from_secs(1)).await;
            metrics = helpers::get_raft_metrics(&state, raft_type).await;
            are_we_voter = metrics
                .membership_config
                .voter_ids()
                .any(|id| id == state.id);
        }
        info!(
            "Node {}: Successfully became a replicated {} raft member",
            state.id,
            raft_type.as_str(),
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn try_become(
    state: &Arc<AppState>,
    raft_type: &RaftType,
    client: &reqwest::Client,
    scheme: &str,
    suffix: &str,
    payload: &[u8],
    this_node: u64,
    nodes: &[Node],
    check_init: bool,
) -> Result<SkipBecome, Error> {
    let mut url = String::with_capacity(48);
    loop {
        // maybe we are initialized in the meantime
        if check_init && helpers::is_raft_initialized(state, raft_type).await? {
            info!(
                "Init check at loop start in try_become - this node became the raft leader in \
            the meantime - skipping init"
            );
            return Ok(SkipBecome::Yes);
        }

        for node in nodes {
            if node.id == this_node {
                debug!("Skipping 'this' node");
                continue;
            }

            url.clear();
            write!(
                url,
                "{}://{}/cluster/{}/{}",
                scheme,
                node.addr_api,
                suffix,
                raft_type.as_str()
            )?;
            debug!("Sending request to {}", url);

            let res = client
                .post(&url)
                .header(HEADER_NAME_SECRET, &state.secret_api)
                .body(payload.to_vec())
                .send()
                .await;
            debug!("raw request to {}: {:?}", url, res);

            match res {
                Ok(resp) => {
                    if resp.status().is_success() {
                        debug!("becoming a member via /{suffix} was successful");
                        return Ok(SkipBecome::No);
                    } else {
                        let body = resp.bytes().await?;
                        let err: Error = serde_json::from_slice(&body)?;

                        // TODO can this still happen after we added the "leave before proceed"?
                        // We can get into this situation when using the cache layer, because it has
                        // no persistence. This race condition can happen for a rolling release
                        // on K8s for instance. While this node may try to become a remote member,
                        // the raft has decided that this node is the new leader.
                        //
                        // -> We must check this after each error to get smooth rolling releases.
                        if let Some((Some(leader_id), Some(node))) = err.is_forward_to_leader() {
                            info!(
                                "Node {} become '{}' member on remote ({}): Remote Node is not the leader - trying next",
                                this_node,
                                raft_type.as_str(),
                                url,
                            );

                            // should never happen at this point
                            if leader_id == this_node {
                                if !helpers::is_raft_initialized(state, raft_type).await? {
                                    let leader = helpers::get_raft_leader(state, raft_type).await;
                                    let metrics = helpers::get_raft_metrics(state, raft_type).await;

                                    panic!(
                                        r#"
    Raft is not initialized when remote node has 'this' as leader.
    This can only happen for an in-memory cache node and a too fast restart.
    Because the in-memory Raft does not save the state between restarts, you must way at least
    for the duration of a leader heartbeat timeout before trying to re-join the cluster.

    Raft Type: {raft_type:?}
    This node: {this_node}
    Leader:    {leader:?}: {node:?}
    Metrics:   {metrics:?}
"#
                                    );
                                }

                                info!(
                                    "This node became the raft leader in the meantime - skipping init"
                                );

                                return Ok(SkipBecome::Yes);
                            }
                        } else {
                            error!(
                                "Node {} become '{}' member on remote ({}): {}",
                                this_node,
                                raft_type.as_str(),
                                url,
                                err
                            );
                        }

                        time::sleep(Duration::from_millis(500)).await;
                    }
                }
                Err(err) => {
                    error!("Node connection error: {}", err);

                    time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }
}

/// Make sure this function does not return a Result, which could get us into a locked situation,
/// because the Raft is set to stopped while we check this.
#[tracing::instrument(skip(state, client, scheme))]
async fn is_remote_cluster_member(
    state: &Arc<AppState>,
    raft_type: &RaftType,
    client: &reqwest::Client,
    scheme: &str,
    this_node: u64,
    nodes: &[Node],
) -> bool {
    let mut url = String::with_capacity(48);

    // "This" Node is the +1 for quorum
    let quorum = nodes.len() / 2;

    // check remote metrics for our existence first
    let mut not_initialized_remotes = 0;

    // we want to re-try twice to really make sure there was no network hiccup
    for _ in 0..2 {
        for node in nodes {
            if node.id == this_node {
                debug!("Skipping 'this' node");
                continue;
            }
            if not_initialized_remotes >= quorum {
                info!(
                    "Found {} remote Nodes that are not initialized - must be a fresh cluster",
                    not_initialized_remotes
                );
            }

            url.clear();
            write!(
                url,
                "{}://{}/cluster/metrics/{}",
                scheme,
                node.addr_api,
                raft_type.as_str()
            )
            .expect("Cannot write into String");

            let res = client
                .get(&url)
                .header(HEADER_NAME_SECRET, &state.secret_api)
                .send()
                .await;

            match res {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let Ok(bytes) = resp.bytes().await else {
                            error!("Success response from remote without body");
                            time::sleep(Duration::from_secs(1)).await;
                            continue;
                        };
                        let metrics = deserialize::<RaftMetrics<u64, Node>>(bytes.as_ref())
                            .expect("Cannot deserialize remote metrics response");

                        let is_member = metrics
                            .membership_config
                            .nodes()
                            .any(|(id, _)| *id == this_node);
                        if is_member {
                            // if there already is a remote cluster, and we are not part of it,
                            // everything should be fine
                            warn!("Found remote metrics and we ({this_node}) are a Raft member");
                            return true;
                        } else {
                            info!(
                                "Found remote metrics, but we ({this_node}) are not a Raft member"
                            );
                            return false;
                        }
                    } else {
                        // We reached the remote node, but it was not possible to get cluster
                        // metrics. This can only mean, that remote is not initialized as well.
                        not_initialized_remotes += 1;

                        let body = resp
                            .bytes()
                            .await
                            .expect("API answer to always have a body");
                        let err: Error =
                            serde_json::from_slice(&body).expect("To always get back a JSON error");
                        error!(
                            "Error retrieving {:?} Raft metrics from remote Node {}: {:?}",
                            raft_type, node.id, err
                        );

                        time::sleep(Duration::from_secs(1)).await;
                    }
                }
                Err(err) => {
                    error!("Node connection error: {}", err);
                    time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    false
}

#[tracing::instrument(skip(state, client, scheme))]
#[allow(clippy::too_many_arguments)]
pub async fn leave_remote_cluster(
    state: &Arc<AppState>,
    raft_type: &RaftType,
    client: &reqwest::Client,
    scheme: &str,
    this_node: u64,
    nodes: &[Node],
    retries: usize,
    stay_as_learner: bool,
) -> Result<(), Error> {
    let mut url = String::with_capacity(48);

    let payload = serialize(&ClusterLeaveReq {
        node_id: this_node,
        stay_as_learner,
    })?;
    let mut left_cluster = false;
    'outer: for _ in 0..retries + 1 {
        for node in nodes {
            if node.id == this_node {
                debug!("Skipping 'this' node");
                continue;
            }

            // We will just try to send our request to all nodes in order without looking up
            // the leader via metrics first, as this can change at any time anyway. The request
            // will only succeed, if the remote node is a leader anyway.
            url.clear();
            write!(
                url,
                "{}://{}/cluster/membership/{}",
                scheme,
                node.addr_api,
                raft_type.as_str()
            )?;

            let res = client
                .delete(&url)
                .header(HEADER_NAME_SECRET, &state.secret_api)
                .body(payload.clone())
                .send()
                .await;

            match res {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!(
                            "This Node {this_node} left the remote {:?} cluster via {}",
                            raft_type, url
                        );
                        left_cluster = true;
                        break 'outer;
                    } else {
                        let body = resp.bytes().await?;
                        let err: Error = serde_json::from_slice(&body)?;
                        error!(
                            "Error removing this Node {} from remote {:?} Raft cluster: {:?}",
                            this_node,
                            raft_type.as_str(),
                            err
                        );
                    }
                }
                Err(err) => {
                    error!("Node {:?} connection error to {}: {}", raft_type, url, err);
                }
            }
        }
        time::sleep(Duration::from_secs(3)).await;
    }
    if !left_cluster {
        return Err(Error::Connect(
            "Could not leave the cluster after trying all nodes once".to_string(),
        ));
    }

    // After removal, query metrics again until this node is fully removed.
    // We need to do this on all nodes, because we don't know if any of them leave or join
    // in the meantime.
    for _ in 0..retries + 1 {
        for node in nodes {
            if node.id == this_node {
                debug!("Skipping 'this' node");
                continue;
            }

            url.clear();
            write!(
                url,
                "{}://{}/cluster/metrics/{}",
                scheme,
                node.addr_api,
                raft_type.as_str()
            )?;

            let Ok(res) = client
                .get(&url)
                .header(HEADER_NAME_SECRET, &state.secret_api)
                .send()
                .await
            else {
                error!(
                    "Unable to reach Node {} via {} to confirm cluster leave via metrics",
                    node.id, url
                );
                continue;
            };

            if res.status().is_success() {
                let bytes = res.bytes().await?;
                let metrics = deserialize::<RaftMetrics<u64, Node>>(bytes.as_ref())?;
                let is_member = metrics
                    .membership_config
                    .nodes()
                    .any(|(id, _)| *id == this_node);
                if is_member {
                    info!(
                        "This Node ({this_node}) is still a Raft member after removal - waiting ..."
                    );
                    time::sleep(Duration::from_secs(1)).await;
                    continue;
                } else {
                    info!("This Node ({this_node}) has been fully removed from the Raft.");
                    return Ok(());
                }
            }
        }
        time::sleep(Duration::from_secs(1)).await;
    }

    error!(
        "Node was removed from the cluster, but was unable to confirm this via metrics - retries exceeded"
    );

    Ok(())
}

// TODO get rid of the duplication here and make it prettier -> figure out generic types properly

#[cfg(feature = "sqlite")]
async fn is_initialized_timeout_sqlite(
    node_id: u64,
    raft: &openraft::Raft<TypeConfigSqlite>,
) -> Result<bool, Error> {
    let has_any_nodes = || {
        raft.metrics()
            .borrow()
            .membership_config
            .membership()
            .nodes()
            .any(|(id, _)| *id == node_id)
    };

    // Do not try to initialize already initialized nodes
    if raft.is_initialized().await? && has_any_nodes() {
        return Ok(true);
    }

    // If it is not initialized, wait long enough to make sure this
    // node is not joined again to an already existing cluster after data loss.
    let heartbeat = raft.config().heartbeat_interval;
    // We will wait for 5 heartbeats to make sure no other cluster is running
    time::sleep(Duration::from_millis(heartbeat * 5)).await;

    // Make sure we are not initialized by now, otherwise go on
    if raft.is_initialized().await? {
        if has_any_nodes() {
            Ok(true)
        } else {
            log_no_membership_error();
            Ok(false)
        }
    } else {
        Ok(false)
    }
}

#[cfg(feature = "cache")]
async fn is_initialized_timeout_cache(
    node_id: u64,
    raft: &openraft::Raft<TypeConfigKV>,
) -> Result<bool, Error> {
    let has_any_nodes = || {
        raft.metrics()
            .borrow()
            .membership_config
            .membership()
            .nodes()
            .any(|(id, _)| *id == node_id)
    };

    // Do not try to initialize already initialized nodes
    if raft.is_initialized().await? && has_any_nodes() {
        return Ok(true);
    }

    // If it is not initialized, wait long enough to make sure this
    // node is not joined again to an already existing cluster after data loss.
    let heartbeat = raft.config().heartbeat_interval;
    // We will wait for 5 heartbeats to make sure no other cluster is running
    time::sleep(Duration::from_millis(heartbeat * 5)).await;

    // Make sure we are not initialized by now, otherwise go on
    if raft.is_initialized().await? {
        if has_any_nodes() {
            Ok(true)
        } else {
            log_no_membership_error();
            Ok(false)
        }
    } else {
        Ok(false)
    }
}

fn set_raft_running(state: &Arc<AppState>, raft_type: &RaftType) {
    match raft_type {
        #[cfg(feature = "sqlite")]
        RaftType::Sqlite => {
            info!("Setting Sqlite Raft to running");
            state
                .raft_db
                .is_raft_stopped
                .store(false, Ordering::Relaxed);
            state
                .raft_db
                .is_startup_finished
                .store(true, Ordering::Relaxed);
        }
        #[cfg(feature = "cache")]
        RaftType::Cache => {
            info!("Setting Cache Raft to running");
            state
                .raft_cache
                .is_raft_stopped
                .store(false, Ordering::Relaxed);
            state
                .raft_cache
                .is_startup_finished
                .store(true, Ordering::Relaxed);
        }
        RaftType::Unknown => unreachable!(),
    }
}

fn log_no_membership_error() {
    error!(
        r#"

    Raft is initialized but the membership config is empty.
    This can usually only happen during the initialization of a fresh cluster, if your
    application crashed or is being force-killed in the middle of a cluster join.

    If this is a single instance, this Node cann probably not recover from this state on its own.
    You can fix this by starting with: `HQL_DANGER_RAFT_STATE_RESET=true`

    If this happens on a cluster Node and the other members are healthy, it may be able to recover
    when the remote leader tries to re-initialize it. If this failes, the easiest and safest fix
    is to delete the volume and let the Node re-join and sync the cluster data cleanly to not end
    up in an inconsistent state.
"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeConfig;

    fn node(id: u64) -> Node {
        Node {
            id,
            addr_raft: format!("n{id}:8100"),
            addr_api: format!("n{id}:8200"),
        }
    }

    /// 010 KD-2, first half. This module resolves "this node" by matching `id`;
    /// `start.rs:65-68` resolves it by indexing `nodes[node_id - 1]`. The two
    /// agree only when the ids are exactly `1..=n` in order, which nothing
    /// validates. With ids `2,3,4` the same `node_id` names two different nodes.
    #[test]
    fn node_identity_is_resolved_by_id_here_and_by_position_in_start() {
        let nodes = vec![node(2), node(3), node(4)];

        assert_eq!(get_this_node(3, &nodes).id, 3);
        assert_eq!(nodes.get(3_usize - 1).expect("index in range").id, 4);
    }

    /// 010 KD-2, second half. `NodeConfig::is_valid` bounds `node_id` against
    /// the *length* of `nodes`, never against the ids in it, so the diverging
    /// shape above is accepted as valid configuration.
    #[test]
    fn is_valid_accepts_a_nodes_list_whose_ids_are_not_positions() {
        let config = NodeConfig {
            node_id: 3,
            nodes: vec![node(2), node(3), node(4)],
            secret_raft: "a".repeat(16),
            secret_api: "b".repeat(16),
            #[cfg(any(feature = "dashboard", feature = "s3"))]
            enc_keys: cryptr::EncKeys {
                enc_key_active: "test".to_string(),
                enc_keys: vec![("test".to_string(), vec![0_u8; 32])],
            },
            #[cfg(feature = "dashboard")]
            password_dashboard: None,
            ..Default::default()
        };

        assert!(config.is_valid().is_ok());
    }

    /// 010 KD-2, third part: when the two routes disagree badly enough that the
    /// id is absent entirely, the join path panics rather than returning a
    /// configuration error.
    #[test]
    #[should_panic(expected = "this node to always exist in all nodes")]
    fn get_this_node_panics_when_the_id_is_absent() {
        let nodes = vec![node(2), node(3)];
        let _ = get_this_node(1, &nodes);
    }

    use test_peers::{Answer, SECRET, closed_addr, peer, some_raft_type, stub_peer};

    /// The bound these tests give the decision. Short, so a refusal is observed quickly.
    const F118_WAIT: Duration = Duration::from_millis(1500);

    /// `033` B-3: one authenticated "not initialized" at N=3 is `⌊3/2⌋ = 1` peer, which with
    /// node 1 is a majority: node 1 initializes, whatever the other peer does.
    #[tokio::test]
    async fn f118_one_explicit_answer_at_n3_is_a_fresh_cluster() {
        let p2 = stub_peer(Answer::NotInitialized).await;
        let nodes = vec![peer(1, &closed_addr()), peer(2, &p2), peer(3, &closed_addr())];
        let skip =
            should_node_1_skip_init(&some_raft_type(), &nodes, SECRET, false, false, F118_WAIT)
                .await
                .expect("one explicit answer is enough at N=3");
        assert!(!skip, "a fresh cluster: node 1 initializes");
    }

    /// `033` B-3: at N=5 the evidence needed is two distinct peers. One is not enough, and the
    /// same peer does not count twice however often it is asked.
    #[tokio::test]
    async fn f118_at_n5_two_distinct_peers_must_answer() {
        let p2 = stub_peer(Answer::NotInitialized).await;
        let one = vec![
            peer(1, &closed_addr()),
            peer(2, &p2),
            peer(3, &closed_addr()),
            peer(4, &closed_addr()),
            peer(5, &closed_addr()),
        ];
        let err =
            should_node_1_skip_init(&some_raft_type(), &one, SECRET, false, false, F118_WAIT)
                .await
                .expect_err("one of the two answers needed");
        let text = err.to_string();
        assert!(text.contains("needs 2 peer(s)") && text.contains("1 did"), "got: {text}");
        assert!(!text.contains("node 2 ("), "node 2 answered and is not silent: {text}");

        let p3 = stub_peer(Answer::NotInitialized).await;
        let two = vec![
            peer(1, &closed_addr()),
            peer(2, &p2),
            peer(3, &p3),
            peer(4, &closed_addr()),
            peer(5, &closed_addr()),
        ];
        assert!(
            !should_node_1_skip_init(&some_raft_type(), &two, SECRET, false, false, F118_WAIT)
                .await
                .expect("two explicit answers at N=5")
        );
    }

    /// An initialized peer is the one answer that ends the decision on its own: node 1 skips
    /// its init and joins, as before.
    #[tokio::test]
    async fn f118_an_initialized_peer_means_join_not_initialize() {
        let p2 = stub_peer(Answer::Initialized).await;
        let nodes = vec![peer(1, &closed_addr()), peer(2, &p2), peer(3, &closed_addr())];
        assert!(
            should_node_1_skip_init(&some_raft_type(), &nodes, SECRET, false, false, F118_WAIT)
                .await
                .expect("an initialized peer is an answer")
        );
        assert_eq!(
            peer_init_evidence(&some_raft_type(), 1, &nodes, SECRET, false, false, F118_WAIT)
                .await
                .unwrap(),
            PeerInitEvidence::Initialized { peer: 2 }
        );
    }

    /// `033` B-3: at N=1 the decision is unchanged. No peer is asked and nothing waits.
    #[tokio::test]
    async fn f118_n1_is_unchanged() {
        let nodes = vec![peer(1, &closed_addr())];
        let started = std::time::Instant::now();
        assert!(
            !should_node_1_skip_init(
                &some_raft_type(),
                &nodes,
                SECRET,
                false,
                false,
                Duration::from_secs(3600)
            )
            .await
            .unwrap()
        );
        assert!(started.elapsed() < Duration::from_secs(1), "no wait at N=1");
    }

    /// Review finding 7: a black-holed peer does not hold the decision. The peers are asked
    /// concurrently, each request capped, so a peer that answers is heard within the cap even
    /// when the peer listed before it never answers. Asked one after another, the first request
    /// took the whole wait and every later peer got a 100 ms floor.
    #[tokio::test]
    async fn f118_a_black_holed_peer_does_not_hold_the_decision() {
        let hung = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hung_addr = hung.local_addr().unwrap().to_string();
        let p3 = stub_peer(Answer::NotInitialized).await;
        let nodes = vec![peer(1, &closed_addr()), peer(2, &hung_addr), peer(3, &p3)];

        let started = std::time::Instant::now();
        let skip = time::timeout(
            Duration::from_secs(30),
            should_node_1_skip_init(
                &some_raft_type(),
                &nodes,
                SECRET,
                false,
                false,
                Duration::from_secs(20),
            ),
        )
        .await
        .expect("bounded")
        .expect("node 3 answered");
        assert!(!skip);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "decided within the per-request cap, not the whole wait: {:?}",
            started.elapsed()
        );
        drop(hung);
    }

    /// A peer that accepts the connection and never answers is bounded by the wait as well.
    #[tokio::test]
    async fn f118_a_peer_that_never_answers_is_bounded() {
        let hung = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hung_addr = hung.local_addr().unwrap().to_string();
        let nodes = vec![peer(1, &closed_addr()), peer(2, &hung_addr), peer(3, &hung_addr)];
        let res = time::timeout(
            Duration::from_secs(10),
            should_node_1_skip_init(&some_raft_type(), &nodes, SECRET, false, false, F118_WAIT),
        )
        .await
        .expect("bounded by the wait, not by the HTTP client's own timeout");
        assert!(res.is_err());
        drop(hung);
    }

    /// `033` B-3, F-118: a peer that cannot be reached is not evidence that the cluster is
    /// fresh. The unrepaired decision seeded node 1's own vote, which satisfied the quorum at
    /// N=3 before any peer was asked, so a connection error on node 2 still returned
    /// "initialize".
    #[tokio::test]
    async fn f118_unreachable_peers_are_not_evidence_of_a_fresh_cluster() {
        let nodes = vec![peer(1, &closed_addr()), peer(2, &closed_addr()), peer(3, &closed_addr())];

        let res = time::timeout(
            Duration::from_secs(10),
            should_node_1_skip_init(&some_raft_type(), &nodes, SECRET, false, false, F118_WAIT),
        )
        .await
        .expect("the decision is bounded and returns");

        let err = res.expect_err(
            "two silent peers are no evidence of a fresh cluster; initializing here is F-118",
        );
        let text = err.to_string();
        assert!(text.starts_with("Startup: "), "a startup error, got: {text}");
        assert!(text.contains("node 2") && text.contains("node 3"), "names the silent peers: {text}");
    }

    /// `033` B-3: an answer the peer refused to authenticate (`401`) counts for nothing. The
    /// unrepaired decision pushed every non-success answer, whatever it was, as a vote for
    /// "not initialized".
    #[tokio::test]
    async fn f118_an_unauthenticated_answer_is_not_evidence() {
        let p2 = stub_peer(Answer::Unauthorized).await;
        let p3 = stub_peer(Answer::Unauthorized).await;
        let nodes = vec![peer(1, &closed_addr()), peer(2, &p2), peer(3, &p3)];

        let res = time::timeout(
            Duration::from_secs(10),
            should_node_1_skip_init(&some_raft_type(), &nodes, SECRET, false, false, F118_WAIT),
        )
        .await
        .expect("the decision is bounded and returns");
        let err = res.expect_err("a 401 is not a peer saying it is not initialized");
        assert!(err.to_string().contains("401"), "says why the peer did not count: {err}");
    }

    /// `033` B-3: the error a peer answers while its raft is **stopped** is not evidence either.
    /// Before this repair a peer answered `400 Config("Raft node has not been initialized")`
    /// both when it was pristine and when it was initialized but not yet running (its start
    /// had not reached `set_raft_running`, or it was shutting down). Counting that answer let
    /// an initialized peer vote "fresh".
    #[tokio::test]
    async fn f118_a_stopped_peer_answer_is_not_evidence() {
        let p2 = stub_peer(Answer::StoppedOrLegacy).await;
        let p3 = stub_peer(Answer::StoppedOrLegacy).await;
        let nodes = vec![peer(1, &closed_addr()), peer(2, &p2), peer(3, &p3)];

        let res = time::timeout(
            Duration::from_secs(10),
            should_node_1_skip_init(&some_raft_type(), &nodes, SECRET, false, false, F118_WAIT),
        )
        .await
        .expect("the decision is bounded and returns");
        res.expect_err("an ambiguous error answer is not a peer saying it is not initialized");
    }
}

/// A stand-in for the peers a pristine node 1 asks, for `033` B-3's tests. Each stub is a real
/// HTTP/2 listener on an ephemeral port, so the request path, the secret header and the
/// status handling are the production ones.
#[cfg(test)]
pub(crate) mod test_peers {
    use crate::app_state::RaftType;
    use crate::network::HEADER_NAME_SECRET;
    use crate::{Error, Node, NodeId};
    use axum::Router;
    use axum::http::HeaderMap;
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use openraft::Membership;
    use std::collections::{BTreeMap, BTreeSet};

    pub(crate) const SECRET: &str = "a-test-api-secret-of-some-length";

    /// What a stub peer answers on `GET /cluster/membership/{raft_type}`.
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Answer {
        /// Authenticated `200` with an empty membership: the explicit "not initialized".
        NotInitialized,
        /// Authenticated `200` with a membership holding node 2: an initialized group.
        Initialized,
        /// `401`, as a peer with another secret answers.
        Unauthorized,
        /// `400 Config`, what an unrepaired peer answered both when pristine and when stopped.
        StoppedOrLegacy,
    }

    pub(crate) fn peer(id: NodeId, addr_api: &str) -> Node {
        Node {
            id,
            addr_raft: "127.0.0.1:1".to_string(),
            addr_api: addr_api.to_string(),
        }
    }

    /// An address nothing listens on: bound, then released.
    pub(crate) fn closed_addr() -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    }

    pub(crate) fn some_raft_type() -> RaftType {
        #[cfg(feature = "sqlite")]
        {
            RaftType::Sqlite
        }
        #[cfg(not(feature = "sqlite"))]
        {
            RaftType::Cache
        }
    }

    pub(crate) async fn stub_peer(answer: Answer) -> String {
        let handler = move |headers: HeaderMap| async move { respond(answer, &headers) };
        let app = Router::new().route("/cluster/membership/{raft_type}", get(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app.into_make_service()).await;
        });
        addr
    }

    fn respond(answer: Answer, headers: &HeaderMap) -> Response {
        let authenticated = headers
            .get(HEADER_NAME_SECRET)
            .is_some_and(|v| v.as_bytes() == SECRET.as_bytes());
        match answer {
            Answer::Unauthorized => Error::Token("Invalid API Secret".into()).into_response(),
            _ if !authenticated => Error::Token("Invalid API Secret".into()).into_response(),
            Answer::NotInitialized => {
                let m = Membership::<NodeId, Node>::default();
                crate::network::serialize_network(&m).into_response()
            }
            Answer::Initialized => {
                let m = Membership::<NodeId, Node>::new(
                    vec![BTreeSet::from([2])],
                    BTreeMap::from([(2, peer(2, "127.0.0.1:1"))]),
                );
                crate::network::serialize_network(&m).into_response()
            }
            Answer::StoppedOrLegacy => {
                Error::Config("Raft node has not been initialized".into()).into_response()
            }
        }
    }
}
