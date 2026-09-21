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

**Disposition appended 2026-09-20; the observation above stands as recorded on
2026-09-19.** Partly closed by `009-configuration-contract`, which claims
`config_toml.rs`, `hiqlite.toml` and `hiqlite.env` and extends `001`'s claim on
`config.rs`, so the two constructors and the two reference files are now owned
together and defended by `C-001`. Not closed: the other 33 `HQL_*` reads in
`backup.rs`, `s3.rs`, `tls.rs`, `init.rs`, `split_brain_check.rs`,
`server/proxy/config.rs`, `dashboard/mod.rs` and `dashboard/session.rs` remain
unclaimed, and each belongs to the spec that will own its subject. F-010 stays
open against that remainder.

### F-011 `evidence`, confidence `high`

**`HQL_SPLIT_BRAIN_INTERVAL` is documented nowhere.** It appears in neither
`hiqlite.toml` nor `hiqlite.env`, both of which document the other `HQL_*`
variables checked (`HQL_BACKUP_CRON`, `HQL_BACKUP_SKIP_VALIDATION`,
`HQL_DANGER_RAFT_STATE_RESET`, `HQL_INSECURE_COOKIE`, `HQL_S3_URL` all appear in
both). Consequence: an operator cannot discover a knob that, per F-009, aborts
the process when set wrongly.

**Closed 2026-09-21 by `010` section 5.** `hiqlite.env` now documents the
variable, its default of 60, what the check does and does not do, and its parse
failure mode. It is documented there only: `config_toml.rs` has no corresponding
key, so a `hiqlite.toml` entry would describe a setting that does not exist. The
entry is retained as the record of the gap. F-009, which is about the parse
`expect` itself, stays open.

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

**Disposition appended 2026-09-20; the observation above stands as recorded on
2026-09-19.** The visibility half is fixed: `spec-spine.toml` now declares the
six crates through `layout.standalone_rust_workspaces`, and
`spec-spine index coverage` lists each example package with its own row. The
adoption half is not: all seven example `.rs` files are still unclaimed, and no
spec claims any of them. F-015 stays open with its scope narrowed to the
unclaimed territory; the configuration blocker it named is gone.

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

**Disposition appended 2026-09-20; the observation above stands as recorded on
2026-09-19.** Half of the inversion is fixed and half is not. Fixed: the
dashboard is declared through `layout.standalone_npm_packages`, so its 24 `.ts`
and `.js` files are in the denominator and its authored `.svelte`, `.css` and
build-config files reach it through `coverage.governed_scope`. Not fixed: the 12
generated `.js` files under `hiqlite/static` are still counted as `hiqlite`
package source and still unclaimed, and the 4 vendored files under
`dashboard/src/spow/` are still counted too. The adoption plan's rung-0 probe
established why (the only key that removes them also exempts them from the
coupling gate), so this half is a tool dependency and not a configuration
oversight. F-016 stays open on the generated-and-vendored denominator problem
alone; no dashboard file is claimed yet either.

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

**Re-evaluated 2026-09-20 against the current `AGENTS.md`, and narrowed.** The
`AGENTS.md` half of the observation no longer holds: that file now names the
pre-existing recipes explicitly as `just check`, `just clippy` and `just test`
(`AGENTS.md:141`) and does not mention `just verify` at all, so the adjacency
this entry described is gone. The three-way name collision itself is unchanged
and still in the tree: `justfile:224` defines `verify`, `justfile:319` defines
`spine-verify`, and `spec-spine verify` is the subcommand CI is forbidden to run
(`000` section 15, `004` B-2). F-018 stays open against the naming, not against
`AGENTS.md`.

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

### F-029 `defect`, confidence `high`

**`purge` removes exclusively where the trait requires inclusive removal.**
`hiqlite/src/store/logs/memory.rs:210` computes
`purge_until = log_id.index - first_offset` and line 217 calls
`logs.drain(..purge_until)`, an exclusive range, so the entry at `log_id.index`
survives the purge that named it. OpenRaft 0.9.24 states the opposite
requirement on the trait method: "Purge logs upto `log_id`, inclusive"
(`openraft-0.9.24/src/storage/v2.rs:138`). Both halves were read from source in
this pass, the trait from the locked crate in the local registry checkout rather
than from published documentation.

The characterization test wave 1 added,
`store::logs::memory::tests::purge_removes_entries_below_the_given_index`
(`memory.rs:269-287`), stores indexes 1 to 5, purges through index 3, and
asserts the remainder is `[3, 4, 5]`; its doc comment states the exclusive rule
as the contract. The test therefore pins the mismatch as expected behavior, so a
repair must replace that expectation rather than add a case beside it.

Configuration: `cache`.

**Consequence: not established.** Nothing was executed for this entry beyond
reading the two sources. What is confirmed is the source-level mismatch and the
test that pins it. The retained entry is also entangled with F-024: because
`last_purged` is never assigned, the reported purge frontier cannot be used to
notice it. Whether any path reaches this store with a non-empty deque was not
established here, and F-021's note (both `get_log_state` consumers in OpenRaft
0.9.24 run on an initialization path where this non-durable store is empty by
construction) applies to that question too.

**Recorded separately, and ahead of its owning spec.** F-021 through F-024 keep
their identifiers and their text; this is a fifth defect in the same file, not a
renumbering. `007-cache-log-store` records four known defects and does not
record this one, so the register is ahead of `007` here. `007` remains
authoritative for its own territory: reconciling KD-5 into it is queued in the
adoption plan's current assignment table and is not done by this change.

### F-030 `contradiction`, confidence `high`

**`005`'s acceptance block asserted a phrase that had been removed from the
document it names, and failed.** The assertion was `grep -q 'changes no runtime
behavior' standards/spec/wal-repair-proposal.md`. Commit `8bce5ca` (PR #7)
rewrote that document's header to record that the repair had been implemented,
removing the phrase. `008` holds an `extends` edge on the file with nature
`superseding`, so the edit was authorized and `spec-spine couple` passed; the
owning spec's assertion about the file is what nobody updated.

**Observed by execution, on 2026-09-20**, in two places: in the working tree,
and in a clean worktree at the unmodified integration head `58ee7fa`, where
`just spine-verify 005` fails at command 4 with exit 1. The failure had been on
the integration branch since PR #7 merged.

**Why no automated control caught it.** Verification here is manual by design,
not missing. `just spine-verify <spec-id>` is a documented command and a named
step in `AGENTS.md`'s gate, and running it is how this entry was observed at
all. What no control does is run it **automatically**: pull-request CI
deliberately does not, because a proposed tree is untrusted input (`000` section
15, `004` B-2); `spec-spine check` compares committed shards and does not
execute acceptance blocks; `spec-spine couple` checks that a claimed path and an
owning spec moved together and does not read what either says. So between two
hand runs, a stale assertion stays undetected, which is what happened here
between PR #7 merging and 2026-09-20.

**Repaired (2026-09-20) by `005` D-7.** The stale assertion is replaced with
one that holds against the document's current text, the original line is kept
beside it as a comment, and `just spine-verify 005` passes. The contradiction
this entry records, an assertion incompatible with the document it names, no
longer exists in the tree.

**The process question it surfaced is tracked separately, and is optional.**
Whether the accepted control stays the existing manual run, or an automated
execution is added on a trusted tree, is an owner question that this entry does
not prejudge: CI's abstention is a deliberate trust boundary, so "add it to CI"
is not an obvious fix. It is carried as W-24 in the adoption plan, which is
where work and decisions live. It is not a finding, and it does not hold this
entry open.

**Not claimed.** That any other acceptance block is currently stale, that the
manual control is inadequate, or that a class of similar failures exists in the
tree. Only `000`, `004` and `005` were executed in this pass, all pass, and
`008`'s block was read but not executed because it runs cargo tests.

## Found by the configuration adoption (2026-09-20)

Recorded by `009-configuration-contract` as known defects. That spec is
authoritative; the entries below exist so later work can cite a stable id. None
is repaired.

### F-031 `defect`, confidence `high`

**`tls_api_danger_tls_no_verify` is documented, unreachable, and fatal to set.**
`hiqlite.toml:194` documents the key. `config_toml.rs:211-212` reads
`tls_raft_danger_tls_no_verify` a **second** time into the variable named
`tls_api_danger_tls_no_verify`; because `t_bool` begins with `map.remove(key)`
and `:194-195` already removed it, the second read always yields `None` and the
API side is always `false`. The documented key itself is never consumed, so it
reaches the unknown-key check (`config_toml.rs:458-461`) and the whole config is
refused with `Unknown Config data`.

**Observed by execution**, both halves:
`config_toml::tests::documented_tls_api_no_verify_key_is_rejected_as_unknown`
and `..::tls_api_no_verify_stays_false_when_the_raft_key_is_set`.

Configuration: the TOML constructor only. The environment route is unaffected,
because `tls.rs:62-69` formats `HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY` per
variant. Consequence: **fail-closed**. API certificate verification stays
enabled, so no security boundary is weakened; a documented escape hatch is
unusable and using it prevents startup. `009` KD-1.

### F-032 `contradiction`, confidence `high`

**`HQL_HEALTH_CHECK_DELAY_SECS` is documented and read nowhere.**
`hiqlite.env:106` sets it with a documented default of 30.
`config_toml.rs:241-242` passes an **empty** `env_var` for
`health_check_delay_secs`, and every typed helper skips the environment lookup
when `env_var.is_empty()`. Both constructors hardcode 30 (`config.rs:199`,
`:365`). The TOML key works and is pinned by
`config_toml::tests::health_check_delay_secs_is_settable_from_toml_only`; the
documented variable does nothing. `network/api.rs:65` names it in a log line,
which makes it look supported. `009` KD-2.

### F-033 `contradiction`, confidence `high`

**`HQL_ENC_KEYS_FROM` is documented and read nowhere.** `hiqlite.env:118`
documents it with `env` and `file:path/to/file` as its two values. No code reads
it: checked across `*.rs`, `*.toml` and `*.env`, where only `hiqlite.env` and
`CHANGELOG.md` mention it. The locked cryptr 0.10.0 reads `ENC_KEYS`,
`ENC_KEY_ACTIVE` and `ENC_KEYS_SEALED` and no `HQL_`-prefixed variable, read
from the crate source in the local registry. The `file:` alternative the comment
offers therefore does not exist on the environment path; the TOML path's
`secrets_file` / `HQL_SECRETS_FILE` mechanism is a different thing that works.
`009` KD-3.

### F-034 `contradiction`, confidence `high`

**One setting has two defaults and two documented defaults.**
`prepared_statement_cache_capacity` is 1000 in the TOML path
(`config_toml.rs:150-151`, documented as 1000 at `hiqlite.toml:69`) and 1024 in
`Default` and the environment path (`config.rs:176`, `:340`, documented as 1024
in the doc comment at `config.rs:58-60`). **Observed by execution** by the test
`prepared_statement_cache_capacity_default_differs_from_the_env_path` in
`config_toml::tests`. The setting has no environment variable in either
path. `009` KD-4.

### F-035 `limit`, confidence `high`

**The environment constructor cannot select the WAL durability mode.**
`config.rs:346-347` hardcodes `wal_sync: LogSync::ImmediateAsync` and
`wal_size: 2 * 1024 * 1024`, as does `Default` at `:178-179`. `HQL_LOG_SYNC` and
`HQL_WAL_SIZE` are read only by the TOML constructor (`config_toml.rs:156`,
`:164`). A deployment configured through the environment always runs the
asynchronous level and cannot select the `Immediate` mode `001` names for
acknowledged writes that must survive a power loss.

**Recorded as a `limit`, not a `defect`, deliberately.** No authored text
promises that route: `hiqlite.env` does not list either variable, and the
environment names exist only as overrides inside the TOML loader, so nothing
disagrees with anything. What was missing is that the limit had never been
stated where a reader would find it; `009` B-5 states it. Evidence: read at
source, not executed. `009` KD-5's sibling, recorded at `009` B-5.

### F-036 `defect`, confidence `high`

**A parse-error message names the wrong type.** `config.rs:335-339` parses
`log_statements`, a `bool`, with
`expect("Cannot parse HQL_LOG_STATEMENTS as u64")`. Operator-visible and
trivially wrong. Untested. `009` KD-5.

### F-037 `defect`, confidence `high`

**A bracketed IPv6 advertised address produces an unparsable listen address.**
`hiqlite/src/start.rs:321-330` (`build_listen_addr`) derives the socket a node
binds by taking the host from `listen_addr_api` / `listen_addr_raft` and the port
from the advertised `addr_api` / `addr_raft`, split with
`str::split_once(':')`. For `[fd00::1]:8100` that first colon is inside the
brackets, so the "port" is `:1]:8100` and the result is `::::1]:8100`.
`SocketAddr::from_str` rejects it at `start.rs:142` and `:228`.

**Observed by execution.** `start::tests::ipv6_advertised_address_produces_an_unparsable_listen_address`
asserts both the exact string and its rejection.

Consequence: the rejection happens inside a task whose `JoinHandle` was dropped
(F-040), so under an unwinding profile the node starts, reports success, and has
no listener on that address; under `panic = "abort"` it ends the process.
`NodeConfig::is_valid` (`config.rs:420-480`) validates no address at all, so
nothing rejects the configuration earlier. `010` KD-1.

### F-038 `defect`, confidence `high`

**`node_id` names a position in one file and an id in another.**
`hiqlite/src/start.rs:65-68` picks the addresses this node binds by indexing
`nodes[node_id - 1]`. `hiqlite/src/init.rs:138-148` (`get_this_node`) picks the
identity this node registers with the cluster by searching `nodes` for
`id == node_id`. `NodeConfig::is_valid` (`config.rs:429-431`) bounds `node_id`
against `nodes.len()` and never inspects the ids in it. `Node`'s doc comment
(`lib.rs:131-133`) requires an `id == 1` to exist and states nothing about the
rest, so nothing authored requires the ids to be `1..=n` in order.

**Observed by execution**, in three parts:
`init::tests::node_identity_is_resolved_by_id_here_and_by_position_in_start`
(with ids `2,3,4` and `node_id = 3` the two routes name different nodes),
`init::tests::is_valid_accepts_a_nodes_list_whose_ids_are_not_positions` (that
shape is accepted as valid), and
`init::tests::get_this_node_panics_when_the_id_is_absent`.

Consequence: a `nodes` list whose ids are not their positions either binds one
node's ports while joining as another, or panics on the join path instead of
returning a configuration error. `010` KD-2.

### F-039 `defect`, confidence `high`

**A node with TLS on both endpoints cannot be shut down without a panic, and no
TLS listener is ever shut down gracefully.** `hiqlite/src/start.rs:125` creates
the `tx_shutdown` watch channel. Exactly two receivers are created, the
`shutdown_signal` future at `:138` and `rx_shutdown` at `:242`, and each is moved
into a task only on the **plaintext** branch of its server. On the TLS branch the
`axum_server` task is spawned with no shutdown future, which the `TODO` comments
at `:143-144` and `:229-230` acknowledge. `.subscribe()` is called nowhere in
`hiqlite/src`.

With `tls_raft` and `tls_api` both set, neither plaintext branch runs, both
receivers drop when `start_node_inner` returns, and
`hiqlite/src/client/mgmt.rs:374-376` then calls `tx.send(true)` on a channel with
no receivers and `expect`s it, with the message "The global Hiqlite shutdown
handler to always listen". With exactly one endpoint on TLS the send succeeds,
but the surviving receiver belongs to the plaintext server, so the TLS listener
keeps accepting until the process exits either way.

**Source-established, not executed.** Reproducing it needs a node with real TLS
material and a real shutdown; `010` section 4 states that limit and its
acceptance block pins the source shape instead. The fault is caused in `010`'s
unit and observed in `003`'s. `010` KD-3.

### F-040 `defect`, confidence `high`

**A listener that cannot bind does not fail startup.** The socket-address parse
(`hiqlite/src/start.rs:142`, `:228`), the TCP bind (`:152-154`, `:238-240`) and
`serve(...)` in all four branches express failure as `expect` or `unwrap`, inside
tasks whose `JoinHandle`s are dropped at `:141`, `:151`, `:227` and `:237`. Six
tasks are spawned during startup and only the two join tasks are ever awaited.

Consequence, and it is the profile split again: under an unwinding profile the
panic stays in its task, `start_node_inner` returns `Ok`, and the node is
reported started with an endpoint that does not exist; under this repository's
`panic = "abort"` (`Cargo.toml:15`) the same failure ends the process. Neither is
a startup error, and for an embedded node the profile is the **consumer's**, as
F-014 records for the same reason. Source-established. Same class as F-025.
`010` KD-4.

## Summary by class

Class is what a finding **is**. State is what has **happened** to it. They are
separate fields and neither is read off the other: a `defect` may be open or
repaired, and a `gap` may be closed at M1 while its entry is retained as a
record.

| class | ids | count |
|---|---|---|
| `defect` | F-001 to F-006, F-009, F-021 to F-024, F-027 to F-029, F-031, F-036 to F-040 | 20 |
| `contradiction` | F-018, F-030, F-032 to F-034 | 5 |
| `gap` | F-010, F-013 | 2 |
| `evidence` | F-011, F-012, F-017, F-019 | 4 |
| `limit` | F-007, F-008, F-015, F-016, F-026, F-035 | 6 |
| `decision` | F-014, F-020, F-025 | 3 |

### Summary by state (2026-09-21)

| state | ids | count |
|---|---|---|
| repaired | F-001, F-002, F-030 | 3 |
| closed, entry retained | F-013 (at M1, by wave 1), F-011 (2026-09-21, by `010`) | 2 |
| open | everything else: F-003 to F-010, F-012, F-014 to F-029, F-031 to F-040 | 35 |

A finding's state answers whether the thing it records is still in the tree, and
nothing else. F-030 is repaired because its contradiction is gone; the optional
process decision it surfaced is W-24's, and a work item's being undecided has
never been a reason to hold a finding open.

Thirty-five open, of which three carry a dated disposition appended on
2026-09-20 recording what has moved since they were written: F-015 (examples
now visible, still unclaimed), F-016 (dashboard now visible, the generated and
vendored
denominator problem unresolved), F-018 (the `AGENTS.md` half resolved, the
three-way naming collision unchanged). A disposition narrows an entry; it does
not close it. An earlier revision said four and listed three.

Open **defects**, which is the subset a repair workstream draws from:
F-003, F-004, F-005, F-006, F-009, F-021, F-022, F-023, F-024, F-027, F-028,
F-029, F-031, F-036, F-037, F-038, F-039, F-040. Eighteen of the twenty
defects; F-001 and F-002 are the two repaired.

**Reclassified on 2026-09-19**, after each class test was applied rather than
assumed: F-007 and F-008 from `defect` to `limit`, because a stated contract with
a caller obligation is not a mismatch; F-010 and F-013 from `contradiction` and
`decision` to `gap`, because incomplete ownership and unclaimed territory are
migration state rather than faults; and the consequences of F-001, F-002, and
F-014 rewritten against traced source, with three earlier claims withdrawn in
place.

Twenty defects: six from the pilot specs, F-009 from the whole-project pass,
five (F-021 to F-024, F-027) found by wave 1, F-028 found while tracing the
`008` repair, F-029 found on 2026-09-20 while re-reading the memory log store
against the locked trait, F-031 and F-036 found by the configuration adoption on
2026-09-20, and F-037 to F-040 found by the node-lifecycle adoption on
2026-09-21. F-013 is closed at M1 by wave 1 and F-011 by `010`; both are
retained as records rather than deleted. No finding in this register authorizes
a repair; each repair is a separate governed change with its own spec and
evidence.

**Repaired so far.** F-001 and F-002, by
`008-wal-append-completion-notification` on 2026-09-19, and F-030, by `005` D-7
on 2026-09-20. A repaired entry is annotated in place and keeps its identifier,
its class, and its original text, so the baseline a repair was reviewed against
stays readable.

**Eighteen defects are open**: F-003 to F-006, F-009, F-021 to F-024, F-027
to F-029, F-031, F-036 to F-040. Two earlier revisions of this paragraph were
stale: one said ten against a table of fourteen, and the next said twelve after
`009` had already added F-031 and F-036. The arithmetic is twenty recorded minus
the two repaired, and this paragraph is the one that has to be recomputed
whenever the class table changes. F-021 through F-024 and F-029 are a separate
cache-log repair that was deliberately not bundled into `008`.
