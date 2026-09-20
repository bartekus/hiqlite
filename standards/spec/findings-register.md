# hiqlite findings register

A source-backed register of what this fork knows about its own defects, gaps,
and undecided contracts, produced by a bounded whole-project assessment on
2026-09-19 against `spec-spine` revision
`aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f`.

This document is a **record, not a mandate**. It schedules nothing and changes
no behavior. Entries carry stable identifiers so a later spec can cite one
instead of restating it, and so a repair can be reviewed against a fixed
baseline. Identifiers are never reused; a finding that turns out to be wrong is
marked withdrawn in place, with the reason.

**Classes.** `defect` is a mismatch confirmed at source between what the code
states or promises and what it does. The existence of a better design is not a
defect: something must actually disagree. `contradiction` is two authored texts,
or a text and the code, making incompatible claims; an incomplete claim is not a
contradiction. `gap` is territory inside the intended adoption scope that no
spec claims yet, which is migration state, not a fault. `evidence` is a claim
whose support is thinner than a reader would assume. `limit` is a deliberate
boundary with a stated consequence. `decision` is an open design or ownership
question that no document answers.

**Confidence.** `high` means read at the cited source or asserted by a passing
test. `medium` means read at source with a consequence inferred from
configuration or control flow that was not executed. `low` means indicated but
not confirmed; no `low` finding is a basis for action on its own.

**What this register is not.** An absent test is not proof of a runtime defect,
and it is recorded as `evidence`, never as `defect`. A passing test and an
existing claim are not proof of correctness. OpenRaft owns the consensus
algorithm and an external state-machine caller owns its own consensus log,
membership, and replay decisions (constitution VII and VIII); findings below
address only hiqlite's side of those boundaries.

---

## Carried forward from the pilot specs

These were recorded by `001` through `003` and are restated here with source
references so later work can cite a stable id. The specs remain authoritative.

### F-001 `defect`, confidence `high`

**A failed append still fires the completion callback.**
`hiqlite-wal/src/writer.rs:174-191`. `complete_append` sends `append_result` on
the ack channel, then runs `persist()?` and calls `callback()`. Nothing branches
on `append_result` being `Err`, so a rejected append acknowledges the error to
the caller and reports completion to OpenRaft.

The root cause is at the adapter boundary: `hiqlite-wal/src/log_store_impl.rs:214`
builds the callback as `Box::new(move || callback.log_io_completed(Ok(())))`, a
`Box<dyn FnOnce() + Send>` that hardcodes success and has no way to carry a
result. The writer therefore could not report an error through it even if it
branched on one.

Configurations: all `LogSync` modes.

**Consequence, traced rather than assumed.** An earlier revision of this entry
said the two events leave OpenRaft disagreeing about whether the log advanced.
Tracing the locked OpenRaft (0.9.24 per `Cargo.lock`; `Cargo.toml:78` requests
`"0.9.21"`) shows a narrower effect. `RaftCore::append_to_log`
(`core/raft_core.rs`) is `self.log_store.append(entries, callback).await?` and
only then `rx.await`. When the adapter returns `Err`, the `?` returns before the
callback is ever awaited and the receiver is dropped, so the stray
`log_io_completed(Ok(()))` finds no receiver and only logs "failed to send log io
completion event" (`storage/callback.rs:41-44`). The practical consequence in
this version is a misleading error log and a callback contract that cannot
express failure, not a corrupted `RaftCore` view of the log. The defect is real
and the API hole is real; the earlier consequence was overstated and is
withdrawn.

Evidence and its limits: `hiqlite-wal/src/writer.rs:483-496`
(`append_failure_is_returned_but_completion_still_fires`) asserts this behavior,
so it is pinned as current, not incidental. No test establishes what OpenRaft
does with the conflicting pair. Recorded at `001` section 8, bullet 1.

**Repaired (2026-09-19) by `008-wal-append-completion-notification`.** The
completion notification is result-bearing and a rejected append now notifies the
rejection. Regression:
`writer::tests::append_rejection_notifies_error_and_never_success`, demonstrated
failing against the pre-repair behavior. The entry stays in the register as the
baseline the repair was reviewed against; the identifier is not reused.

### F-002 `defect`, confidence `high`

**A blocking persistence failure after the acknowledgement drops the callback
without reporting an error to OpenRaft.** `hiqlite-wal/src/writer.rs:307-315`.
The `complete_append(...)?` call propagates a flush error out of the writer
loop, so the callback is never invoked and no explicit `log_io_completed(Err)`
is sent.

Configuration: `LogSync::Immediate`, where the persistence step is
`flush_blocking`. `ImmediateAsync` calls `flush_async`, which starts writeback
and can fail on the same path.

**Consequence, traced.** An earlier revision said OpenRaft is left without any
completion event. It is not, and the real chain is worse in one way and better in
another. The ack was already sent as `Ok`, so the adapter's `append()` returned
`Ok` and `RaftCore` is parked on `rx.await`. `complete_append(...)?` at
`writer.rs:307-315` returns from `run()` (`writer.rs:211-232`), which ends the
dedicated OS thread spawned at `writer.rs:135`; that thread's `JoinHandle` is
discarded, so the underlying IO error is never surfaced anywhere. The boxed
callback is dropped with the loop, dropping the `LogFlushed` and its oneshot
sender, so `rx.await` does resolve, as `RecvError`, which `RaftCore` maps to
`StorageIOError::write_logs`. OpenRaft therefore learns that something failed but
never learns what: the real `io::Error` is replaced by a channel-receive error.
Worse, the writer thread is gone, so every subsequent append fails and the node's
log store is dead until restart.

Evidence and its limits:
`writer.rs:465-479` (`persistence_failure_suppresses_completion_callback`)
proves the helper suppresses the callback; it does not exercise loop exit, thread
death, or what OpenRaft observes. The chain above was read at source in the
locked OpenRaft and in the WAL crate, and was **not executed**. Recorded at `001`
section 8, bullet 2.

**Repaired (2026-09-19) by `008-wal-append-completion-notification`.** The
persistence cause is notified to OpenRaft before the error propagates, and the
writer's termination is reported through an `ERROR` log rather than vanishing
with a discarded `JoinHandle`. The report covers exactly one termination, `run`
returning `Err`; a panic inside `run` is reported by the panic hook and an abort
by neither, which `008` section 3.6 states rather than widening the runtime to
cover. The fail-stop policy is deliberately unchanged:
the writer still terminates and the log store is still dead until restart.
Regressions: `writer::tests::persistence_failure_notifies_then_terminates_the_writer`
and `log_store_impl::tests::append_adapter_forwards_a_persistence_failure_to_openraft`,
both demonstrated failing against the pre-repair behavior. The second was also
run against a mutation that repaired only the writer and left the adapter
hardcoding `Ok(())`, and it failed there too, which is what makes it evidence
about the adapter rather than about the writer. The previously inferred
consequence is now **observed**: on the pre-repair behavior that test reports
`when Write Logs: channel closed`.

### F-003 `defect`, confidence `high`

**Internal snapshot publication is not atomic.** Publication uses `copy` rather
than a same-directory staging rename and does not sync before returning, so a
partial final UUID file can be observed. `001` and `002` own the surrounding
contract; recorded at `002` section 7, bullet 1. Consequence: a reader can
select a truncated snapshot.

### F-004 `defect`, confidence `high`

**Install publishes the received snapshot before restore.** A failed restore
leaves the published file eligible for startup selection. `002` section 7,
bullet 2. Consequence: a failed install can be chosen on the next start.

### F-005 `defect`, confidence `high`

**The internal lock marker does not prevent concurrent owners.** Safe use
depends on deployment-level exclusivity, which the code does not enforce. `002`
section 7, bullet 3. This is an exclusive-access assumption that `000` section 7
states hiqlite owns, so the gap is on hiqlite's side of the boundary.

### F-006 `defect`, confidence `high`

**Startup does not fall back to an older valid snapshot.** It selects the
greatest UUID and then validates the embedded snapshot id, with no fallback when
the newest file is corrupt. `002` section 7, bullet 4.

### F-007 `limit`, confidence `high`

**Ordinary cluster mode is at-least-once by stated contract.** Writes carry a
process-local correlation identifier and no durable client operation identity,
and hiqlite neither persists a response receipt nor suppresses a later duplicate.
`003` section 3 states this and places the matching obligation on the caller:
callers MUST treat retry after an ambiguous outcome as potentially repeating the
operation.

**Reclassified from `defect` on 2026-09-19.** Nothing in the code or its
documentation promises exactly-once in this mode, so there is no mismatch. The
earlier classification inferred a defect from the availability of a better
design, which is the inference the class definition excludes; that the external
state-machine engine implements a bounded receipt window (`003` section 6) shows
the alternative exists, not that this contract is broken. Recorded so a future
spec proposing durable idempotency has a stated baseline. Now `003` section 8.

### F-008 `limit`, confidence `high`

**The reconnect window and the request timeout are fixed, and nested.** The
late-response window is ten seconds and starts after a successful reconnect; the
outer request wait is 120 seconds.

**Reclassified from `defect` on 2026-09-19, and one claim withdrawn.** The
earlier entry said the two constants can disagree about how long an outcome
remains recoverable. `003` section 5 already describes them as composing rather
than competing: if reconnect takes longer than the window, buffered entries
remain until a connection succeeds or the outer wait expires. That claim was
wrong and is withdrawn rather than carried forward. What remains is that neither
value is configurable and neither carries a recorded rationale. That is a limit
with an open question attached, not a mismatch. Now `003` section 8.

---

## New findings from this assessment

### F-009 `defect`, confidence `high`

**An unvalidated environment read panics instead of failing as configuration.**
`hiqlite/src/split_brain_check.rs:25-29` reads `HQL_SPLIT_BRAIN_INTERVAL` and
calls `.expect("Cannot parse HQL_SPLIT_BRAIN_INTERVAL as u64")`. A malformed
value panics inside a spawned task.

Configuration: any build that runs the split-brain checker. Consequence:
`Cargo.toml` sets `panic = "abort"` for `[profile.release]`, so in a release
build this panic terminates the process rather than surfacing a configuration
error. Evidence and its limits: read at source and at `Cargo.toml:11-15`; the
abort path was not executed in this assessment, so the consequence is inferred
from the profile setting rather than observed. The same pattern appears at
`hiqlite/src/s3.rs:50-59` (`HQL_S3_BUCKET`, `HQL_S3_REGION`, `HQL_S3_KEY`,
`HQL_S3_SECRET`) and `hiqlite/src/server/proxy/config.rs:45`
(`HQL_SECRET_API`), where `.expect` is used on a missing value.

Next action: fold into the configuration-contract spec of wave 2; decide
whether a malformed or missing value is a startup error or a panic, and state
it once for all readers.

### F-010 `gap`, confidence `high`

**The configuration surface is dispersed and only partly claimed.** `001` claims
`hiqlite/src/config.rs` as a `file` unit. The tree holds 46 `env::var` reads, 20
of them in `config.rs`. The remaining 26 live in
`backup.rs`, `s3.rs`, `tls.rs`, `init.rs`, `split_brain_check.rs`,
`server/proxy/config.rs`, `dashboard/mod.rs`, and `dashboard/session.rs`, and
include the `HQL_DANGER_RAFT_STATE_RESET`, `HQL_BACKUP_SKIP_VALIDATION`,
`HQL_INSECURE_COOKIE`, and `HQL_TLS_*_DANGER_TLS_NO_VERIFY` escape hatches.
`hiqlite/src/config_toml.rs` is unclaimed although it loads the same contract
from `hiqlite.toml`.

**Reclassified from `contradiction` on 2026-09-19.** The earlier entry said the
ledger "implies a boundary the code does not respect". Re-examined, nothing
authored makes that implication: `001` claims one file and never states that
`config.rs` is the whole configuration surface, and a claim that covers part of a
contract is incomplete, not contradictory. Dispersed configuration and partial
ownership are ordinary migration state.

What is real, and unchanged: no single authored text states what configures
hiqlite, the coupling gate defends `config.rs` while `config_toml.rs` and the
other 26 reads move freely, and F-009's panic-on-parse pattern lives in that
unclaimed remainder. Consequence: a reader has no one place to learn the
configuration contract, and half of it changes without an owning spec. Evidence
and its limits: counted by grep across `hiqlite/src` and `hiqlite-wal/src`; the
semantics of each variable were not individually verified. Addressed by wave 2.

### F-011 `evidence`, confidence `high`

**`HQL_SPLIT_BRAIN_INTERVAL` is documented nowhere.** It appears in neither
`hiqlite.toml` nor `hiqlite.env`, both of which document the other `HQL_*`
variables checked (`HQL_BACKUP_CRON`, `HQL_BACKUP_SKIP_VALIDATION`,
`HQL_DANGER_RAFT_STATE_RESET`, `HQL_INSECURE_COOKIE`, `HQL_S3_URL` all appear in
both). Consequence: an operator cannot discover a knob that, per F-009, aborts
the process when set wrongly.

### F-012 `evidence`, confidence `high`

**Committed generated assets have no drift check.** `hiqlite/static` is build
output: `dashboard/svelte.config.js` configures `@sveltejs/adapter-static` with
`pages` and `assets` set to `../hiqlite/static` and `precompress: true`, which
is where the 18 `.gz` and 18 `.br` files come from. `hiqlite/src/dashboard/static_files.rs:16`
embeds that directory with `rust-embed` `#[folder = "static"]`, so the committed
bytes are what a built binary serves.

Nothing verifies that the committed output matches the source it claims to come
from. `justfile:225-226` comments out `just build ui` inside the `verify` recipe
with the note that the UI is checked into git, and neither
`.github/workflows/code_style.yaml` nor `.github/workflows/spec-spine.yaml`
builds the dashboard. Consequence: the shipped dashboard can silently diverge
from `dashboard/src`, and a reviewer reading the Svelte source is not
necessarily reading what ships.

Second consequence, on the ledger: the 12 uncompressed `.js` files under
`hiqlite/static` are counted in the coverage denominator as unclaimed source,
so build output inflates the reported migration debt. See F-016.

### F-013 `gap`, confidence `high`

**Cache state machine and log-store adapter are unclaimed.**
`hiqlite/src/store/state_machine/memory/` (6 files, 2192 lines: the cache state
machine and the KV, dlock, TTL, and notify handlers) and
`hiqlite/src/store/logs/` (2 files, 238 lines: the OpenRaft log-store adapter and
its in-memory variant) have no owning spec. `002` claims the SQLite state machine
directory; nothing claims these.

**Reclassified from `decision` on 2026-09-19, and an earlier claim withdrawn.**
The earlier entry said a boundary statement "promises coverage the ledger does
not carry". It does not. Constitution VII says hiqlite "owns, **and this corpus
may specify**" these areas, and `000` section 7 is likewise a statement of
responsibility, not of existing spec coverage. Being hiqlite's responsibility
establishes what a spec would be permitted to claim, never that one already does,
and reading it otherwise would collapse exactly the distinction constitution XII
draws. This is an adoption gap: unclaimed territory inside the intended scope,
which is migration state and not a fault. Closed at M1 by wave 1.

### F-014 `decision`, confidence `medium`

**The split-brain watchdog does nothing under either panic strategy, for
different reasons.** The earlier entry said it "converts a checker failure into
process termination ... up to ten minutes later". That conflated the two panic
strategies and is withdrawn. Corrected below, with observation and inference
kept apart.

**Observed at source.** `hiqlite/src/split_brain_check.rs:12-22`: `spawn` starts
`check_split_brain` as a tokio task, keeps its `JoinHandle`, and starts a second
task that loops on `time::sleep(600)` then `assert!(!handle.is_finished())`,
under the comment "TODO just a safety net until everything runs super smooth and
stable". `check_split_brain` (line 24 onward) is an unbounded `loop`, so it
returns only by panicking. `spawn` returns `()`; the watchdog's own `JoinHandle`
is dropped immediately, and the call site at `hiqlite/src/start.rs:114` discards
the result. No `std::panic::set_hook` exists anywhere in `hiqlite/src` or
`hiqlite-wal/src`. `Cargo.toml:15` sets `panic = "abort"` under
`[profile.release]`, inherited by `[profile.profiling]`; no other profile sets
it, so dev and test builds use the default unwind.

**Inferred, not executed.** Under `panic = "abort"`, the checker's own panic
terminates the process at the moment it happens. The watchdog can therefore never
observe a finished handle, because there is no process left to observe it: under
abort the watchdog is unreachable with respect to its stated purpose, and
termination is caused by the original panic, not by the assertion. Under
unwinding, the checker's panic is caught by the tokio runtime and stored in its
`JoinHandle`, which nobody awaits; up to 600 seconds later the watchdog's
`assert!` fires and panics the watchdog task, whose own handle was already
dropped, so that panic is stored and discarded too. Net effect under unwind:
both tasks die, split-brain checking stops silently, the default panic hook
prints to stderr, and nothing else reports it or terminates anything.

**Why this matters beyond the profile in this repository.** Cargo profile
settings apply to the workspace being built. hiqlite is primarily an embeddable
library, so a downstream application linking it builds under **its own** profile;
`panic = "abort"` here governs this repository's own release binaries, not every
consumer. The unwind path is therefore the reachable one for an unknown share of
users.

Consequence: a fault in an observability path silently disables that
observability, and the mechanism intended to catch it cannot report it. The owner
has directed that runtime behavior be preserved during retroactive adoption, so
this is recorded as found. Whether the watchdog should be repaired, removed, or
replaced by a reported error is a future policy decision with no default here.

### F-015 `limit`, confidence `high`

**The examples are invisible to the ledger by configuration.** `Cargo.toml:4`
excludes `examples` from the workspace, and `spec-spine.toml` leaves
`layout.standalone_rust_workspaces` empty, so all 35 tracked files under
`examples/` (6 crates, 7 `.rs` files, 5 migration `.sql` files, 2 config files)
are outside the indexed denominator. They are not outside CI:
`.github/workflows/code_style.yaml` runs `just clippy-examples`.

Consequence: the examples are the fork's executable user documentation and are
compiled by CI, yet no coverage number describes them and no spec can claim them
until the layout declares them. Next action: wave 6, with the config change
probed against the pinned revision before adoption.

### F-016 `limit`, confidence `high`

**The dashboard is invisible to the ledger by configuration, while its build
output is counted as source.** `spec-spine.toml` sets
`npm_workspaces = ["package.json", "pnpm-workspace.yaml"]`, both at the
repository root, and neither file exists; the manifest is `dashboard/package.json`
and `layout.standalone_npm_packages` is empty. All 81 tracked files under
`dashboard/` are therefore outside the denominator, while the 12 generated `.js`
files under `hiqlite/static` are inside it and counted as unclaimed.

Consequence: the denominator is inverted for this product surface. It omits
1,000+ lines of authored Svelte and TypeScript and includes minified build
output. Any coverage percentage quoted before this is corrected describes a
denominator that nobody chose deliberately.

### F-017 `evidence`, confidence `high`

**The behavioral evidence surface is almost entirely unclaimed, and one claimed
test is itself unverified end to end.** `hiqlite/tests/cluster/` holds 16 test
files and 2,295 lines. Exactly one, `self_heal.rs`, is claimed (by `002`). The
other 15, which cover backup and restore, batches, cache, dlock, migrations,
listen and notify, learner-only and remote-only modes, transactions, and type
conversions, are unclaimed, so the tests that would substantiate a future claim
are not themselves governed.

Separately, `002` section 7, bullet 6 records that the repository cluster test
did not reach its self-healing section during the pilot's local run, because it
stopped progressing in an earlier remote-client phase. Consequence: the single
claimed integration test has a recorded non-completion, and its replacement
evidence is a focused storage test with a narrower reach. Not re-run here: the
task scope excludes expensive cluster runs, and this is reported unresolved
rather than guessed at.

### F-018 `contradiction`, confidence `high`

**Three different things are called "verify".** `just verify` is the
pre-existing full local sweep (`justfile:224-231`: check, clippy,
clippy-examples, test, msrv-verify). `just spine-verify <id>` runs one spec's
acceptance block. `spec-spine verify` is the underlying subcommand, which `000`
section 15 and `004` B-2 forbid CI from running. `AGENTS.md` mentions `just
verify` as the fuller local sweep two sections after the `spine-verify` table.

Consequence: a reader or an agent can conflate "run verify" with "run the
acceptance block", and one of the two is explicitly forbidden in CI while the
other is what CI already runs. Low severity, trivially fixed by naming.

### F-019 `evidence`, confidence `high`

**Two feature surfaces are linted but never tested in CI.**
`.github/workflows/code_style.yaml` runs `cargo clippy`, `just clippy`, `just
clippy-examples`, and `just test-no-s3`. The test step is `cargo test --features
cache,counters,dlock,listen_notify,macros,toml,external-state-machine` plus the
defaults (`auto-heal`, `backup`, `sqlite`, `toml`). The `dashboard` and `server`
features are not enabled for any CI test run, and `s3` is deliberately skipped
(`TEST_SKIP_S3_RESTORE=true`, with the recipe comment that S3 tests cannot run
in GitHub CI).

Consequence: everything behind `#[cfg(feature = "dashboard")]` and
`#[cfg(feature = "server")]`, including the dashboard session, password, and
query paths and the proxy, compiles and lints in CI but is never executed there.
This is an evidence limit, not a claim that those paths are broken.

### F-020 `decision`, confidence `high`

**One contract is split across a claimed and an unclaimed file.** `001` claims
`hiqlite/src/config.rs`; `hiqlite/src/config_toml.rs`, which parses
`hiqlite.toml` into the same configuration, is unclaimed, as are `hiqlite.toml`
and `hiqlite.env` themselves. Editing `config.rs` requires an authoring edit to
`001`; editing `config_toml.rs` requires nothing. Consequence: the coupling gate
defends half of one contract. Next action: wave 2 resolves this by claiming the
contract rather than the file.

---

## Found by wave 1 (2026-09-19)

Recorded by `006-cache-state-machine` and `007-cache-log-store` as known
defects. Those specs are authoritative; the entries below exist so later work
can cite a stable id. None is repaired.

### F-021 `defect`, confidence `high`

**`get_log_state` always reports `last_log_id: None`.**
`hiqlite/src/store/logs/memory.rs:117-120` reads the last log id as
`logs.get(logs.len())`, which is always one past the end of the deque.
**Observed by execution**, not inference: the added characterization test
`store::logs::memory::tests::get_log_state_reports_no_last_log_id_even_after_append`
stores two entries and observes `None`.

Configuration: `cache`. Consequence, traced but not executed: both consumers in
OpenRaft 0.9.24 (`StorageHelper::get_initial_state` and `last_membership_in_log`)
run on the initialization path, where this non-durable store is empty by
construction, so `None` is accidentally correct and the defect is latent there.
Whether any path reaches it with a non-empty deque was **not** established.
`007` KD-1.

### F-022 `defect`, confidence `high`

**A `debug_assert!` in `truncate` compares an offset to an absolute index.**
`memory.rs:186`. Equal only while the deque front is index 0, so it fires in a
debug build on any truncate after a purge advanced the front. Untested: no test
covers a non-zero front offset. `007` KD-2.

### F-023 `defect`, confidence `medium`

**`try_get_log_entries` underflows on an exclusive end bound of zero.**
`memory.rs:64-68` computes `*i - 1`. Debug panics; release wraps to `u64::MAX`
and then panics at the `logs.front().expect(...)`. **Reachability not
established**: whether OpenRaft requests `0..0` was not determined, which is why
this is `medium` and why it is recorded as an arithmetic defect rather than a
demonstrated failure. `007` KD-3.

### F-024 `defect`, confidence `high`

**`purge` never updates `last_purged`.** `memory.rs:28` is returned by
`get_log_state` but assigned only in `new()`. Consequence inferred:
`last_purged_log_id` is under-reported, masked by the same emptiness that masks
F-021. `007` KD-4.

### F-025 `decision`, confidence `high`

**A dead cache handler thread panics the applying task.** Every handler send in
`hiqlite/src/store/state_machine/memory/state_machine.rs` uses `.expect(...)`.
Crash-over-divergence is a defensible choice for a replicated state machine and
is written down nowhere in the code; under this repository's `panic = "abort"`
release profile it becomes process termination, and under a downstream
consumer's unwinding profile it does not. Recorded as a decision because the
intent is sound but unstated. Untested. `006` KD-1.

### F-026 `limit`, confidence `high`

**Lock validity is a compile-time constant.** `LOCK_VALID_SECONDS = 10`
(`dlock_handler.rs:13`), with no configuration. A critical section legitimately
longer than ten seconds has no supported way to extend it and nothing detects
the overrun. Described as found per the owner's direction; no lease design is
proposed. `006` KD-2.

### F-027 `defect`, confidence `high`

**`cache_idx` is an unvalidated index.** `.get(cache_idx).unwrap()` panics out of
range. In-range-ness holds only while client and server are built from the same
generated cache enum; a log entry from a build with more cache variants would
panic every node applying it. Untested. `006` KD-3.

### F-028 `defect`, confidence `high`

**A truncated entry stream is acknowledged and notified as a successful
append.** `hiqlite-wal/src/writer.rs`, the collection loop
`while let Ok(Some((id, bytes))) = rx.recv()`. A `recv` error, which is what a
dropped entry sender produces, is indistinguishable from the `None` that marks a
normal end of stream: both end the loop with `res` still `Ok`. The writer then
acknowledges success and notifies success for an append that did not receive
every entry.

Configurations: all `LogSync` modes. Found while tracing the notification paths
for the `008` repair and deliberately left unrepaired there, because it is a
different defect from F-001 and F-002 and its fix changes what the writer does
with a partial append. Recorded at `008` KD-2. Untested.

## Summary by class

| class | ids | count |
|---|---|---|
| `defect` | F-001 to F-006, F-009, F-021 to F-024, F-027, F-028 | 13 |
| `contradiction` | F-018 | 1 |
| `gap` | F-010, F-013 | 2 |
| `evidence` | F-011, F-012, F-017, F-019 | 4 |
| `limit` | F-007, F-008, F-015, F-016, F-026 | 5 |
| `decision` | F-014, F-020, F-025 | 3 |

**Reclassified on 2026-09-19**, after each class test was applied rather than
assumed: F-007 and F-008 from `defect` to `limit`, because a stated contract with
a caller obligation is not a mismatch; F-010 and F-013 from `contradiction` and
`decision` to `gap`, because incomplete ownership and unclaimed territory are
migration state rather than faults; and the consequences of F-001, F-002, and
F-014 rewritten against traced source, with three earlier claims withdrawn in
place.

Thirteen defects: six from the pilot specs, F-009 from the whole-project pass,
five (F-021 to F-024, F-027) found by wave 1, and F-028 found while tracing the
`008` repair. F-013 is closed at M1 by wave 1 and
is retained as a record rather than deleted. No finding in this register authorizes a repair; each repair is
a separate governed change with its own spec and evidence.

**Repaired so far.** F-001 and F-002, by
`008-wal-append-completion-notification` on 2026-09-19. A repaired entry is
annotated in place and keeps its identifier and its original text, so the
baseline a repair was reviewed against stays readable. The remaining ten
defects are open, and F-021 through F-024 are a separate cache-log repair that
was deliberately not bundled into `008`.
