use chrono::{DateTime, Utc};
use openraft::LogState;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ops::Add;
use std::sync::atomic::AtomicU64;
use std::thread;
use tokio::sync::oneshot;
use tokio::task;
use tracing::{debug, error, info, warn};

pub(crate) const LOCK_VALID_SECONDS: i64 = 10;

pub enum LockRequest {
    /// used for a first try lock without coming from a queue
    Lock(LockRequestPayload),
    /// used after an await to acquire the lock now
    Acquire(LockRequestPayload),
    Release(LockReleasePayload),
    Await(LockAwaitPayload),
    SnapshotBuild(oneshot::Sender<HashMap<String, LockQueue>>),
    SnapshotInstall((HashMap<String, LockQueue>, oneshot::Sender<()>)),
}

pub struct LockRequestPayload {
    pub key: Cow<'static, str>,
    pub log_id: u64,
    pub ack: oneshot::Sender<LockState>,
}

pub struct LockReleasePayload {
    pub key: Cow<'static, str>,
    pub id: u64,
}

pub struct LockAwaitPayload {
    pub key: Cow<'static, str>,
    pub id: u64,
    pub ack: oneshot::Sender<LockState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LockState {
    Locked(u64),
    Queued(u64),
    Released,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockQueue {
    current_ticket: Option<u64>,
    exp: i64,
    queue: VecDeque<u64>,
}

pub fn spawn() -> flume::Sender<LockRequest> {
    spawn_with_lease(LOCK_VALID_SECONDS)
}

/// Same handler with an explicit lease length, in seconds.
///
/// This exists so the lease *expiry* paths can be tested without waiting out the production
/// constant. It is **not** a configuration knob and it is not a repair: a longer or tunable
/// lease does not make a dead holder detectable any sooner than its own deadline, and `006`
/// KD-2 stays exactly as recorded. `spawn` is the only caller outside tests.
pub(crate) fn spawn_with_lease(lease_seconds: i64) -> flume::Sender<LockRequest> {
    let (tx, rx) = flume::unbounded();
    task::spawn(handler(rx, lease_seconds));
    tx
}

/// Send a lock state to a waiting client, reporting rather than panicking if it has gone away.
///
/// Every one of these was an `ack.send(..).unwrap()`. The receiver lives in the caller's
/// future for a leader-local await, so a cancelled `lock()` future (a timeout, a `select!`, an
/// aborted task) drops it, and the handler died with it. That takes every lock on the node
/// with it, which is the failure upstream PR #352 repaired for the release path and left
/// standing on every other path.
///
/// Returns whether the client actually received it.
fn answer(
    key: &str,
    id: u64,
    ack: oneshot::Sender<LockState>,
    state: LockState,
) -> bool {
    match ack.send(state) {
        Ok(()) => true,
        Err(state) => {
            debug!(
                "Lock client for {key} / {id} has gone away before receiving {state:?}; \
                 the handler keeps running"
            );
            false
        }
    }
}

async fn handler(rx: flume::Receiver<LockRequest>, lease_seconds: i64) {
    // Lease timing (`exp`) intentionally uses this node's wall clock. All lock decisions are made
    // by the Raft leader's handler, so they are always consistent with the leader's clock. The
    // per-node `exp` copies in the state machine diverge by clock skew, but that is benign: Raft
    // never verifies state machine equality, and the new leader evaluates leases with its own
    // clock. A deterministic timestamp inside the raft entry would require changing the entry
    // format, which would break log compatibility for rolling upgrades. Keep the clocks within
    // ~1s of each other so a 10s lease survives failover comfortably.
    let mut locks: HashMap<String, LockQueue> = HashMap::new();
    let mut queues: HashMap<String, Vec<(u64, oneshot::Sender<LockState>)>> = HashMap::new();

    while let Ok(req) = rx.recv_async().await {
        match req {
            LockRequest::Lock(LockRequestPayload { key, log_id, ack }) => {
                let now = Utc::now().timestamp();
                if let Some(lock) = locks.get_mut(key.as_ref()) {
                    // If the lease of the current holder has expired, the holder is considered
                    // dead. Any ticket at the front of the queue that had a full lease window to
                    // reclaim the lock but did not is dead as well: drop it (and wake its client,
                    // if it is still waiting) so it can never block the lock forever.
                    if lock.exp < now {
                        while let Some(&ticket) = lock.queue.front()
                            && ticket != log_id
                        {
                            lock.queue.pop_front();
                            if let Some(acks) = queues.get_mut(key.as_ref())
                                && let Some(pos) = acks.iter().position(|(i, _)| *i == ticket)
                            {
                                let (_, ack) = acks.swap_remove(pos);
                                answer(&key, ticket, ack, LockState::Released);
                            }
                        }
                    }

                    if lock.exp < now || lock.current_ticket.is_none() {
                        let front = lock.queue.front();
                        if let Some(ticket) = front {
                            if *ticket == log_id {
                                lock.queue.pop_front();
                                lock.current_ticket = Some(log_id);
                                lock.exp = now + lease_seconds;
                                if !answer(&key, log_id, ack, LockState::Locked(log_id)) {
                                    // Granted to a client that is no longer there. Hand it on
                                    // now instead of holding it for a full lease window.
                                    lock.current_ticket = None;
                                    lock.exp = now + lease_seconds;
                                }
                            } else {
                                lock.queue.push_back(log_id);
                                answer(&key, log_id, ack, LockState::Queued(log_id));
                            }
                        } else {
                            lock.current_ticket = Some(log_id);
                            lock.exp = now + lease_seconds;
                            if !answer(&key, log_id, ack, LockState::Locked(log_id)) {
                                lock.current_ticket = None;
                            }
                        }
                    } else {
                        lock.queue.push_back(log_id);
                        answer(&key, log_id, ack, LockState::Queued(log_id));
                    }
                } else {
                    locks.insert(
                        key.to_string(),
                        LockQueue {
                            current_ticket: Some(log_id),
                            exp: now + lease_seconds,
                            queue: Default::default(),
                        },
                    );
                    if !answer(&key, log_id, ack, LockState::Locked(log_id)) {
                        locks.remove(key.as_ref());
                    }
                }
            }

            LockRequest::Acquire(LockRequestPayload { key, log_id, ack }) => {
                let now = Utc::now().timestamp();
                // A client whose bounded await timed out re-requests here, and its earlier
                // `Await` registration is still in `queues`. Nothing answers it once this ticket
                // is granted, so it stayed until the key's queue emptied completely, which on a
                // key that is never idle is never (found by the AI review of `b5039d2`). The
                // client is no longer listening on it; drop it.
                if let Some(acks) = queues.get_mut(key.as_ref()) {
                    acks.retain(|(i, _)| *i != log_id);
                }
                if let Some(lock) = locks.get_mut(key.as_ref()) {
                    // F-102: a waiter's bounded await ends in this re-request, so this is where
                    // a holder whose lease ran out without a release is noticed. Before, only a
                    // fresh `Lock` looked at `exp`, and a waiter that was never woken sat out
                    // the client's 120-second request timeout.
                    if lock.exp < now {
                        if lock.current_ticket.is_some_and(|holder| holder != log_id) {
                            lock.current_ticket = None;
                        }
                        // Same dead-ticket handling as in `Lock`: a front ticket that had its
                        // whole window and did not claim is gone.
                        while let Some(&ticket) = lock.queue.front()
                            && ticket != log_id
                            && lock.current_ticket.is_none()
                        {
                            lock.queue.pop_front();
                            if let Some(acks) = queues.get_mut(key.as_ref())
                                && let Some(pos) = acks.iter().position(|(i, _)| *i == ticket)
                            {
                                let (_, ack) = acks.swap_remove(pos);
                                answer(&key, ticket, ack, LockState::Released);
                            }
                        }
                    }

                    if lock.current_ticket == Some(log_id) {
                        // Already ours: a retry after a lost response. Answer it again, with a
                        // lease that has time left in it rather than whatever remained.
                        lock.exp = now + lease_seconds;
                        answer(&key, log_id, ack, LockState::Locked(log_id));
                    } else if lock.current_ticket.is_some() {
                        // Someone else holds the lock (e.g. our lease expired and the lock was
                        // re-granted). Keep our place, never a second copy of it, and report
                        // back so the client can wait again.
                        if !lock.queue.contains(&log_id) {
                            lock.queue.push_back(log_id);
                        }
                        answer(&key, log_id, ack, LockState::Queued(log_id));
                    } else if let Some(first) = lock.queue.front() {
                        if *first == log_id {
                            lock.queue.pop_front();
                            lock.current_ticket = Some(log_id);
                            lock.exp = now + lease_seconds;
                            if !answer(&key, log_id, ack, LockState::Locked(log_id)) {
                                lock.current_ticket = None;
                                lock.exp = now + lease_seconds;
                            }
                        } else {
                            // Our ticket is not the promoted one anymore: keep or take a place.
                            if !lock.queue.contains(&log_id) {
                                lock.queue.push_back(log_id);
                            }
                            answer(&key, log_id, ack, LockState::Queued(log_id));
                        }
                    } else {
                        // Nobody is queued and nobody holds the lock -> take it directly.
                        lock.current_ticket = Some(log_id);
                        lock.exp = now + lease_seconds;
                        if !answer(&key, log_id, ack, LockState::Locked(log_id)) {
                            lock.current_ticket = None;
                        }
                    }
                } else {
                    // The lock was fully removed while this request was in flight. Grant a fresh
                    // one so the client never hangs.
                    locks.insert(
                        key.to_string(),
                        LockQueue {
                            current_ticket: Some(log_id),
                            exp: now + lease_seconds,
                            queue: Default::default(),
                        },
                    );
                    if !answer(&key, log_id, ack, LockState::Locked(log_id)) {
                        locks.remove(key.as_ref());
                    }
                }
            }

            LockRequest::Release(LockReleasePayload { key, id }) => {
                let now = Utc::now().timestamp();
                let mut full_remove = false;

                if let Some(lock) = locks.get_mut(key.as_ref()) {
                    if lock.current_ticket == Some(id) {
                        lock.current_ticket = None;

                        // Wake the front of the queue so it can acquire. A front ticket whose
                        // client is provably gone (the send fails, or it never registered an
                        // await) must not sit there holding up everyone behind it.
                        //
                        // `exp` is refreshed here, and that is the load-bearing part. It is the
                        // deadline by which the *promoted* ticket has to act, and the eviction
                        // loops in `Lock` and `Await` are keyed on `exp < now`. Before this,
                        // `exp` still belonged to the holder that just released, so a promoted
                        // ticket whose client had gone away was never evicted while that stale
                        // deadline was in the future, and a third client asking for the same
                        // lock was queued behind a ticket that would never move.
                        lock.exp = now + lease_seconds;

                        loop {
                            let Some(&first) = lock.queue.front() else {
                                full_remove = true;
                                break;
                            };

                            let delivered = match queues.get_mut(key.as_ref()) {
                                Some(acks) => match acks.iter().position(|(i, _)| *i == first) {
                                    Some(pos) => {
                                        let (_, ack) = acks.swap_remove(pos);
                                        answer(&key, first, ack, LockState::Released)
                                    }
                                    // No registered await for the promoted ticket. It may not
                                    // have got round to awaiting yet, so it keeps its place and
                                    // the refreshed deadline bounds how long that can last.
                                    None => true,
                                },
                                None => true,
                            };

                            if delivered {
                                break;
                            }
                            // The client behind the promoted ticket is gone. Drop it now rather
                            // than making the next one wait out a whole lease window.
                            lock.queue.pop_front();
                        }
                    } else {
                        // The lease expired and the lock was granted to another ticket, or this
                        // is a duplicate release. Releasing an already released / re-granted lock
                        // is safe to ignore. Panicking here would kill the whole dlock handler.
                        warn!(
                            "Ignoring release for lock {key} / {id}: current holder is not this \
                            ticket (current_ticket: {:?})",
                            lock.current_ticket
                        );
                    }
                }

                if full_remove {
                    locks.remove(key.as_ref());
                    // Nothing is queued, so nothing is waiting: wake any straggler that is and
                    // drop the entry, which used to accumulate for the life of the process.
                    if let Some(acks) = queues.remove(key.as_ref()) {
                        for (waiter, ack) in acks {
                            answer(&key, waiter, ack, LockState::Released);
                        }
                    }
                }
            }

            LockRequest::Await(LockAwaitPayload { key, id, ack }) => {
                // F-102: an await only waits. It is not a Raft entry: an embedded client sends
                // it to its **own** node's handler, which may be a follower, so anything it
                // changed here was a change to one node's copy of replicated state. It used to
                // evict tickets and grant `Locked` from that node's view; a follower whose view
                // lagged the leader's could grant a lock the leader had given to someone else.
                // Every state change now goes through `Lock`, `Acquire` or `Release`, which
                // every node applies in log order, and an await that would have been granted
                // is answered `Released` so its client claims through the leader instead.
                //
                // The holder's lease is deliberately not judged here. This node's clock may be
                // ahead of the leader's, and an await that answered `Released` on its own view
                // of `exp` sent its client round a loop of Raft writes the leader kept answering
                // `Queued`. Expiry is judged by `Acquire`, which the client's bounded await ends
                // in.
                match locks.get(key.as_ref()) {
                    Some(lock)
                        if lock.current_ticket.is_some()
                            || lock.queue.front().is_some_and(|front| *front != id) =>
                    {
                        let acks = queues.entry(key.to_string()).or_default();
                        // A client that timed out and awaits again replaces its old
                        // registration. Left in place, the dead entry is found first, fails to
                        // deliver, and gets the live ticket dropped. The replaced one is
                        // **answered**, not dropped: for a remote client its receiver is a
                        // server task that treats a dropped channel as a broken invariant.
                        if let Some(pos) = acks.iter().position(|(i, _)| *i == id) {
                            let (_, old) = acks.swap_remove(pos);
                            answer(&key, id, old, LockState::Released);
                        }
                        acks.push((id, ack));
                    }
                    // Nothing holds it, or it is ours to claim, or the lock was removed while
                    // this await was in flight: re-request.
                    _ => {
                        answer(&key, id, ack, LockState::Released);
                    }
                }
            }

            LockRequest::SnapshotBuild(ack) => {
                if ack.send(locks.clone()).is_err() {
                    warn!("Snapshot build requester for the dlock handler has gone away");
                }
            }

            LockRequest::SnapshotInstall((data, ack)) => {
                locks = data;
                // Installed state has no waiters: the awaiters registered on this node belong
                // to the state that was just replaced, so anything still parked is woken and
                // told to re-request rather than left waiting on a queue that no longer exists.
                for (key, acks) in queues.drain() {
                    for (waiter, waiter_ack) in acks {
                        answer(&key, waiter, waiter_ack, LockState::Released);
                    }
                }
                if ack.send(()).is_err() {
                    warn!("Snapshot install requester for the dlock handler has gone away");
                }
            }
        }

        // `queues` only ever shrank element by element, so an emptied vector stayed in the map
        // for the life of the process, once per key that was ever awaited.
        queues.retain(|_, acks| !acks.is_empty());
    }

    debug!("DLock handler exiting");

    // Nothing else will ever answer these.
    for (key, acks) in queues.drain() {
        for (waiter, ack) in acks {
            answer(&key, waiter, ack, LockState::Released);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use tokio::sync::oneshot;

    pub(super) fn send(tx: &flume::Sender<LockRequest>, req: LockRequest) {
        tx.send(req).expect("handler to be running");
    }

    pub(super) async fn lock(tx: &flume::Sender<LockRequest>, key: &str, log_id: u64) -> LockState {
        let (ack, rx) = oneshot::channel();
        send(
            tx,
            LockRequest::Lock(LockRequestPayload {
                key: Cow::Owned(key.to_string()),
                log_id,
                ack,
            }),
        );
        rx.await.unwrap()
    }

    pub(super) async fn acquire(tx: &flume::Sender<LockRequest>, key: &str, log_id: u64) -> LockState {
        let (ack, rx) = oneshot::channel();
        send(
            tx,
            LockRequest::Acquire(LockRequestPayload {
                key: Cow::Owned(key.to_string()),
                log_id,
                ack,
            }),
        );
        rx.await.unwrap()
    }

    pub(super) async fn await_lock(tx: &flume::Sender<LockRequest>, key: &str, id: u64) -> LockState {
        let (ack, rx) = oneshot::channel();
        send(
            tx,
            LockRequest::Await(LockAwaitPayload {
                key: Cow::Owned(key.to_string()),
                id,
                ack,
            }),
        );
        rx.await.unwrap()
    }

    pub(super) fn release(tx: &flume::Sender<LockRequest>, key: &str, id: u64) {
        send(
            tx,
            LockRequest::Release(LockReleasePayload {
                key: Cow::Owned(key.to_string()),
                id,
            }),
        );
    }

    #[tokio::test]
    async fn lock_release_roundtrip() {
        let tx = spawn();
        assert_eq!(lock(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock(&tx, "k", 2).await, LockState::Queued(2));
        release(&tx, "k", 1);
        // F-102: no current holder anymore, so the await tells the queued ticket to claim, and
        // the claim is a replicated `Acquire`. It used to grant `Locked` from the await itself,
        // which is a change no other node applied.
        assert_eq!(await_lock(&tx, "k", 2).await, LockState::Released);
        assert_eq!(acquire(&tx, "k", 2).await, LockState::Locked(2));
        release(&tx, "k", 2);
    }

    #[tokio::test]
    async fn duplicate_release_is_ignored() {
        let tx = spawn();
        assert_eq!(lock(&tx, "k", 1).await, LockState::Locked(1));
        release(&tx, "k", 1);
        // second release of the same ticket must not panic the handler
        release(&tx, "k", 1);
        // handler is still alive
        assert_eq!(lock(&tx, "k", 2).await, LockState::Locked(2));
    }

    #[tokio::test]
    async fn release_after_lock_was_removed_is_ignored() {
        let tx = spawn();
        assert_eq!(lock(&tx, "k", 1).await, LockState::Locked(1));
        release(&tx, "k", 1); // no waiters -> lock removed entirely
        release(&tx, "k", 1); // stale release must be a no-op
        assert_eq!(lock(&tx, "k", 2).await, LockState::Locked(2));
    }

    #[tokio::test]
    async fn acquire_after_lock_was_removed_grants_fresh() {
        let tx = spawn();
        assert_eq!(lock(&tx, "k", 1).await, LockState::Locked(1));
        release(&tx, "k", 1); // removed
        // a client re-claiming with an old ticket must not hang or panic
        assert_eq!(acquire(&tx, "k", 1).await, LockState::Locked(1));
        release(&tx, "k", 1);
    }

    #[tokio::test]
    async fn await_when_lock_was_removed_returns_released() {
        let tx = spawn();
        assert_eq!(lock(&tx, "k", 1).await, LockState::Locked(1));
        release(&tx, "k", 1); // removed
        // an in-flight await must be answered, not hang
        assert_eq!(await_lock(&tx, "k", 1).await, LockState::Released);
        assert_eq!(acquire(&tx, "k", 1).await, LockState::Locked(1));
    }

    #[tokio::test]
    async fn late_release_after_takeover_is_ignored() {
        let tx = spawn();
        assert_eq!(lock(&tx, "k", 1).await, LockState::Locked(1));
        release(&tx, "k", 1);
        // lock is free again, ticket 2 takes it
        assert_eq!(lock(&tx, "k", 2).await, LockState::Locked(2));
        // the old holder (ticket 1) releases late -> must be ignored, not panic
        release(&tx, "k", 1);
        // ticket 2 still holds the lock and can release it normally
        release(&tx, "k", 2);
        assert_eq!(lock(&tx, "k", 3).await, LockState::Locked(3));
    }
}

#[cfg(test)]
mod lease_tests {
    use super::tests::{release, send};
    use super::*;
    use std::borrow::Cow;
    use std::time::Duration;
    use tokio::sync::oneshot;

    /// A lease short enough to wait out. `exp` is a Unix second, so this is the smallest value
    /// that can be exceeded by waiting, and the test waits past it rather than up to it.
    const SHORT_LEASE: i64 = 1;

    async fn wait_out_the_lease() {
        // One second of lease plus the sub-second remainder of the current wall-clock second.
        tokio::time::sleep(Duration::from_millis(2_100)).await;
    }

    /// A `lock` that fails the test rather than hanging when the handler is not there.
    ///
    /// The helpers in the parent module `.expect(..)` on the send and `.unwrap()` on the
    /// receive, both of which depend on how promptly a panicked task's channel ends are
    /// dropped. Bounding it here makes "the handler died" a failed assertion instead of a
    /// hung test, which is what lets these tests be run against the unrepaired handler at all.
    async fn lock_bounded(tx: &flume::Sender<LockRequest>, key: &str, log_id: u64) -> LockState {
        let (ack, rx) = oneshot::channel();
        tx.send(LockRequest::Lock(LockRequestPayload {
            key: Cow::Owned(key.to_string()),
            log_id,
            ack,
        }))
        .expect("the handler must still be accepting requests");

        tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .expect("the handler must answer within five seconds")
            .expect("the handler must not drop the answer channel")
    }

    /// Three awaiters queued on one key are each promoted in turn, and none is left parked.
    ///
    /// F-102: the cluster suite's distributed-lock phase queues three handles on one key and
    /// awaits them, and one run stalled there for 120 seconds with no further output. Nothing
    /// in this module drove more than one awaiter through a promotion chain, so the chain
    /// `Release` walks (refresh `exp`, wake the front, drop a dead ticket and promote the next
    /// in the same pass) was only ever exercised one link at a time.
    ///
    /// Every wait is bounded, so a lost wake is a failed assertion naming which link broke
    /// rather than a hung test.
    #[tokio::test(flavor = "multi_thread")]
    async fn three_queued_awaiters_are_each_promoted_in_turn() {
        let tx = spawn();

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        for id in 2..=4 {
            assert_eq!(
                lock_bounded(&tx, "k", id).await,
                LockState::Queued(id),
                "ticket {id} should be queued behind the holder"
            );
        }

        // All three park before any release, which is the ordering the cluster phase produces.
        let mut awaiting = Vec::new();
        for id in 2..=4 {
            let tx = tx.clone();
            awaiting.push((
                id,
                tokio::spawn(async move { super::tests::await_lock(&tx, "k", id).await }),
            ));
        }

        // Give the handler time to register all three awaits before the first release, so the
        // test exercises promotion of a registered awaiter rather than the "has not awaited
        // yet" path, which `023` B-3 keeps deliberately.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let mut holder = 1;
        for (id, handle) in awaiting {
            release(&tx, "k", holder);

            let state = tokio::time::timeout(Duration::from_secs(5), handle)
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "awaiter {id} was never woken after {holder} released: the promotion \
                         chain stopped at this link"
                    )
                })
                .expect("the awaiting task must not panic");

            // The protocol has two ways to be granted, and `client::dlock` handles both:
            // `Locked` directly, or `Released` meaning "your ticket was promoted, claim it".
            // Anything else leaves a caller with no lock and nothing to re-request.
            match state {
                LockState::Locked(got) => assert_eq!(got, id, "the wrong ticket was granted"),
                LockState::Released => {
                    let claimed = tokio::time::timeout(
                        Duration::from_secs(5),
                        super::tests::acquire(&tx, "k", id),
                    )
                    .await
                    .unwrap_or_else(|_| {
                        panic!("awaiter {id} was promoted but could not then claim the lock")
                    });
                    assert_eq!(
                        claimed,
                        LockState::Locked(id),
                        "a promoted ticket must be claimable with the same id"
                    );
                }
                other => panic!("awaiter {id} got {other:?}, which is neither a grant nor a \
                                 promotion it can act on"),
            }
            holder = id;
        }

        // And the key is usable afterwards: the last holder releases and a new caller gets it.
        release(&tx, "k", holder);
        assert_eq!(
            lock_bounded(&tx, "k", 99).await,
            LockState::Locked(99),
            "after the whole chain drains, the key must be free"
        );
    }

    /// Register an await and then drop the receiver, which is what a cancelled `lock()` future
    /// does: a timeout, a `select!` arm losing, or an aborted task.
    fn await_then_abandon(tx: &flume::Sender<LockRequest>, key: &str, id: u64) {
        let (ack, rx) = oneshot::channel();
        send(
            tx,
            LockRequest::Await(LockAwaitPayload {
                key: Cow::Owned(key.to_string()),
                id,
                ack,
            }),
        );
        drop(rx);
    }

    /// Upstream PR #352 stopped a stale release from panicking the handler. The rest of the
    /// arms kept their `ack.send(..).unwrap()`, so a client that goes away between sending a
    /// request and receiving its answer still killed the handler, and with it every lock on
    /// the node.
    ///
    /// The reachable case is a leader-local await, whose receiver lives in the caller's future:
    /// a cancelled `lock()` drops it. This drives that directly, then asserts the handler is
    /// still serving an unrelated key.
    #[tokio::test]
    async fn an_abandoned_waiter_never_kills_the_handler() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Queued(2));
        await_then_abandon(&tx, "k", 2);

        // The release wakes the abandoned waiter. Before this repair the send failure was
        // handled here and nowhere else; the other arms panicked.
        release(&tx, "k", 1);

        assert_eq!(
            lock_bounded(&tx, "unrelated", 10).await,
            LockState::Locked(10),
            "the handler must still be serving other keys"
        );
        assert!(!tx.is_disconnected(), "the handler task must still be alive");
    }

    /// A grant that nobody received is not a held lock.
    ///
    /// The `Lock` arm sets `current_ticket` and *then* answers. If the answer cannot be
    /// delivered, the ticket belongs to a client that will never release it, so the lock was
    /// held for a whole lease window by nobody. It is now handed straight on.
    #[tokio::test]
    async fn a_grant_that_was_never_received_does_not_hold_the_lock() {
        let tx = spawn_with_lease(SHORT_LEASE);

        let (ack, rx) = oneshot::channel();
        send(
            &tx,
            LockRequest::Lock(LockRequestPayload {
                key: Cow::Owned("k".to_string()),
                log_id: 1,
                ack,
            }),
        );
        drop(rx);

        // Synchronized on the handler having processed the first request: this second one is
        // answered only after it, because the handler is a single loop over one channel.
        assert_eq!(
            lock_bounded(&tx, "k", 2).await,
            LockState::Locked(2),
            "an undelivered grant must not make the next caller wait out a lease"
        );
    }

    /// The liveness hole PR #352 left standing.
    ///
    /// `Release` used to set `current_ticket = None`, wake the front of the queue, and leave
    /// `exp` belonging to the holder that had just released. The eviction loops in `Lock` and
    /// `Await` are keyed on `exp < now`, so while that stale deadline was still in the future
    /// a promoted ticket whose client had gone away was never evicted, and a third client
    /// asking for the same lock was queued behind a ticket that would never move.
    ///
    /// Here the promoted waiter has been abandoned, so the release drops it immediately and
    /// the third client gets the lock without waiting for anything.
    #[tokio::test]
    async fn a_dead_promoted_ticket_does_not_block_the_next_caller() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Queued(2));
        await_then_abandon(&tx, "k", 2);

        release(&tx, "k", 1);

        assert_eq!(
            lock_bounded(&tx, "k", 3).await,
            LockState::Locked(3),
            "ticket 2's client is gone, so ticket 3 must not be queued behind it"
        );
    }

    /// A TTL takeover followed by the old holder's release must leave the lock usable.
    ///
    /// This is the sequence Rahi pinned an unreleased upstream commit for. The release half is
    /// PR #352's; what is added here is that the takeover, the stale release, and an unrelated
    /// key are all asserted in one run, so "does not kill the handler" and "does not block
    /// unrelated acquisitions" are both demonstrated rather than argued.
    #[tokio::test]
    async fn a_stale_release_after_takeover_blocks_nothing() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "other", 100).await, LockState::Locked(100));

        wait_out_the_lease().await;

        // Ticket 1's lease has expired, so ticket 2 takes over.
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Locked(2));

        // The old holder finally releases. It is not the current holder any more.
        release(&tx, "k", 1);

        // The taker still holds it.
        assert_eq!(
            lock_bounded(&tx, "k", 3).await,
            LockState::Queued(3),
            "the stale release must not have freed the lock ticket 2 holds"
        );
        // An unrelated key is untouched throughout.
        release(&tx, "other", 100);
        assert_eq!(lock_bounded(&tx, "other", 101).await, LockState::Locked(101));
        assert!(!tx.is_disconnected());
    }

    /// Node restart, interrupted operation: the holder died with the node without releasing.
    ///
    /// What survives a restart is the snapshot, whose `exp` is an absolute Unix second and is
    /// therefore already in the past by the time it is installed. Reacquisition is bounded by
    /// that: the first requester after the install takes over, with no wait.
    ///
    /// Stated rather than implied: this is the *snapshot* path. The log-replay path is the
    /// next test, and it behaves differently, which is the point of having both.
    #[tokio::test]
    async fn an_interrupted_lease_is_reacquired_immediately_from_a_snapshot() {
        let before = spawn_with_lease(SHORT_LEASE);
        assert_eq!(lock_bounded(&before, "k", 1).await, LockState::Locked(1));

        let (ack, rx) = oneshot::channel();
        send(&before, LockRequest::SnapshotBuild(ack));
        let snapshot = rx.await.unwrap();
        assert!(snapshot.contains_key("k"), "the held lock is in the snapshot");

        // The node goes away without ticket 1 ever releasing.
        drop(before);

        let after = spawn_with_lease(SHORT_LEASE);
        let (ack, rx) = oneshot::channel();
        send(&after, LockRequest::SnapshotInstall((snapshot, ack)));
        rx.await.unwrap();

        wait_out_the_lease().await;

        assert_eq!(
            lock_bounded(&after, "k", 2).await,
            LockState::Locked(2),
            "a lease whose deadline has passed is reacquirable by the next caller"
        );
    }

    /// Node restart, completed operation: the holder released before the node went away.
    ///
    /// Both entries are in the log, so both are replayed, and the lock is free the moment
    /// replay finishes. No lease window is waited out at all.
    #[tokio::test]
    async fn a_completed_operation_leaves_nothing_to_wait_for_after_a_restart() {
        let after = spawn_with_lease(SHORT_LEASE);

        // Replay of the committed entries, in log order.
        assert_eq!(lock_bounded(&after, "k", 1).await, LockState::Locked(1));
        release(&after, "k", 1);

        assert_eq!(
            lock_bounded(&after, "k", 2).await,
            LockState::Locked(2),
            "a released lease is free immediately after replay"
        );
    }

    /// Replay of an interrupted operation re-grants the lease with a deadline computed at
    /// replay time, not at the time the entry was written.
    ///
    /// So the bound on reacquiring a lease whose owner died with the node is **one lease
    /// window measured from the restart**, not from the original acquisition. That is a real
    /// limitation and it is recorded here as executed behavior rather than described.
    #[tokio::test]
    async fn replaying_an_unreleased_lock_re_grants_it_for_one_more_lease_window() {
        let after = spawn_with_lease(SHORT_LEASE);

        // Replay: the `Lock` entry is in the log and its `LockRelease` is not.
        assert_eq!(lock_bounded(&after, "k", 1).await, LockState::Locked(1));

        assert_eq!(
            lock_bounded(&after, "k", 2).await,
            LockState::Queued(2),
            "replay re-grants the lease to a ticket whose client no longer exists"
        );

        wait_out_the_lease().await;

        assert_eq!(
            lock_bounded(&after, "k", 3).await,
            LockState::Locked(3),
            "and it is reacquirable after exactly one lease window from the restart"
        );
    }

    /// The awaiter map used to grow: entries were removed element by element and an emptied
    /// vector stayed for the life of the process, once per key ever awaited.
    ///
    /// Asserted through behavior rather than by reaching into the map, because the map is a
    /// local of the handler task: a fully released lock is removed outright, and a waiter
    /// registered against it is woken rather than left parked.
    #[tokio::test]
    async fn a_fully_released_lock_wakes_its_stragglers() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));

        // A waiter that registered against a ticket which is not in the queue at all.
        let (ack, rx) = oneshot::channel();
        send(
            &tx,
            LockRequest::Await(LockAwaitPayload {
                key: Cow::Owned("k".to_string()),
                id: 99,
                ack,
            }),
        );

        release(&tx, "k", 1);

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), rx)
                .await
                .expect("a straggler must be woken within five seconds")
                .expect("the handler must not drop the answer channel"),
            LockState::Released,
            "a straggler on a removed lock is told to re-request, not left waiting"
        );
    }

    /// Register an await and hand back its receiver, so a test can bound it.
    fn await_registered(
        tx: &flume::Sender<LockRequest>,
        key: &str,
        id: u64,
    ) -> oneshot::Receiver<LockState> {
        let (ack, rx) = oneshot::channel();
        send(
            tx,
            LockRequest::Await(LockAwaitPayload {
                key: Cow::Owned(key.to_string()),
                id,
                ack,
            }),
        );
        rx
    }

    async fn acquire_bounded(tx: &flume::Sender<LockRequest>, key: &str, log_id: u64) -> LockState {
        let (ack, rx) = oneshot::channel();
        send(
            tx,
            LockRequest::Acquire(LockRequestPayload {
                key: Cow::Owned(key.to_string()),
                log_id,
                ack,
            }),
        );
        tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .expect("the handler must answer within five seconds")
            .expect("the handler must not drop the answer channel")
    }

    /// F-102. A holder whose release never arrives, because it died or its release was lost,
    /// wakes nobody: lease expiry is only noticed when a request arrives, and a parked waiter
    /// sends none. Its only way out is the client's bounded await ending in a re-request with
    /// its own ticket, and that re-request used to be queued again behind the dead holder,
    /// because `Acquire` never looked at `exp`. So the waiter sat out the 120-second request
    /// timeout, which is the 119.9 seconds F-102 measured.
    #[tokio::test]
    async fn a_waiter_whose_holder_never_releases_claims_after_one_lease() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Queued(2));
        let mut parked = await_registered(&tx, "k", 2);

        // Ticket 1 never releases. Past its lease, nothing has woken the waiter.
        wait_out_the_lease().await;
        assert!(
            parked.try_recv().is_err(),
            "nothing wakes a parked waiter when a lease expires; that is why the client bounds it"
        );

        // The client's bounded await ends, and it re-requests with its ticket.
        assert_eq!(
            acquire_bounded(&tx, "k", 2).await,
            LockState::Locked(2),
            "a re-request after the holder's lease must claim the lock, not queue behind it"
        );
    }

    /// F-102. An await is not a Raft entry, and for an embedded client it runs on that client's
    /// own node, which may be a follower. It must change nothing: it used to grant `Locked` and
    /// evict tickets from the local view. Here it would have granted ticket 2; instead it says
    /// "claim", and the lock is still ticket 2's to claim, not a fresh caller's.
    #[tokio::test]
    async fn an_await_changes_no_lock_state() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Queued(2));
        release(&tx, "k", 1);

        let rx = await_registered(&tx, "k", 2);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), rx)
                .await
                .unwrap()
                .unwrap(),
            LockState::Released,
            "an await that would have been granted is told to claim through a replicated request"
        );
        assert_eq!(
            lock_bounded(&tx, "k", 3).await,
            LockState::Queued(3),
            "the await did not take the lock, so ticket 2 is still first in line"
        );
        assert_eq!(acquire_bounded(&tx, "k", 2).await, LockState::Locked(2));
    }

    /// F-102. A re-request while someone else holds the lock keeps the ticket's place and never
    /// adds a second copy. A duplicate used to stay at the front after the ticket's first copy
    /// was granted and released, and made the next caller wait out a lease behind nobody.
    #[tokio::test]
    async fn a_re_request_never_duplicates_its_ticket() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Queued(2));
        assert_eq!(acquire_bounded(&tx, "k", 2).await, LockState::Queued(2));
        assert_eq!(acquire_bounded(&tx, "k", 2).await, LockState::Queued(2));
        release(&tx, "k", 1);
        assert_eq!(acquire_bounded(&tx, "k", 2).await, LockState::Locked(2));
        // A retry after a lost response is answered again, not queued.
        assert_eq!(acquire_bounded(&tx, "k", 2).await, LockState::Locked(2));
        release(&tx, "k", 2);
        assert_eq!(
            lock_bounded(&tx, "k", 3).await,
            LockState::Locked(3),
            "no stale copy of ticket 2 may be left in front of the next caller"
        );
    }

    /// F-102. A client whose await timed out awaits again with the same ticket. The dead
    /// registration used to stay first in the list, fail to deliver on the next release, and
    /// get the live ticket dropped from the queue, so its new await was never answered.
    #[tokio::test]
    async fn a_timed_out_await_does_not_cost_the_live_ticket_its_place() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Queued(2));
        assert_eq!(lock_bounded(&tx, "k", 3).await, LockState::Queued(3));

        await_then_abandon(&tx, "k", 2);
        let live = await_registered(&tx, "k", 2);
        release(&tx, "k", 1);

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), live)
                .await
                .expect("the live await must be answered within five seconds")
                .unwrap(),
            LockState::Released
        );
        assert_eq!(acquire_bounded(&tx, "k", 2).await, LockState::Locked(2));
    }

    /// A replaced registration is answered, never dropped. For a remote client the receiver
    /// is a server task in `network::api` that used to `expect` an answer, so dropping it
    /// panicked that task, and under `panic = "abort"` ended the node. Found in review.
    #[tokio::test]
    async fn a_replaced_await_is_answered_not_dropped() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Queued(2));
        let first = await_registered(&tx, "k", 2);
        let _second = await_registered(&tx, "k", 2);

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), first)
                .await
                .expect("the replaced await must be answered within five seconds")
                .expect("the replaced await must be answered, not have its channel dropped"),
            LockState::Released
        );
    }

    /// An await does not judge a lease on its own node's clock. A follower whose clock ran
    /// ahead answered `Released` for the last seconds of every lease while the leader kept
    /// answering `Queued`, and its client looped on Raft writes. Found in review.
    #[tokio::test]
    async fn an_await_does_not_judge_the_lease() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Queued(2));
        wait_out_the_lease().await;

        let mut parked = await_registered(&tx, "k", 2);
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            parked.try_recv().is_err(),
            "an expired holder is noticed by `Acquire`, not answered by an await"
        );
        assert_eq!(acquire_bounded(&tx, "k", 2).await, LockState::Locked(2));
    }

    /// A ticket that re-requests through `Acquire` leaves no registration behind. Its earlier
    /// `Await` is answered or dropped, never kept for a client that has moved on.
    #[tokio::test]
    async fn an_acquire_leaves_no_stale_await_registration() {
        let tx = spawn_with_lease(SHORT_LEASE);

        assert_eq!(lock_bounded(&tx, "k", 1).await, LockState::Locked(1));
        assert_eq!(lock_bounded(&tx, "k", 2).await, LockState::Queued(2));
        let stale = await_registered(&tx, "k", 2);
        wait_out_the_lease().await;
        // The client's await timed out; it claims through `Acquire` instead.
        assert_eq!(acquire_bounded(&tx, "k", 2).await, LockState::Locked(2));

        // The old registration is gone: its sender was dropped, not left in the map.
        let got = tokio::time::timeout(Duration::from_secs(5), stale)
            .await
            .expect("the stale registration must not be held open");
        assert!(got.is_err(), "the stale registration is dropped, got {got:?}");
    }
}
