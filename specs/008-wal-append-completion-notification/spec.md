---
id: "008-wal-append-completion-notification"
title: "Repair the WAL append completion notification contract"
status: draft
created: "2026-09-19"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "001-wal-durability-and-completion"
amends: ["001-wal-durability-and-completion"]
# 7, D-4: this spec's `## Verification` block IS 001's acceptance from now on, and 001's own
# file is not edited. Whole-block replacement is the mechanism's unit, so that block carries
# every obligation 001 declared, not only the commands this repair touched.
amends_verification: ["001-wal-durability-and-completion"]
amends_sections:
  - "2-append-events"
  - "7-acceptance-boundary"
  - "8-known-defects"
extends:
  - spec: "001-wal-durability-and-completion"
    unit: { kind: directory, path: "hiqlite-wal/src/" }
    nature: superseding
  - spec: "002-snapshot-publication-and-recovery"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/sqlite/" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/wal-repair-proposal.md" }
    nature: superseding
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Repairs F-001 and F-002 by making the WAL writer's log I/O completion
  notification result-bearing, delivering the underlying cause to OpenRaft
  before a persistence failure ends the writer, and reporting that termination.
  The existing fail-stop policy is preserved. Amends 001's append-event,
  acceptance-boundary, and known-defect sections.
---

# 008: Repair the WAL append completion notification contract

## 1. Purpose

`001` recorded two defects in the append path and deliberately left them unfixed
(`001` section 8, bullets 1 and 2; `F-001` and `F-002`):

- A rejected append still reported `log_io_completed(Ok(()))`.
- A persistence failure after the acknowledgement dropped the completion
  callback, so OpenRaft learned that something failed and never learned what.

Both have one root cause: `writer::Action::Append` carried a
`Box<dyn FnOnce() + Send>` with no result parameter, and the OpenRaft adapter
boxed it as a hardcoded `Ok(())`. The writer could not report a failure through
that channel even when it knew about one.

This spec defines the repaired contract and is the authority for the change.
`001` section 8 requires that future work "MUST amend this contract rather than
silently rewriting its baseline", so this spec amends rather than replaces:
`001`'s text stays the contract as it stood, and the sections named in
`amends_sections` are read through this one.

**This is a behavioral repair spec, not a retroactive adoption spec.** Its
acceptance therefore carries regression tests demonstrated failing against the
pre-repair behavior and passing after the repair (section 4.4).

## 2. Territory

This spec claims no new origin. It `extends`:

- `{ kind: directory, path: "hiqlite-wal/src/" }`, owned by `001`, with
  `nature: superseding` for the append-completion behavior only. `001`'s vote,
  truncation, purge, recovery, and synchronization-mode contracts are untouched
  and keep their authority.
- `{ kind: directory, path: "hiqlite/src/store/state_machine/sqlite/" }`, owned
  by `002`, with `nature: additive`. The only change there is the mechanical
  call-site adaptation of a test helper that constructs `Action::Append`
  (`state_machine.rs`); no `002` behavior changes.
- Three `standards/spec/` documents owned by `005`:
  `wal-repair-proposal.md` with `nature: superseding`, because seven of its
  claims are corrected in place rather than added to;
  `findings-register.md` and `adoption-plan.md` with `nature: additive`,
  because each gains a record without any existing entry being rewritten.
  Without these the coupling gate refuses the change against its actual pull
  request base (`C-001`, three paths), which is the gate doing its job: those
  documents are `005`'s territory, and `references` is a non-owning edge.

The `extends` edges are declared because `000` section 4 requires a spec that
touches a unit another spec owns to declare a unit-level claim rather than rely
on an `amends` edge alone. A passing coupling result is not a substitute for
that: the gate checks that an owning spec moved in the same range, which the
`amends` edge would also satisfy, while the authored rule asks for the explicit,
attributed second claim.

**Boundary.** OpenRaft owns what `RaftCore` does with a completion result
(constitution VII). This spec owns only what hiqlite reports to it, and makes no
claim about the resulting cluster behavior.

## 3. Behavior

### 3.1 The three notified outcomes

`Action::Append` carries `AppendCompletion`, a
`Box<dyn FnOnce(Result<(), io::Error>) + Send>`. For every append the writer
loop dispatches to `complete_append`, exactly one of these MUST be delivered:

| Path | Acknowledgement | Notification | Writer |
|---|---|---|---|
| Accepted, persistence step succeeded | `Ok(())` | `Ok(())` | continues |
| Rejected (append error) | `Err(cause)` | `Err(cause)` | continues, unless the persistence step also failed |
| Accepted, persistence step failed | `Ok(())` | `Err(cause)` | terminates |

The notification MUST carry the underlying cause. An `Error::IO` MUST keep its
`io::ErrorKind`; every other variant MUST become `ErrorKind::Other` carrying the
same `Display` text. The acknowledgement channel keeps the typed `Error`, and
because `Error` is not `Clone` the notification carries a reproduction of the
same cause rather than the same value.

Where an append is both rejected and followed by a failing persistence step, the
notification MUST name the rejection. That is the cause the caller was already
given on the acknowledgement channel, and reporting two different causes for one
append is what this repair exists to prevent.

### 3.2 Ordering, unchanged

`001` section 2's ordering is preserved and restated: the acknowledgement MUST
be sent before the mode-specific persistence step, and the notification MUST
follow that step. A caller that returns on the acknowledgement has not waited
for persistence.

The persistence step MUST still run for a rejected append. Bytes written before
the rejection are already in the active mapping and `is_dirty` is already set,
so skipping it would change durability behavior, which this repair does not do.

### 3.3 Notification cardinality

On the paths in section 3.1, the notification MUST be delivered exactly once.

`LogFlushed::log_io_completed` consumes `self` and `AppendCompletion` is a
`FnOnce`, so **at most once** holds by construction. **At least once** does not:
a path that returns or drops the callback before invoking it delivers zero, and
zero is exactly what F-002 was. Exactly-once is therefore an implementation
obligation with test evidence (section 4), not a property the types guarantee.

The obligation is scoped to the three paths above: an append the writer loop
dispatched to `complete_append`. Section 5 names the paths that are outside it.

### 3.4 Per-mode statements

Constitution IX requires each durability statement to name its configuration.

- **`LogSync::Immediate`.** The persistence step is `flush_blocking`. A failure
  here is a genuine durability failure: the entries were acknowledged and the
  flush did not return successfully. The notification carries that cause and the
  writer terminates.
- **`LogSync::ImmediateAsync`.** The persistence step is `flush_async`, which
  only *starts* writeback. A failure means writeback could not be started. A
  success notification means writeback was requested and MUST NOT be read as
  stable storage, and a later loss can still occur with no error on this path.
- **`LogSync::IntervalMillis(ms)`.** There is no per-append persistence step, so
  there is no failure to report. Exactly one success notification MUST still be
  delivered per append, and it means only that the bytes entered the mapping.

### 3.5 The adapter boundary

`hiqlite-wal/src/log_store_impl.rs`'s `RaftLogStorage::append` MUST forward the
writer's result verbatim:
`Box::new(move |res| callback.log_io_completed(res))`. This closure is the
OpenRaft error boundary and is where the defect lived; it MUST stay a pure
forward with no substituted value.

This is a requirement on the adapter, not a guarantee of `AppendCompletion`'s
signature. The `Result` parameter makes the failure expressible; a closure may
still discard its argument, and baseline B in section 4.2 and 4.4 is exactly
such a closure. What holds the requirement is the adapter-boundary test.

### 3.6 Writer termination and its report

The failure policy is unchanged: a persistence failure still propagates out of
the writer loop and ends the writer thread, after the notification has been
delivered. This spec introduces no surviving poisoned writer, no automatic
recovery, no restart policy, and no process abort.

**A termination the writer can describe MUST be reported, and that is exactly
`run` returning `Err`.** The mechanism is a single `ERROR` log emitted where the
thread closure observes that `Err`, naming the WAL base path and the cause. The
thread's `JoinHandle` is deliberately **not** retained: retaining or joining it
would be lifecycle management, and nothing in the crate manages that lifecycle.
What was missing was the report, and the log is the smallest mechanism that
observes that exit.

**The limit of the claim.** This is error reporting, not process supervision,
and the two are not the same set of events:

- `run` returning `Err` is reported. This is the path the repair is about, and
  a persistence failure reaches it.
- A **panic** inside `run` unwinds past the closure without producing an `Err`,
  so it is reported by the process panic hook and not by this log. The existing
  `oversized_entry_panics_and_kills_writer` test pins a panicking termination
  that this report does not cover.
- An **abort, a signal, or any other process termination** is reported by
  neither.

This spec deliberately does not widen the runtime to close that gap: catching
panics or supervising the thread is lifecycle machinery and a separate decision
(section 6). The requirement above is scoped to what the implementation actually
observes, so that no reader takes the log's absence as evidence that the writer
is alive.

### 3.7 Public API surface

`Action` is re-exported from `hiqlite-wal`'s crate root, so changing its
`callback` field type is a **breaking change to `hiqlite-wal`'s public API**.
`AppendCompletion` is exported alongside it so the type is nameable. The only
consumer in this repository is a test helper in
`hiqlite/src/store/state_machine/sqlite/state_machine.rs`, adapted in the same
change. `hiqlite`'s own public API is unchanged.

No version bump and no publication is part of this change; `hiqlite-wal` stays
at `0.14.0` here and the release decision belongs to the maintainer.

## 4. Evidence and its limits

### 4.1 What the writer-level tests establish

`append_rejection_notifies_error_and_never_success`,
`persistence_failure_notifies_error_before_propagating`, and
`rejection_takes_precedence_over_a_failing_persistence_step` drive
`complete_append` directly with an injected result and an injected persistence
outcome. They establish the notified value, its cause text, and that exactly one
notification is produced. They do not exercise the writer loop.

`success_notifies_once_per_append_in_every_log_sync_mode` and
`persistence_failure_notifies_then_terminates_the_writer` drive a real writer
thread through `Action::Append`. The first covers all three `LogSync` modes on
the success path; the second covers the acknowledged-then-failed path.

**How the end of service is observed, and how precisely.** The second test
asserts that the writer's `flume::Sender` becomes disconnected. `run` owns the
only `Receiver` for that channel and holds it for as long as it is on the stack,
so a disconnect establishes positively that `run` has left and the writer can
receive no further action. It does **not** establish that the writer's OS thread
has finished: the thread closure runs its error report after `run` returns, so
the thread is still live at the moment the disconnect becomes observable. The
claim carried here is the one the observation supports: the sole `Action`
receiver is gone and no further append can be received.

It begins with a healthy append, so the disconnect is a change of state rather
than a condition that was already true. That healthy append is synchronized on
its **completion notification**, not on its acknowledgement: `complete_append`
sends the acknowledgement before the persistence step, so arming a fault
injection on the acknowledgement would leave the healthy append's `persist` call
still ahead of the writer and able to consume the injection armed for the next
append. Waiting for `Ok(())` on the healthy notification places the arming
strictly after that append's persistence step returned, and the writer serves
actions one at a time, so the injection can only be taken by the append it was
armed for. The absence of an acknowledgement for a later append is asserted
afterwards as the consequence of the policy, not as the proof of the end of
service: not observing a reply within a budget is consistent with a writer that
is merely slow, and on its own establishes nothing.

**Where ordering is established and where cardinality is.** The two are proved
in different places on purpose. `complete_append` is the single ordering point
for every mode: the acknowledgement, the persistence step, and the notification
are sequenced there and nowhere else, so the mode does not change the order and
the helper-level test proves it for all three. It proves it deterministically,
by observing both channels from *inside* the injected persistence step, which a
test outside the writer thread cannot do without racing it. The per-mode test
therefore establishes what only a live writer can: that each mode's real
persistence step produces exactly one success notification per append, three
appends running. Reading the per-mode test as an ordering proof would overstate
it: awaiting the acknowledgement before the notification does not establish
that the notification was not already queued.

`writer_termination_is_reported` captures `tracing` `ERROR` output and asserts
the report names the WAL that died. It establishes that the report is emitted on
the `run`-returns-`Err` path, not that any operator consumes it and not that any
other kind of termination is reported (section 3.6).

`injections_are_isolated_per_wal_and_consumed_exactly_once` and
`a_dropped_guard_disarms_an_unconsumed_injection` are about the test harness
rather than the writer: they establish that two WALs can hold outstanding
injections at once, that each consumes exactly its own, that an unarmed WAL
consumes none, and that an unconsumed injection does not outlive its guard. Both
are direct calls with no threads and no timing budget, so their result does not
depend on scheduling or test order. They establish nothing about the writer
loop; that is what the live-writer tests above are for. D-3 records why the
mechanism needed repairing.

### 4.2 What the adapter-boundary test establishes, and why that boundary

Writer-level testing cannot prove correct forwarding through the adapter: the
defect was in the adapter, and a writer test never executes
`log_io_completed`.

`append_adapter_forwards_a_persistence_failure_to_openraft` therefore drives the
real `RaftLogStorage::append` on a real `LogStore` over a real WAL directory,
through `openraft::storage::RaftLogStorageExt::blocking_append`. That is
OpenRaft's own public wrapper: it constructs a real `LogFlushed`, calls
`append`, and awaits the real completion oneshot, mapping both a dropped sender
and a notified `Err` into a `StorageError`. The test asserts the returned error
names the injected cause.

This is the test that catches an adapter hardcoding `Ok(())`. With the writer
reporting correctly, a hardcoded success makes that append return `Ok` although
the flush failed, and the assertion fails. Demonstrated at section 4.4.

**The type does not make this test redundant.** `AppendCompletion`'s
`Result<(), io::Error>` parameter makes the failure *expressible*, which the
previous `FnOnce()` did not. It does not make ignoring the failure
unrepresentable: a closure may discard its argument, and baseline B in section
4.4 is exactly such a closure, compiling cleanly and reporting success for a
failed append. Correct forwarding is therefore a property of the adapter, held
by this test, and not a guarantee of the signature.

**Why this boundary and not a Raft node.** `LogFlushed::new` is `pub(crate)` in
the locked openraft (0.9.24, and identically in 0.9.25), so no downstream test
can construct the callback directly. The only other route to a real one is a
running `Raft`, which would add a network and state-machine stack and could not
inject a deterministic flush failure at a known append. `blocking_append` is the
smallest construction that uses the real type and the real channel.

**What it does not establish.** It does not establish what `RaftCore` does with
an error notification. That is OpenRaft's contract, and claiming it from a local
control-flow read would violate constitution VII.

### 4.3 Scheduling-dependent outcomes on the rejection path

On the rejection path the adapter's `append()` returns `Err`, and
`RaftCore::append_to_log` takes its `?` before awaiting the completion receiver.
The writer thread invokes the notification independently, so two orderings are
possible and neither is guaranteed:

- the notification's send succeeds because the receiver is still alive, and
  `RaftCore` never reads the value it holds; or
- the receiver was already dropped, the send fails, and openraft logs "failed to
  send log io completion event" (`storage/callback.rs`).

`RaftCore`'s outcome is the same either way: the error returned by `append()`.
An earlier draft of the proposal claimed the failed-send log as a guaranteed
consequence. It is not; it is one of two scheduling-dependent outcomes and is
recorded here as such.

### 4.4 The before-state demonstration

Required for a behavioral repair spec. All three baselines were built by
reverting behavior while keeping the post-repair API, so the observed failures
are behavioral and not build breaks.

**Baseline A, pre-repair writer behavior** (`complete_append` sending the ack,
then `persist()?`, then `callback(Ok(()))`; the thread closure discarding
`run`'s result). Six tests failed:

```
log_store_impl::tests::append_adapter_forwards_a_persistence_failure_to_openraft
writer::tests::append_rejection_notifies_error_and_never_success
writer::tests::persistence_failure_notifies_error_before_propagating
writer::tests::persistence_failure_notifies_then_terminates_the_writer
writer::tests::rejection_takes_precedence_over_a_failing_persistence_step
writer::tests::writer_termination_is_reported
```

The adapter test's failure message on baseline A is the traced F-002
consequence, observed rather than inferred:
`openraft must receive the underlying cause, not a closed-channel error; got:
when Write Logs: channel closed`.

**Baseline B, repaired writer with the adapter defect alone** (the adapter boxed
as `move |_res| callback.log_io_completed(Ok(()))`). This isolates the hardcoded
`Ok(())` from the writer behavior and confirms the adapter test is what catches
it.

**Baseline C, the single-slot fault mechanism** (`writer::fault` holding one
`Mutex<Option<String>>`, with the guard API kept so the failure is behavioral
and not a build break). This is the defect D-3 records, and the isolation
regression is what catches it.

Baseline A was run with
`cargo +1.95.0 test -p hiqlite-wal --lib --features oversized-entry-error` and
reported `17 passed; 6 failed`. Baseline B was run with
`cargo +1.95.0 test -p hiqlite-wal --lib log_store_impl` and reported
`0 passed; 1 failed`, the failure being
`a persistence failure must reach openraft as an error: ()`, which is the
hardcoded success arriving where an error was required. Baseline C was run with
`cargo +1.95.0 test -p hiqlite-wal --lib writer::tests::injections_are_isolated_per_wal_and_consumed_exactly_once -- --exact`
and reported `0 passed; 1 failed`, the failure being
`arming a second WAL must not discard the first WAL's injection: left: 0, right: 1`,
which is the overwrite itself rather than a downstream symptom of it.

After restoring the repair, `cargo +1.95.0 test -p hiqlite-wal --lib` reports
`24 passed; 0 failed` and the same run with `--features oversized-entry-error`
reports `25 passed; 0 failed`. The repair pull request carries the same record.

**Replaced tests.** Two of `001`'s acceptance commands named tests that asserted
the defective behavior and cannot survive the repair:

- `append_failure_is_returned_but_completion_still_fires` pinned F-001. Replaced
  by `append_rejection_notifies_error_and_never_success`.
- `persistence_failure_suppresses_completion_callback` pinned F-002's
  suppression. Replaced by `persistence_failure_notifies_error_before_propagating`.

`append_result_precedes_persistence_and_completion` is kept: the ordering it
pins is unchanged, and only its callback signature was adapted. `001`'s four
further commands (`reader`, `metadata`, and the two `wal` roll-over tests) are
untouched by this repair as well, so five of its seven are unaffected. All five
are carried forward verbatim into this spec's block, which is now `001`'s
acceptance (D-4). `001`'s own file,
including its `## Verification` section, is byte-identical to what it was before
this repair.

`append_adapter_reports_a_rejected_append_as_an_error` is **not** a fail-then-pass
regression. A rejection reached the caller before the repair too, on the
acknowledgement path. It is characterization: it pins that the adapter never
turns a rejection into a successful storage call.

## 5. Known defects

**KD-1. Pre-dispatch failures are not covered and notify nothing.** If
`send_async(Action::Append)` fails, the boxed callback is dropped with the
returned `SendError` and is never invoked; OpenRaft resolves the completion
receiver as a `RecvError`. The same holds if an entry send fails before the
writer receives the action. Section 3.3's exactly-once obligation deliberately
does not extend to these paths, because they were not traced here.

**KD-2. A truncated entry stream is notified as success.** The writer's
collection loop is `while let Ok(Some(..)) = rx.recv()`, so a disconnect is
indistinguishable from a normal end of stream: `res` stays `Ok`, and the append
is acknowledged and notified as a success even though not every entry arrived.
This is pre-existing, is outside F-001 and F-002, and is left unrepaired here.
Registered as F-028.

**KD-3. The report has no consumer, and it does not cover every termination.**
A `run`-returns-`Err` termination is logged and nothing acts on it. The node's
log store is dead until restart, which is the preserved policy, but no health
check is wired to the report, and a panicking or aborted writer produces no such
log at all (section 3.6). Absence of the report is therefore not evidence that
the writer is alive.

**KD-4. The durability boundary is still unvalidated.** `001` section 8 bullet 3
stands unchanged: no power-cut harness exists, so `Immediate`'s flush is tested
for call ordering and error propagation only.

## 6. Out of scope

- **Option (b) of the proposal**, a surviving writer with a defined poisoned
  state, and any automatic recovery, restart policy, or process abort. The owner
  decided to preserve the existing termination.
- **Runtime supervision of the writer thread.** Catching panics, retaining and
  joining the `JoinHandle`, or wiring a health check to the report are lifecycle
  machinery, and none is added here. Section 3.6 states the reporting claim
  narrowly instead of widening the runtime so that a broader claim could be
  made.
- **F-021 through F-024**, the cache-log defects. They are a separate repair and
  are deliberately not bundled here.
- **F-003 through F-020, F-025 through F-027**, and everything in `002` and
  `003`.
- **Ratification and merge.** This spec is `draft`, like every spec in the
  corpus.

## 7. Resolved decisions

**D-1 (2026-09-19, the notification carries `io::Error`, not `Error`).** The
writer performs the mapping and the adapter forwards verbatim. The alternative,
passing the crate's `Error` and mapping in the adapter, leaves a substituted
value in exactly the closure the defect lived in. Forwarding verbatim makes the
untested residue a one-line pure forward.

**D-2 (2026-09-19, termination is reported by log, not by a retained
`JoinHandle`).** The proposal's option (c) suggested surfacing the thread's
`Err` through its join handle. Error reporting and lifecycle management are
different concerns: nothing joins this thread, and adding a handle to hold would
be lifecycle machinery in service of a report. The log is smaller and observes the
termination the writer can describe, which is `run` returning `Err`; section 3.6
states what it does not cover.

**D-3 (2026-09-19, fault injection is `cfg(test)`-only and isolated per WAL,
with guarded ownership).** `writer::fault` compiles out entirely outside tests,
so the writer carries no production branch for it.

Injections are held per WAL base path and owned by the guard that armed them. A
single shared slot was not enough, and the first implementation here had that
defect: arming a second WAL overwrote an unconsumed injection armed for a first
one, and the path comparison at consumption could only turn that into a silently
missed injection, never restore it. That is a real interleaving, not a
theoretical one, because the tests that arm an injection are `#[tokio::test]`
functions the harness may run concurrently. The remedy is isolation by WAL
identity, not a stress loop, a rerun, or `--test-threads=1`: serializing the
suite would hide the defect rather than remove it, and would leave it live for
any future test that arms two WALs at once.

`ArmedPersistenceFailure` owns the outstanding count for its path and disarms
what its WAL did not consume when it drops, so an injection cannot survive into
a later test that reuses the directory.
`writer::tests::injections_are_isolated_per_wal_and_consumed_exactly_once` arms
two distinct paths before either is consumed and asserts that each receives its
own injection exactly once and that an unarmed path receives none;
`a_dropped_guard_disarms_an_unconsumed_injection` covers the cleanup. Both are
direct calls with no threads, so neither depends on scheduling or test order.
Evidence limit: they establish the isolation property of the mechanism, not
anything about the writer loop, which the live-writer tests cover. The isolation
is also within one test process. The WAL directories these tests use are fixed
relative paths under `test_data/`, so two `cargo test` processes running this
crate at once still collide on disk; that is a pre-existing property of the
crate's test fixtures, not of the injection store, and the acceptance block runs
its commands one at a time.

**D-4 (2026-09-19, `001`'s acceptance is replaced through `amends_verification`;
`001`'s file is not edited).** The pinned revision
`aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f` supports acceptance replacement. A
live spec that `amends` another may also name it in `amends_verification`, and
`spec-spine verify <amended-id>` then builds its plan from the **amending**
spec's `## Verification` section and prints an attribution line saying whose
block it ran. Confirmed against the installed `spec-spine 0.20.0` on this
corpus: `spec-spine verify 001` reports
`acceptance amended by 008-wal-append-completion-notification`.

The semantics that bind this spec, read from the tool at that revision:

- **The unit is the whole block, not a command.** `001`'s own section is not
  read at all once this spec holds it. Every obligation `001` declared is
  therefore carried forward into this spec's block explicitly, including the
  five commands the repair does not touch. Replacing `001`'s acceptance with
  only the new append tests would silently drop them.
- **`draft` is not an obstacle.** Only a `superseded` or `retired` amender is
  skipped, which returns the acceptance to whatever held it before. Every spec
  in this corpus is `draft`, and a draft amender holds the acceptance.
- **The declaration is validated.** An `amends_verification` entry that is not
  also in `amends` is refused (`V-018`); two live specs naming the same target
  are refused (`V-019`); a cycle in the replacement chain is refused (`V-020`).
  This spec declares both edges on `001` and is the only spec naming it.
- **Resolution reads the corpus, not `.derived/`**, so `verify` still works on a
  tree whose shards are stale.

**An earlier draft of this decision was wrong and is withdrawn.** It recorded
that the pinned tool cannot express acceptance replacement and, on that basis,
edited `001`'s executable block as a self-issued exception to the contract's
"the amended `spec.md` is not edited" rule. The probe behind it did not find
`amends_verification`, which has been in the tool since well before the pinned
revision. No exception was needed and none is claimed: `001`'s `## Verification`
section is byte-identical to what it was before this repair, and the supported
mechanism carries the replacement. The recommendation that a later pin add such
a feature is withdrawn with it.

Separately probed on the same revision, and unchanged: `amends_sections` is
recorded verbatim and is **not** resolved against the amended document. A bogus
anchor compiles and lints clean. The three values in this spec's frontmatter are
a reader's pointer into `001`, not a checked reference, and no gate will catch it
if `001` is later re-headed.

## Verification

Run with `just spine-verify 008`. **This block is `001`'s acceptance as well as
this spec's** (D-4). `001`'s own file is not edited, and `spec-spine verify 001`
prints the attribution line naming this spec before it runs a command.

Replacement is whole-block: the mechanism's unit is the `## Verification`
section, not a command. Every obligation `001` declared is therefore carried
forward here explicitly. The first group is `001`'s block with its two obsolete
commands replaced and nothing else changed; the second group is what this repair
adds. Each command names one test and fails if the behavior it names changes.

Every command below was confirmed to select exactly one test and run it: an
`--exact` filter that matches nothing exits `0` having run zero tests, which is
not acceptance evidence, so the reported `1 passed` was read for each line
rather than the exit code alone.

```verify:cli
# --- 001's acceptance, carried forward ---
cargo test -p hiqlite-wal --lib writer::tests::append_result_precedes_persistence_and_completion -- --exact
# was persistence_failure_suppresses_completion_callback, which pinned F-002's suppression
cargo test -p hiqlite-wal --lib writer::tests::persistence_failure_notifies_error_before_propagating -- --exact
# was append_failure_is_returned_but_completion_still_fires, which pinned F-001
cargo test -p hiqlite-wal --lib writer::tests::append_rejection_notifies_error_and_never_success -- --exact
cargo test -p hiqlite-wal --lib reader::tests::logs_action_reports_read_errors -- --exact
cargo test -p hiqlite-wal --lib metadata::tests::metadata_overwrite_replaces_existing -- --exact
cargo test -p hiqlite-wal --lib wal::tests::roll_over_purge_front -- --exact
cargo test -p hiqlite-wal --lib wal::tests::roll_over_truncate_end -- --exact
# --- what this repair adds ---
cargo test -p hiqlite-wal --lib writer::tests::rejection_takes_precedence_over_a_failing_persistence_step -- --exact
cargo test -p hiqlite-wal --lib writer::tests::success_notifies_once_per_append_in_every_log_sync_mode -- --exact
cargo test -p hiqlite-wal --lib writer::tests::persistence_failure_notifies_then_terminates_the_writer -- --exact
cargo test -p hiqlite-wal --lib writer::tests::writer_termination_is_reported -- --exact
cargo test -p hiqlite-wal --lib writer::tests::injections_are_isolated_per_wal_and_consumed_exactly_once -- --exact
cargo test -p hiqlite-wal --lib writer::tests::a_dropped_guard_disarms_an_unconsumed_injection -- --exact
cargo test -p hiqlite-wal --lib log_store_impl::tests::append_adapter_forwards_a_persistence_failure_to_openraft -- --exact
cargo test -p hiqlite-wal --lib --features oversized-entry-error log_store_impl::tests::append_adapter_reports_a_rejected_append_as_an_error -- --exact
```
