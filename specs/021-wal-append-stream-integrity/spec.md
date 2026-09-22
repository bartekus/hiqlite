---
id: "021-wal-append-stream-integrity"
title: "Repair the truncated WAL append stream and the terminal-writer call paths"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "001-wal-durability-and-completion"
  - "008-wal-append-completion-notification"
amends:
  - "001-wal-durability-and-completion"
  - "008-wal-append-completion-notification"
# D-4: this spec's `## Verification` block IS 008's acceptance from now on, and 008's file is
# not edited. It does NOT take 001's: 008 holds that one and only one spec may. Whole-block
# replacement is the mechanism's unit, so the block below carries every obligation 008 declared
# plus this repair's.
amends_verification:
  - "008-wal-append-completion-notification"
amends_sections:
  - "3-behavior"
  - "8-known-defects"
extends:
  - spec: "001-wal-durability-and-completion"
    unit: { kind: directory, path: "hiqlite-wal/src/" }
    nature: superseding
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Repairs F-028: the WAL writer could not tell a dropped entry sender from the
  marker that ends a healthy stream, so a truncated append was acknowledged and
  notified as a success. The writer now names the truncation on both channels
  and ends, and every adapter call path that panicked or waited on a terminal
  writer returns a named storage error instead. Amends 001 and 008 and carries
  their acceptance.
---

# 021: Repair the truncated WAL append stream and the terminal-writer call paths

## 1. Purpose

`008` repaired what the WAL writer reports when an append is rejected or its
persistence fails, and recorded as KD-2 a defect it deliberately did not repair:
the writer cannot tell a **truncated** entry stream from a complete one. That is
F-028, and this spec repairs it.

Repairing it forces a second question that `008` did not have to answer. Once a
truncated append ends the writer, every other call path into that writer has to
say what happens to work that is queued behind it, and today most of them
`unwrap` or `expect` on a channel whose other end has gone away. Those two
belong in one change: a failure policy whose consequence is a panic in the
caller is not a failure policy.

One responsibility: **an append that did not receive every entry is never
reported as a success, and no caller of a writer that has ended is left
panicking or waiting.**

## 2. Territory

**Extends** `001`'s `directory` unit `hiqlite-wal/src/` with nature
`superseding`, which is the edge `008` used for the same unit. `001` still
`establishes` it and this spec does not re-declare it.

**Amends** `001` and `008`, and carries the acceptance for both (D-4).
Neither `spec.md` is edited; each keeps its text as it stood.

Three files change: `writer.rs` (the collection loop and the completion
helper), `error.rs` (one new variant), and `log_store_impl.rs` (the adapter's
call paths).

**Ownership boundary.** OpenRaft owns when it appends and what it does with a
storage error. `008` section 3 established that the error reaches `RaftCore`;
this spec adds one more cause to the set of things that can be reported and does
not restate what OpenRaft then does with it.

## 3. Behavior

### B-1. The entry stream has three endings, not two

The collection loop used to be
`while let Ok(Some((id, bytes))) = rx.recv()`, which ends identically for all
three of `Ok(Some(..))` running out, `Ok(None)`, and `Err(Disconnected)`. It is
now an explicit `match` over those three:

- `Ok(Some(entry))`: an entry, collected.
- `Ok(None)`: the producer's **end-of-stream marker**. The batch is complete.
- `Err(_)`: the entry sender was **dropped**. The batch is incomplete, and the
  writer does not know how many entries it was supposed to receive.

The third case sets `Error::IncompleteAppend`, a new variant carrying how many
entries arrived and how many of those were persisted. It is a distinct variant
rather than a reuse of `Error::Internal` because it is a distinct condition: the
bytes that did arrive are fine and on their way to disk, and it is the *batch*
that is incomplete.

### B-2. A truncated append fails on both channels, exactly once each

`008` section 3.1 separated the acceptance acknowledgement from the persistence
completion notification, and this repair keeps that separation exactly. A
truncated append:

- **acknowledges** `Err(Error::IncompleteAppend(..))` on the `oneshot`, so the
  caller is told rather than left waiting;
- runs the persistence step for whatever was already written, because those
  bytes are in the mapping regardless;
- **notifies** exactly one completion, an `io::Error` reproduction of the same
  cause, so `RaftCore` sees the failure through `LogFlushed::log_io_completed`.

Exactly one acknowledgement and exactly one completion per dispatched append,
unchanged from `008`. The ordering is unchanged too: acknowledge, persist,
notify.

### B-3. A truncated append is terminal for the writer

`008` section 3.1 recorded two outcomes and their policies: a **rejection** does
not end the writer, and a **persistence failure** notifies and then ends it.
A truncated stream is a third, and its policy is **terminal**.

The reason is not severity, it is ignorance. After a rejection the writer knows
exactly what it did and did not write. After a truncation it holds a prefix of a
batch whose extent it cannot know, and it cannot tell "the producer died after
3 of 10" from "the producer died after 10 of 10 but before the marker". A later
append could be the continuation of a batch that never fully arrived, and the
writer has no way to refuse only that one. So it refuses all of them by ending.

This is implemented by giving the completion helper a `terminal: Option<Error>`
argument, which is `Some` only on this path. A terminal reason wins over a
persistence failure in the same append, because it is the more specific account
of why the writer must stop.

### B-4. A clean empty batch is a success and writes nothing

A batch with no entries and a proper end-of-stream marker is acknowledged
`Ok(())` and notified once with a success, and the writer keeps serving. It is
the one case that ends the collection loop on its first iteration with
`Ok(None)`, and stating it is what makes it distinguishable from a stream that
disconnected before its first entry, which is the other way to end that loop
having received nothing.

It also **writes nothing**, so nothing is marked dirty and the persistence step
has nothing to flush. The previous code forced the dirty flag unconditionally
and performed a full flush for a batch that had appended no bytes. The flag is
now set only when at least one entry was appended.

### B-5. A terminal writer fails its callers rather than panicking them

When the writer ends, the `flume` receiver it owned is dropped. Work still in
the channel is dropped with it, and its acknowledgement senders drop with that.
Before this repair the adapter's response to every one of those was a panic or a
wait:

| path | before | after |
|---|---|---|
| `append`, awaiting the acknowledgement | `ack_rx.await.unwrap()` | a named `StorageIOError` on `Logs` / `Write` |
| `save_vote`, sending | `.expect("Writer to always be running")` | a named `StorageIOError` on `Vote` / `Write` |
| `save_vote`, awaiting | `rx.await.unwrap()` | the same |
| `read_vote`, awaiting | `rx.await.unwrap()` | a named `StorageIOError` on `Vote` / `Read` |
| `get_log_state`, awaiting | `rx.await.unwrap()` | a named `StorageIOError` on `Logs` / `Read` |
| `truncate` and `purge`, awaiting | `rx.await.unwrap()` | a named `StorageIOError` on `Logs` / `Write` |
| `try_get_log_entries`, sending and receiving | `.expect(..)` and `.unwrap()` | a named `StorageIOError` on `Logs` / `Read` |

All of them now go through one helper that says the thread is no longer running.
The calling task for these paths is `RaftCore`'s, so the previous behavior ended
the process under this repository's `panic = "abort"` release profile and killed
that task silently under a consumer's unwinding profile. Neither is a reported
failure, which is the same class of defect `008` repaired one level down.

Three serialization `unwrap`s on the same paths are returned as storage errors
too (`save_vote`, `purge`'s `last_log`, `append`'s per-entry encode, and
`read_vote`'s decode). `append`'s in particular mattered for B-1: a
serialization panic unwinds past the loop that feeds the writer and drops the
entry sender, which is precisely the truncation this spec repairs, so leaving it
a panic would have manufactured the condition it now reports.

### B-6. What the persisted prefix is, and what it is not

The entries that arrived before the truncation are written with a valid CRC, a
valid id and a header update, and if the batch crossed a WAL file boundary the
sealed file was flushed blocking. **A truncated append is therefore not a torn
record**, and nothing in `check_repair_data_integrity` will notice or roll back
the missing suffix. "Recovery tolerates torn trailing records" is not an answer
to this defect, and section 4 states what was done instead of assuming it.

What the prefix is: a consecutive run of complete records with no hole, readable
after a fresh open, with `last_log_id` reporting the last of them.

What it is not: durable, in any mode this repair did not make durable. Under
`LogSync::ImmediateAsync` the persistence step starts a writeback and does not
wait for it, and under `LogSync::IntervalMillis` the per-append path performs no
flush at all. Calling either of those "persisted" is the confusion `001` exists
to prevent, and section 4 states exactly what the evidence establishes.

### B-8. A torn trailing record is recovered differently from a prefix, and tested

Added 2026-09-22, as evidence for behavior that already existed and had no test.
A torn record is one whose bytes only partly reached the file. Three crash states
are constructed and reopened from disk with no header update on the way out:

- **Torn, past the header's `data_end`.** The integrity scan finds the record,
  its CRC fails, and it is ignored. The complete prefix is recovered, readable,
  and the next append writes over the torn bytes.
- **Complete, past the header.** The header is rewritten only for a file's first
  record and at a flush, so a complete record can sit past it. The scan recovers
  it.
- **Torn, inside the header's range.** A power loss can persist the header page
  and not the data page, because `msync` does not order pages. With `auto-heal`
  the WAL rolls back to the complete prefix. **Without `auto-heal` opening the
  WAL fails with `Integrity`**, so the node refuses to start rather than serve the
  record. Neither case ever reads a torn record as valid.

**For a consumer without `auto-heal`**, which is Rahi's current feature set, the
third case is a node that needs operator intervention after a power loss. Under
`LogSync::Immediate` the rolled-back records were never acknowledged, because the
acknowledgement follows the flush; under `ImmediateAsync` they may have been.

### B-9. No acknowledgement in the WAL crate ends a thread, and a stalled test fails

Added 2026-09-22. F-112 repaired the reader. Independent review found the same
class in the writer: the purge and vote acknowledgements and the shutdown
acknowledgement were `unwrap`ped, so a requester cancelled during a teardown
ended the writer, and under `panic = "abort"` the process. They are now sends
whose failure means only that nobody is listening. `ShutdownHandle::shutdown`
built the reader's shutdown message with `send_async` and never awaited it, so
the message was never sent; it is now a `try_send`, because the reader also ends
when its last sender drops and a shutdown must not wait on it.

**F-114.** CI's `Check` on `d45826c` stalled in
`a_truncated_append_leaves_a_recoverable_prefix_in_every_log_sync_mode` until it
was cancelled 26 minutes in. Reproduced locally: two failures in 177 full-suite
runs, one of them that stall and one a race in the writer tests' `append` helper,
which `unwrap`ped an end-of-stream send into a receiver the writer had already
dropped by rejecting the entry. The helper no longer treats that send as
mandatory, and every wait in the stalling test is bounded and names its step.
After both changes the full suite ran 400 times without a failure. **The stalled
step was never identified**, so this is evidence that the stall no longer
appears, not a diagnosis; a recurrence now fails naming its step instead of
holding a runner, and both CI jobs have a sixty-minute limit.

### B-7. A rejected append reports why, not that a channel closed

The adapter sends a batch's entries to the writer over a bounded channel and
then waits on an acknowledgement. The writer stops reading a batch the moment it
has decided that batch's outcome, so its `drop(rx)` makes the adapter's next
send fail. Returning that `SendError` handed openraft
`sending on a closed channel` and discarded `WalSizeExceeded`, which was already
on its way down the acknowledgement channel that the early return abandoned.

Which error a caller saw depended on nothing but who won that race. A failed
send now waits for the writer's verdict, and three outcomes are named: the
writer's own error; a success reported for a batch that was never finished
being sent, which is a broken contract and is refused as one rather than
returned to openraft as a successful append; and no verdict at all, which is
what a panic in the writer looks like from here.

F-105. B-5 is the same principle on the other channel: a terminal writer fails
its callers with a reason instead of panicking them.

## 4. Evidence and its limits

Three tests, all of which fail against the unrepaired implementation.

**`a_truncated_entry_stream_never_reports_success_in_any_log_sync_mode`**
(`writer.rs`). Six cases: two disconnection points, before the first entry and
after a prefix, across all three `LogSync` modes. Each asserts the
acknowledgement is `Error::IncompleteAppend`, that exactly one completion
arrives and is an error naming the truncation, and that the writer has ended.
**Observed failing** against the unrepaired loop: the first case reports the
acknowledgement as a success, which is F-028 exactly.

**`a_clean_empty_batch_succeeds_and_notifies_once`** (`writer.rs`). Three cases,
one per mode. Asserts the success acknowledgement, exactly one success
completion, and that the writer keeps serving.

**`a_truncated_append_leaves_a_recoverable_prefix_in_every_log_sync_mode`**
(`log_store_impl.rs`). This is the recovery half, and it runs through the
adapter rather than the WAL internals. Per mode: a healthy append, then a
200-entry batch cut off mid-stream, then assertions that the acknowledgement and
the completion both fail, that an append and a vote dispatched afterwards come
back as **errors rather than panics**, that the batch **crossed a WAL file
boundary** before it was cut off, and then a fresh `LogStore::start` on the same
directory with `get_log_state` and `try_get_log_entries` read back a consecutive
prefix with no hole and an empty answer past its end.

The reopen is synchronized on the **advisory lock actually clearing**, bounded
at five seconds, not on a sleep. That is load-bearing and is itself a finding
about the product: a terminal writer acknowledges and notifies *before* it
returns, and it releases its lock only when it returns, so an acknowledgement is
not a signal that the storage is free.

What the acceptance does **not** establish:

- **Nothing is killed.** No process is aborted and no power is lost, so what is
  demonstrated is that the persisted prefix is consecutive and readable after a
  fresh open, **not** that any mode made it durable. `ImmediateAsync` and
  `IntervalMillis` are named in the test's own doc comment for that reason.
- **No Raft group runs.** The completion reaching `RaftCore` is `008`'s
  evidence, through `RaftLogStorageExt::blocking_append`, and is carried forward
  unchanged. This spec adds no new claim about what OpenRaft does with the error.
- **The terminal-writer table of B-5 is only partly executed.** The append and
  vote paths are asserted against a live terminal writer. `get_log_state`,
  `read_vote`, `truncate`, `purge` and `try_get_log_entries` are source changes
  of the same shape with no test of their own.
- **Only one truncation point per batch is exercised**, at the producer side.
  A sender that is dropped between two `flume` sends inside the same bounded
  channel is the same condition from the writer's view, which is why one shape
  covers both, but that equivalence is read from the channel's semantics rather
  than executed.
- **No two-process case.** Everything here is in one process.

## 5. Known defects

**KD-1. A terminal writer leaves its lock file behind.** The graceful shutdown
tail removes `lock.hql`; the error return path does not reach it, so the file
survives with the advisory lock released. The next start therefore takes the
"this is not a clean start" branch and runs the deep integrity check, which is
the conservative outcome and is why this is recorded rather than repaired here.
Exclusive access is W-21's subject and this file is one of its inputs.

**KD-2. The acknowledgement is delivered before the writer has released
anything.** B-2's ordering is `008`'s and is correct for what it promises, but it
means that a caller which treats a failed acknowledgement as "the storage is now
idle" is wrong. The regression test works around this explicitly; nothing in the
public API says it.

**KD-3. `008` KD-1 is untouched.** A failure *before* an append is dispatched
still notifies nothing, because no callback exists yet. That is a different
defect from this one and is not repaired here.

## 6. Resolved decisions

**D-1 (2026-09-21, a truncated append ends the writer).** The alternative was to
treat it as an ordinary rejection and keep serving, which is what the oversized
entry does under its feature flag. Declined, for B-3's reason: an oversized
entry is a known quantity and a truncated batch is not. The cost is that a
single dropped sender takes the node's log storage out of service, which is
accepted because the alternative is a writer that may silently splice two
batches together.

**D-2 (2026-09-21, a new error variant rather than a reused one).**
`Error::IncompleteAppend` is added to a public enum, which is a breaking change
for any downstream `match` over it. Taken deliberately: `Error::Internal` is
already the sink for channel send failures, and a caller that wants to
distinguish "the batch was cut off" from "something internal went wrong" could
not. Recorded in the release notes as an API change rather than smuggled in.

**D-3 (2026-09-21, the adapter's panics are repaired in the same change).**
They could have been a separate spec. They are not, because B-3 is what makes
them reachable in normal operation: before this repair the writer ended only on
a persistence failure, and after it a dropped sender ends it too. Shipping the
termination without the call-path repair would have converted a silent success
into a process abort.

**D-4 (2026-09-21, this block is `008`'s acceptance, and deliberately not
`001`'s).** The first attempt claimed both and the tool refused it (`V-019`):
only one spec may hold another's acceptance, and `008` already holds `001`'s.
That refusal is correct and the resolution is the honest one rather than a
workaround. `008`'s acceptance becomes this block, which carries all
fifteen of `008`'s commands plus this repair's eight.

What that does to `001` was **measured rather than assumed**, because the first
draft of this note asserted the pessimistic reading. The pinned tool resolves
the replacement transitively: `001`'s acceptance is `008`'s, `008`'s is this
one, and `spec-spine verify 001` therefore runs this block. All three of
`verify 001`, `verify 008` and `verify 021` report `passed (23 command(s))`,
which is this block's size, so no obligation is stranded at any link in the
chain.

## 7. Out of scope

- **Supervision of the writer thread.** W-06, and `008` section 6 already
  declined it. This spec makes one more thing end the writer; it adds no
  supervisor.
- **`008` KD-1**, the pre-dispatch failure with no callback to fire.
- **Exclusive access and the lock file's lifecycle.** W-21.
- **Durability under a killed process.** No test here kills anything.
- **The cache group's in-memory log store.** `007`, amended by `020`.
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 021`. **This block is `008`'s acceptance as well as
this spec's** (D-4). `008`'s own file is not edited, and `spec-spine verify 008`
prints the attribution line naming this spec before running a command. `001`'s
acceptance resolves through `008` to this block as well; D-4 records that this
was measured, not assumed.

Each command below was confirmed to select exactly one test and run it.

```verify:cli
# Package names, not library names: the downstream release renamed the three packages
# (`031` B-2), and `-p` takes a package name. `use hiqlite::..` is unaffected.
# --- 008's acceptance, which is also 001's, carried forward unchanged ---
cargo test -p hiqlite-wal-patched --lib writer::tests::append_result_precedes_persistence_and_completion -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::persistence_failure_notifies_error_before_propagating -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::append_rejection_notifies_error_and_never_success -- --exact
cargo test -p hiqlite-wal-patched --lib reader::tests::logs_action_reports_read_errors -- --exact
cargo test -p hiqlite-wal-patched --lib metadata::tests::metadata_overwrite_replaces_existing -- --exact
cargo test -p hiqlite-wal-patched --lib wal::tests::roll_over_purge_front -- --exact
cargo test -p hiqlite-wal-patched --lib wal::tests::roll_over_truncate_end -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::rejection_takes_precedence_over_a_failing_persistence_step -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::success_notifies_once_per_append_in_every_log_sync_mode -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::persistence_failure_notifies_then_terminates_the_writer -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::writer_termination_is_reported -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::injections_are_isolated_per_wal_and_consumed_exactly_once -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::a_dropped_guard_disarms_an_unconsumed_injection -- --exact
cargo test -p hiqlite-wal-patched --lib log_store_impl::tests::append_adapter_forwards_a_persistence_failure_to_openraft -- --exact
cargo test -p hiqlite-wal-patched --lib --features oversized-entry-error log_store_impl::tests::append_adapter_reports_a_rejected_append_as_an_error -- --exact
# --- what this repair adds ---
cargo test -p hiqlite-wal-patched --lib writer::tests::a_truncated_entry_stream_never_reports_success_in_any_log_sync_mode -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::a_clean_empty_batch_succeeds_and_notifies_once -- --exact
cargo test -p hiqlite-wal-patched --lib log_store_impl::tests::a_truncated_append_leaves_a_recoverable_prefix_in_every_log_sync_mode -- --exact
# B-8: torn versus complete trailing records, after a crash, with and without auto-heal
cargo test -p hiqlite-wal-patched --lib wal::tests::a_torn_record_past_the_header_is_dropped_and_the_prefix_recovers -- --exact
cargo test -p hiqlite-wal-patched --lib wal::tests::a_complete_record_past_the_header_is_recovered_not_dropped -- --exact
cargo test -p hiqlite-wal-patched --lib wal::tests::a_torn_record_inside_the_header_is_rolled_back_or_refused -- --exact
cargo test -p hiqlite-wal-patched --lib --features auto-heal wal::tests::a_torn_record_inside_the_header_is_rolled_back_or_refused -- --exact
# B-9: no acknowledgement ends a thread, and the stalling test is bounded
sh -c '! grep -nE "ack\.send\(.*\)\.unwrap\(\)|Shutdown handler to always wait" hiqlite-wal/src/writer.rs hiqlite-wal/src/reader.rs'
sh -c 'grep -q "self.tx_read.try_send(reader::Action::Shutdown)" hiqlite-wal/src/shutdown.rs'
sh -c 'grep -q "stalled at: {step}" hiqlite-wal/src/log_store_impl.rs'
sh -c 'grep -q "timeout-minutes: 60" .github/workflows/code_style.yaml'
# the three endings, pinned at the expressions. The `while let Ok(Some(..))` that collapsed
# the last two into the first must not come back.
sh -c '! grep -q "while let Ok(Some((id, bytes))) = rx.recv()" hiqlite-wal/src/writer.rs'
sh -c 'grep -q "IncompleteAppend" hiqlite-wal/src/error.rs'
# no call path into the writer may panic its caller when the writer has ended
sh -c '! grep -q "expect(\"Writer to always be running\")" hiqlite-wal/src/log_store_impl.rs'
sh -c '! grep -q "expect(\"LogsReader to always be listening\")" hiqlite-wal/src/log_store_impl.rs'
sh -c 'grep -q "fn thread_gone" hiqlite-wal/src/log_store_impl.rs'
# B-7 / F-105: the rejection cause survives, whoever wins the race
cargo test -p hiqlite-wal-patched --lib log_store_impl::tests::a_writer_that_stopped_reading_is_reported_by_its_verdict_not_by_the_channel -- --exact
sh -c 'grep -q "fn writer_verdict" hiqlite-wal/src/log_store_impl.rs'
sh -c 'grep -c "writer_verdict::<T>(ack_rx)" hiqlite-wal/src/log_store_impl.rs | grep -q "^2$"'
```
