use crate::NodeId;
use crate::app_state::{AppState, RaftType};
use crate::membership_gate::{ADMISSION_WAIT, COMMIT_VISIBLE_WAIT, MembershipHeld};
use crate::network::{AppStateExt, Error, fmt_ok, get_payload, validate_secret};
use crate::{Node, helpers};
use axum::body;
use axum::body::Body;
use axum::extract::Path;
use axum::http::HeaderMap;
use axum::response::Response;
use openraft::error::{CheckIsLeaderError, ForwardToLeader, RaftError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info, warn};

#[derive(Debug, Serialize, Deserialize)]
pub struct LearnerReq {
    pub node_id: u64,
    pub addr_api: String,
    pub addr_raft: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClusterLeaveReq {
    pub node_id: u64,
    pub stay_as_learner: bool,
}

#[tracing::instrument(skip_all)]
pub(crate) async fn add_learner(
    state: AppStateExt,
    headers: HeaderMap,
    Path(raft_type): Path<RaftType>,
    body: body::Bytes,
) -> Result<Response, Error> {
    validate_secret(&state, &headers)?;
    // F-069: `unknown` is a value this path parameter deserializes to, and the helpers below
    // answered it with a panic about a build configuration.
    raft_type.selected()?;

    if helpers::is_raft_stopped(&state, &raft_type)
        || !helpers::is_raft_initialized(&state, &raft_type).await?
    {
        return Err(Error::Error("Raft is not initialized".into()));
    }
    are_we_leader(&state, &raft_type).await?;

    let LearnerReq {
        node_id,
        addr_api,
        addr_raft,
    } = get_payload(&headers, body)?;
    let node = Node {
        id: node_id,
        addr_raft,
        addr_api,
    };
    info!("{:?} requests to be added as {:?} Learner", node, raft_type);
    let lock = admit_membership_change(&state, &raft_type).await?;
    let nid = node.id;
    let res = helpers::add_new_learner(&state, &raft_type, node, &lock).await;
    match res {
        Ok(_) => {
            wait_until_visible(&state, &raft_type, &lock, "a committed learner", |m| {
                m.membership_config.membership().get_node(&nid).is_some()
            })
            .await?;

            // give it a second to sync before dropping the lock
            time::sleep(Duration::from_millis(1000)).await;
            drop(lock);
            info!("Added node {nid} as commited {:?} learner", raft_type);
            fmt_ok(headers, ())
        }
        Err(err) => {
            error!("Error adding node as {:?} learner: {:?}", raft_type, err);
            Err(err)
        }
    }
}

/// Changes specified learners to members, or remove members.
#[tracing::instrument(skip_all)]
pub(crate) async fn become_member(
    state: AppStateExt,
    headers: HeaderMap,
    Path(raft_type): Path<RaftType>,
    body: body::Bytes,
) -> Result<Response, Error> {
    validate_secret(&state, &headers)?;
    // F-069: `unknown` is a value this path parameter deserializes to, and the helpers below
    // answered it with a panic about a build configuration.
    raft_type.selected()?;

    if helpers::is_raft_stopped(&state, &raft_type)
        || !helpers::is_raft_initialized(&state, &raft_type).await?
    {
        return Err(Error::Error("Raft is not initialized".into()));
    }
    are_we_leader(&state, &raft_type).await?;

    let payload = get_payload::<LearnerReq>(&headers, body)?;
    let lock = admit_membership_change(&state, &raft_type).await?;
    info!("{:?} Node membership request: {:?}", raft_type, payload);

    let metrics = helpers::get_raft_metrics(&state, &raft_type).await;
    debug!("{:?} Members before add: {:?}", raft_type, metrics);

    let is_voter = metrics
        .membership_config
        .voter_ids()
        .any(|id| id == payload.node_id);
    if is_voter {
        info!(
            "Node {} is a voter already - nothing left to do",
            payload.node_id
        );
        return fmt_ok(headers, ());
    }

    let mut nodes_set = metrics
        .membership_config
        .voter_ids()
        .collect::<BTreeSet<u64>>();
    nodes_set.insert(payload.node_id);

    match helpers::change_membership(&state, &raft_type, nodes_set, true, &lock).await {
        Ok(_) => {
            let nid = payload.node_id;
            wait_until_visible(&state, &raft_type, &lock, "a committed voter", |m| {
                m.membership_config.voter_ids().any(|id| id == nid)
            })
            .await?;

            // give it a second to sync before dropping the lock
            time::sleep(Duration::from_millis(1000)).await;
            drop(lock);
            info!("Added node {} as {:?} member", payload.node_id, raft_type);
            fmt_ok(headers, ())
        }
        Err(err) => {
            error!("Error adding node as member: {:?}", err);
            Err(err)
        }
    }
}

/// Why a node may not commit a membership change, or `Ok` if it may.
///
/// Extracted as a pure decision so the reasoning can be tested without a running cluster.
/// F-107 was a hole in exactly this decision, and the sequence that exposed it is not one a
/// test can schedule: this node left the cache cluster and a peer's leave request arrived one
/// millisecond later.
pub(crate) fn membership_change_allowed(
    is_shutting_down: bool,
    leader: Option<NodeId>,
    this_node: NodeId,
    this_node_is_voter: bool,
) -> Result<(), Error> {
    // A node on its way out of the membership it would be changing. The leaving is not atomic
    // with the answer given here, so refusing early sends the caller to a node that will still
    // be a member when it acts.
    if is_shutting_down {
        return Err(Error::LeaderChange(
            "this node is shutting down and cannot serve a membership change; ask another node"
                .into(),
        ));
    }

    match leader {
        None => Err(Error::LeaderChange("Leader election in progress".into())),
        Some(leader_id) if leader_id != this_node => Err(Error::LeaderChange(
            "this node is not the leader for this raft".into(),
        )),
        // Being the leader is not sufficient, and this is the invariant openraft asserts
        // internally. A leader that has removed **itself** from the voters keeps reporting
        // leadership for a window, and a membership change committed in that window reaches
        // `append_membership` on a node openraft no longer considers entitled to make one. In a
        // test build that is a `debug_assert!` panic; in a release build it is the same state
        // change, unchecked.
        Some(_) if !this_node_is_voter => Err(Error::LeaderChange(
            "this node reports itself leader but is no longer a voter, so it cannot commit a \
             membership change; ask another node"
                .into(),
        )),
        Some(_) => Ok(()),
    }
}

async fn are_we_leader(state: &AppStateExt, raft_type: &RaftType) -> Result<(), Error> {
    let leader = helpers::get_raft_leader(state, raft_type).await;

    // The non-leader case keeps its richer error, which carries the leader's address so a
    // caller can forward rather than guess. Everything else goes through the decision above.
    if let Some(leader_id) = leader
        && leader_id != state.id
    {
        let metrics = helpers::get_raft_metrics(state, raft_type).await;
        let Some(leader_node) = metrics.membership_config.membership().get_node(&leader_id) else {
            return Err(Error::Error(
                format!("Leader {leader_id} not found in membership config").into(),
            ));
        };

        let err = RaftError::APIError(CheckIsLeaderError::ForwardToLeader(ForwardToLeader {
            leader_id: Some(leader_id),
            leader_node: Some(leader_node.clone()),
        }));
        return Err(Error::CheckIsLeaderError(Box::new(err)));
    }

    let this_node_is_voter = helpers::get_raft_metrics(state, raft_type)
        .await
        .membership_config
        .voter_ids()
        .any(|id| id == state.id);

    membership_change_allowed(
        state
            .is_shutting_down
            .load(std::sync::atomic::Ordering::Relaxed),
        leader,
        state.id,
        this_node_is_voter,
    )
}

/// The membership decision, taken from this node's metrics as they are now.
///
/// Only meaningful under the membership gate, which is the only place it is called from.
pub(crate) fn decide_membership_change_now(
    state: &Arc<AppState>,
    raft_type: &RaftType,
) -> Result<(), Error> {
    if helpers::is_raft_stopped(state, raft_type) {
        return Err(Error::LeaderChange(
            "this node's raft is stopped and cannot serve a membership change; ask another node"
                .into(),
        ));
    }
    let metrics = match raft_type {
        #[cfg(feature = "sqlite")]
        RaftType::Sqlite => state.raft_db.raft.metrics().borrow().clone(),
        #[cfg(feature = "cache")]
        RaftType::Cache => state.raft_cache.raft.metrics().borrow().clone(),
        RaftType::Unknown => return Err(Error::Config("unknown raft type".into())),
    };
    // openraft's first assertion in `append_membership` is on the server state, not on the
    // reported leader id, and the two can disagree while a self-removal is being applied.
    if metrics.current_leader == Some(state.id) && metrics.state != openraft::ServerState::Leader {
        return Err(Error::LeaderChange(
            "this node reports itself leader but its raft is no longer in the leader state; ask \
             another node"
                .into(),
        ));
    }
    membership_change_allowed(
        state.membership.is_closed(),
        metrics.current_leader,
        state.id,
        metrics
            .membership_config
            .voter_ids()
            .any(|id| id == state.id),
    )
}

/// F-107: admit a membership change through the gate, decided under the lock.
pub(crate) async fn admit_membership_change(
    state: &Arc<AppState>,
    raft_type: &RaftType,
) -> Result<MembershipHeld, Error> {
    state
        .membership
        .admit(ADMISSION_WAIT, || {
            decide_membership_change_now(state, raft_type)
        })
        .await
}

/// Wait, bounded, until this node's metrics show what an admitted change committed.
///
/// These loops used to be unbounded while holding the membership lock, which let one stuck
/// change hold every other membership change and the shutdown drain indefinitely.
async fn wait_until_visible(
    state: &Arc<AppState>,
    raft_type: &RaftType,
    _held: &MembershipHeld,
    what: &str,
    done: impl Fn(&openraft::RaftMetrics<NodeId, Node>) -> bool,
) -> Result<(), Error> {
    let deadline = time::Instant::now() + COMMIT_VISIBLE_WAIT;
    loop {
        let metrics = match raft_type {
            #[cfg(feature = "sqlite")]
            RaftType::Sqlite => state.raft_db.raft.metrics().borrow().clone(),
            #[cfg(feature = "cache")]
            RaftType::Cache => state.raft_cache.raft.metrics().borrow().clone(),
            RaftType::Unknown => return Err(Error::Config("unknown raft type".into())),
        };
        if done(&metrics) {
            return Ok(());
        }
        if time::Instant::now() >= deadline {
            return Err(Error::Timeout(format!(
                "{raft_type:?} membership change was accepted but this node did not observe {what} \
                 within {COMMIT_VISIBLE_WAIT:?}: {:?}",
                metrics.membership_config.membership()
            )));
        }
        info!("Waiting for {what} ({raft_type:?})");
        time::sleep(Duration::from_millis(500)).await;
    }
}

pub(crate) async fn get_membership(
    state: AppStateExt,
    headers: HeaderMap,
    Path(raft_type): Path<RaftType>,
) -> Result<Response, Error> {
    validate_secret(&state, &headers)?;
    // F-069: `unknown` is a value this path parameter deserializes to, and the helpers below
    // answered it with a panic about a build configuration.
    raft_type.selected()?;

    if helpers::is_raft_stopped(&state, &raft_type)
        || !helpers::is_raft_initialized(&state, &raft_type).await?
    {
        return Err(Error::Config("Raft node has not been initialized".into()));
    }

    let metrics = helpers::get_raft_metrics(&state, &raft_type).await;
    let mut members = metrics.membership_config;

    // it is possible to end up in a race condition on rolling releases
    if members.nodes().count() == 0 {
        time::sleep(Duration::from_millis(1000)).await;
        let metrics = helpers::get_raft_metrics(&state, &raft_type).await;
        members = metrics.membership_config;
        debug!("Membership after 1000ms timeout: {:?}", members);

        // if we still have no members, return an error
        return Err(Error::Config(
            "Node is initialized but has no members".into(),
        ));
    }

    fmt_ok(headers, members.membership())
}

/// Changes specified learners to members, or remove members.
pub(crate) async fn post_membership(
    state: AppStateExt,
    headers: HeaderMap,
    Path(raft_type): Path<RaftType>,
    body: body::Bytes,
) -> Result<Response, Error> {
    validate_secret(&state, &headers)?;
    // F-069: `unknown` is a value this path parameter deserializes to, and the helpers below
    // answered it with a panic about a build configuration.
    raft_type.selected()?;

    if helpers::is_raft_stopped(&state, &raft_type)
        || !helpers::is_raft_initialized(&state, &raft_type).await?
    {
        return Err(Error::Config("Raft node has not been initialized".into()));
    }

    // The richer refusal first, so a follower still answers with the leader's address.
    are_we_leader(&state, &raft_type).await?;
    let payload = get_payload::<BTreeSet<NodeId>>(&headers, body)?;
    // F-107: this path changed membership with neither the decision nor the lock.
    let _held = admit_membership_change(&state, &raft_type).await?;
    helpers::change_membership(&state, &raft_type, payload, false, &_held).await?;

    // retain false removes current cluster members if they do not appear in the new list
    fmt_ok(headers, ())
}

#[tracing::instrument(skip_all)]
pub async fn leave_cluster(
    state: AppStateExt,
    headers: HeaderMap,
    Path(raft_type): Path<RaftType>,
    body: body::Bytes,
) -> Result<Response, Error> {
    validate_secret(&state, &headers)?;
    // F-069: `unknown` is a value this path parameter deserializes to, and the helpers below
    // answered it with a panic about a build configuration.
    raft_type.selected()?;

    if helpers::is_raft_stopped(&state, &raft_type)
        || !helpers::is_raft_initialized(&state, &raft_type).await?
    {
        return Err(Error::Config("Raft node has not been initialized".into()));
    }
    are_we_leader(&state, &raft_type).await?;

    let payload = get_payload::<ClusterLeaveReq>(&headers, body)?;
    let held = admit_membership_change(&state, &raft_type).await?;
    leave_cluster_exec(&state.0, &raft_type, payload, &held).await?;

    Ok(Response::new(Body::empty()))
}

/// Remove a node from a raft group. Requires the membership gate, held by an admitted request or
/// by a drained shutdown performing its own self-leave.
pub(crate) async fn leave_cluster_exec(
    state: &Arc<AppState>,
    raft_type: &RaftType,
    payload: ClusterLeaveReq,
    held: &MembershipHeld,
) -> Result<(), Error> {
    info!("{:?} Node {:?}", raft_type, payload);

    let mut metrics = helpers::get_raft_metrics(state, raft_type).await;
    let is_member = metrics
        .membership_config
        .nodes()
        .any(|(id, _)| *id == payload.node_id);

    if is_member {
        warn!(
            "Node {} ({:?}) is a cluster member - removing it",
            payload.node_id, raft_type
        );
        let is_voter = metrics
            .membership_config
            .voter_ids()
            .any(|id| id == payload.node_id);

        if is_voter {
            warn!("Node {} ({:?}) is a Voter", payload.node_id, raft_type);
            if let Err(err) = helpers::remove_voter(
                state,
                raft_type,
                payload.node_id,
                payload.stay_as_learner,
                held,
            )
            .await
            {
                error!(
                    "Error removing Node {} ({:?}) from Voters: {:?}",
                    payload.node_id, raft_type, err
                );
                return Err(err);
            }
            let nid = payload.node_id;
            wait_until_visible(state, raft_type, held, "the node leaving the voters", |m| {
                !m.membership_config.voter_ids().any(|id| id == nid)
            })
            .await?;
        } else if !payload.stay_as_learner {
            warn!(
                "Node {} ({:?}) is a Learner and should not stay one",
                payload.node_id, raft_type
            );
            if let Err(err) = helpers::remove_learner(state, raft_type, payload.node_id, held).await
            {
                error!(
                    "Error removing Node {} ({:?}) from Learners: {:?}",
                    payload.node_id, raft_type, err
                );
                return Err(err);
            }
            let nid = payload.node_id;
            wait_until_visible(state, raft_type, held, "the learner leaving", |m| {
                !m.membership_config.nodes().any(|(id, _)| *id == nid)
            })
            .await?;
        }
    }

    metrics = helpers::get_raft_metrics(state, raft_type).await;
    info!(
        "Node {} ({:?}) has left the cluster: {:?}",
        payload.node_id,
        raft_type,
        metrics.membership_config.membership()
    );

    Ok(())
}

/// Get the latest metrics of the cluster
pub(crate) async fn metrics(
    state: AppStateExt,
    headers: HeaderMap,
    Path(raft_type): Path<RaftType>,
) -> Result<Response, Error> {
    validate_secret(&state, &headers)?;
    raft_type.selected()?;

    let metrics = helpers::get_raft_metrics(&state, &raft_type).await;
    fmt_ok(headers, &metrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F-107, driven as a decision rather than as a race.
    ///
    /// The sequence that produced the defect cannot be scheduled by a test: this node left the
    /// cache cluster and a peer's leave request arrived one millisecond later, so
    /// `are_we_leader` said yes and openraft then refused the membership append the answer had
    /// authorized. Every input combination is enumerated here instead.
    #[test]
    fn a_node_leaving_its_own_cluster_may_not_commit_a_membership_change() {
        // The case that was missing, and the whole reason this exists: leader by its own
        // metrics, already out of the voter set.
        let err = membership_change_allowed(false, Some(1), 1, false)
            .expect_err("a leader that is no longer a voter must be refused");
        let msg = err.to_string();
        assert!(msg.contains("no longer a voter"), "got: {msg}");
        assert!(
            msg.contains("ask another node"),
            "the caller must be told what to do instead, got: {msg}"
        );

        // Shutting down wins over everything, including a node that still looks healthy.
        let err = membership_change_allowed(true, Some(1), 1, true)
            .expect_err("a shutting-down node must be refused");
        assert!(err.to_string().contains("shutting down"), "got: {err}");

        // No leader, and a leader that is someone else.
        assert!(
            membership_change_allowed(false, None, 1, true)
                .expect_err("no leader")
                .to_string()
                .contains("election in progress")
        );
        assert!(
            membership_change_allowed(false, Some(2), 1, true)
                .expect_err("not the leader")
                .to_string()
                .contains("not the leader")
        );

        // And the one case that must still be allowed, or nothing could ever join or leave.
        membership_change_allowed(false, Some(1), 1, true)
            .expect("a voting leader that is not shutting down may commit a membership change");
    }

    /// The refusals are all `LeaderChange`, which the HTTP layer maps to `409 CONFLICT`, and
    /// `leave_remote_cluster` already walks to the next node on a non-success. A refusal is
    /// therefore a redirect, not a failed leave.
    #[test]
    fn every_refusal_tells_the_caller_to_try_elsewhere() {
        for (shutting, leader, voter) in [
            (true, Some(1u64), true),
            (false, Some(1u64), false),
            (false, None, true),
        ] {
            let err = membership_change_allowed(shutting, leader, 1, voter)
                .expect_err("this combination must be refused");
            assert!(
                matches!(err, Error::LeaderChange(_)),
                "a refusal must be retryable elsewhere, got: {err:?}"
            );
        }
    }
}
