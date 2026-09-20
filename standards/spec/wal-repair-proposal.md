# WAL append and completion error contract: repair proposal

A source-backed proposal for repairing F-001 and F-002. **Nothing here is
implemented**, and this document changes no runtime behavior. It exists so the
repair can be authorized against a traced contract rather than a guess, and so
the one remaining design decision is visible before anyone writes code.

Traced on 2026-09-19 against the locked dependency graph. Where this document
says "observed", it was read at source; where it says "inferred", a consequence
follows from control flow that was **not executed**.

---

## 1. The pinned contract, traced

**Version.** `hiqlite/Cargo.toml` requests `openraft = { version = "0.9.21",
features = ["serde", "storage-v2"] }`, a caret requirement. `Cargo.lock` resolves
it to **0.9.24**, which is the version this proposal is written against. The
0.9.25 source is also present in the local registry and is identical on every
path below; the difference does not matter here, but the requested, locked, and
read versions are three different numbers and are named separately on purpose.

**What OpenRaft asks of an append.** `RaftCore::append_to_log`
(`openraft-0.9.24/src/core/raft_core.rs`):

```rust
let (tx, rx) = C::AsyncRuntime::oneshot();
let log_io_id = LogIOId::new(vote, Some(last_log_id));
let callback = LogFlushed::new(log_io_id, tx);

self.log_store.append(entries, callback).await?;      // (1)
rx.await
    .map_err(|e| StorageIOError::write_logs(AnyError::error(e)))?   // (2) sender dropped
    .map_err(|e| StorageIOError::write_logs(AnyError::error(e)))?;  // (3) callback Err
Ok(())
```

**What the callback is.** `openraft-0.9.24/src/storage/callback.rs`:

```rust
pub struct LogFlushed<C> { log_io_id: LogIOId<C::NodeId>, tx: OneshotSenderOf<C, Result<LogIOId<C::NodeId>, io::Error>> }

/// Report log io completion event.
/// It will be called when the log is successfully appended to the storage or an error occurs.
pub fn log_io_completed(self, result: Result<(), io::Error>) { ... }
```

Four properties follow, and all four matter to the repair:

1. **The error channel already exists.** `log_io_completed` takes
   `Result<(), io::Error>` and forwards `Err(e)` to `RaftCore` at (3). hiqlite is
   not missing an OpenRaft capability; it is discarding one.
2. **Cardinality is exactly one, enforced by the type.** `log_io_completed`
   takes `self` by value over a oneshot sender, so it cannot be called twice.
   Calling it zero times is possible and is what F-002 does.
3. **Dropping the callback is a third signal.** If `LogFlushed` is dropped
   without being called, its `tx` drops and (2) fires: `RaftCore` gets a
   `StorageIOError` built from a channel-receive error, with no underlying cause.
4. **A returned `Err` short-circuits the callback entirely.** The `?` at (1)
   returns before `rx.await`, so a callback fired after a failed `append()` has
   no receiver.

## 2. Where hiqlite loses the error

**The adapter.** `hiqlite-wal/src/log_store_impl.rs:199-234`:

```rust
let callback = Box::new(move || callback.log_io_completed(Ok(())));   // line 214
self.writer.send_async(writer::Action::Append { rx, callback, ack }).await ...
// stream entries, then:
ack_rx.await.unwrap().map_err(|err| StorageIOError::write_logs(&err))?;
Ok(())
```

Line 214 is the root cause. The writer's `Action::Append` carries
`callback: Box<dyn FnOnce() + Send>` (`hiqlite-wal/src/writer.rs:21`), a type
with **no result parameter**, and the adapter hardcodes `Ok(())` when boxing it.
Even a writer that knew the append had failed could not say so through this
channel.

**The writer.** `hiqlite-wal/src/writer.rs:174-191`:

```rust
fn complete_append<F>(append_result: Result<(), Error>, ack: oneshot::Sender<Result<(), Error>>,
                      callback: Box<dyn FnOnce() + Send>, persist: F) -> Result<(), Error>
where F: FnOnce() -> Result<(), Error> {
    if let Err(err) = ack.send(append_result) { error!(...); }   // note: sends Err too
    persist()?;
    callback();
    Ok(())
}
```

and the call site at `writer.rs:307-315`, inside `run()` (`writer.rs:211`, a
`-> Result<(), Error>` loop on a dedicated OS thread spawned at `writer.rs:135`
whose `JoinHandle` is discarded):

```rust
complete_append(res, ack, callback, || {
    if sync == LogSync::Immediate { flush_blocking(&mut wal, &mut buf, &mut is_dirty)?; }
    else if sync == LogSync::ImmediateAsync { wal.active().flush_async()?; }
    Ok(())
})?;
```

## 3. The two failures, end to end

### F-001, append rejection

Observed: `ack.send(append_result)` forwards the `Err`, then `persist()` runs and
`callback()` fires `log_io_completed(Ok(()))` unconditionally. The test at
`writer.rs:483-496` pins this.

Inferred: the adapter maps the ack error and returns `Err` from `append()`, so
`RaftCore` takes the `?` at (1) and drops `rx`. The success callback then finds
no receiver, and `log_io_completed` logs "failed to send log io completion event"
(`callback.rs:41-44`). **`RaftCore`'s view of the log is not corrupted in this
version.** The defect is a callback contract that cannot express failure plus a
misleading log line, not a divergent log state. An earlier finding claimed the
latter; that claim is withdrawn.

### F-002, persistence failure after acknowledgement

Observed: the ack was already sent as `Ok`, so `append()` returned `Ok`.
`persist()?` then returns `Err` out of `complete_append`, and the `?` at
`writer.rs:315` returns it out of `run()`.

Inferred: `run()` returning ends the writer's OS thread; its `JoinHandle` was
discarded at line 135, so the underlying `Error` is **never surfaced anywhere**.
The boxed callback is dropped with the loop, so `LogFlushed` drops, so `RaftCore`
resolves at (2) with a `RecvError` mapped to `StorageIOError::write_logs`.
OpenRaft learns that something failed and never learns what. Every later append
then fails because the writer channel has no receiver: the node's log store is
dead until restart.

**This is the more serious of the two**, and it was previously recorded as "the
completion event is lost", which understated the thread death and overstated the
silence toward OpenRaft.

## 4. Proposed repair

### 4.1 Success completion and error notification are different responsibilities

The repair turns on one distinction the current code does not make:

- **Success completion** says the entries reached the durability level that this
  `LogSync` mode promises. It is the only thing that may produce
  `log_io_completed(Ok(()))`.
- **Error notification** says the append or its persistence failed, and carries
  the cause. It is `log_io_completed(Err(io_error))`, and today it never happens.

Both are the adapter's responsibility, because only the adapter holds the
`LogFlushed`. The writer's responsibility is to report which of the two occurred,
which it currently cannot.

### 4.2 API change, the smallest that closes the hole

Change the callback carried by `writer::Action::Append` from
`Box<dyn FnOnce() + Send>` to a result-carrying form, for example
`Box<dyn FnOnce(Result<(), Error>) + Send>`, and have the adapter box it as
`move |r| callback.log_io_completed(r.map_err(into_io_error))`.

This is internal to `hiqlite-wal` plus its one adapter, changes no public hiqlite
API, and makes the three outcomes expressible. `complete_append` then becomes:
on append rejection, send the ack and invoke the callback with the error; on
persistence failure, invoke the callback with the error before propagating; on
success, invoke it with `Ok(())` exactly once.

### 4.3 Cardinality and writer behavior after failure

`LogFlushed::log_io_completed` consumes `self`, so the repair must guarantee
**exactly one** call per append. The failure mode to avoid is a path that both
notifies and drops, or that returns early before notifying. The repair therefore
notifies before propagating any error out of `complete_append`.

What the writer does **after** a persistence failure is the open decision. Three
options, with the trade-off stated rather than chosen:

- **(a) Keep exiting the loop, but notify first.** Smallest change. Preserves
  today's fail-stop behavior; the node's log store stays dead until restart, but
  OpenRaft now receives the real cause instead of a `RecvError`.
- **(b) Keep the loop alive and fail subsequent appends explicitly.** The writer
  survives, reports the error per append, and the node degrades instead of
  silently losing its writer thread. More behavior change, and it needs a defined
  poisoned state.
- **(c) Exit, and surface the thread's `Err` through the join handle** so the
  process learns its log writer died. Orthogonal to (a) and (b), and closes the
  "discarded at line 135" hole independently.

**Recommendation: (a) plus (c)** for the repair, with (b) deferred. (a) and (c)
restore correct notification without changing the failure policy, which is what
the defect is actually about; (b) changes what the node does after a durability
failure and deserves its own decision and its own evidence.

### 4.4 LogSync configurations

`LogSync::Immediate` uses `flush_blocking` and is where a persistence failure is
a genuine durability failure: the entries were acknowledged and are not on disk.
`LogSync::ImmediateAsync` calls `flush_async`, which only starts writeback, so an
error there means writeback could not be started; a later loss can still occur
without any error on this path, and the repair must not imply otherwise. Any
third mode that does no persistence at all has no failure to report and must
still produce exactly one success completion. The amending spec states each mode
separately, as constitution IX requires.

### 4.5 Regression tests and the smallest sufficient boundary

Three tests, all in `hiqlite-wal`, no cluster run:

1. **Append rejection notifies an error.** Replaces
   `append_failure_is_returned_but_completion_still_fires` (`writer.rs:483-496`),
   which asserts the defect. The replacement asserts the callback receives the
   error and that no success is reported. The amending spec names the retired
   test rather than deleting it silently.
2. **Persistence failure notifies an error, through the loop.** The existing
   `persistence_failure_suppresses_completion_callback` (`writer.rs:465-479`)
   exercises `complete_append` directly and so cannot see the loop exit. The new
   test drives an `Action::Append` through `run()` with an injected blocking
   flush failure and asserts exactly one error notification.
3. **Success still notifies exactly once**, per `LogSync` mode.

**Smallest sufficient integration boundary.** The writer loop, not the OpenRaft
adapter. Everything the repair changes is observable at `Action::Append` in and
callback out; driving it through `RaftLogStorage::append` would add a Raft type
config and prove nothing extra about the notification. Whether `RaftCore` then
behaves well on an error notification is OpenRaft's contract, not hiqlite's, and
must not be claimed as hiqlite evidence (constitution VII).

**Demonstration required.** Tests 1 and 2 must be shown failing against the
current implementation and passing after the repair, and the spec records where
that was demonstrated. Test 1 is a true fail-then-pass because the old behavior
is pinned by the test it replaces. This is the repair standard, distinct from the
characterization standard that retroactive adoption specs use.

## 5. Ownership, amendment, and acceptance relationships

`001-wal-durability-and-completion` owns `hiqlite-wal/src/` as a `directory`
unit and `hiqlite/src/config.rs` as a `file` unit. `hiqlite-wal/src/writer.rs`
and `hiqlite-wal/src/log_store_impl.rs` both fall inside that directory claim,
so the repair touches no unclaimed territory.

- **Edge: `amends: ["001-wal-durability-and-completion"]`**, with
  `amends_sections` naming the append-events and known-defects anchors. This is
  prescribed by the spec being amended: `001` section 8 states that future work
  "MUST amend this contract rather than silently rewriting its baseline". Per
  `000` section 4, `amends` resolves to spec ids and grants co-authority over the
  amended `spec.md`, which is what lets the repair restate the append contract
  without rewriting `001` in place.
- **Not `establishes`.** `000` section 4 allows one origin per unit and `001`
  holds it. **Not `supersedes`**: `001`'s snapshot, vote, truncation, purge, and
  recovery contracts are unaffected and must keep their authority.
- **`extends` is not needed** for the WAL directory, since `amends` already
  grants co-authority over the amended spec's territory for the amended
  sections. If review prefers an explicit unit-level claim, `extends` on
  `{ kind: directory, path: "hiqlite-wal/src/" }` with `nature: superseding` is
  the narrower alternative, and either satisfies the coupling gate.
- **`001` is not edited to record that it was amended.** `000` section 4 and the
  contract both say the amended `spec.md` stays the contract as it stood; the
  inbound view is `spec-spine registry relationships 001-wal-durability-and-completion`.
- **Replacement acceptance.** `001`'s acceptance block names
  `append_failure_is_returned_but_completion_still_fires`. When that test is
  retired, `001`'s block would fail, so the amending spec must carry the
  replacement acceptance and state that it supersedes that line. This is a real
  coupling between the two specs' verification blocks and is the main reason the
  repair cannot be done as an unattributed code change.

## 6. What is deliberately not decided here

- **Whether the writer survives a persistence failure** (section 4.3, option b).
  This is a durability policy decision for the owner, and the proposal is
  explicitly structured so the notification repair does not wait on it.
- **The exact `Error` to `io::Error` mapping.** It must preserve the cause;
  which variant carries what is an implementation detail for review.
- **Everything else in the register.** F-003 through F-008, the power-cut
  harness, and anything in `002` or `003` stay out.
