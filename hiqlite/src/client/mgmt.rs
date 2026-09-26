use crate::app_state::AppState;
use crate::membership_gate::SHUTDOWN_DRAIN;
use crate::client::stream::ClientStreamReq;
use crate::helpers::deserialize;
use crate::network::HEADER_NAME_SECRET;
use crate::{Client, Error};
use openraft::ServerState;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time;
use tracing::{debug, error, info};

#[cfg(feature = "cache")]
use crate::network::management::{self, ClusterLeaveReq};
#[cfg(feature = "sqlite")]
use crate::store::state_machine::sqlite::writer::WriterRequest;
#[cfg(any(feature = "sqlite", feature = "cache"))]
use crate::{Node, NodeId};
#[cfg(any(feature = "sqlite", feature = "cache"))]
use openraft::RaftMetrics;
#[cfg(any(feature = "sqlite", feature = "cache"))]
use std::clone::Clone;
#[cfg(any(feature = "sqlite", feature = "cache"))]
use std::sync::atomic::Ordering;

impl Client {
    /// Get cluster metrics for the database Raft.
    #[cfg(feature = "sqlite")]
    pub async fn metrics_db(&self) -> Result<RaftMetrics<NodeId, Node>, Error> {
        if let Some(state) = &self.inner.state {
            let metrics = state.raft_db.raft.metrics().borrow().clone();
            Ok(metrics)
        } else {
            let url = self
                .build_addr("/cluster/metrics/sqlite", &self.inner.leader_db)
                .await;
            self.get_metrics_remote(url).await
        }
    }

    /// Get cluster metrics for the cache Raft.
    #[cfg(feature = "cache")]
    pub async fn metrics_cache(&self) -> Result<RaftMetrics<NodeId, Node>, Error> {
        if let Some(state) = &self.inner.state {
            let metrics = state.raft_cache.raft.metrics().borrow().clone();
            Ok(metrics)
        } else {
            let url = self
                .build_addr("/cluster/metrics/cache", &self.inner.leader_cache)
                .await;
            self.get_metrics_remote(url).await
        }
    }

    // This is separated from the `self.send_with_retry_db()` to avoid recursion on leader unreachable
    async fn get_metrics_remote(&self, url: String) -> Result<RaftMetrics<NodeId, Node>, Error> {
        // This should never be called if we have a local client with its own replicated data
        debug_assert!(
            self.inner.state.is_none(),
            "get_metrics_remote should never be called with local state"
        );
        debug_assert!(
            self.inner.api_secret.is_some(),
            "api_secret should always exist for remote clients"
        );

        let res = self
            .inner
            .client
            .as_ref()
            .unwrap()
            .get(url)
            .header(HEADER_NAME_SECRET, self.inner.api_secret.as_ref().unwrap())
            .send()
            .await?;

        if res.status().is_success() {
            let bytes = res.bytes().await?;
            let resp = deserialize(bytes.as_ref())?;
            Ok(resp)
        } else {
            let err = res.json::<Error>().await?;
            Err(err)
        }
    }

    /// The terminal failure that has taken this node out of service, if there is one.
    ///
    /// `None` while the node is serving. `Some(..)` once a component it depends on has failed:
    /// the WAL writer thread ending for any reason including a panic inside it, a listener that
    /// stopped serving, or the cache state machine refusing committed work.
    ///
    /// **Nothing clears it and nothing restarts the failed component.** hiqlite does not end
    /// this process either: what an embedding application does about an out-of-service node is
    /// the application's decision, and restarting the node is the recovery path. Returns `None`
    /// for a remote client, which has no local node to speak for.
    pub fn node_failure(&self) -> Option<crate::lifecycle::NodeFailure> {
        self.inner
            .state
            .as_ref()
            .and_then(|state| state.lifecycle.failure().cloned())
    }

    /// `Err` once this node is out of service, with an account of why.
    ///
    /// Every operation of an embedded client goes through this: each rate-limit gate, which
    /// every write, consistent query and cache operation passes, and each local read. What a
    /// caller is refused with is therefore the same account the health endpoint gives. A
    /// remote client is not refused here; the node it talks to refuses it.
    pub fn ensure_node_available(&self) -> Result<(), Error> {
        match &self.inner.state {
            Some(state) => state.lifecycle.ensure_available(),
            None => Ok(()),
        }
    }

    /// Where this node's startup recovery stands (`037`).
    ///
    /// After a start, each Raft group's state machine applies the log the node held, and until it
    /// has, the node is up but does not serve: health checks, `/health`, `/ready` and every
    /// operation of this client refuse with [`Error::Recovering`]. A consumer uses this to report
    /// "recovering" rather than "down". `None` for a remote client, which has no local node; the
    /// node it talks to refuses with the same error.
    pub fn recovery_state(&self) -> Option<crate::RecoveryState> {
        self.inner.state.as_ref().map(|state| state.recovery_state())
    }

    /// `Err(Error::Recovering)` until the database group has finished its startup recovery.
    #[cfg(feature = "sqlite")]
    pub(crate) fn ensure_db_recovered(&self) -> Result<(), Error> {
        match &self.inner.state {
            Some(state) => state.raft_db.recovery.ensure_complete(),
            None => Ok(()),
        }
    }

    /// `Err(Error::Recovering)` until the cache group has finished its startup recovery.
    #[cfg(feature = "cache")]
    pub(crate) fn ensure_cache_recovered(&self) -> Result<(), Error> {
        match &self.inner.state {
            Some(state) => state.raft_cache.recovery.ensure_complete(),
            None => Ok(()),
        }
    }

    /// Check the cluster health state for the database Raft.
    ///
    /// Not healthy until the database group has finished its startup recovery (`037`): the error
    /// is then [`Error::Recovering`].
    #[cfg(feature = "sqlite")]
    pub async fn is_healthy_db(&self) -> Result<(), Error> {
        self.ensure_node_available()?;
        self.ensure_db_recovered()?;
        let metrics = self.metrics_db().await?;
        metrics.running_state?;
        if metrics.current_leader.is_some() {
            if metrics.state == ServerState::Learner
                || metrics.state == ServerState::Follower
                || metrics.state == ServerState::Leader
            {
                Ok(())
            } else {
                Err(Error::Connect(format!(
                    "The DB leader voting process has not finished yet - server state: {:?}",
                    metrics.state
                )))
            }
        } else {
            // tracing::error!("Unhealthy DB");
            Err(Error::LeaderChange(
                "The DB leader voting process has not finished yet".into(),
            ))
        }
    }

    /// Check the cluster health state for the cache Raft.
    ///
    /// Not healthy until the cache group has finished its startup recovery (`037`).
    #[cfg(feature = "cache")]
    pub async fn is_healthy_cache(&self) -> Result<(), Error> {
        self.ensure_node_available()?;
        self.ensure_cache_recovered()?;
        let metrics = self.metrics_cache().await?;
        metrics.running_state?;
        if metrics.current_leader.is_some() {
            if metrics.state == ServerState::Learner
                || metrics.state == ServerState::Follower
                || metrics.state == ServerState::Leader
            {
                Ok(())
            } else {
                Err(Error::Connect(format!(
                    "The cache leader voting process has not finished yet - server state: {:?}",
                    metrics.state
                )))
            }
        } else {
            // tracing::error!("Unhealthy cache");
            Err(Error::LeaderChange(
                "The cache leader voting process has not finished yet".into(),
            ))
        }
    }

    /// Wait until the database Raft is healthy, for as long as it takes.
    ///
    /// **This never returns if the node never becomes healthy**, which includes the case where
    /// it has failed terminally. Prefer [`Self::wait_until_healthy_db_timeout`] in anything
    /// that has to make progress; this signature is kept because it is the published one.
    #[cfg(feature = "sqlite")]
    pub async fn wait_until_healthy_db(&self) {
        loop {
            match self.is_healthy_db().await {
                Ok(_) => {
                    return;
                }
                Err(err) => {
                    debug!("Waiting for healthy Raft DB: {:?}", err);
                    info!("Waiting for healthy Raft DB");
                    time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    /// Wait until the database Raft is healthy, or give up.
    ///
    /// Returns the last health error when `timeout` elapses, so a caller that cannot make
    /// progress finds out rather than hanging. It also stops early with `Error::NodeFailed` if
    /// the node has failed terminally, because no amount of further waiting changes that.
    #[cfg(feature = "sqlite")]
    pub async fn wait_until_healthy_db_timeout(&self, timeout: Duration) -> Result<(), Error> {
        self.wait_until_healthy_timeout(timeout, "DB", || self.is_healthy_db())
            .await
    }

    /// Wait until the cache Raft is healthy, for as long as it takes.
    ///
    /// The same caveat as [`Self::wait_until_healthy_db`]: it never returns if the node never
    /// becomes healthy.
    #[cfg(feature = "cache")]
    pub async fn wait_until_healthy_cache(&self) {
        loop {
            match self.is_healthy_cache().await {
                Ok(_) => {
                    return;
                }
                Err(err) => {
                    debug!("Waiting for healthy Raft cache: {:?}", err);
                    info!("Waiting for healthy Raft cache");
                    time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    /// Wait until the cache Raft is healthy, or give up.
    #[cfg(feature = "cache")]
    pub async fn wait_until_healthy_cache_timeout(&self, timeout: Duration) -> Result<(), Error> {
        self.wait_until_healthy_timeout(timeout, "cache", || self.is_healthy_cache())
            .await
    }

    /// The shared body of the two bounded waits.
    #[cfg(any(feature = "sqlite", feature = "cache"))]
    async fn wait_until_healthy_timeout<F, Fut>(
        &self,
        timeout: Duration,
        what: &str,
        mut check: F,
    ) -> Result<(), Error>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<(), Error>>,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut last: Option<Error>;
        loop {
            // A terminal failure is not something waiting resolves.
            self.ensure_node_available()?;

            match check().await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    debug!("Waiting for healthy Raft {what}: {err:?}");
                    last = Some(err);
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(last.unwrap_or_else(|| {
                    Error::Timeout(
                        format!("the {what} raft did not become healthy in time"),
                    )
                }));
            }
            let _ = &last;
            time::sleep(Duration::from_millis(500).min(deadline - tokio::time::Instant::now()))
                .await;
        }
    }

    /// Perform a graceful shutdown for this Raft node.
    /// Works on local clients only and can't shut down remote nodes.
    ///
    /// The shutdown adds a 10 delay on purpose for smoothing out Kubernetes rolling releases and
    /// make the whole process more graceful, because a whole new leader election might be necessary.
    ///
    /// In future versions, there will be the possibility to trigger a graceful leader election
    /// upfront, but this has not been stabilized in this version.
    pub async fn shutdown(&self) -> Result<(), Error> {
        if let Some(state) = &self.inner.state {
            match tokio::time::timeout(
                SHUTDOWN_WAIT,
                Self::shutdown_execute(
                    state,
                    #[cfg(feature = "cache")]
                    self.inner.tls_config.is_some(),
                    #[cfg(feature = "cache")]
                    self.inner.tls_no_verify,
                    #[cfg(feature = "cache")]
                    &self.inner.tx_client_cache,
                    #[cfg(feature = "sqlite")]
                    &self.inner.tx_client_db,
                    &self.inner.tx_shutdown,
                ),
            )
            .await
            {
                // The shutdown's own result: a drain timeout that stopped nothing, or a component
                // that did not stop, used to be discarded here and reported as success.
                Ok(res) => res,
                Err(_) => Err(shutdown_wait_elapsed()),
            }
        } else {
            Err(Error::Error(
                "Shutdown for remote Raft clients is not yet implemented".into(),
            ))
        }
    }

    #[allow(unused_variables)]
    pub(crate) async fn shutdown_execute(
        state: &Arc<AppState>,
        #[cfg(feature = "cache")] with_tls: bool,
        #[cfg(feature = "cache")] tls_no_verify: bool,
        #[cfg(feature = "cache")] tx_client_cache: &flume::Sender<ClientStreamReq>,
        #[cfg(feature = "sqlite")] tx_client_db: &flume::Sender<ClientStreamReq>,
        tx_shutdown: &Option<watch::Sender<bool>>,
    ) -> Result<(), Error> {
        // F-107: the sequence runs in its own task. Both callers bound how long they wait, and
        // dropping this future used to cancel the sequence wherever it was, including between
        // one raft group's stop and the next. A caller that stops waiting now only stops waiting.
        tokio::spawn(Self::shutdown_run(
            state.clone(),
            #[cfg(feature = "cache")]
            with_tls,
            #[cfg(feature = "cache")]
            tls_no_verify,
            #[cfg(feature = "cache")]
            tx_client_cache.clone(),
            #[cfg(feature = "sqlite")]
            tx_client_db.clone(),
            tx_shutdown.clone(),
        ))
        .await
        .map_err(|err| Error::Error(format!("the shutdown task did not complete: {err}").into()))?
    }

    #[allow(unused_assignments)]
    #[allow(unused_variables)]
    async fn shutdown_run(
        state: Arc<AppState>,
        #[cfg(feature = "cache")] with_tls: bool,
        #[cfg(feature = "cache")] tls_no_verify: bool,
        #[cfg(feature = "cache")] tx_client_cache: flume::Sender<ClientStreamReq>,
        #[cfg(feature = "sqlite")] tx_client_db: flume::Sender<ClientStreamReq>,
        tx_shutdown: Option<watch::Sender<bool>>,
    ) -> Result<(), Error> {
        info!("Starting Node shutdown");

        #[allow(unused_mut)]
        let mut is_single_instance: bool;
        #[cfg(feature = "cache")]
        {
            let node_count = state
                .raft_cache
                .raft
                .metrics()
                .borrow()
                .membership_config
                .nodes()
                .count();
            is_single_instance = node_count == 1;
        }
        #[cfg(feature = "sqlite")]
        {
            let node_count = state
                .raft_db
                .raft
                .metrics()
                .borrow()
                .membership_config
                .nodes()
                .count();
            is_single_instance = node_count == 1;
        }

        // F-101: before anything is asked to stop, so the watchers do not report the
        // components this shutdown is about to end as failures.
        state.lifecycle.begin_shutdown();
        state.is_shutting_down.store(true, Ordering::Relaxed);
        // F-107: no membership change is admitted from here on, on either raft group.
        state.membership.close();

        // This pre-shutdown delay is not strictly necessary, but it makes rolling releases
        // smoother, especially with ephemeral storage. It also allows to set a ready check
        // interval of 3 seconds while it will still catch it before it actually starts the
        // shutdown, so services can stop sending requests to this node.
        if !is_single_instance {
            time::sleep(Duration::from_millis(9500)).await;
        }

        // F-107: wait, bounded, for a membership change admitted before `close` to finish, and
        // hold the gate until every component has stopped. On timeout nothing is stopped: see
        // `membership_gate` for why that and not a stop under the running change.
        let held = match state.membership.drain(SHUTDOWN_DRAIN).await {
            Ok(Some(held)) => held,
            Ok(None) if state.membership.stopped_cleanly() => {
                info!("This node has already been shut down");
                return Ok(());
            }
            Ok(None) => {
                return Err(Error::Error(
                    "an earlier shutdown of this node did not stop every component; see its \
                     error. Storage ownership is still held, and ending the process releases it"
                        .into(),
                ));
            }
            Err(err) => {
                error!("{err}");
                return Err(err);
            }
        };

        // A component that fails to stop no longer returns early and leaves the ones after it
        // running. Every stop is attempted; the first failure is returned.
        let mut first_err: Option<Error> = None;

        #[cfg(feature = "cache")]
        {
            let mut metrics = state.raft_cache.raft.metrics().borrow().clone();

            for _ in 0..5 {
                if metrics.current_leader.is_some() {
                    break;
                }
                info!("Delaying cache cluster leave because of no existing leader");
                time::sleep(Duration::from_secs(1)).await;
                metrics = state.raft_cache.raft.metrics().borrow().clone();
            }

            if !state.raft_cache.cache_storage_disk {
                // If we run an entirely in-memory cache and therefore lose the Raft state
                // and membership between restarts, we should always leave the cluster cleanly
                // before doing a shutdown.
                info!("Leaving in-memory-only cache cluster");

                let client = crate::http_client::build_http_client(tls_no_verify);
                let scheme = if with_tls { "https" } else { "http" };

                // The same decision every other membership change takes, minus the closed
                // gate, which this shutdown closed itself and holds.
                let may_leave_locally = metrics.state == ServerState::Leader
                    && management::membership_change_allowed(
                    false,
                    metrics.current_leader,
                    state.id,
                        metrics.membership_config.voter_ids().any(|id| id == state.id),
                    )
                    .is_ok();

                if may_leave_locally {
                    if let Err(err) = management::leave_cluster_exec(
                        &state,
                        &crate::app_state::RaftType::Cache,
                        ClusterLeaveReq {
                            node_id: state.id,
                            stay_as_learner: false,
                        },
                        &held,
                    )
                    .await
                    {
                        tracing::error!("Error leaving the Cache cluster: {:?}", err);
                    }
                } else {
                    // Bounded, because the gate is held: leaving through another node is an HTTP
                    // walk over the peers with a thirty-second client timeout each. A leave that
                    // does not finish is logged and the stop goes on, as it did when the leave
                    // failed; the peers see this node go away either way.
                    match time::timeout(
                        REMOTE_LEAVE_BOUND,
                        crate::init::leave_remote_cluster(
                            &state,
                            &crate::app_state::RaftType::Cache,
                            &client,
                            scheme,
                            state.id,
                            &state.nodes,
                            0,
                            false,
                        ),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {}
                        Ok(Err(err)) => {
                            tracing::error!("Error leaving the Cache cluster: {:?}", err)
                        }
                        Err(_) => tracing::error!(
                            "Leaving the Cache cluster did not finish within \
                             {REMOTE_LEAVE_BOUND:?}; stopping anyway"
                        ),
                    }
                }

                info!("Left in-memory-only cache cluster successfully");
            }

            info!("Shutting down raft cache layer");

            // TODO as soon openraft-0.10 is out, we will be able to trigger a pre-emptive
            //  leader switch, if this node is the leader. This will smoth out things even more.

            state
                .raft_cache
                .is_raft_stopped
                .store(true, Ordering::Relaxed);
            note_stop(&mut first_err, "the cache raft", state.raft_cache.raft.shutdown().await);
            if let Some(handle) = &state.raft_cache.shutdown_handle {
                note_stop(&mut first_err, "the cache log writer", handle.shutdown().await);
            }
            let _ = tx_client_cache.send_async(ClientStreamReq::Shutdown).await;
        };

        #[cfg(feature = "sqlite")]
        {
            info!("Shutting down raft sqlite layer");

            for _ in 0..5 {
                if state
                    .raft_db
                    .raft
                    .metrics()
                    .borrow()
                    .current_leader
                    .is_some()
                {
                    break;
                }
                info!("Delaying sqlite raft shutdown because of no existing leader");
                time::sleep(Duration::from_secs(1)).await;
            }

            // TODO as soon openraft-0.10 is out, we will be able to trigger a pre-emptive
            //  leader switch, if this node is the leader. This will smoth out things even more.

            state.raft_db.is_raft_stopped.store(true, Ordering::Relaxed);

            note_stop(&mut first_err, "the sqlite raft", state.raft_db.raft.shutdown().await);
            info!("Shutting down sqlite logs writer");
            note_stop(
                &mut first_err,
                "the sqlite WAL writer",
                state.raft_db.shutdown_handle.shutdown().await,
            );

            info!("Shutting down sqlite writer");
            let (tx_sm, rx_sm) = tokio::sync::oneshot::channel();
            let writer_stopped = match state
                .raft_db
                .sql_writer
                .send_async(WriterRequest::Shutdown(tx_sm))
                .await
            {
                Ok(()) => rx_sm.await.map_err(|_| {
                    Error::Error("the SQLite writer ended without acknowledging shutdown".into())
                }),
                Err(_) => Err(Error::Error(
                    "the SQLite writer was no longer listening for shutdown".into(),
                )),
            };
            note_stop(&mut first_err, "the SQLite writer", writer_stopped);

            let _ = tx_client_db.send_async(ClientStreamReq::Shutdown).await;
        }

        // Recorded under the gate, and after every stop was attempted, so a second shutdown does
        // not repeat the sequence against stopped components, nor report it as clean.
        state.membership.mark_stopped(&held, first_err.is_none());

        if let Some(tx) = tx_shutdown {
            tx.send(true)
                .expect("The global Hiqlite shutdown handler to always listen");
        }

        if let Some(err) = first_err {
            // Storage ownership is deliberately kept: a component that did not stop cleanly may
            // still touch the data directory. Dropping this state releases it.
            error!("Shutdown finished with a component that did not stop cleanly: {err}");
            return Err(err);
        }

        // Last, and the ordering is the point: every raft group, the WAL writer and the SQLite
        // writer have acknowledged their shutdown above, so nothing in this process can still
        // write to the data directory. Only now may another node have it.
        state.release_storage_ownership();

        info!("Shutdown complete");
        drop(held);
        Ok(())
    }
}

/// How long `Client::shutdown` and `ShutdownHandle::wait` wait for the shutdown sequence.
pub(crate) const SHUTDOWN_WAIT: Duration = Duration::from_secs(15);

/// How long a shutting-down node spends leaving an in-memory cache cluster through a peer.
#[cfg(feature = "cache")]
const REMOTE_LEAVE_BOUND: Duration = Duration::from_secs(10);

/// What a caller that stopped waiting is told. The sequence goes on in its own task for as long
/// as the runtime does; ending the process before it finishes is a crash for whatever had not
/// stopped yet, which the Raft log and the WAL are built to recover from.
pub(crate) fn shutdown_wait_elapsed() -> Error {
    Error::Timeout(format!(
        "the shutdown did not finish within {SHUTDOWN_WAIT:?}. It continues in the background \
         while this runtime lives; ending the process now is a crash for any component it had \
         not stopped yet"
    ))
}

/// Record a component that did not stop cleanly, and keep going.
fn note_stop<E: Into<Error>>(first: &mut Option<Error>, what: &str, res: Result<(), E>) {
    if let Err(err) = res {
        let err = err.into();
        error!("Shutdown: {what} did not stop cleanly: {err}");
        first.get_or_insert(err);
    }
}
