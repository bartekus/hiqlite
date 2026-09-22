use crate::app_state::AppState;
use crate::helpers::deserialize;
use crate::network::HEADER_NAME_SECRET;
use crate::{Error, Node};
use openraft::{RaftMetrics, StoredMembership};
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::{task, time};
use tracing::{debug, error, warn};

/// Read the check interval, as a configuration value rather than as a panic.
///
/// F-009: this was `.expect("Cannot parse HQL_SPLIT_BRAIN_INTERVAL as u64")` inside a spawned
/// task, so a malformed value ended the process under `panic = "abort"` and killed one task
/// silently under unwinding. For an embedded node that profile belongs to the consumer, so
/// neither outcome is something hiqlite gets to choose. It is now a startup error the caller
/// receives from the constructor.
pub(crate) fn split_brain_interval_from(
    raw: Option<&str>,
) -> Result<Duration, crate::Error> {
    let raw = raw.unwrap_or("60");
    let secs = raw.trim().parse::<u64>().map_err(|err| {
        crate::Error::Startup(
            format!("HQL_SPLIT_BRAIN_INTERVAL must be a whole number of seconds: {err}").into(),
        )
    })?;
    if secs == 0 {
        return Err(crate::Error::Startup(
            "HQL_SPLIT_BRAIN_INTERVAL must be greater than zero".into(),
        ));
    }
    Ok(Duration::from_secs(secs))
}

pub fn spawn(
    state: Arc<AppState>,
    nodes: Vec<Node>,
    tls: bool,
) -> Result<(), crate::Error> {
    let interval = split_brain_interval_from(env::var("HQL_SPLIT_BRAIN_INTERVAL").ok().as_deref())?;

    // The watchdog is gone, and F-014 is why. Under `panic = "abort"` the checker's own panic
    // already terminated the process, so the watchdog was unreachable for its stated purpose;
    // under unwinding, which is the default for dev and test profiles and for any downstream
    // consumer that does not set abort, both the checker and the watchdog panicked into
    // `JoinHandle`s nobody awaited, so split-brain checking stopped silently and nothing
    // terminated. An `assert!` in a task nobody joins is not a safety net.
    //
    // What replaces it is the checker not panicking: its one unvalidated read is now validated
    // above, before the task is spawned.
    task::spawn(Box::pin(check_split_brain(state, nodes, tls, interval)));
    Ok(())
}

async fn check_split_brain(
    state: Arc<AppState>,
    nodes: Vec<Node>,
    tls: bool,
    interval: Duration,
) {

    loop {
        time::sleep(interval).await;

        #[cfg(feature = "sqlite")]
        match state.raft_db.raft.current_leader().await {
            None => {
                warn!("Node {}: No leader for DB", state.id);
            }
            Some(leader_expected) => {
                debug!("Node {}: Raft DB Leader: {}", state.id, leader_expected);
                let metrics = state.raft_db.raft.metrics().borrow().clone();
                let membership = metrics.membership_config;

                if let Err(err) = check_compare_membership(
                    &state,
                    &nodes,
                    membership,
                    leader_expected,
                    "sqlite",
                    tls,
                )
                .await
                {
                    error!(
                        "Node {}: Error during check_compare_membership: {}",
                        state.id, err
                    );
                }
            }
        };

        #[cfg(feature = "cache")]
        match state.raft_cache.raft.current_leader().await {
            None => {
                warn!("Node {}: No leader for Cache", state.id);
            }
            Some(leader_expected) => {
                debug!("Node {}: Raft Cache Leader: {}", state.id, leader_expected);
                let metrics = state.raft_cache.raft.metrics().borrow().clone();
                let membership = metrics.membership_config;

                if let Err(err) = check_compare_membership(
                    &state,
                    &nodes,
                    membership,
                    leader_expected,
                    "cache",
                    tls,
                )
                .await
                {
                    error!(
                        "Node {}: Error during check_compare_membership: {}",
                        state.id, err
                    );
                }
            }
        };
    }
}

fn check_nodes_in_members(
    node_id: u64,
    typ: &str,
    node_id_remote: u64,
    nodes: &[Node],
    membership: &Arc<StoredMembership<u64, Node>>,
) {
    let members = membership.nodes().map(|(id, _)| *id).collect::<Vec<_>>();

    for node in nodes {
        if !members.contains(&node.id) {
            warn!(
                r#"

Node {}: {} node {} not in membership config from Node {node_id_remote}: {:?}
If the missing node is up and running, this is a split brain and should not happen.
If however the missing node is currently offline or just starting up, you can ignore this message.
"#,
                node_id, typ, node.id, members
            );
        }
    }
}

async fn check_compare_membership(
    state: &Arc<AppState>,
    nodes: &[Node],
    membership: Arc<StoredMembership<u64, Node>>,
    leader_expected: u64,
    path: &str,
    tls: bool,
) -> Result<(), Error> {
    let nodes_to_check = nodes
        .iter()
        .filter(|node| node.id != leader_expected)
        .collect::<Vec<_>>();

    if nodes_to_check.is_empty() {
        return Ok(());
    }

    let scheme = if tls { "https" } else { "http" };

    let client = reqwest::Client::new();
    for node in nodes_to_check {
        let url = format!("{}://{}/cluster/metrics/{}", scheme, node.addr_api, path);
        let res = client
            .get(&url)
            .header(HEADER_NAME_SECRET, &state.secret_api)
            .send()
            .await?;
        if !res.status().is_success() {
            let err = res.json::<Error>().await;
            error!("Error metrics lookup to {}: {:?}", url, err);
            continue;
        }

        let bytes = res.bytes().await?;
        let metrics = deserialize::<RaftMetrics<u64, Node>>(&bytes)?;
        let members = metrics.membership_config;

        check_nodes_in_members(state.id, path, node.id, nodes, &members);

        if members != membership {
            error!(
                "Difference in membership config on Node {} for {}:\n\nlocal:\n{:?}\n\nremote ({}):\n{:?}",
                state.id, path, membership, node.id, members
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F-009, under the profile a test can run in.
    ///
    /// `hiqlite-abort-probe` runs the same call under `panic = "abort"`, which is the profile
    /// that made this defect fatal and which no test can use, because Rust's test harness
    /// requires unwinding.
    #[test]
    fn a_malformed_split_brain_interval_is_a_startup_error() {
        assert_eq!(
            split_brain_interval_from(None).unwrap(),
            Duration::from_secs(60),
            "the documented default is unchanged"
        );
        assert_eq!(
            split_brain_interval_from(Some(" 15 ")).unwrap(),
            Duration::from_secs(15)
        );

        let err = split_brain_interval_from(Some("not a number"))
            .expect_err("a malformed value is a configuration error, not a panic");
        let text = err.to_string();
        assert!(text.starts_with("Startup: "), "got: {text}");
        assert!(
            text.contains("HQL_SPLIT_BRAIN_INTERVAL"),
            "the operator is told which variable, got: {text}"
        );

        let err = split_brain_interval_from(Some("0"))
            .expect_err("a zero interval would spin the checker");
        assert!(err.to_string().contains("greater than zero"), "got: {err}");
    }
}
