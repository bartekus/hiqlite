---
id: "022-replicated-cache-command-compatibility"
title: "Refuse replicated cache commands this build cannot apply, instead of panicking or skipping"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "006-cache-state-machine"
  - "007-cache-log-store"
amends: ["006-cache-state-machine"]
# D-4: this spec's `## Verification` block IS 006's acceptance from now on, and 006's own file
# is not edited. Whole-block replacement is the mechanism's unit, so the block carries every
# obligation 006 declared plus this repair's.
amends_verification: ["006-cache-state-machine"]
amends_sections:
  - "3-behavior"
  - "5-known-defects"
extends:
  - spec: "006-cache-state-machine"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/memory/" }
    nature: superseding
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/client/" }
    nature: additive
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/network/" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/app_state.rs" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Repairs F-027. A replicated cache command naming an index this node does not
  have, or a variant its feature set cannot execute, stopped being an
  unvalidated index or an unreachable arm and is now a named terminal failure:
  application of committed work stops at that entry, the entry is not claimed
  as applied, and every later cache read and write on the node is refused
  rather than answered from a state machine that stopped advancing.
---

# 022: Refuse replicated cache commands this build cannot apply

## 1. Purpose

`006` adopted the cache state machine as found and recorded at KD-3 that
`cache_idx` is an unvalidated index: `.get(cache_idx).unwrap()` panics out of
range, and in-range-ness holds only while every participant was built from the
same generated cache enum. That is F-027, and this spec repairs it.

Tracing it found the same shape a second time. The `CacheRequest` variant set is
deliberately **feature-independent**, because the variant order is part of the
raft log format (the comment on the enum says so). A node built without
`counters`, `dlock` or `listen_notify_local` can therefore be handed a committed
entry it has no handler for, and three read-only variants can be handed one that
should never have been replicated at all. Every one of those arms was an
`unreachable!`, which is a panic in the raft apply path. Same defect, different
axis: one is an index the build does not have, the other is a command the build
cannot execute.

One responsibility: **what a node does with a committed cache entry it cannot
apply.**

## 2. Territory

**Extends** `006`'s `directory` unit with nature `superseding` for the apply
path. `006` still `establishes` it.

Three further units are `extends`ed additively, because the repair is not
confined to the state machine and saying otherwise would be false: the client
read and write paths (`003`'s `hiqlite/src/client/`), the streaming API's cache
read handler (`003`'s `hiqlite/src/network/`), and the shared state that carries
the failure between them (`010`'s `hiqlite/src/app_state.rs`). One new variant
is added to `hiqlite/src/error.rs`, which no spec claims.

**Ownership boundary.** OpenRaft owns what it does with a storage error returned
from `apply`, and this spec states only that returning one is how application
stops. Nothing here claims what the rest of the cluster does: the other nodes
are unaffected and keep applying, which is the point. A node that cannot apply
committed work removes **itself**.

## 3. Behavior

### B-1. Every entry is classified before anything is done for it

`apply` checks each `EntryPayload::Normal` before executing any part of it, on
two axes:

- **Index.** If the command names a `cache_idx` and that index is not less than
  the number of caches this node has, it cannot be applied.
- **Variant.** If the command is one this build has no handler for, it cannot be
  applied. Three cases hold under every feature set, because they are protocol
  violations rather than feature gaps: `Get`, `CounterGet` and `LockAwait` are
  served locally or by the leader and must never appear in the log. The rest are
  feature gaps: `ClearCounters`, `CounterSet`, `CounterAdd` and `CounterDel`
  without `counters`; `Notify` without `listen_notify_local`; `Lock` and
  `LockRelease` without `dlock`.

Checking first is also what makes the surviving `.get(cache_idx).unwrap()` calls
sound: an out-of-range index never reaches them. The repair does not sprinkle
error handling through twenty match arms; it puts one gate in front of them.

### B-2. Application stops at the offending entry, and does not claim it

Entries **before** the offending one in the same batch keep their effect and are
recorded: `last_applied_log_id` is the entry before it. The offending entry is
not applied, and nothing reports that it was.

`apply` then returns a `StorageError`, which is what stops OpenRaft applying the
rest of the committed log on this node. The alternative that F-027 describes,
skipping the entry, is the one outcome that must not happen: a node that skips a
committed entry and carries on has silently diverged from every node that
applied it, while continuing to look healthy.

### B-3. It is terminal, and a later batch does not resume

The failure is recorded in a `OnceLock` shared with the node's cache state. A
later `apply` is refused before it looks at its entries. The offending entry is
committed, so there is no batch after it that is safe to apply: applying one
would leave a hole at a known index. A restart does not clear it either, because
the same entry is replayed.

### B-4. Reads and writes are refused, not answered

Stopping application is only half of it. Cache **reads** do not go through the
raft log at all: they are served straight from the per-cache handler threads,
locally and over the streaming API. A node that stopped applying but kept
answering reads would be serving state that is knowingly behind, which is the
misleading read B-2 exists to prevent one level up.

So every cache path on the node refuses while the failure is set:

| path | before | after |
|---|---|---|
| `Client::get_bytes`, local | `.unwrap()` on the index, `.expect` on the send | refuses with the recorded failure; an unknown index is a named error |
| `Client::get_snapshot`, local | the same | the same |
| `Client::counter_get`, local | the same | the same |
| `Client::cache_req_retry`, every write | unguarded | refuses before dispatching |
| the streaming API's `KVGet` | `.unwrap()`, `.expect`, and `unreachable!` on a mismatched payload | all three are returned as errors |

Writes are refused for the same reason: a write accepted into a log this node
has stopped applying would be reported as a success and never take effect here.

### B-5. The failure is named, and says what to do

`Error::CacheIncompatible` is a new variant. Its message names the command
variant, the log index, and what specifically could not be done, and states that
application has stopped, that a restart replays the same entry, and that the
cluster has to be brought to one cache definition and feature set. Over HTTP it
maps to `503 Service Unavailable`: the node cannot serve this cache at all, and
no retry against it will change that.

### B-6. What this does not change

A healthy batch applies exactly as before. The `unreachable!` arms are kept
where they were, and they are now genuinely unreachable rather than
optimistically so. `006`'s KD-1, the dead handler thread, is untouched: the
`.expect(..)` on a handler send is a different failure and is not repaired here.

## 4. Evidence and its limits

Five tests in `hiqlite/src/store/state_machine/memory/state_machine.rs`, driving
`RaftStateMachine::apply` directly against a state machine built from a
two-variant cache enum. Four of the five **fail against the unrepaired
implementation**, three of them by the panic F-027 describes; the fifth is the
healthy-batch test, which passes both before and after and is there to show the
repair changes nothing else.

- an out-of-range index stops application at that entry, leaves the frontier at
  the entry before, and records the index and the variant;
- a later batch of otherwise-valid entries is still refused, and the frontier
  does not move;
- a command that must never be replicated stops application under every feature
  set;
- the recorded failure produces the `Error::CacheIncompatible` a caller is
  refused with, naming the index and what to do;
- a compatible batch still applies completely.

What the acceptance does **not** establish:

- **No `AppState` is constructed, so no served request is refused in a test.**
  The fourth test asserts the shared value and the error built from it, which is
  what B-4's paths use, not a request served over the streaming API. That needs
  a running node, which is the cluster surface `012` owns, and B-4's table is
  otherwise a source change.
- **No cluster diverges in a test.** The scenario F-027 describes is two builds
  with different cache enums in one cluster, and nothing here stands that up.
  What is demonstrated is the receiving node's behavior given such an entry.
- **Nothing establishes what OpenRaft does with the returned `StorageError`.**
  B-2 states that returning it is how application stops; that is the trait's
  contract, not hiqlite evidence.
- **The feature-gap half is not executed.** The tests run with `counters`,
  `dlock` and `listen_notify_local` enabled, so the arms that classify a missing
  feature are compiled out. The protocol-violation half, which holds under every
  feature set, is what the third test exercises.

## 5. Known defects

**KD-1. The refusal is per node and is not visible to the cluster.** A node that
stops applying removes itself from useful service, and the others keep going.
Nothing publishes the reason to a peer, an operator dashboard, or a health
endpoint: it is an error on the next request and a log line. Making it visible
is a lifecycle and readiness question, which is W-22's.

**KD-2. `006` KD-1 is untouched.** A dead handler thread still panics the
applying task through `.expect(..)`, and that is a different failure from an
entry this build cannot apply.

**KD-3. The classification is a denylist of what cannot be applied, not an
allowlist of what can.** A future variant added to the end of `CacheRequest`,
which is where the enum's comment says new variants go, is not automatically
classified as unsupported by an older build: it deserializes as an unknown
discriminant and fails earlier, in `bincode`, with a decode error rather than
this named one. That path was not traced here.

## 6. Resolved decisions

**D-1 (2026-09-21, stop rather than skip, and stop rather than panic).** Three
options: skip the entry, panic, or stop. Skipping is silent divergence and is
what F-027 names as the thing to avoid. Panicking is the current behavior and is
not a reported failure under either panic profile: it ends the process where the
profile aborts and kills the raft apply task silently where it unwinds. Stopping
is the only one that is both deterministic across nodes and observable.

**D-2 (2026-09-21, terminal, with no recovery path in this spec).** The
offending entry is committed. Nothing this node can do locally makes it
applicable, so there is no "retry" or "resume" that is not a lie. The recovery
path is an operator bringing the cluster to one cache definition, which is what
the message says. Automatic recovery is deliberately not offered.

**D-3 (2026-09-21, reads are refused too).** The alternative was to stop
applying and keep serving reads, on the argument that stale cache data is
usually acceptable. Declined: the point of the cache raft group is that every
node agrees, and a node that has stopped applying is not merely stale, it is
stopped at a known point with no bound on how far behind it will get.

**D-4 (2026-09-21, this block is `006`'s acceptance).** `amends_verification`
replaces `006`'s block whole. All nineteen of its commands are carried forward
unchanged, including the `grep` for the `unreachable!` on a replicated `Get`,
which still holds: B-6 keeps that arm and B-1 makes it genuinely unreachable.

## 7. Out of scope

- **The distributed lock lease.** `006` KD-2 and F-026.
- **The dead handler thread.** `006` KD-1.
- **Node readiness and lifecycle policy.** W-22.
- **The cache log store.** `007`, amended by `020`.
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 022`. **This block is `006`'s acceptance as well as
this spec's** (D-4). `006`'s own file is not edited, and `spec-spine verify 006`
prints the attribution line naming this spec before it runs a command.

```verify:cli
# --- 006's acceptance, carried forward unchanged ---
cargo test -p hiqlite --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::collision_bump_keeps_both_keys -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::refreshed_key_is_not_deleted_at_old_expiry -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::refreshed_key_expires_at_new_expiry -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::clear_removes_pending_expiry -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::snapshot_roundtrip_preserves_expiries -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache store::state_machine::memory::cache_ttl_handler::tests::old_seconds_expiries_are_normalized_on_install -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache store::state_machine::memory::kv_handler::tests::get_remove_and_replace_are_atomic_per_key -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache store::state_machine::memory::state_machine::serialized_enum_order::cache_request_variant_order_is_stable -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache,in-memory-snapshots store::state_machine::memory::state_machine::tests::in_memory_only_does_not_require_data_dir -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache,in-memory-snapshots store::state_machine::memory::state_machine::tests::read_current_snapshot_skips_temp_files -- --exact
cargo test -p hiqlite --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::lock_release_roundtrip -- --exact
cargo test -p hiqlite --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::duplicate_release_is_ignored -- --exact
cargo test -p hiqlite --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::release_after_lock_was_removed_is_ignored -- --exact
cargo test -p hiqlite --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::acquire_after_lock_was_removed_grants_fresh -- --exact
cargo test -p hiqlite --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::await_when_lock_was_removed_returns_released -- --exact
cargo test -p hiqlite --lib --no-default-features --features dlock store::state_machine::memory::dlock_handler::tests::late_release_after_takeover_is_ignored -- --exact
grep -q 'LOCK_VALID_SECONDS: i64 = 10' hiqlite/src/store/state_machine/memory/dlock_handler.rs
sh -c 'grep -q "unreachable!(\"a CacheRequest::Get should never come through the Raft\")" hiqlite/src/store/state_machine/memory/state_machine.rs'
# --- what this repair adds ---
cargo test -p hiqlite --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::an_out_of_range_cache_index_stops_application_at_that_entry -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::a_later_batch_is_refused_once_the_node_is_incompatible -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::a_command_that_must_never_be_replicated_stops_application -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::the_recorded_failure_is_what_callers_are_refused_with -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache,counters,dlock,listen_notify_local store::state_machine::memory::state_machine::cache_compatibility_tests::a_compatible_batch_still_applies_completely -- --exact
# the gate itself, pinned at its expressions
sh -c 'grep -q "fn unsupported_reason" hiqlite/src/store/state_machine/memory/state_machine.rs'
sh -c 'grep -q "CacheIncompatible" hiqlite/src/error.rs'
sh -c 'grep -q "fn ensure_cache_compatible" hiqlite/src/app_state.rs'
```
