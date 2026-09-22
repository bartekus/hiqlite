//! What a node does when something it depends on has failed.
//!
//! Three questions had no answer before this module existed, and `010` B-7 and F-025, F-039 and
//! F-040 are what not answering them looked like:
//!
//! - **Who consumes a failure.** A WAL writer that ended, a listener that could not bind and a
//!   background task that panicked all expressed themselves as a log line or a panic inside a
//!   task whose `JoinHandle` was dropped. Nothing above them could tell a node that had lost
//!   its log storage from one that was healthy.
//! - **What it does to readiness.** Nothing. A node whose storage had died kept answering
//!   `/health` and `/ready` on the strength of its Raft metrics.
//! - **Who decides the process ends.** The panic profile did, which for an embedded node is the
//!   **consumer's** profile and not hiqlite's. Under `panic = "abort"` a parse error in an
//!   environment variable ended the whole application; under unwinding the same error killed
//!   one task silently.
//!
//! This module answers all three the same way: a failure is recorded, it is terminal, it makes
//! the node unavailable, and **it never ends the process**. Ending the process is the embedding
//! application's decision, and hiqlite's job is to tell it.

use crate::Error;
use std::sync::{Arc, OnceLock};
use tracing::error;

/// A component whose failure takes the node out of service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedComponent {
    /// The durable Raft log writer for the SQLite group.
    WalWriter,
    /// The SQLite state machine's single writer thread.
    SqliteWriter,
    /// A listener that was supposed to be serving.
    Listener,
    /// The cache Raft group could not apply committed work.
    CacheStateMachine,
}

impl FailedComponent {
    fn as_str(&self) -> &'static str {
        match self {
            FailedComponent::WalWriter => "the Raft log WAL writer",
            FailedComponent::SqliteWriter => "the SQLite writer",
            FailedComponent::Listener => "a network listener",
            FailedComponent::CacheStateMachine => "the cache state machine",
        }
    }
}

/// A terminal node failure: what failed, and why.
#[derive(Debug, Clone)]
pub struct NodeFailure {
    pub component: FailedComponent,
    pub reason: String,
}

impl NodeFailure {
    pub fn message(&self) -> String {
        format!(
            "this node is out of service because {} failed: {}. It does not restart the failed \
             component and it does not end this process; restart the node to recover",
            self.component.as_str(),
            self.reason
        )
    }
}

/// The node's terminal-failure record, shared by everything that has to honour it.
///
/// `OnceLock` on purpose: the **first** failure is the one that explains the node's state, and a
/// later one is usually a consequence of it. Nothing clears it, because nothing here restarts a
/// failed storage component: an automatic restart of a writer whose durability guarantees have
/// already been broken is how a node comes back looking healthy with a hole in its log.
#[derive(Debug, Clone, Default)]
pub struct NodeLifecycle {
    failure: Arc<OnceLock<NodeFailure>>,
}

impl NodeLifecycle {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record a terminal failure. The first one wins.
    ///
    /// Returns `true` when this call was the one that recorded it, so a caller can log once.
    pub(crate) fn fail(&self, component: FailedComponent, reason: impl Into<String>) -> bool {
        let failure = NodeFailure {
            component,
            reason: reason.into(),
        };
        let message = failure.message();
        if self.failure.set(failure).is_ok() {
            error!("{message}");
            true
        } else {
            false
        }
    }

    /// The recorded failure, if there is one.
    pub fn failure(&self) -> Option<&NodeFailure> {
        self.failure.get()
    }

    /// `Err` once the node is out of service.
    ///
    /// This is what every refused operation goes through, so the account a caller gets is the
    /// same one the health endpoint gives.
    pub fn ensure_available(&self) -> Result<(), Error> {
        match self.failure.get() {
            None => Ok(()),
            Some(failure) => Err(Error::NodeFailed(failure.message().into())),
        }
    }
}

/// Watch a WAL log store's writer thread and take the node out of service if it ends.
///
/// The watch has three states and all three are handled here, which is the point of it having
/// three: a reported failure, a **closed** channel (the thread ended without reporting, which is
/// what a panic looks like from outside it), and still running. The third is the only one that
/// does nothing.
pub(crate) fn watch_wal_writer(
    lifecycle: NodeLifecycle,
    mut rx: tokio::sync::watch::Receiver<Option<String>>,
    what: &'static str,
) {
    tokio::task::spawn(async move {
        loop {
            if let Some(reason) = rx.borrow_and_update().clone() {
                lifecycle.fail(FailedComponent::WalWriter, format!("{what}: {reason}"));
                return;
            }
            if rx.changed().await.is_err() {
                // The sender was dropped without a reason. The thread is gone either way, and
                // saying more than that would be inventing a cause.
                lifecycle.fail(
                    FailedComponent::WalWriter,
                    format!(
                        "{what}: the writer thread ended without reporting a reason, which is \
                         what a panic in it looks like from outside"
                    ),
                );
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_failure_is_the_one_that_is_kept() {
        let lifecycle = NodeLifecycle::new();
        assert!(lifecycle.ensure_available().is_ok());

        assert!(lifecycle.fail(FailedComponent::WalWriter, "disk went away"));
        assert!(
            !lifecycle.fail(FailedComponent::Listener, "and then this"),
            "a later failure does not replace the one that explains the node's state"
        );

        let failure = lifecycle.failure().expect("a failure was recorded");
        assert_eq!(failure.component, FailedComponent::WalWriter);
        assert!(failure.reason.contains("disk went away"));
    }

    #[test]
    fn a_failed_node_refuses_with_an_account_of_why() {
        let lifecycle = NodeLifecycle::new();
        lifecycle.fail(FailedComponent::SqliteWriter, "the writer thread ended");

        let err = lifecycle
            .ensure_available()
            .expect_err("a failed node refuses");
        let text = err.to_string();
        assert!(text.starts_with("NodeFailed: "), "got: {text}");
        assert!(text.contains("the SQLite writer"), "got: {text}");
        assert!(
            text.contains("does not end this process"),
            "the caller is told that the decision to exit is theirs, got: {text}"
        );
        assert!(
            text.contains("restart the node to recover"),
            "the caller is told the recovery path, got: {text}"
        );
    }

    /// The watch's three states, including the one that is not a message.
    #[tokio::test]
    async fn a_writer_thread_that_ends_without_a_reason_still_fails_the_node() {
        let lifecycle = NodeLifecycle::new();
        let (tx, rx) = tokio::sync::watch::channel(None::<String>);
        watch_wal_writer(lifecycle.clone(), rx, "sqlite logs");

        assert!(lifecycle.ensure_available().is_ok());

        // What a panicking writer thread looks like: the sender is dropped, never used.
        drop(tx);

        for _ in 0..200 {
            if lifecycle.failure().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let failure = lifecycle
            .failure()
            .expect("a writer thread that is gone takes the node out of service");
        assert_eq!(failure.component, FailedComponent::WalWriter);
        assert!(
            failure.reason.contains("without reporting a reason"),
            "got: {}",
            failure.reason
        );
    }

    #[tokio::test]
    async fn a_reported_writer_failure_names_its_cause() {
        let lifecycle = NodeLifecycle::new();
        let (tx, rx) = tokio::sync::watch::channel(None::<String>);
        watch_wal_writer(lifecycle.clone(), rx, "sqlite logs");

        tx.send(Some("injected persistence failure".to_string()))
            .unwrap();

        for _ in 0..200 {
            if lifecycle.failure().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let failure = lifecycle.failure().expect("the failure is recorded");
        assert!(failure.reason.contains("injected persistence failure"));
        assert!(failure.reason.contains("sqlite logs"));
    }
}
