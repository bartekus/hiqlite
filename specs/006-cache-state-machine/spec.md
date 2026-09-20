---
id: "006-cache-state-machine"
title: "Define the in-memory cache state machine and its handlers"
status: draft
created: "2026-09-19"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "002-snapshot-publication-and-recovery"
origin:
  retroactive: true
  paths: ["hiqlite/src/store/state_machine/memory/"]
establishes:
  - { kind: directory, path: "hiqlite/src/store/state_machine/memory/" }
references:
  - unit: { kind: directory, path: "hiqlite/src/store/state_machine/sqlite/" }
    role: "the sibling state machine, for contrast"
  - unit: { kind: file, path: "hiqlite/src/store/logs/memory.rs" }
    role: "the log store this state machine's Raft group uses"
summary: >
  Records the existing in-memory cache state machine: the CacheRequest and
  CacheResponse contract, deterministic apply across the per-cache handler
  threads, TTL expiry, the distributed lock queue as found, snapshot build and
  install under both snapshot features, and the panic-on-failure behavior of
  the handler channels. Retroactive: describes what is there, repairs nothing.
---

# 006: Define the in-memory cache state machine and its handlers

## 1. Purpose

`002` specified the SQLite state machine. Its own section and constitution VII
name "the SQLite **and cache** state machines" as hiqlite territory, but no spec
has ever claimed the cache side. This spec adopts it as found.

Scope is one coherent responsibility: **what a committed cache log entry does to
in-memory state, and how that state is captured and restored**. It is not a
claim over the cache client API, the cache Raft group's log storage (`007`), or
membership.

The spec is `draft`. It repairs nothing, and every defect it records is left
unfixed here, per constitution VI.

## 2. Territory

**Establishes** `hiqlite/src/store/state_machine/memory/` as a `directory` unit:
`mod.rs`, `state_machine.rs`, `kv_handler.rs`, `cache_ttl_handler.rs`,
`dlock_handler.rs`, and `notify_handler.rs`.

No unit here is claimed by another spec, so `establishes` is the correct edge
and no `extends` is needed (`000` section 4, one origin per unit).
`hiqlite/src/store/state_machine/sqlite/` and `hiqlite/src/store/logs/memory.rs`
are **referenced**, not claimed.

**Ownership boundary.** OpenRaft owns the consensus that delivers entries to
this state machine: election, quorum, commit ordering, and membership are not
claimed here and no guarantee of theirs is cited as evidence that hiqlite
implements them (constitution VII). What hiqlite owns, and what this spec
specifies, is the state machine on the other side of `RaftStateMachine`: the
effect of an applied entry, the snapshot it can produce, and the state a
snapshot install leaves behind. Where a property depends on OpenRaft delivering
committed entries in order on every node, the spec says so rather than claiming
the ordering itself.

## 3. Behavior

### B-1. The request and response contract

`CacheRequest` (`state_machine.rs:63-126`) is the replicated command type for
`TypeConfigKV`. Variants: `Get`, `Put`, `GetRemove`, `Replace`, `Delete`,
`Clear`, `ClearCounters`, `ClearAll`, `Notify`, `Lock`, `LockAwait`,
`LockRelease`, `CounterGet`, `CounterSet`, `CounterAdd`, `CounterDel`.
`CacheResponse` (`state_machine.rs:129-137`) is `Empty`, `Ok`, `Lock`, `Value`,
or `CounterValue`.

Several variants are feature-gated in construction but **not** in the enum:
counter and lock and notify variants carry `#[allow(dead_code)]` and exist in
every build. This is deliberate and load-bearing: the enum is serialized into
the Raft log, so variant order is part of the wire format. A build with
different features must deserialize entries produced by a build with others.
Variant order therefore MUST NOT change, and
`serialized_enum_order::cache_request_variant_order_is_stable` exists to detect
it.

`CacheRequest::Get` MUST NOT reach `apply`. It is `unreachable!()` there
(`state_machine.rs:635`), because cache reads are served locally and never go
through the Raft log.

### B-2. Apply is deterministic, and delegates to per-cache handler threads

`apply` (`state_machine.rs:613`) takes the state-machine write lock once, then
for each entry records `last_applied_log_id` and dispatches by payload.
`EntryPayload::Blank` yields `CacheResponse::Empty`; `EntryPayload::Membership`
updates the stored membership; `EntryPayload::Normal` dispatches on the variant.

State does not live in the state machine. Each cache index owns a handler thread
reached through an unbounded channel (`tx_caches`), with a parallel TTL channel
(`tx_ttls`) and, under their features, lock and notify channels. Apply sends to
those channels and, where a response is required, awaits a oneshot.

Two properties this arrangement depends on, both recorded rather than enforced:

- **Determinism** comes from every node applying the same committed entries in
  the same order and each handler being single-threaded per cache index.
  OpenRaft supplies the ordering; hiqlite supplies the per-index serialization.
- **Deadlock freedom** while the state-machine lock is held depends on handlers
  never taking that lock. The comment at `state_machine.rs:677-679` states this
  for `GetRemove`, and it holds for every awaited handler call.

`cache_idx` indexes the handler vectors with `.get(cache_idx).unwrap()`. An
out-of-range index panics. The index comes from a generated cache enum, so it is
in range for any client built against the same enum; a log entry from a build
with more cache variants would not be (see KD-3).

### B-3. TTL expiry is a registration, and a re-put without a TTL clears it

`Put` and `Replace` with `expires: Some(exp)` send `TtlRequest::Ttl((exp, key))`;
with `expires: None` they send `TtlRequest::Clear(key)`, so that a previously
registered expiry cannot delete a freshly written value
(`state_machine.rs:643-666`). Expiry is wall-clock based and evaluated by the
TTL handler.

Expiries are part of the snapshot: `snapshot_roundtrip_preserves_expiries`
covers the round trip, and `old_seconds_expiries_are_normalized_on_install`
covers a normalization applied to older snapshots on install.

### B-4. Distributed locks, described as found

Recorded, not designed. `dlock_handler.rs` keeps, per key, a `LockQueue` of
`current_ticket`, an expiry `exp`, and a FIFO `queue` of waiting ticket ids. A
`Lock` request either takes the lock, returning `LockState::Locked(id)`, or is
queued, returning `LockState::Queued(id)`; `LockAwait` resolves when the ticket
is reached or the lock was removed; `LockRelease` releases only when the ticket
matches the current holder, and is otherwise ignored.

A lock is valid for `LOCK_VALID_SECONDS = 10` (`dlock_handler.rs:13`), a
compile-time constant. Expiry is evaluated against **this node's wall clock**
(`Utc::now()`), which the code comments on directly at `dlock_handler.rs:64-66`:
per-node `exp` copies diverge under clock skew, and the code argues this is
benign because the decisions themselves are made in the replicated log rather
than from the local clock.

This spec **describes** that arrangement and does not endorse it. It is not a
lease: nothing fences a holder that has stopped responding, and nothing prevents
two nodes from disagreeing about whether an expiry has passed. Designing lease
semantics, or integrating lock lifetime with membership, is explicitly out of
scope (section 6).

Behavior around removal and takeover is covered by six focused tests, which this
spec cites rather than restates: `lock_release_roundtrip`,
`duplicate_release_is_ignored`, `release_after_lock_was_removed_is_ignored`,
`acquire_after_lock_was_removed_grants_fresh`,
`await_when_lock_was_removed_returns_released`, and
`late_release_after_takeover_is_ignored`.

### B-5. Snapshots, under both snapshot strategies

`TypeConfigKV` declares `SnapshotData` differently by feature (`mod.rs:19-35`):
`tokio::fs::File` by default, so snapshots stream from disk zero-copy, and
`Cursor<Vec<u8>>` under `in-memory-snapshots`, so a cache-only node needs no
`data_dir` at the cost of holding the snapshot in memory and losing zero-copy
streaming. `in-memory-snapshots` is deliberately not part of `full`.

`build_snapshot` has one implementation per strategy
(`state_machine.rs:176` and `:191`), as does `install_snapshot`
(`:944`, `:971`) and `get_current_snapshot` (`:993`, `:1014`). A snapshot
captures each cache's key-value contents, the registered expiries, and, under
`dlock`, the lock queues, via the `SnapshotBuild` and `SnapshotInstall` handler
requests.

Two behaviors are pinned by tests and are part of this contract:
`in_memory_only_does_not_require_data_dir` and
`read_current_snapshot_skips_temp_files`, the second being the cache-side
counterpart of the staging-file discipline `002` records for SQLite.

### B-6. Handler channel failures panic

Every send to a handler channel uses `.expect(...)`
(`state_machine.rs:650`, `:660`, `:664`, and throughout), as does every awaited
response. A handler thread that has died therefore panics the applying task
rather than returning a storage error to OpenRaft.

This is recorded as found, and it is a real choice rather than an oversight: a
cache state machine that silently skipped an applied entry would diverge from
its peers, and divergence is worse than a crash. What is not recorded anywhere
in the code is that reasoning, and `panic = "abort"` in this repository's release
profile turns that panic into process termination. See KD-1.

## 4. Evidence and its limits

This is a retroactive adoption spec over working code, so its acceptance block
characterizes rather than demonstrating a change: every line names a specific
behavior claimed above and fails if that behavior changes
(`standards/spec/templates/spec-template.md`). Sixteen focused tests already
existed in this territory and are cited by exact path under the exact features
they need, including two that require `in-memory-snapshots`; no test was written
or modified for this spec.

What the block establishes: TTL registration, refresh, clearing, and snapshot
round-trip; lock release, duplicate release, release and acquire after removal,
await after removal, and late release after takeover; atomic `GetRemove` and
`Replace` per key; the in-memory snapshot strategy needing no `data_dir`;
temp-file skipping on snapshot read; and the stability of `CacheRequest`'s
variant order.

What it does **not** establish, stated next to the claims it limits:

- **Nothing here exercises a real Raft group.** Every cited test drives a
  handler or the state machine directly. Determinism across nodes (B-2) rests on
  OpenRaft delivering the same committed entries in the same order, which is
  OpenRaft's contract and is not evidence hiqlite produces it.
- **Cross-node convergence is untested here.** `hiqlite/tests/cluster/cache.rs`
  and `dlock.rs` exercise the cache through a running cluster, but they are
  unclaimed and outside this spec's acceptance, which stays cheap and focused.
  Their existence is not cited as evidence for a claim in this spec.
- **No test covers clock skew** between nodes for B-4, or the behavior of a lock
  whose holder disappears. The comment at `dlock_handler.rs:64-66` is an
  argument, not evidence.
- **No test covers B-6.** No test kills a handler thread and observes the panic.
- **`Notify` and the counter variants are claimed but thinly evidenced.** They
  are described from source; only their presence in the stable variant order is
  asserted.

## 5. Known defects

**KD-1. A dead handler thread panics the applying task, and the release profile
turns that into process termination.** B-6. The crash-over-divergence choice is
defensible and is nowhere written down in the code, and the escalation to
process abort is a property of `[profile.release]` in this repository's
`Cargo.toml` rather than of this module, so a downstream consumer building with
unwinding gets different behavior from the same code. Recorded, not fixed:
changing it is a behavioral decision with its own evidence.

**KD-2. Lock validity is a compile-time constant with no configuration.**
`LOCK_VALID_SECONDS = 10` (`dlock_handler.rs:13`). An application whose critical
section legitimately exceeds ten seconds has no supported way to extend it, and
nothing detects the overrun. Recorded as found, per the scope limit in section 6.

**KD-3. `cache_idx` is an unvalidated index into the handler vectors.**
`.get(cache_idx).unwrap()` panics on an out-of-range index. In-range-ness is a
property of client and server being built from the same generated cache enum;
nothing in the state machine validates it, and a log entry produced by a build
with more cache variants would panic every node that applies it. No test covers
this.

## 6. Out of scope

- **Lease semantics, fencing tokens, and any redesign of `dlock`.** B-4 describes
  what is there. A lease design is a separate spec and a separate change.
- **Membership integration**, including lock lifetime tied to node liveness.
- The cache **client API** and the request path that produces `CacheRequest`s;
  `003` owns the client, and the boundary between them is not re-drawn here.
- `hiqlite/src/store/logs/memory.rs`, the log store for this Raft group. `007`.
- The cluster tests that exercise cache and dlock end to end. Wave 6.
- Every repair for KD-1 to KD-3.

## 7. Resolved decisions

**D-1 (2026-09-19, `establishes` a directory rather than six file units).** The
six files are one responsibility with one entry point: `apply` dispatches to all
of them and the snapshot captures all of them. A directory claim states that,
and it means a new handler file added to the module is governed on arrival
rather than silently unclaimed. The cost is that the claim is coarse, which
section 4's evidence limits offset by naming what is and is not actually
established.

**D-2 (2026-09-19, `implementation: complete`).** The obligations this spec
places on the tree are that the described code exists and that its acceptance
passes, and both hold: 14 cited tests, all passing under the features named.
`complete` is not a claim that the behavior is fully evidenced, which section 4
denies in detail, nor a claim about approval. The thinly evidenced areas are
recorded there rather than hidden behind a narrower status, because narrowing
the status would misreport the tree while leaving the same gaps unstated.

## Verification

```verify:cli
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
```
