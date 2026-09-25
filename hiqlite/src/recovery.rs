//! `037`: a node does not serve until its startup recovery has finished.
//!
//! A Raft group's state machine is brought up to date after a start by applying the entries of
//! its log, and OpenRaft only applies them once a leader has committed them. hiqlite persists no
//! committed index, so on every start the applied state trails the log until the first commit
//! of the new term, and until then the state machine holds less than the node acknowledged:
//!
//! - after an unclean stop, `auto-heal` deletes the SQLite database and rebuilds it from the log,
//!   so it starts **empty** (F-134, what Rauthy observed);
//! - the in-memory cache state machine starts from its latest snapshot, or empty, on every start;
//! - a restored or reset node replays whatever its log holds.
//!
//! Before this module, health and readiness only asked whether a leader was known, which is true
//! before the first entry is applied, so a consumer that waited for health and then read saw that
//! partial state. Here each group records, when it starts, the last log index its local log
//! holds, and the node counts as recovered once the state machine has applied it. Until then
//! `/health`, `/ready`, the client's health checks and every client operation refuse with
//! [`Error::Recovering`](crate::Error::Recovering), which a consumer can tell apart from a node
//! that is down.
//!
//! The target is local on purpose: it needs no peer to be computed, and whatever this node held
//! at start is what it may have acknowledged. A follower whose uncommitted tail is replaced by a
//! new leader's log can never apply the old target; it counts as recovered once its log has been
//! cut below the target and everything it now holds is applied.

use crate::{Error, Node, NodeId};
use openraft::RaftMetrics;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::watch;
use tracing::info;

/// The node is still applying the log it held at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryProgress {
    /// The Raft group: `"db"` or `"cache"`.
    pub group: &'static str,
    /// The last log index the state machine has applied, if any.
    pub applied: Option<u64>,
    /// The last log index the node's log held when it started. Recovery is complete once it has
    /// been applied.
    pub target: u64,
    /// The state machine was rebuilt from the log because the previous run did not stop cleanly
    /// (the unclean-stop marker, handled by `auto-heal`).
    pub unclean_stop: bool,
}

impl RecoveryProgress {
    pub fn message(&self) -> String {
        let applied = self
            .applied
            .map(|a| a.to_string())
            .unwrap_or_else(|| "nothing".to_string());
        let cause = if self.unclean_stop {
            ", rebuilding the state machine after an unclean stop"
        } else {
            ""
        };
        format!(
            "the {} raft is recovering{cause}: applied {applied} of {} log entries it held at start",
            self.group, self.target
        )
    }
}

/// Whether a local node has finished its startup recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryState {
    /// Every Raft group has applied the log it held at start. This is permanent for the life of
    /// the node.
    Complete,
    /// At least one group is still applying; one entry per such group.
    Recovering(Vec<RecoveryProgress>),
}

impl RecoveryState {
    pub fn is_complete(&self) -> bool {
        matches!(self, RecoveryState::Complete)
    }
}

const NONE: u64 = u64::MAX;

/// One Raft group's startup recovery. Cheap to clone; every clone sees the same state.
#[derive(Debug, Clone)]
pub(crate) struct StartupRecovery {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    group: &'static str,
    target: Option<u64>,
    unclean_stop: bool,
    complete: AtomicBool,
    applied: AtomicU64,
}

impl StartupRecovery {
    /// `target` is the last log index the group's log held at start; `None`, an empty log, means
    /// there is nothing to recover.
    pub(crate) fn new(group: &'static str, target: Option<u64>, unclean_stop: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                group,
                target,
                unclean_stop,
                complete: AtomicBool::new(target.is_none()),
                applied: AtomicU64::new(NONE),
            }),
        }
    }

    /// Follow the group's metrics until recovery is complete, then stop.
    pub(crate) fn watch(&self, mut metrics: watch::Receiver<RaftMetrics<NodeId, Node>>) {
        if self.is_complete() {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                let (applied, last_log, has_leader) = {
                    let m = metrics.borrow_and_update();
                    (
                        m.last_applied.map(|id| id.index),
                        m.last_log_index,
                        m.current_leader.is_some(),
                    )
                };
                if this.observe(applied, last_log, has_leader) {
                    return;
                }
                // The sender is dropped when the raft shuts down; nothing is left to recover.
                if metrics.changed().await.is_err() {
                    return;
                }
            }
        });
    }

    /// Record one observation; `true` once recovery is complete.
    fn observe(&self, applied: Option<u64>, last_log: Option<u64>, has_leader: bool) -> bool {
        if self.is_complete() {
            return true;
        }
        self.inner
            .applied
            .store(applied.unwrap_or(NONE), Ordering::Relaxed);

        let Some(target) = self.inner.target else {
            return true;
        };
        let reached = match applied {
            Some(a) if a >= target => true,
            // A follower whose uncommitted tail was replaced by the leader's log: the target no
            // longer exists, and everything the log now holds is applied.
            _ => has_leader && last_log.is_some_and(|l| l < target) && applied == last_log,
        };
        if reached {
            self.inner.complete.store(true, Ordering::Release);
            info!(
                "Startup recovery of the {} raft complete: applied {:?}, held {target} at start",
                self.inner.group, applied
            );
        }
        reached
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.inner.complete.load(Ordering::Acquire)
    }

    /// `None` once complete.
    pub(crate) fn progress(&self) -> Option<RecoveryProgress> {
        if self.is_complete() {
            return None;
        }
        let applied = self.inner.applied.load(Ordering::Relaxed);
        Some(RecoveryProgress {
            group: self.inner.group,
            applied: (applied != NONE).then_some(applied),
            target: self.inner.target.unwrap_or(0),
            unclean_stop: self.inner.unclean_stop,
        })
    }

    /// `Err(Error::Recovering)` until recovery is complete.
    pub(crate) fn ensure_complete(&self) -> Result<(), Error> {
        match self.progress() {
            None => Ok(()),
            Some(progress) => Err(Error::Recovering(progress.message().into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_log_has_nothing_to_recover() {
        let r = StartupRecovery::new("db", None, false);
        assert!(r.is_complete());
        assert!(r.ensure_complete().is_ok());
    }

    #[test]
    fn a_leader_is_not_enough() {
        let r = StartupRecovery::new("db", Some(191), true);
        assert!(!r.observe(None, Some(192), true));
        assert!(!r.observe(Some(190), Some(192), true));
        let err = r.ensure_complete().unwrap_err();
        assert!(err.is_recovering(), "{err}");
        let text = err.to_string();
        assert!(text.starts_with("Recovering: "), "{text}");
        assert!(text.contains("applied 190 of 191"), "{text}");
        assert!(text.contains("unclean stop"), "{text}");

        assert!(r.observe(Some(191), Some(192), true));
        assert!(r.is_complete());
        assert!(r.ensure_complete().is_ok());
        // permanent
        assert!(r.observe(None, None, false));
    }

    #[test]
    fn a_replaced_tail_completes_once_the_rest_is_applied() {
        let r = StartupRecovery::new("cache", Some(50), false);
        // the tail is cut to 45 but not yet applied, or no leader has been heard from
        assert!(!r.observe(Some(40), Some(45), true));
        assert!(!r.observe(Some(45), Some(45), false));
        assert!(r.observe(Some(45), Some(45), true));
    }

    #[test]
    fn a_lagging_follower_with_its_whole_log_is_not_complete() {
        let r = StartupRecovery::new("db", Some(50), false);
        // everything applied that it holds, but it still holds the target: not a replaced tail
        assert!(!r.observe(Some(49), Some(50), true));
    }
}
