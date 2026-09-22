---
id: "020-cache-log-store-contract-repair"
title: "Repair the cache log store against the locked OpenRaft log-storage contract"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "006-cache-state-machine"
  - "007-cache-log-store"
amends: ["007-cache-log-store"]
# D-3: this spec's `## Verification` block IS 007's acceptance from now on, and 007's own file
# is not edited again. Whole-block replacement is the mechanism's unit, so the block carries
# every obligation 007 declared, not only the commands this repair touched.
amends_verification: ["007-cache-log-store"]
amends_sections:
  - "3-behavior"
  - "4-evidence-and-its-limits"
  - "5-known-defects"
extends:
  - spec: "007-cache-log-store"
    unit: { kind: directory, path: "hiqlite/src/store/logs/" }
    nature: superseding
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/cache-log-repair-proposal.md" }
    nature: superseding
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Repairs all six known defects of the cache Raft group's in-memory log store
  against the locked OpenRaft 0.9.24 log-storage traits: the last-log frontier
  and its empty-store fallback, inclusive purge with purge-frontier
  bookkeeping, tolerant range reads with clamping, and the debug assertions
  that compared an offset to an absolute index. Adopts one lock order, data
  then logs, held together across the purge update. Amends 007's behavior,
  evidence and known-defect sections and carries its acceptance.
---

# 020: Repair the cache log store against the locked OpenRaft log-storage contract

## 1. Purpose

`007-cache-log-store` adopted `hiqlite/src/store/logs/` as found and recorded
six known defects in it, four when it was written and two reconciled into it on
2026-09-21 from the findings register. This spec repairs all six and the
seventh divergence `007` records without an identifier of its own.

One responsibility: **make the in-memory log store answer the five
`RaftLogReader` and `RaftLogStorage` methods the way the locked OpenRaft states
they must be answered**. Nothing here changes what the store is for, and `007`
B-1's statement that it is deliberately non-durable stands unchanged.

The analysis this repair was reviewed against is
`standards/spec/cache-log-repair-proposal.md`, traced on 2026-09-21. Where this
spec and that document differ, the two open design points the proposal declined
to decide are decided here (section 6) and this spec governs.

## 2. Territory

**Extends** `007`'s `directory` unit `hiqlite/src/store/logs/` with nature
`superseding`: the behavior described by `007` B-4 and B-5, and the defects at
its KD-1 to KD-6, are replaced by section 3 below. One origin per unit is
preserved: `007` still `establishes` the directory and this spec does not
re-declare it.

**Amends** `007` sections 3, 4 and 5, and carries `007`'s acceptance under
`amends_verification` (D-3). `007`'s own `spec.md` is not edited by this change;
its text is the contract as it stood.

**Ownership boundary, unchanged from `007` section 2.** OpenRaft owns when a log
store is asked to append, truncate, purge or read. This spec specifies only what
hiqlite's implementation does with the request it is handed. No claim below is
evidence about OpenRaft's call pattern, and section 4 states plainly which of
these paths have never been observed being driven by a Raft group.

## 3. Behavior

All line references are `hiqlite/src/store/logs/memory.rs` after this repair.
Every requirement quoted from OpenRaft is read from `openraft-0.9.24` in the
local registry checkout, not from published documentation.

### B-1. One lock order: `data`, then `logs`

The store holds two locks, `Arc<Mutex<LogData>>` and
`Arc<RwLock<VecDeque<Entry>>>`. Before this repair only `get_log_state` took
both, so the file had no stated ordering. It now has one, and it is the order
`get_log_state` already used: **`data` first, then `logs`**.

`purge` is the method the repair gives a second lock to, and it takes them in
that order and **holds both together** for the whole update. The alternative the
proposal named, computing under `logs` and assigning under `data` afterwards,
is declined: it leaves a window in which the deque is purged and the frontier
still reports the old value, and the frontier is exactly what a reader uses to
interpret an empty deque (B-2). Correctness of the pair is the point; a shorter
critical section over an in-memory deque is not worth a torn answer.

`save_vote`, `read_vote`, `append` and `truncate` each take at most one lock,
so the order constrains nothing there.

### B-2. `get_log_state` reports the real frontier, and falls back to the purge point

`last_log_id` is `logs.back().map(|entry| entry.log_id)`, falling back to
`last_purged_log_id` when the deque is empty.

Two requirements are met together and cannot be met apart. The non-empty case
is the trait's "the log id of the last present log entry"; the empty case is
`LogState::last_log_id`'s "otherwise the same value as `last_purged_log_id`"
(`storage/mod.rs:146-148`). Before the repair the first was `None` because
`VecDeque::get(len)` is always one past the end (`007` KD-1), and the second was
`None` because `purge` never assigned the frontier (`007` KD-4), so the two
happened to agree. Repairing either alone makes the store inconsistent with
itself, which is why B-2 and B-3 are one change.

### B-3. `purge` removes inclusively and records the frontier

`purge(log_id)` removes every entry **up to and including** `log_id.index`, which
is the trait's "Purge logs upto `log_id`, inclusive" (`storage/v2.rs:138`). The
change is `drain(..purge_until)` to `drain(..=purge_until)`: `drain(..n)` removes
`n` elements and so kept the entry the purge named. `truncate`'s arithmetic is
identical and is **not** changed, because `VecDeque::truncate(n)` *keeps* `n`
elements and therefore already removes the entry at the named index. One
expression, two standard-library methods, opposite meanings at the boundary.

`last_purged` is assigned under the same lock acquisition, and only ever moves
forward: a purge naming an index at or below the recorded frontier changes
nothing.

Three boundary cases are decided rather than asserted:

- **An empty store.** The frontier still advances. A purge naming an index this
  store never held is a statement about what has been purged, and the store has
  to be able to answer B-2's empty case with it.
- **An index below the front.** Already purged past. Nothing is removed and the
  frontier does not move.
- **An index beyond the last entry.** The deque is emptied, clamped to its
  length. Before the repair the bound was checked only by a `debug_assert!`, so
  a release build reached `drain` with an out-of-range bound and panicked.

### B-4. `try_get_log_entries` tolerates a range the store does not hold

The trait states that "Entry that is not found is allowed"
(`storage/mod.rs:162-167`). Four changes make that true:

- An exclusive end bound of `0` returns no entries instead of computing `0 - 1`
  (`007` KD-3): a debug build panicked and a release build wrapped to `u64::MAX`
  and then panicked below. An exclusive start bound of `u64::MAX` is handled the
  same way.
- An **empty deque** returns no entries instead of `expect`ing on `front()`
  (`007` KD-6).
- The requested range is **clamped** to the intersection with what the deque
  holds. A start below the front is raised to the front; an end above the back
  is lowered to the back; a range entirely outside returns no entries.
- The surviving `debug_assert!` checks the clamped range against the entries
  actually selected, which is a statement that can be true, rather than
  asserting that the caller asked for a range the store holds.

`Bound::Unbounded` as an end bound still panics. That is unchanged and
deliberate: it is a caller error with no sensible answer, and no repair here
turns it into a silent empty result.

### B-5. The debug assertions compare like with like

`truncate`'s assertion compared a deque offset to an absolute log index (`007`
KD-2), which are equal only while the front sits at index 0, so it fired on any
truncate after a purge advanced the front. It now asserts that the entry at the
computed offset, **if there is one**, carries the log index the caller named.
The `is_none_or` closes the second half of KD-2: `logs.get(truncate_from)` is
legitimately `None` for a truncate at exactly one past the last entry, which is
a legal no-op call, and the old `unwrap()` panicked on it.

`truncate` gains one non-debug guard. A `log_id.index` at or below the front
clears the deque rather than underflowing the subtraction, which a release build
did silently.

### B-6. `append` is unchanged

`007` B-3 stands as written. The completion callback is still immediate and
unconditional, because for an in-memory store "the entries are in the deque" is
the whole of the IO. This spec adds no persistence step that could fail and
therefore adds no failure to report.

## 4. Evidence and its limits

Eleven tests in `hiqlite/src/store/logs/memory.rs` replace the two
characterization tests `007` added. Each was **run against the pre-repair
implementation and observed to fail**, by splicing the repaired test module onto
the unrepaired one: 0 passed, 11 failed. That is the bar `007` D-1 set when it
wrote a test that recorded a defect so the repair would be visibly a change.

Two tests are replacements rather than additions, and the expectation is
replaced rather than extended so the corpus does not assert both outcomes:

- `get_log_state_reports_the_last_stored_entry` replaces
  `get_log_state_reports_no_last_log_id_even_after_append`.
- `purge_removes_the_entry_it_names_and_records_the_frontier` replaces
  `purge_removes_entries_below_the_given_index`, whose doc comment stated the
  exclusive rule as if it were the contract.

One test is an interleaving regression rather than a per-method one.
`a_concurrent_reader_never_sees_a_purged_deque_beside_a_stale_frontier` runs
four readers against a store being purged entry by entry and asserts every
observation they make, so a torn state fails the test whenever it is observed.
It uses `task::yield_now` rather than a sleep, so it is not timing-dependent;
what it is **not** is a proof that no interleaving exists that it failed to
schedule.

What the acceptance does **not** establish:

- **Nothing here runs a Raft group.** Every test drives `LogStoreMemory`
  directly. That OpenRaft calls these methods with the indices B-3 and B-4
  assume remains OpenRaft's contract and not hiqlite evidence, exactly as `007`
  section 4 said.
- **Reachability is still not established** for the two defects whose
  reachability `007` KD-3 and KD-6 left open. The repair makes the store answer
  correctly if asked; it does not establish that anything asks.
- **`append` completion is still not observed through OpenRaft.**
- **OpenRaft's own storage conformance suite has not been run.** It is tracked
  as a separate evidence item; see section 5.
- **The lock order is stated and used, not enforced.** Nothing mechanically
  prevents a later edit from taking `logs` then `data`, which would be the
  deadlock this file has never had.

## 5. Known defects

**KD-1. The OpenRaft storage conformance suite is not run, and this spec does
not close that.** `openraft::testing::Suite` (`src/testing/suite.rs`, an ungated
public module at `lib.rs:64`) would exercise every clause of the log-storage
contract against this store, which is stronger evidence than any of the eleven
tests above. It is not run here.

Its dependency is stated precisely, because the proposal's section 5.3 and the
adoption plan's W-04 row describe it loosely enough to be read as blocking on
the whole cache policy question, and it does not. `Suite` is generic over a
`StoreBuilder<C, LS, SM, G>` that must produce **both** a `RaftLogStorage` and a
`RaftStateMachine` (`testing/store_builder.rs:26-34`), and `test_store` runs
state-machine cases in the same bundle with no log-only subset. So the single
dependency is a builder that can construct `006`'s `StateMachineMemory` in a
test, which is a fixture, not a decision. It does **not** depend on closing
F-025, F-026 or any lock policy question.

Recorded, not closed. The eleven tests prove the six repairs; the suite would
prove conformance.

## 6. Resolved decisions

**D-1 (2026-09-21, one lock order, both locks held together).** The proposal's
section 4.2 named this open and gave two options with their costs. Owner
direction was to adopt `data` then `logs` and to hold them together across the
deque and frontier update. Recorded as B-1. The rejected alternative is the
narrower window, whose cost is a moment in which a reader sees a purged deque
beside a stale frontier; B-2's empty-store fallback is read directly from that
frontier, so that window is a wrong answer and not merely a stale one.

**D-2 (2026-09-21, bounded per-method and interleaving tests now, conformance
suite tracked separately).** The proposal's 5.2 against 5.3. Both, in order:
the bounded set lands with the repair and the suite is carried as KD-1 with its
actual dependency named. Taking the suite first would have made a log-store
repair wait on a state-machine fixture.

**D-3 (2026-09-21, this block is `007`'s acceptance).** `amends_verification`
replaces `007`'s `## Verification` block whole. Two of `007`'s seven commands
named the tests that pinned the repaired behavior and one grepped for the
defective `logs.get(logs.len())` expression; all three are replaced below and
marked. The remaining four are carried forward unchanged, because whole-block
replacement is the mechanism's unit and dropping an obligation silently is the
failure this note exists to prevent.

**D-4 (2026-09-21, `007` is reconciled first, then amended).** The proposal's
section 4.5 offered two routes and recommended the second. It was taken: KD-5
and KD-6 landed in `007` by a separate change on 2026-09-21 that repaired
nothing, so `007` is accurate about its own territory whether or not this repair
follows. This spec therefore amends a complete list of six.

## 7. Out of scope

- **The cache state machine.** `006`, including F-025 and F-027.
- **Durability for this store.** `007` B-1 records non-durability as the
  contract, not as a defect, and nothing here changes it.
- **`hiqlite-wal`.** `001`, amended by `008`.
- **Membership, leases, and live peer recovery.**
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 020`. **This block is `007`'s acceptance as well as
this spec's** (D-3). `007`'s own file is not edited, and `spec-spine verify 007`
prints the attribution line naming this spec before it runs a command.

Each command below was confirmed to select exactly one test and run it: an
`--exact` filter that matches nothing exits `0` having run zero tests, which is
not acceptance evidence, so the reported `1 passed` was read for each line
rather than the exit code alone.

```verify:cli
# Package names, not library names: the downstream release renamed the three packages
# (`031` B-2), and `-p` takes a package name. `use hiqlite::..` is unaffected.
# --- 007's acceptance, carried forward ---
# was get_log_state_reports_no_last_log_id_even_after_append, which pinned KD-1
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::get_log_state_reports_the_last_stored_entry -- --exact
# was purge_removes_entries_below_the_given_index, which pinned KD-5
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::purge_removes_the_entry_it_names_and_records_the_frontier -- --exact
# was a grep for `logs.get(logs.len()).map(|entry| entry.log_id)`, the KD-1 expression this
# repair removed. Replaced with the expression that must be there now.
sh -c 'grep -q "logs.back().map(|entry| entry.log_id)" hiqlite/src/store/logs/memory.rs'
sh -c 'grep -q "callback.log_io_completed(Ok(()));" hiqlite/src/store/logs/memory.rs'
grep -q 'pub fn logs_dir_cache' hiqlite/src/store/logs/mod.rs
grep -q 'pub fn logs_dir_db' hiqlite/src/store/logs/mod.rs
sh -c '! grep -qE "fs::write|File::create|sync_all" hiqlite/src/store/logs/memory.rs'
# --- what this repair adds ---
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::get_log_state_falls_back_to_the_purge_frontier_when_empty -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::purge_advances_the_frontier_monotonically_and_on_an_empty_store -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::purge_beyond_the_last_entry_empties_the_store_without_panicking -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::truncate_after_a_purge_advanced_the_front_does_not_fire_the_assertion -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::truncate_one_past_the_end_is_a_no_op -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::an_exclusive_end_bound_of_zero_returns_no_entries -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::reading_an_empty_store_returns_no_entries -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::a_range_is_clamped_to_what_the_store_actually_holds -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::memory::tests::a_concurrent_reader_never_sees_a_purged_deque_beside_a_stale_frontier -- --exact
# the inclusive purge, pinned at the expression: `drain(..n)` removes n and kept the entry the
# purge named, which is the KD-5 defect.
sh -c 'grep -q "logs.drain(..=purge_until);" hiqlite/src/store/logs/memory.rs'
```
