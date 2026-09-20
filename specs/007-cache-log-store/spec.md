---
id: "007-cache-log-store"
title: "Define the in-memory log store for the cache Raft group"
status: draft
created: "2026-09-19"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "001-wal-durability-and-completion"
  - "006-cache-state-machine"
origin:
  retroactive: true
  paths: ["hiqlite/src/store/logs/"]
establishes:
  - { kind: directory, path: "hiqlite/src/store/logs/" }
references:
  - unit: { kind: directory, path: "hiqlite-wal/src/" }
    role: "the durable log store this one is the non-durable counterpart to"
  - unit: { kind: directory, path: "hiqlite/src/store/state_machine/memory/" }
    role: "the state machine whose Raft group uses this log store"
summary: >
  Records the existing non-durable in-memory log store that backs the cache
  Raft group: its RaftLogReader and RaftLogStorage implementation, the
  deliberate absence of persistence, the immediate completion callback, the
  truncate and purge index arithmetic, and the data-directory helpers. Adopts
  as found and records a confirmed off-by-one in get_log_state.
---

# 007: Define the in-memory log store for the cache Raft group

## 1. Purpose

`001` specified `hiqlite-wal`, the durable log store for the SQLite Raft group.
The cache Raft group uses a different one: a non-durable, in-memory log store
that no spec has claimed. This spec adopts it as found.

One responsibility: **what OpenRaft's log-storage traits do for the cache group,
and what durability they deliberately do not provide**. It is the log-storage
counterpart to `006`'s state machine.

The spec is `draft`. It repairs nothing, including the defect at KD-1.

## 2. Territory

**Establishes** `hiqlite/src/store/logs/` as a `directory` unit: `mod.rs` and
`memory.rs`. Neither is claimed by another spec, so `establishes` is correct and
no `extends` is needed (`000` section 4).

`hiqlite-wal/src/` and `hiqlite/src/store/state_machine/memory/` are
**referenced**, not claimed: `001` and `006` own them.

**Ownership boundary.** OpenRaft owns the consensus algorithm and the rules
about *when* it asks a log store to append, truncate, or purge. This spec
specifies only hiqlite's side of `RaftLogReader` and `RaftLogStorage` for
`TypeConfigKV`: what each method does to the in-memory deque and what it reports
back. No OpenRaft guarantee is cited as evidence for a hiqlite behavior, and no
claim here asserts that the cache group's log is safe under any consensus
property that OpenRaft, not hiqlite, provides.

## 3. Behavior

### B-1. The store is deliberately non-durable, and that is the contract

`LogStoreMemory` (`memory.rs:33-36`) holds an
`Arc<RwLock<VecDeque<Entry<TypeConfigKV>>>>` and an `Arc<Mutex<LogData>>` with
`last_purged` and `vote`. Nothing is written to disk, and `new()`
(`memory.rs:39-51`) starts empty with a deque capacity of 1000.

This is the cache group's intended durability level: cache state is
reconstructible, so its Raft log is not persisted. **Every durability claim `001`
makes about `hiqlite-wal` is therefore false of this store**, and the two must
not be confused. A restart loses the entire cache log and the persisted vote,
and the node rejoins with an empty log.

`mod.rs` gates `pub mod memory` on the `cache` feature and provides two
data-directory helpers: `logs_dir_db` (`sqlite`) yielding `{data_dir}/logs` and
`logs_dir_cache` (`cache`) yielding `{data_dir}/logs_cache`. The second names a
directory this store never writes to; it exists because the cache group's
*snapshots* may be file-backed (`006` B-5), not its log.

### B-2. Vote is stored in memory and read back unchanged

`save_vote` (`memory.rs:143`) takes the `LogData` mutex and stores
`Some(*vote)`; `read_vote` returns it. No durability, consistent with B-1.

### B-3. Append is synchronous and completes immediately

`append` (`memory.rs:154-171`) takes the deque write lock, pushes every entry
with `push_back`, releases the lock, then calls
`callback.log_io_completed(Ok(()))` and returns `Ok(())`.

Completion is immediate and unconditional because there is no persistence step
that could fail: for an in-memory store, "the entries are in the deque" is the
whole of the IO. This is the correct shape here, and it is worth stating
explicitly because the durable store's equivalent path is where `001` records
two defects (F-001 and F-002 in `standards/spec/findings-register.md`): the
callback is invoked exactly once, on the success path, after the state change.

Entries are pushed in iteration order and no index arithmetic is performed, so
`append` preserves whatever order OpenRaft supplied.

### B-4. Truncate and purge are offset arithmetic over the deque

Both convert an absolute log index into a deque offset using the front entry's
index as the base, and both return early on an empty deque.

`truncate(log_id)` (`memory.rs:174-191`) removes `[log_id.index, +inf)` by
computing `truncate_from = log_id.index - first_offset` and calling
`logs.truncate(truncate_from)`, keeping the entries below the given index.

`purge(log_id)` (`memory.rs:193-220`) removes everything below `log_id.index` by
computing `purge_until = log_id.index - first_offset` and calling
`logs.drain(..purge_until)`. `last_purged` in `LogData` is **not** updated by
`purge`; it is only ever read.

`try_get_log_entries(range)` (`memory.rs:54-108`) converts the range the same
way and returns a cloned `Vec`. Its end-bound handling computes `*i - 1` for an
exclusive end, which underflows for an exclusive end of `0`; see KD-3.

### B-5. Several invariants are debug-only

`try_get_log_entries`, `truncate`, and `purge` carry `debug_assert!`s over their
index arithmetic. They are checks, not enforcement: in a release build the same
malformed input proceeds silently. One of them is itself wrong; see KD-2.

## 4. Evidence and its limits

This is a retroactive adoption spec, so the acceptance characterizes the
behavior that is there rather than demonstrating a change. No test existed in
this territory before this spec; two focused tests were added as
characterization, and they are the only tests written for wave 1.

`get_log_state_reports_no_last_log_id_even_after_append` records KD-1 as
observed behavior, so a repair has a stated baseline and so the repair is
visibly a change rather than a silent correction.
`purge_removes_entries_below_the_given_index` pins B-4's purge direction, which
is the arithmetic most likely to be inverted by a careless edit.

What the acceptance does **not** establish:

- **Nothing here runs a Raft group.** Both tests drive `LogStoreMemory` directly.
  Whether OpenRaft calls `truncate` and `purge` with the indices B-4 assumes is
  OpenRaft's contract, not hiqlite evidence.
- **`append` completion is not observed through OpenRaft.** B-3 is read from
  source; no test asserts that `RaftCore` receives the completion.
- **`truncate` and `try_get_log_entries` have no test.** Only `purge` does.
- **No test covers a non-zero `first_offset`**, which is exactly the condition
  under which KD-2's assertion misfires and under which the offset arithmetic
  stops being the identity.
- **No test covers concurrent access** to the deque and the `LogData` mutex,
  nor the behavior when the store is used after a purge that emptied it.
- **Durability is not tested, because there is none to test.** B-1 is a
  statement about what the code does not do, and its evidence is the absence of
  any filesystem write in the module.

## 5. Known defects

**KD-1. `get_log_state` always reports `last_log_id: None`.**
`memory.rs:117-120` reads the last log id with
`logs.get(logs.len()).map(|entry| entry.log_id)`. `VecDeque::get(len())` is
always out of bounds, so the expression is `None` for every non-empty deque.
Confirmed by execution:
`get_log_state_reports_no_last_log_id_even_after_append` stores two entries and
observes `None`.

Consequence, traced but **not executed**: the two consumers in the pinned
OpenRaft (0.9.24) are `StorageHelper::get_initial_state` and
`StorageHelper::last_membership_in_log`. Both are on the initialization path,
and at initialization this store is empty by construction, so `None` happens to
be the correct answer and the defect is latent there. In
`last_membership_in_log` a `None` last log id yields a scan range of
`[start, 0)`, which is empty. Whether any path reaches `get_log_state` with a
non-empty deque was **not** established, and that is an open question rather
than a claim in either direction.

Recorded, not fixed. The repair is a one-line change, and it is still a
behavioral change to a consensus-adjacent component that deserves its own spec
and its own evidence, per constitution VI.

**KD-2. A `debug_assert!` in `truncate` compares an offset to an absolute
index.** `memory.rs:186` asserts
`truncate_from == logs.get(truncate_from).unwrap().log_id.index as usize`.
`truncate_from` is an offset from the front of the deque and the right-hand side
is an absolute log index; they are equal only while `first_offset == 0`, so the
assertion fires in a debug build on any truncate after a purge has advanced the
front. Untested, because no test covers a non-zero `first_offset`.

**KD-3. `try_get_log_entries` underflows on an exclusive end bound of zero.**
`memory.rs:64-68` computes `end = *i - 1` for `Bound::Excluded(i)`. For `i == 0`
this underflows: a debug build panics, and a release build wraps to `u64::MAX`,
after which the `logs.front().expect(...)` at `memory.rs:76-79` panics on an
empty deque. Whether OpenRaft ever requests `0..0` was not established, so the
reachability is unknown and this is recorded as a defect in the arithmetic
rather than as a demonstrated runtime failure.

**KD-4. `purge` never updates `last_purged`.** `LogData::last_purged`
(`memory.rs:28`) is returned by `get_log_state` but is never assigned outside
`new()`, so it stays `None` for the life of the store even after `purge`
succeeds. Consequence, inferred: `last_purged_log_id` is under-reported to
OpenRaft, which is masked by the same emptiness that masks KD-1. No test covers
it.

## 6. Out of scope

- **The cache state machine.** `006`.
- **`hiqlite-wal`,** its durability contract, and the F-001 and F-002 repair.
  `001`, and `standards/spec/wal-repair-proposal.md`.
- **Membership, leases, and live peer recovery.**
- **Every repair for KD-1 to KD-4**, including the one-line `get_log_state` fix.
- The cluster tests that exercise the cache group end to end. Wave 6.

## 7. Resolved decisions

**D-1 (2026-09-19, the characterization test records the defect rather than
avoiding it).** `get_log_state_reports_no_last_log_id_even_after_append` asserts
behavior KD-1 calls wrong. The alternative was to write no test for
`get_log_state` at all, leaving the defect unobserved and the repair
unmeasurable. The test is named and commented so it reads as a record rather
than an endorsement, and the amending spec that repairs KD-1 will replace it and
say so, which is the pattern the spec template prescribes for a repair that
retires a test pinning old behavior.

**D-2 (2026-09-19, `implementation: complete` with four known defects).** The
obligations this spec places on the tree are that the described code exists and
that its acceptance passes; both hold. Known defects do not make an adoption
spec incomplete, or no retroactive spec over imperfect code could ever be
complete, and constitution VI exists precisely so they are recorded instead.
Section 4 states where evidence is thin, which is the honest narrowing, rather
than narrowing the status field.

## Verification

```verify:cli
cargo test -p hiqlite --lib --no-default-features --features cache store::logs::memory::tests::get_log_state_reports_no_last_log_id_even_after_append -- --exact
cargo test -p hiqlite --lib --no-default-features --features cache store::logs::memory::tests::purge_removes_entries_below_the_given_index -- --exact
sh -c 'grep -q "logs.get(logs.len()).map(|entry| entry.log_id)" hiqlite/src/store/logs/memory.rs'
sh -c 'grep -q "callback.log_io_completed(Ok(()));" hiqlite/src/store/logs/memory.rs'
grep -q 'pub fn logs_dir_cache' hiqlite/src/store/logs/mod.rs
grep -q 'pub fn logs_dir_db' hiqlite/src/store/logs/mod.rs
sh -c '! grep -qE "fs::write|File::create|sync_all" hiqlite/src/store/logs/memory.rs'
```
