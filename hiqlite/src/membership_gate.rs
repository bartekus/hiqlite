//! F-107: the one place a membership change and a shutdown meet.
//!
//! Before this module a membership change was guarded by `raft_lock`, but not consistently:
//!
//! - the leader and voter decision was taken **before** the lock, so a request could be
//!   authorized against one membership and act on another after waiting for the lock;
//! - two paths that change membership never asked the decision at all (`post_membership` and
//!   the raft stream's `RemoveMembershipCache`);
//! - shutdown stopped both raft groups without the lock, so a stop could run while a change
//!   was still in flight on the same node.
//!
//! The ordering this module enforces, for both raft groups, which share one gate:
//!
//! 1. **Admission closes first.** Shutdown calls [`MembershipGate::close`] before anything else.
//!    From then on [`MembershipGate::admit`] refuses without waiting for the lock, with
//!    `Error::LeaderChange`, which the HTTP layer answers `409` and which `leave_remote_cluster`
//!    treats as "ask another node".
//! 2. **Every admitted change runs under the lock, and is decided under it.** The decision is a
//!    closure evaluated only after the lock is held, so it reads the membership the change will
//!    act on. A request that queued before shutdown closed admission and acquires the lock
//!    afterwards sees the closed gate and is refused: the flag is stored before shutdown
//!    acquires the lock, and the mutex orders that store before every later acquisition.
//! 3. **Shutdown drains, bounded.** [`MembershipGate::drain`] waits for the lock for at most the
//!    bound it is given. On timeout **nothing is stopped**: it returns `Error::Timeout`, the gate
//!    stays closed, and the node keeps serving everything except membership changes. The caller
//!    may call shutdown again, or end the process. Ending the process is a crash, which Raft is
//!    built to tolerate; stopping a raft group under a membership change that is still running is
//!    not, and it is what this ordering exists to prevent.
//! 4. **Every stop runs under the lock**, held by the shutdown until both raft groups, the WAL
//!    writer and the SQLite writer have stopped. A stopped gate stays closed, so no change is
//!    ever admitted against a stopped raft.
//!
//! **Deadlock.** While shutdown holds the lock it waits on: the cache self-leave (a local
//! membership change, or an HTTP call to another node's leader), `Raft::shutdown` for both
//! groups, the WAL writer and the SQLite writer. None of those take this lock. The only inbound
//! path that takes it is a membership request, and every one of those checks the closed gate
//! **before** waiting, so none can queue behind the shutdown and hold up the raft stream that
//! carries the replication a remote leave needs. Every wait on the lock is also bounded.

use crate::Error;
use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// How long a membership request waits for another membership change on this node to finish.
pub(crate) const ADMISSION_WAIT: Duration = Duration::from_secs(10);

/// How long shutdown waits for an admitted membership change to finish before it gives up
/// without stopping anything. Sized so that the multi-node pre-shutdown delay plus this bound
/// stays inside the fifteen seconds `Client::shutdown` allows the whole shutdown.
pub(crate) const SHUTDOWN_DRAIN: Duration = Duration::from_secs(5);

/// How long an admitted change waits for its own result to become visible in local metrics.
pub(crate) const COMMIT_VISIBLE_WAIT: Duration = Duration::from_secs(10);

/// How long one openraft membership call may run under the gate. `add_learner(.., true)` waits
/// for the learner to catch up and `change_membership` for a commit, and neither is bounded by
/// openraft. Cancelling either future is safe: openraft serializes membership changes itself and
/// refuses a second one while the first is in progress, so releasing the gate early cannot let
/// two run at once.
pub(crate) const MEMBERSHIP_OP_BOUND: Duration = Duration::from_secs(30);

const CLOSED: &str = "this node is shutting down and cannot serve a membership change; ask \
                      another node";

#[derive(Debug, Default)]
pub(crate) struct MembershipGate {
    lock: Arc<Mutex<()>>,
    closed: AtomicBool,
    stopped: AtomicBool,
    stopped_cleanly: AtomicBool,
}

/// Exclusive membership authority on this node, for as long as it is held.
///
/// Returned by an admitted change and by a drained shutdown. Functions that change membership
/// take a reference to one, so no caller can reach them without having gone through the gate.
#[derive(Debug)]
pub(crate) struct MembershipHeld {
    _guard: OwnedMutexGuard<()>,
}

impl MembershipGate {
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Admit one membership change, or refuse it.
    ///
    /// `decide` runs only once the lock is held, so whatever it reads is what the change acts
    /// on. The lock wait is bounded by `wait`; a timeout is a `LeaderChange` refusal, so the
    /// caller is sent to retry rather than told the change failed.
    pub(crate) async fn admit(
        &self,
        wait: Duration,
        decide: impl FnOnce() -> Result<(), Error>,
    ) -> Result<MembershipHeld, Error> {
        if self.is_closed() {
            return Err(Error::LeaderChange(Cow::Borrowed(CLOSED)));
        }
        let guard = tokio::time::timeout(wait, self.lock.clone().lock_owned())
            .await
            .map_err(|_| {
                Error::LeaderChange(Cow::Owned(format!(
                    "another membership change on this node was still running after {wait:?}; \
                     ask again, or ask another node"
                )))
            })?;
        // Checked again under the lock: this request may have queued before shutdown closed
        // admission.
        if self.is_closed() {
            return Err(Error::LeaderChange(Cow::Borrowed(CLOSED)));
        }
        decide()?;
        Ok(MembershipHeld { _guard: guard })
    }

    /// Stop admitting membership changes. Irreversible for the life of this node.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    /// Wait, bounded, for every admitted membership change to finish, and take the lock so
    /// that none can start.
    ///
    /// `Ok(None)` means an earlier shutdown already stopped this node. `Err` means the bound
    /// passed with a change still running, and the caller must not stop anything.
    pub(crate) async fn drain(&self, bound: Duration) -> Result<Option<MembershipHeld>, Error> {
        self.close();
        let guard = tokio::time::timeout(bound, self.lock.clone().lock_owned())
            .await
            .map_err(|_| {
                Error::Timeout(format!(
                    "shutdown did not start: a membership change admitted before shutdown began \
                     was still running after {bound:?}. Nothing was stopped; this node refuses \
                     further membership changes and may be shut down again, or its process ended"
                ))
            })?;
        if self.stopped.load(Ordering::SeqCst) {
            return Ok(None);
        }
        Ok(Some(MembershipHeld { _guard: guard }))
    }

    /// Record that the stop sequence has run, and whether every component stopped, so a later
    /// shutdown does not repeat it and does not report success for one that failed.
    ///
    /// Takes the held authority to make it impossible to record from outside a drained shutdown.
    pub(crate) fn mark_stopped(&self, _held: &MembershipHeld, cleanly: bool) {
        self.stopped_cleanly.store(cleanly, Ordering::SeqCst);
        self.stopped.store(true, Ordering::SeqCst);
    }

    /// Whether the stop sequence that ran stopped every component. Meaningful once `drain` has
    /// returned `Ok(None)`.
    pub(crate) fn stopped_cleanly(&self) -> bool {
        self.stopped_cleanly.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    //! Interleavings are driven explicitly, with tokio's clock paused, so each test runs the same
    //! schedule every time. Nothing here depends on a race being won.

    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::oneshot;

    fn voter_decision(voter: &AtomicBool) -> Result<(), Error> {
        crate::network::management::membership_change_allowed(
            false,
            Some(1),
            1,
            voter.load(Ordering::SeqCst),
        )
    }

    /// The original failure, then the repaired ordering, on the same schedule.
    ///
    /// Node 1 is leader and voter. A shutdown self-leave holds the lock and removes node 1 from
    /// the voters. A peer's request arrived while it did. Decided before the lock, as
    /// `are_we_leader` did, the request is authorized against the old membership and acts on
    /// the new one: that is the change openraft's `append_membership` asserts against. Decided
    /// under the lock it is refused.
    #[tokio::test(start_paused = true)]
    async fn a_decision_taken_before_the_lock_acts_on_a_membership_it_did_not_see() {
        let gate = Arc::new(MembershipGate::default());
        let voter = Arc::new(AtomicBool::new(true));

        // The old ordering: decide, then lock.
        let held = gate.admit(ADMISSION_WAIT, || Ok(())).await.unwrap();
        let old_decision = voter_decision(&voter);
        assert!(
            old_decision.is_ok(),
            "the request is authorized while node 1 is a voter"
        );
        voter.store(false, Ordering::SeqCst); // the self-leave commits
        drop(held);
        let _acting = gate.lock.clone().lock_owned().await;
        assert!(
            !voter.load(Ordering::SeqCst) && old_decision.is_ok(),
            "the old ordering acts as a non-voter on an authorization given to a voter"
        );
        drop(_acting);

        // The repaired ordering: the same schedule through `admit`.
        voter.store(true, Ordering::SeqCst);
        let held = gate.admit(ADMISSION_WAIT, || Ok(())).await.unwrap();
        let (tx, rx) = oneshot::channel();
        let (g, v) = (gate.clone(), voter.clone());
        let request = tokio::spawn(async move {
            let _ = tx.send(());
            g.admit(ADMISSION_WAIT, || voter_decision(&v)).await
        });
        rx.await.unwrap();
        tokio::task::yield_now().await;
        voter.store(false, Ordering::SeqCst);
        drop(held);
        let err = request
            .await
            .unwrap()
            .expect_err("decided under the lock, it is refused");
        assert!(matches!(err, Error::LeaderChange(_)), "got {err:?}");
        assert!(err.to_string().contains("no longer a voter"), "got {err}");
    }

    /// An admitted change finishes before shutdown stops anything, and nothing is admitted after
    /// shutdown begins, including a request that queued before it.
    #[tokio::test(start_paused = true)]
    async fn shutdown_waits_for_an_admitted_change_and_admits_nothing_after() {
        let gate = Arc::new(MembershipGate::default());
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));

        let in_flight = gate.admit(ADMISSION_WAIT, || Ok(())).await.unwrap();

        // Queued before shutdown closes admission.
        let g = gate.clone();
        let queued = tokio::spawn(async move { g.admit(ADMISSION_WAIT, || Ok(())).await });
        tokio::task::yield_now().await;

        gate.close();
        let (g, o) = (gate.clone(), order.clone());
        let shutdown = tokio::spawn(async move {
            let held = g.drain(SHUTDOWN_DRAIN).await;
            o.lock().unwrap().push("shutdown holds the gate");
            held
        });
        tokio::task::yield_now().await;

        // A new request after close is refused at once, without waiting for the lock.
        let err = gate.admit(ADMISSION_WAIT, || Ok(())).await.unwrap_err();
        assert!(err.to_string().contains("shutting down"), "got {err}");
        assert!(
            !shutdown.is_finished(),
            "shutdown must wait for the change in flight"
        );

        order.lock().unwrap().push("in-flight change finished");
        drop(in_flight);

        let queued = queued.await.unwrap();
        assert!(
            matches!(queued, Err(Error::LeaderChange(_))),
            "a request queued before shutdown and served after it must be refused: {queued:?}"
        );
        let held = shutdown.await.unwrap().unwrap().expect("not stopped yet");
        assert_eq!(
            *order.lock().unwrap(),
            vec!["in-flight change finished", "shutdown holds the gate"]
        );

        // Stopped: every later request is refused, and a second shutdown does not stop again,
        // and says whether the first one stopped everything.
        gate.mark_stopped(&held, false);
        drop(held);
        assert!(gate.admit(ADMISSION_WAIT, || Ok(())).await.is_err());
        assert!(gate.drain(SHUTDOWN_DRAIN).await.unwrap().is_none());
        assert!(
            !gate.stopped_cleanly(),
            "a failed stop is not reported as a clean one"
        );
    }

    /// A change that does not finish holds shutdown for the bound and no longer, and on timeout
    /// shutdown stops nothing. It does not fall through to stopping under the change.
    #[tokio::test(start_paused = true)]
    async fn a_shutdown_that_cannot_drain_stops_nothing_and_can_be_retried() {
        let gate = Arc::new(MembershipGate::default());
        let stops = AtomicUsize::new(0);
        let stuck = gate.admit(ADMISSION_WAIT, || Ok(())).await.unwrap();

        let started = tokio::time::Instant::now();
        match gate.drain(SHUTDOWN_DRAIN).await {
            Ok(_) => {
                stops.fetch_add(1, Ordering::SeqCst);
            }
            Err(err) => {
                assert!(matches!(err, Error::Timeout(_)), "got {err:?}");
                assert!(err.to_string().contains("Nothing was stopped"), "got {err}");
            }
        }
        assert_eq!(
            started.elapsed(),
            SHUTDOWN_DRAIN,
            "bounded, and by exactly the bound"
        );
        assert_eq!(
            stops.load(Ordering::SeqCst),
            0,
            "nothing may be stopped on timeout"
        );
        assert!(
            gate.is_closed(),
            "admission stays closed after a failed shutdown"
        );
        assert!(gate.admit(ADMISSION_WAIT, || Ok(())).await.is_err());

        // Once the change finishes, shutdown can be retried and succeeds.
        drop(stuck);
        assert!(gate.drain(SHUTDOWN_DRAIN).await.unwrap().is_some());
    }

    /// Cancelling a shutdown while it waits leaves nothing held and admission closed.
    #[tokio::test(start_paused = true)]
    async fn a_cancelled_shutdown_wait_holds_nothing_and_keeps_admission_closed() {
        let gate = Arc::new(MembershipGate::default());
        let in_flight = gate.admit(ADMISSION_WAIT, || Ok(())).await.unwrap();

        tokio::select! {
            _ = gate.drain(SHUTDOWN_DRAIN) => panic!("cannot drain while a change is admitted"),
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
        drop(in_flight);

        assert!(gate.admit(ADMISSION_WAIT, || Ok(())).await.is_err());
        assert!(
            gate.lock.clone().try_lock_owned().is_ok(),
            "the cancelled wait must not have left the lock held"
        );
        assert!(gate.drain(SHUTDOWN_DRAIN).await.unwrap().is_some());
    }

    /// A membership request waits for another change for a bounded time, and a timeout sends it
    /// elsewhere rather than failing it. Cache and SQLite changes share this one gate.
    #[tokio::test(start_paused = true)]
    async fn admission_waits_for_a_bounded_time_across_both_raft_groups() {
        let gate = MembershipGate::default();
        let cache_change = gate.admit(ADMISSION_WAIT, || Ok(())).await.unwrap();

        let started = tokio::time::Instant::now();
        let sqlite_change = gate.admit(ADMISSION_WAIT, || Ok(())).await;
        assert_eq!(started.elapsed(), ADMISSION_WAIT);
        assert!(
            matches!(sqlite_change, Err(Error::LeaderChange(_))),
            "got {sqlite_change:?}"
        );
        drop(cache_change);
        assert!(gate.admit(ADMISSION_WAIT, || Ok(())).await.is_ok());
    }

    /// A refused decision releases the lock.
    #[tokio::test(start_paused = true)]
    async fn a_refused_decision_holds_nothing() {
        let gate = MembershipGate::default();
        let refused = gate
            .admit(ADMISSION_WAIT, || Err(Error::LeaderChange("no".into())))
            .await;
        assert!(refused.is_err());
        assert!(gate.admit(Duration::ZERO, || Ok(())).await.is_ok());
    }
}
