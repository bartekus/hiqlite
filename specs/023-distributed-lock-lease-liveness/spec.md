---
id: "023-distributed-lock-lease-liveness"
title: "Repair the distributed-lock handler's liveness and state the lease limits it does not remove"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "006-cache-state-machine"
  - "022-replicated-cache-command-compatibility"
amends: ["022-replicated-cache-command-compatibility"]
# D-5: this spec's `## Verification` block IS 022's acceptance from now on, which is also 006's
# through 022. Whole-block replacement is the mechanism's unit, so the block carries every
# obligation 022 declared plus this repair's.
amends_verification: ["022-replicated-cache-command-compatibility"]
amends_sections:
  - "5-known-defects"
extends:
  - spec: "006-cache-state-machine"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/memory/" }
    nature: superseding
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Completes what upstream PR #352 started. Every remaining acknowledgement in
  the lock handler could kill the whole handler when its client had gone away,
  a promoted ticket inherited the released holder's deadline and could block
  every later caller, an undelivered grant held the lock for a full lease
  window, and the awaiter map never shrank. States, with executed evidence,
  the lease limits none of this removes, and declines a longer or configurable
  TTL as a repair for them.
---

# 023: Repair the distributed-lock handler's liveness, and state the lease limits it does not remove

## 1. Purpose

Upstream PR #352, "Make the distributed-lock handler fault-tolerant and
deadlock-free", is in this fork's base. It repaired the release path: a stale
holder releasing after a TTL takeover used to panic the handler, and now it is
warned about and ignored. That repair is real and is preserved unchanged.

It is also not the whole of the problem, and one downstream consumer is
currently pinning that unreleased upstream commit for exactly this reason. This
spec is what the rest of it looks like.

Three things survive PR #352:

- **Every other acknowledgement is still `ack.send(..).unwrap()`.** The release
  path was made non-fatal and the fourteen other sends were not. A client that
  goes away between sending a request and receiving its answer therefore still
  kills the handler task, and with it every lock on the node.
- **A promoted ticket inherits the deadline of the holder that released.** The
  eviction loops are keyed on that deadline, so a promoted ticket whose client
  is gone is not evicted while it is still in the future, and everyone behind it
  waits.
- **The awaiter map only ever shrinks element by element.**

One responsibility: **a lock handler that no single client can take down or
stall, and an honest statement of what its lease does and does not promise.**

## 2. Territory

**Extends** `006`'s `directory` unit with nature `superseding` for
`dlock_handler.rs`. `006` still `establishes` it, and `022` extends the same
unit for the apply path; this spec's surface is disjoint from that one.

**Amends** `022`'s known-defects section and carries its acceptance (D-5), which
is also `006`'s through `022`.

**Ownership boundary.** The lease is a hiqlite construct, not a Raft one: Raft
orders the `Lock` and `LockRelease` entries and says nothing about liveness
between them. Nothing here is a distributed lease in the sense of a fenced,
clock-bounded mutual-exclusion primitive, and section 5 says so in the terms a
caller needs.

## 3. Behavior

### B-1. No client can kill the handler

Every acknowledgement goes through one helper that reports rather than panics,
and returns whether the client actually received it. The reachable case is a
**leader-local await**, whose receiver lives in the caller's own future: a
cancelled `lock()`, a lost `select!` arm, or an aborted task drops it. Before
this, that dropped the handler task with it.

Two snapshot acknowledgements are included, and the handler's exit path now
wakes anything still parked instead of dropping it silently.

### B-2. A grant nobody received is not a held lock

`Lock`, `Acquire` and `Await` each set `current_ticket` and then answer. If the
answer cannot be delivered, that ticket belongs to a client that will never
release it, so the lock was held by nobody for a whole lease window. The grant
is now undone at once: the ticket is cleared, and a lock that was created by
that same request is removed.

### B-3. A promoted ticket gets its own deadline

`Release` sets `current_ticket` to `None` and wakes the front of the queue. It
now also **refreshes `exp`**, and that is the load-bearing change.

`exp` is read by the eviction loops in `Lock` and `Await` as `exp < now`. Before
this it still belonged to the holder that had just released, so while that stale
deadline was in the future a promoted ticket whose client had gone away was
never evicted, and a third client asking for the same lock was queued behind a
ticket that would never move. With the refresh, `exp` means "the deadline by
which the current holder **or the promoted front ticket** has to act", which is
what the eviction loops were always assuming.

Additionally, when waking the promoted ticket **fails**, that ticket is known
dead immediately and is dropped there and then rather than after a lease window;
the next one is promoted in the same pass. A front ticket with no registered
await keeps its place, because it may simply not have got round to awaiting yet,
and the refreshed deadline bounds how long that can last.

### B-4. The awaiter map shrinks

An emptied acknowledgement vector is pruned each iteration; a lock that is fully
removed takes its waiters with it, each woken with `Released` so it re-requests
rather than staying parked; and a snapshot install wakes everything parked
against the state it replaced.

### B-5. The lease length is unchanged, and injectable only for tests

`LOCK_VALID_SECONDS` is still `10` and is still not configurable. A second
constructor takes the lease length so the expiry paths can be tested without
waiting out ten seconds per case, and `spawn` is its only non-test caller.

This is deliberately **not** offered as a repair. A longer lease does not make a
dead holder detectable any sooner than its own deadline; it makes every
detection later. A configurable one moves the choice to the operator without
changing what any value of it can promise. `006` KD-2 and F-026 stand exactly as
recorded.

### B-6. A waiter is bounded by the lease, and only replicated requests change lock state

F-102, repaired 2026-09-22. Read from source, not reproduced: the stall measured
119.9 seconds against the client's 120-second request timeout, and four things in
the source explain a waiter lasting exactly that long.

1. **Nothing wakes a parked waiter when a lease expires.** Expiry is noticed only
   when a request arrives. A holder that dies, or whose release is lost, leaves
   its waiters parked, and a parked waiter sends nothing.
2. **The waiter's wait was bounded only by the request timeout.** It now lasts at
   most one lease plus two seconds (`AWAIT_BOUND`, derived from
   `LOCK_VALID_SECONDS`), and then the client re-requests with its own ticket.
3. **That re-request (`Acquire`) never looked at `exp`**, so it was queued again
   behind the dead holder, and it pushed a **second copy** of its ticket. It now
   clears a holder whose lease is over, drops front tickets that had their window,
   answers a retry for a lock it already holds with `Locked` again, and never
   duplicates a ticket.
4. **An await changed replicated state outside Raft.** An embedded client sends
   its await to its **own** node's handler, which may be a follower. That path
   evicted tickets and could grant `Locked` from the local view, so a follower
   lagging the leader could grant a lock the leader had given to someone else.
   An await now only registers, replacing any earlier registration of the same
   ticket, or answers `Released`; every state change goes through `Lock`,
   `Acquire` or `Release`, which every node applies in log order.

**Corrected in review of the candidate, 2026-09-22.** Independent review found
that replacing a registration dropped the old answer channel, and a remote
client's server task `expect`ed an answer on it: under `panic = "abort"` that
ended the node. A replaced registration is now answered `Released`, and the
server task treats a dropped channel as `Released` rather than panicking. Review
also found that an await judging the lease on its own node's clock could send a
waiter round a loop of Raft writes when that clock ran ahead of the leader's; an
await no longer looks at `exp` at all, and a client whose awaits are answered at
once repeatedly backs off. A retried `Acquire` for a lock the ticket already holds
now refreshes the lease instead of returning one that may have run out.

**AI review of `b5039d2`.** A waiter whose bounded await timed out claims through
`Acquire`, and its earlier `Await` registration stayed in the handler's map until
the key's queue emptied completely, which on a key that is never idle is never.
`Acquire` now drops that ticket's registrations; `an_acquire_leaves_no_stale_await_registration`
was observed failing without it.

**Consequence for callers.** A queued caller whose holder dies acquires within
about one lease plus the await bound of the holder's grant, instead of failing
after 120 seconds. A promoted awaiter now always claims through a replicated
`Acquire`, one extra round trip.

## 4. Evidence and its limits

Nine tests in `dlock_handler.rs`, in a module beside the six PR #352 left,
which are unchanged and still pass. Eight were written with this spec; the
ninth was added on 2026-09-22 and is described below.

**Four fail against the unrepaired handler**, run and observed: 4 passed, 4
failed.

- an abandoned waiter never kills the handler, and an unrelated key is still
  served afterwards;
- a grant that was never received does not hold the lock;
- a dead promoted ticket does not block the next caller;
- a fully released lock wakes its stragglers.

A ninth was added on 2026-09-22, after F-102: **three queued awaiters are each
promoted in turn.** Nothing here drove the promotion chain more than one link at
a time, and B-3's `Release` walks it (refresh `exp`, wake the front, drop a dead
ticket and promote the next in the same pass). The test queues three awaiters on
one key, parks all three before any release, and bounds every wait so a lost
wake names the link that broke instead of hanging. It passes, sixty runs of
sixty, which is what moves F-102's suspicion off this handler rather than
leaving it pointed here.

Writing it corrected an assumption: a promoted awaiter is answered `Released`,
not `Locked`, and `client::dlock` re-requests with the same ticket. The first
draft asserted `Locked` and was wrong about the protocol. The test now accepts
either grant shape and follows the client's own loop.

**Four pass both before and after**, because they characterize behavior this
spec describes rather than changes, which is why they are here:

- a stale release after a TTL takeover blocks nothing, including an unrelated
  key, and the handler survives it. This is PR #352's repair, and what is added
  is that the takeover, the stale release and the unrelated key are asserted in
  one run;
- an interrupted lease is reacquired immediately from a snapshot, because the
  snapshot's `exp` is an absolute second and is already in the past when it is
  installed;
- a completed operation leaves nothing to wait for after a restart, because both
  log entries replay;
- replaying an unreleased `Lock` re-grants it **for one more lease window**,
  measured from the restart rather than from the original acquisition.

Every test that waits out a lease uses a one-second lease and waits past it,
which is safe in one direction: the assertion is about a deadline having passed.
Nothing here polls for a state to arrive.

What the acceptance does **not** establish:

- **One process, one handler.** No cluster, no failover, no leader change. Every
  claim is about the handler's own behavior given a sequence of requests.
- **Clock skew is not exercised.** The handler's comment states that `exp` uses
  the deciding node's wall clock and that nodes should stay within about a
  second of each other. Nothing here tests what happens when they do not.
- **The restart tests replay by hand.** They send the `Lock` and `LockRelease`
  requests a replay would produce; no raft replays anything.
- **No client is fenced.** Section 5.
- **The `Acquire` and `Await` undo paths in B-2 are not separately tested.** The
  `Lock` one is; the other two are the same change applied to the same shape.

**B-6 (F-102), four tests, each observed failing against the handler as it
was before the repair and passing after:** a waiter whose holder never releases
claims with one re-request after the lease, where the original handler queued it
again; an await that would have been granted changes nothing, where the original
granted `Locked` itself; a re-request never duplicates its ticket, where the
original left a stale copy in front of the next caller; and a timed-out await does
not cost the live ticket its place, where the original dropped it. One existing
test, `lock_release_roundtrip`, asserted the local grant and now asserts
`Released` followed by a replicated `Acquire`; it asserted the behavior item 4
removes, and is replaced rather than kept.

**What B-6 does not establish.** The client's bounded await is not exercised by
any unit test, because it needs a running node; the cluster suite's lock phase
is the only thing that runs it. The F-102 stall was never reproduced on demand,
so the claim is that the source explains a waiter lasting exactly the request
timeout and that each mechanism is now tested closed, not that the observed run
was caused by one particular mechanism.

## 5. Known defects

**KD-1. This is a lease, not a fence, and a holder is never told it lost the
lock.** When a lease expires and another ticket takes over, the original holder
receives nothing. It is still inside its critical section, still believes it
holds the lock, and its eventual release is ignored. **Two clients can therefore
be inside the same critical section at the same time**, and the only thing
bounding that is that the first one's work outlasted its lease. There is no
fencing token, nothing a downstream store can check, and no way for a caller to
ask whether it still holds the lock.

This is the single most important thing a caller needs to know, and no repair in
this spec touches it.

**KD-2. Ten seconds is the whole of the mutual-exclusion guarantee.** A critical
section that legitimately runs longer than `LOCK_VALID_SECONDS` has no supported
way to extend itself, and nothing detects the overrun. `006` KD-2 and F-026,
unchanged. B-5 says why a bigger number is not the answer.

**KD-3. Leases survive a restart for up to one more lease window.** Log replay
re-executes a `Lock` whose `LockRelease` never made it into the log, with a
deadline computed at replay time. A lock whose owner died with the node is
therefore held by a ticket that does not exist, for one further lease window
measured from the restart. Demonstrated by
`replaying_an_unreleased_lock_re_grants_it_for_one_more_lease_window`, and
bounded, but not free.

**KD-4. A memory-only cache node loses all lock state on restart.** With
`cache_storage_disk = false` the cache Raft log is the non-durable in-memory
store (`007`), so a restarted node has no locks at all until a snapshot install
or a new log entry arrives. What a peer believes about a lock and what this node
believes can differ for that window.

**KD-6. After a holder dies, the next grant goes to whoever re-requests first.**
Every parked waiter's bounded await ends at about the same time, and the first
re-request to reach the leader evicts the front tickets ahead of it. Liveness is
bounded; order is not preserved across a dead holder.

**KD-7. Nodes can still disagree about a lease.** `exp` is computed from each
node's clock at apply time (the handler's own comment). B-6 removes the one path
that acted on a follower's view without Raft; it does not make the lease a
cluster-wide fact.

**KD-5. A front ticket that never awaits still occupies the queue for a lease
window.** B-3 keeps it deliberately, because "has not awaited yet" and "is gone"
are indistinguishable at that moment. The refreshed deadline bounds it; nothing
shortens it.

## 6. Resolved decisions

**D-1 (2026-09-21, report instead of panic, everywhere).** The alternative was
to keep `unwrap` on the paths where a dropped receiver "should not happen". That
is the reasoning PR #352 had to overturn once already for the release path.
Every one of these receivers belongs to a caller's future, and a caller's future
can be cancelled.

**D-2 (2026-09-21, refresh `exp` on release rather than re-key the eviction on
something else).** The alternative was a per-ticket promotion deadline, which is
a new field in `LockQueue` and therefore a change to a type that is serialized
into cache snapshots. Declined: it would break snapshot compatibility for a
rolling upgrade to buy precision this handler does not otherwise have.

**D-3 (2026-09-21, an undelivered grant is undone immediately).** The
alternative was to let the lease expire naturally, which is what happened
before. Declined because the information is already in hand: a failed send is
proof the client is gone, and waiting a lease window to act on proof is a
choice, not a constraint.

**D-4 (2026-09-21, the lease length stays a constant).** Owner direction, and
section B-5 gives the reason: a configurable TTL is not a correctness repair and
recording it as one would misdescribe what was fixed. The test-only constructor
is named so it cannot be mistaken for a knob.

**D-5 (2026-09-21, this block is `022`'s acceptance).** Which is `006`'s through
`022`. All twenty-six of `022`'s commands are carried forward unchanged.

**D-6 (2026-09-22, bound the waiter in the client, not with a timer in the
handler).** A handler timer would have to change lock state on every node on
that node's clock, which is the non-replicated change B-6 removes from the await
path. A client that re-requests puts the decision in a Raft entry, where every
node applies it in the same order.

## 7. Out of scope

- **Fencing tokens, and any cross-node mutual-exclusion guarantee.** KD-1 is
  recorded, not repaired. `000`'s scope discipline names distributed leases as
  out of scope for this corpus.
- **Lease renewal or extension.** KD-2.
- **The cache apply path.** `022`.
- **Cluster-level evidence.** `012`.
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 023`. **This block is `022`'s acceptance as well as
this spec's** (D-5), and `022`'s is `006`'s, so `spec-spine verify 006` and
`verify 022` both resolve here and print the attribution line.

```verify:cli
# Package names, not library names: the downstream release renamed the three packages
# (`031` B-2), and `-p` takes a package name. `use hiqlite::..` is unaffected.
# --- 022's acceptance, which is also 006's, carried forward unchanged ---
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::collision_bump_keeps_both_keys -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::refreshed_key_is_not_deleted_at_old_expiry -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::refreshed_key_expires_at_new_expiry -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::clear_removes_pending_expiry -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::snapshot_roundtrip_preserves_expiries -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::old_seconds_expiries_are_normalized_on_install -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::state_machine::memory::kv_handler::tests::get_remove_and_replace_are_atomic_per_key -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::state_machine::memory::state_machine::serialized_enum_order::cache_request_variant_order_is_stable -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache,in-memory-snapshots store::state_machine::memory::state_machine::tests::in_memory_only_does_not_require_data_dir -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache,in-memory-snapshots store::state_machine::memory::state_machine::tests::read_current_snapshot_skips_temp_files -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::lock_release_roundtrip -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::duplicate_release_is_ignored -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::release_after_lock_was_removed_is_ignored -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::acquire_after_lock_was_removed_grants_fresh -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::await_when_lock_was_removed_returns_released -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::late_release_after_takeover_is_ignored -- --exact
grep -q 'LOCK_VALID_SECONDS: i64 = 10' hiqlite/src/store/state_machine/memory/dlock_handler.rs
sh -c 'grep -q "unreachable!(\"a CacheRequest::Get should never come through the Raft\")" hiqlite/src/store/state_machine/memory/state_machine.rs'
cargo test -p hiqlite-patched --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::an_out_of_range_cache_index_stops_application_at_that_entry -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::a_later_batch_is_refused_once_the_node_is_incompatible -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::a_command_that_must_never_be_replicated_stops_application -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::the_recorded_failure_is_what_callers_are_refused_with -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::a_compatible_batch_still_applies_completely -- --exact
sh -c 'grep -q "fn unsupported_reason" hiqlite/src/store/state_machine/memory/state_machine.rs'
sh -c 'grep -q "CacheIncompatible" hiqlite/src/error.rs'
sh -c 'grep -q "fn ensure_cache_compatible" hiqlite/src/app_state.rs'
# --- what this repair adds ---
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::an_abandoned_waiter_never_kills_the_handler -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::a_grant_that_was_never_received_does_not_hold_the_lock -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::a_dead_promoted_ticket_does_not_block_the_next_caller -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::a_stale_release_after_takeover_blocks_nothing -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::an_interrupted_lease_is_reacquired_immediately_from_a_snapshot -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::a_completed_operation_leaves_nothing_to_wait_for_after_a_restart -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::replaying_an_unreleased_lock_re_grants_it_for_one_more_lease_window -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::a_fully_released_lock_wakes_its_stragglers -- --exact
# F-102: the promotion chain, driven more than one link at a time
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::three_queued_awaiters_are_each_promoted_in_turn -- --exact
# B-6 / F-102: a waiter is bounded by the lease, and an await changes nothing
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::a_waiter_whose_holder_never_releases_claims_after_one_lease -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::an_await_changes_no_lock_state -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::a_re_request_never_duplicates_its_ticket -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::a_timed_out_await_does_not_cost_the_live_ticket_its_place -- --exact
sh -c 'grep -q "time::timeout(AWAIT_BOUND, self.lock_await(" hiqlite/src/client/dlock.rs'
sh -c 'grep -q "LOCK_VALID_SECONDS as u64 + 2" hiqlite/src/client/dlock.rs'
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::a_replaced_await_is_answered_not_dropped -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::an_await_does_not_judge_the_lease -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::lease_tests::an_acquire_leaves_no_stale_await_registration -- --exact
sh -c '! grep -q "to always get an answer from the kv handler" hiqlite/src/network/api.rs'
# no acknowledgement in the lock handler may panic its own task
sh -c '! grep -q "ack.send(LockState::" hiqlite/src/store/state_machine/memory/dlock_handler.rs'
sh -c 'grep -q "fn answer(" hiqlite/src/store/state_machine/memory/dlock_handler.rs'
```
