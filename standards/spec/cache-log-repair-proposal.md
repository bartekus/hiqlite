# Cache log store contract: repair proposal

A source-backed proposal for repairing F-021 through F-024, F-029, and F-047,
the last of which this trace found and the register now carries.

**Status, 2026-09-21: proposed, not authorized.** Nothing here is scheduled or
implemented. `007-cache-log-store` is the current contract for
`hiqlite/src/store/logs/` and remains authoritative until a repair spec
supersedes or amends it. This document is the traced analysis a repair would be
reviewed against, in the shape `standards/spec/wal-repair-proposal.md` used for
`008`.

Traced on 2026-09-21 against the locked dependency graph. Where this document
says **observed**, it was read at source or executed; where it says
**inferred**, a consequence follows from control flow that was *not* executed.

---

## 1. The pinned contract, traced

**Version.** `hiqlite/Cargo.toml` requests `openraft = { version = "0.9.21",
features = ["serde", "storage-v2"] }`, a caret requirement. `Cargo.lock`
resolves it to **0.9.24**, which is what this proposal is written against and
what F-029 already cites. The 0.9.25 source is also in the local registry; the
five method contracts below are byte-identical between the two.

All quotations are from
`openraft-0.9.24/src/storage/v2.rs` and `openraft-0.9.24/src/storage/mod.rs` in
the local registry checkout, not from published documentation.

### 1.1 What the trait requires, method by method

| method | locked requirement | source |
|---|---|---|
| `RaftLogStorage::get_log_state` | "Returns the last deleted log id and the last log id... The returned `last_log_id` could be the log id of the last present log entry, or the `last_purged_log_id` if there is no entry at all." | `v2.rs:59-66` |
| `LogState::last_log_id` (field) | "The log id of the last present entry if there are any entries. Otherwise the same value as `last_purged_log_id`." | `mod.rs:146-148` |
| `RaftLogReader::try_get_log_entries` | "The start value is inclusive in the search and the stop value is non-inclusive: `[start, stop)`." and "**Entry that is not found is allowed.**" | `mod.rs:162-167` |
| `RaftLogStorage::append` | entries readable when the method returns; callback when persisted; "There must not be a **hole** in logs." | `v2.rs:110-129` |
| `RaftLogStorage::truncate` | "Truncate logs **since** `log_id`, inclusive... It must not leave a **hole** in logs." | `v2.rs:131-136` |
| `RaftLogStorage::purge` | "Purge logs **upto** `log_id`, inclusive... It must not leave a **hole** in logs." | `v2.rs:138-143` |

The trait as a whole also requires that "Logs must be consecutive" and that "All
write-IO must be serialized" (`v2.rs:44-48`).

### 1.2 Who calls each method, and when

This matters more than usual here, because it is what separates a contract
violation from an outage. Observed by grep over the locked crate, excluding its
own test suite.

| method | call site | phase |
|---|---|---|
| `get_log_state` | `storage/helper.rs:74`, inside `StorageHelper::get_initial_state` | startup only |
| `get_log_state` | `storage/helper.rs:282`, inside `last_membership_in_log`, itself reached from `get_membership` at `helper.rs:105` | startup only |
| `purge` | `storage/helper.rs:116`, the "clean the hole between last_log_id and last_applied" branch | startup only |
| `purge` | `core/raft_core.rs:1646` | **runtime** |
| `truncate` | `core/raft_core.rs:1650` | **runtime** |
| `try_get_log_entries` | replication and `last_membership_in_log` | **runtime** |

**Both consumers of `get_log_state` are on the startup path.** F-021 already
recorded that; this proposal can now name both sites rather than one.

## 2. Where the implementation diverges

All line references are `hiqlite/src/store/logs/memory.rs` at the integration
head this was traced against.

### 2.1 `get_log_state` reports `None` twice over (F-021, F-024)

```rust
let last_log_id = {
    let logs = self.logs.read().await;
    logs.get(logs.len()).map(|entry| entry.log_id)   // :118-121
};
```

`VecDeque::get(len)` is always one past the end, so `last_log_id` is `None`
whatever the store holds. **Observed by execution**:
`get_log_state_reports_no_last_log_id_even_after_append` stores two entries and
sees `None`.

There is a **second** divergence in the same six lines that no finding has
recorded yet. The trait says that when there are no entries, `last_log_id` must
equal `last_purged_log_id`, not `None`. The current code returns `None` for the
empty case as well, and because `last_purged` is never assigned (F-024,
`:28` set only in `new()`, never in `purge`) the two happen to agree today. A
repair that fixes only the `logs.get(logs.len())` expression would fix the
non-empty case and leave the empty case wrong the moment F-024 is also fixed.
**The two must be repaired together or the store becomes inconsistent with
itself.** That is the single most important structural point in this document.

### 2.2 `purge` removes exclusively where the trait requires inclusive (F-029)

```rust
let purge_until = (log_id.index - first_offset) as usize;   // :210
logs.drain(..purge_until);                                  // :218
```

`drain(..n)` removes `n` elements, so the element at offset `n`, which is the
entry at `log_id.index`, survives the purge that named it.

### 2.3 `truncate` is correct, and the reason is worth writing down

```rust
let truncate_from = (log_id.index - first_offset) as usize;  // :186
logs.truncate(truncate_from);                                // :189
```

The arithmetic is identical to `purge`'s, and here it is **right**:
`VecDeque::truncate(n)` *keeps* `n` elements, so the element at offset `n` is
removed, which is the inclusive semantics `truncate` requires. `drain(..n)`
*removes* `n` elements, so the element at offset `n` is kept.

One expression, two standard-library methods, opposite meanings at the boundary.
A repair must not "make purge consistent with truncate" by copying the shape; it
must change `drain(..purge_until)` to `drain(..=purge_until)` and leave
`truncate` alone. `007` B-4 describes both as "offset arithmetic over the deque",
which is true and is exactly why the difference is easy to miss.

### 2.4 The `truncate` debug assertion compares an offset to an index (F-022)

```rust
debug_assert!(truncate_from == logs.get(truncate_from).unwrap().log_id.index as usize);  // :187
```

Equal only while `first_offset == 0`. It fires in a debug build on any truncate
after a purge has advanced the front. **Additionally**, and not recorded by
F-022: `logs.get(truncate_from).unwrap()` panics when `truncate_from ==
logs.len()`, which is a truncate at exactly one past the last entry, a legal
no-op call. Both halves are debug-only.

### 2.5 `try_get_log_entries` has two contract violations, not one

**F-023, the recorded one.** `Bound::Excluded(i) => *i - 1` (`:66`) underflows on
an exclusive end bound of zero: debug panics, release wraps to `u64::MAX` and
then panics at the `expect` below. Reachability was not established when F-023
was written and is not established here either.

**The unrecorded one.** For an empty store and any `end > 0`:

```rust
let first_log_id = logs
    .front()
    .expect("to have at least 1 entry in logs as long as end > 0")   // :76-78
```

The trait states plainly that "Entry that is not found is allowed". A reader
asked for a range the store does not hold must return an empty `Vec`, not panic.
This is a reachable-by-contract panic against a method the replication path
calls at runtime. Recorded as **F-047**; see section 6.

### 2.6 One more panic the offsets can reach

`purge`'s `debug_assert!(logs.len() >= purge_until)` (`:211-216`) is debug-only,
so in release a purge naming an index beyond the last stored entry reaches
`drain(..purge_until)` with an out-of-range bound, which panics. Recorded here as
part of the same family rather than as a separate finding, because the repair
that makes `purge` inclusive has to decide what an out-of-range purge means
anyway.

## 3. What is actually broken today, and what is not

This section exists so that a repair is not sold as an outage fix.

**Latent, as far as can be established.** F-021 and F-024 are both read only by
the two startup-path consumers in section 1.2, and `LogStoreMemory` is by
construction empty at startup: it is selected only when
`node_config.cache_storage_disk` is false (`store/mod.rs:161`), and the cache
state machine it pairs with is in-memory too, so `last_applied` is `None` on the
same startup. The "clean the hole" purge at `helper.rs:110-119` therefore does
not trigger, and `last_membership_in_log` computes `end = 0` and never loops.

**Live at runtime, with no established consequence.** F-029's exclusive purge
runs on every `raft_core.rs:1646` purge. Its effect is that one entry survives
each purge and becomes the new front. Because `first_offset` is read from the
front everywhere else, the arithmetic stays self-consistent, which is why
nothing observably breaks. The cost is a permanently retained entry per purge
cycle and a purge frontier that is never reported, the latter masked by F-024.

**Not established at all.** F-023's and F-047's reachability. Whether OpenRaft
ever requests `0..0`, or a range against an empty store, was not determined by
execution in this pass and should not be asserted by a repair spec either.

**The honest summary**: this is a store that violates four clauses of its trait
and currently gets away with all of them because of how narrowly it is used. The
risk being bought down by a repair is not a present outage; it is that any change
in OpenRaft's call pattern, any move to a durable variant of this store, or any
reuse of it under a persistent state machine turns latent into live, with
silent log loss as the failure mode.

## 4. Proposed repair

Five changes, each independently reviewable.

### 4.1 `get_log_state` returns the real frontier, in both cases

```rust
let last_log_id = {
    let logs = self.logs.read().await;
    logs.back().map(|entry| entry.log_id)
}
.or(last_purged_log_id);
```

`back()` for the non-empty case, falling back to `last_purged_log_id` for the
empty case as `mod.rs:146-148` requires. This closes F-021 and the second
divergence of section 2.1 together.

### 4.2 `purge` assigns `last_purged` and removes inclusively

```rust
logs.drain(..=purge_until);
// and, under the same lock ordering as the existing data lock:
self.data.lock().await.last_purged = Some(log_id);
```

This closes F-029 and F-024. The two must land together for the reason section
2.1 gives: fixing 4.1 without 4.2 makes the empty case report a stale `None`;
fixing 4.2 without 4.1 leaves the non-empty case reporting `None` over a
now-correct purge frontier.

**Lock ordering is a real question here, not a detail.** `purge` currently holds
only `logs.write()`; assigning `last_purged` needs `data.lock()`. Every other
method takes at most one of the two. Taking both introduces the first ordering
constraint in the file, and `get_log_state` takes them in the order
`data` then `logs` (`:115-121`). A repair must either take them in that same
order in `purge`, or narrow the window by computing under `logs` and assigning
under `data` after the write lock is dropped, at the cost of a moment in which
the deque is purged and the frontier is not yet updated. Both are defensible;
the first is simpler and the second is closer to the existing style. This is
named as an open design point, not decided here.

### 4.3 `try_get_log_entries` tolerates a missing range

Saturating arithmetic for the exclusive end bound, and an empty-store early
return rather than an `expect`:

```rust
Bound::Excluded(i) => match i.checked_sub(1) { Some(e) => e, None => return Ok(Vec::default()) },
```

and, replacing `:76-81`,

```rust
let Some(front) = logs.front() else { return Ok(Vec::default()) };
```

with the remaining range clamped to what the deque actually holds. This closes
F-023 and F-047 and makes the method match "Entry that is not found is allowed".

### 4.4 The `truncate` assertion compares like with like

```rust
debug_assert!(logs.get(truncate_from).is_none_or(|e| e.log_id.index == log_id.index));
```

Closes F-022 including the `unwrap` half of section 2.4. `truncate` itself is
unchanged, for the reason section 2.3 gives.

### 4.5 `007` records the fifth defect it currently omits

`007` records KD-1 through KD-4 and does not record F-029. Whatever else a repair
spec does, the register and `007` must stop disagreeing. Two routes, and the
choice is the owner's:

- the repair spec `amends` `007`'s known-defects section, adding KD-5 and marking
  all five repaired, which is `008`'s precedent and keeps `007`'s text as it
  stood; or
- a separate reconciliation lands KD-5 into `007` first, and the repair then
  amends a complete list.

The first is fewer changes and one review. The second leaves a cleaner history if
the repair is deferred, because `007` becomes accurate whether or not the repair
happens. **Recommendation: the second**, precisely because this proposal
authorizes nothing and the register has been ahead of `007` since 2026-09-20.

## 5. Regression strategy

### 5.1 One existing test pins the wrong answer and must be replaced, not extended

`purge_removes_entries_below_the_given_index` (`memory.rs:269-287`) stores
indexes 1 to 5, purges through index 3, and asserts the remainder is
`[3, 4, 5]`. Its doc comment states the exclusive rule as if it were the
contract. Under 4.2 the correct remainder is `[4, 5]`.

This is the same situation `008` faced and it has the same answer: the
expectation is replaced, and the replacement is carried through the governed
mechanism rather than left asserting the old outcome. A repair that adds a new
inclusive test beside the old exclusive one would leave the corpus asserting both.

`get_log_state_reports_no_last_log_id_even_after_append` is in the same position
for 4.1.

### 5.2 The bounded strategy

Per-method tests, each of which fails against the current code:

| test | closes |
|---|---|
| `get_log_state` reports the last entry after append | F-021 |
| `get_log_state` reports `last_purged` when the deque is empty | 2.1's second half |
| `purge` removes the entry it names and updates `last_purged` | F-029, F-024 |
| `purge` then `truncate` keeps the assertion quiet in a debug build | F-022 |
| `truncate` at one past the end is a no-op and does not panic | 2.4's second half |
| `try_get_log_entries` with an exclusive end bound of `0` returns empty | F-023 |
| `try_get_log_entries` against an empty store returns empty | F-047 |

All are cheap, all run in the library's unit-test binary, and none needs a
cluster. This is the strategy this proposal recommends for a bounded repair.

### 5.3 The stronger option, and its real cost

OpenRaft 0.9.24 ships a storage conformance suite: `openraft::testing::Suite`
(`src/testing/suite.rs`), an ungated public module (`lib.rs:64`). `Suite::test_all`
would exercise every clause in section 1.1 against this store, and it is far
stronger evidence than any hand-written set.

It is not free. `Suite` is generic over a `StoreBuilder<C, LS, SM, G>` that must
produce **both** a `RaftLogStorage` and a `RaftStateMachine`
(`testing/store_builder.rs:26-34`), and `test_store` runs state-machine cases in
the same bundle with no log-only subset. Wiring it therefore reaches into
`hiqlite/src/store/state_machine/memory/`, which is `006`'s unit, and would drag
`006`'s open evidence gaps (W-05) and F-027 into a repair scoped to the log store.

**This is a genuine owner decision and is not taken here.** Stated plainly: the
bounded option in 5.2 proves the five repairs; the suite in 5.3 proves the store
conforms, costs a cross-spec change, and is the better long-term answer if the
cache surface is going to be worked on anyway.

## 6. The finding this trace added

**F-047, `defect`, confidence `high`**, recorded in the register by the same
change that added this document.
`try_get_log_entries` panics on an empty store for any non-zero end bound.
`hiqlite/src/store/logs/memory.rs:76-78` calls
`logs.front().expect("to have at least 1 entry in logs as long as end > 0")`
after the `end < start` early return, so a request for a range the store does
not hold panics instead of returning an empty `Vec`. OpenRaft 0.9.24 states the
opposite requirement on the trait method: "Entry that is not found is allowed"
(`openraft-0.9.24/src/storage/mod.rs:167`). Source-established; reachability not
established, for the same reason F-023's is not. Distinct from F-023, which is
about the `*i - 1` underflow at an end bound of zero.

It is recorded, not repaired, and it is recorded ahead of `007` in the same way
F-029 was. `007` now omits two of the six defects in its own unit; section 4.5
covers reconciling both.

## 7. What is deliberately not decided here

- **Whether to repair at all, and when.** W-04 is a queue row; this is analysis
  against which a repair could be authorized.
- **The lock-ordering choice in 4.2.** Both options are stated with their costs.
- **The reconciliation route in 4.5**, though a recommendation is given.
- **Bounded tests versus the conformance suite** (5.2 against 5.3).
- **Anything about `006`.** F-025 and F-027 sit in the cache state machine and
  are named here only where 5.3's cost depends on them.
- **Anything durable.** `007` B-1 records non-durability as the contract, not as
  a defect, and nothing here proposes changing it.
- **Ratification, enforcement, publication and release.**
