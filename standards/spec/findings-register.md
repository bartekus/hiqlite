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

**Incidental observation appended 2026-09-21, and it is not an experiment.**
While running the `sqlite-only` example for `017`, two instances of that example
were started against the same data directory by accident. The second was not
refused: it started, ran, and on shutdown the two raced on the lock file, one of
them panicking at `hiqlite-wal/src/writer.rs:576` with
`LockFile removal failed: IO(Os { code: 2, kind: NotFound })`. Recorded because
it is the first time this repository has seen two processes share storage, and
because the observed failure mode is a panic during shutdown rather than a
refusal at startup. It is **not** controlled evidence: the overlap was
unintentional, the timing was not arranged, and a single occurrence establishes
neither reproducibility nor the general behavior. W-21 is where that would be
established, and it is undecided.

**Repaired 2026-09-21 by `024-exclusive-storage-ownership`.** A node now takes
an exclusive `fs4` advisory lock on `{data_dir}/hiqlite-owner.lock` before the
restore, before the reset check and before either state machine is constructed,
and releases it at the end of shutdown, after both raft groups, the WAL writer
and the SQLite writer have stopped. A contender is refused with
`Error::StorageInUse` having opened, created and removed nothing; a filesystem
that rejects the lock is refused just as explicitly rather than treated as
probably fine.

Demonstrated with **two real processes**, four cases: refusal without data
mutation, refusal while a child holds it, release on orderly shutdown, and
release after the owner calls `abort()`, which is the property the marker file
could never have because there is no cleanup step that has to run. Four further
tests cover the identity questions a path-based or pid-based implementation
would fail: a second node in the same process, and an aliased path through a
symlink.

Not established, and stated in `024` section 5: network filesystems, which
remain an unsupported storage arrangement (KD-1); a plain `fork` child, which
inherits the lock (KD-2); and anything about a running node, since the tests
drive the module directly. The old `state_machine/lock` marker is unchanged and
keeps answering the different question `auto-heal` reads it for.

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

**Disposition appended 2026-09-21 by `019-dashboard-build-contract`**, which ran
the comparison this entry says nothing performs. Both halves came back negative
and they are separate findings: F-092, the build is not reproducible because
`kit.version.name` is unset and SvelteKit stamps a millisecond timestamp into
`version.json`; and F-093, the committed artifact does not come from this source
at this lockfile, measured as 64 emitted files against 55 committed. F-012 stays
open: the missing check is still missing, and F-092 is why it cannot be written
before a one-line configuration change.

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

**Closed 2026-09-21 by `017-examples-as-documentation`**, which establishes all
seven. The entry is retained as the record of both halves: the configuration
blocker, fixed on 2026-09-20, and the unclaimed territory, closed here. What is
still open is a separate thing with its own identifier: nothing runs the
examples, which is F-081.

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

**Disposition appended 2026-09-21 by `012-cluster-integration-evidence`.** Both
halves have moved, and neither is closed by the same mechanism. The first half,
fifteen unclaimed files, is **closed at M1**: `012` establishes all fifteen and
maps each phase to the guarantee it establishes (`012` B-1). The second half is
**diagnosed, not closed**: the stall is a connect-versus-publish race in
`Client::remote`, recorded as F-051 with a measured 175 ms losing window, and it
is still in the tree. The entry stays open on that basis. A third fact the
original entry did not have: the non-completion is not specific to self-healing
at all, because `self_heal.rs` is phase 15 of a fifteen-phase sequential test
(F-048), so any stall anywhere before it produces the same report.

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

**Repaired 2026-09-21 by `020-cache-log-store-contract-repair`.** `get_log_state` now reads `logs.back()` and falls back to the purge frontier when the deque is empty (`020` B-2). The characterization test that pinned this was replaced, not extended.

### F-022 `defect`, confidence `high`

**A `debug_assert!` in `truncate` compares an offset to an absolute index.**
`memory.rs:186`. Equal only while the deque front is index 0, so it fires in a
debug build on any truncate after a purge advanced the front. Untested: no test
covers a non-zero front offset. `007` KD-2.

**Repaired 2026-09-21 by `020-cache-log-store-contract-repair`.** The assertion now compares the entry at the computed offset against the log index the caller named, and tolerates the `None` that a truncate at one past the end produces (`020` B-5).

### F-023 `defect`, confidence `medium`

**`try_get_log_entries` underflows on an exclusive end bound of zero.**
`memory.rs:64-68` computes `*i - 1`. Debug panics; release wraps to `u64::MAX`
and then panics at the `logs.front().expect(...)`. **Reachability not
established**: whether OpenRaft requests `0..0` was not determined, which is why
this is `medium` and why it is recorded as an arithmetic defect rather than a
demonstrated failure. `007` KD-3.

**Repaired 2026-09-21 by `020-cache-log-store-contract-repair`.** An exclusive end bound of zero returns no entries (`020` B-4).

### F-024 `defect`, confidence `high`

**`purge` never updates `last_purged`.** `memory.rs:28` is returned by
`get_log_state` but assigned only in `new()`. Consequence inferred:
`last_purged_log_id` is under-reported, masked by the same emptiness that masks
F-021. `007` KD-4.

**Repaired 2026-09-21 by `020-cache-log-store-contract-repair`.** `purge` assigns `last_purged` under the same lock acquisition that drains the deque, and the frontier only moves forward (`020` B-3).

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

**Still standing on 2026-09-21, and deliberately.**
`023-distributed-lock-lease-liveness` repaired the handler's liveness and
declined to touch this: a longer lease does not make a dead holder detectable
any sooner than its own deadline, and a configurable one moves the choice to the
operator without changing what any value of it can promise. The lease length is
injectable for tests only, so the expiry paths can be exercised without waiting
out ten seconds per case, and `spawn` remains its only non-test caller.

What `023` did add is the statement this entry was missing. `023` KD-1: this is
a lease and not a fence. A holder whose lease expires is told nothing, stays
inside its critical section, and has its eventual release ignored, so two
clients can be inside the same critical section at once and the only bound on
that is that the first one's work outlasted its lease. There is no fencing
token. `023` KD-3 and KD-4 add the restart cases: replay re-grants an unreleased
lock for one further lease window measured from the restart, and a memory-only
cache node has no lock state at all until a snapshot or a new entry arrives.

### F-027 `defect`, confidence `high`

**`cache_idx` is an unvalidated index.** `.get(cache_idx).unwrap()` panics out of
range. In-range-ness holds only while client and server are built from the same
generated cache enum; a log entry from a build with more cache variants would
panic every node applying it. Untested. `006` KD-3.

**Repaired 2026-09-21 by `022-replicated-cache-command-compatibility`, and
widened on the way.** Tracing the index found the same shape on a second axis:
the `CacheRequest` variant set is feature-independent by design, because the
variant order is part of the log format, so a node built without `counters`,
`dlock` or `listen_notify_local` can be handed a committed entry it has no
handler for, and three read-only variants can be handed one that should never
have been replicated at all. Every one of those arms was an `unreachable!`.

Both axes are now classified before an entry is executed. An entry that cannot
be applied stops application at that point, is not counted as applied, and sets
a terminal failure that refuses every later apply, every cache read, and every
cache write on the node, named as `Error::CacheIncompatible` and mapped to
`503`. No longer untested: five tests, four of which fail against the
unrepaired implementation and three of those by the panic this entry describes.
Not established, and stated in `022` section 4: no cluster diverges in a test,
no served request is refused in a test, and the feature-gap half is compiled out
of the test build.

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

**Repaired 2026-09-21 by `021-wal-append-stream-integrity`.** The collection
loop is now an explicit three-way `match`: `Ok(Some(..))` collects,
`Ok(None)` is the end-of-stream marker, and `Err` is the dropped sender, which
sets the new `Error::IncompleteAppend`. A truncated append acknowledges that
error, notifies exactly one failure carrying the same cause, and ends the
writer, for the reason `021` B-3 gives: the writer holds a prefix of a batch
whose extent it cannot know. No longer untested: six cases, two disconnection
points across all three `LogSync` modes, observed failing against the
unrepaired loop. A clean empty batch is separately pinned as a success (`021`
B-4), and the persisted prefix is read back through the adapter after a fresh
open, across a WAL file boundary (`021` section 4).

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

**Reconciled 2026-09-21.** `007` now records this as its KD-5, so the register
and the spec no longer disagree. The register is no longer ahead of `007` here.

**Repaired 2026-09-21 by `020-cache-log-store-contract-repair`.** `purge` now
calls `drain(..=purge_until)`, so the entry the purge names is removed (`020`
B-3). `truncate` is deliberately unchanged, for the reason `020` B-3 states.

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

### F-041 `defect`, confidence `high`

**A half-configured TLS endpoint downgrades silently.**
`hiqlite/src/tls.rs:71-82` (`ServerTlsConfig::from_env`) selects `Specific` only
when `HQL_TLS_{variant}_KEY` **and** `HQL_TLS_{variant}_CERT` are both set. If
exactly one is set, the branch simply fails and the function falls through: with
`HQL_TLS_AUTO_CERTS` off the endpoint runs in plaintext, and with it on the
endpoint runs with a self-signed certificate no client verifies. Nothing logs,
warns or errors, and `NodeConfig::is_valid` inspects no TLS material.

**Observed by execution**, both halves, in
`tls_env::from_env_branches_including_the_silent_downgrade`.

Consequence: one misspelled variable name yields a running node with weaker
transport than was configured, and the only way to notice is to inspect the
wire. `011` KD-1.

### F-042 `defect`, confidence `high`

**Two booleans four lines apart disagree about whether a typo is fatal.**
`hiqlite/src/tls.rs:58-60` reads `HQL_TLS_AUTO_CERTS` with
`parse::<bool>().unwrap_or(false)`, so `HQL_TLS_AUTO_CERTS=ture` silently means
"off", which through F-041 can mean plaintext. `:64-69` reads
`HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY` with
`parse::<bool>().expect("Cannot parse HQL_TLS_*_DANGER_TLS_NO_VERIFY to bool")`,
so the same class of typo panics. The rest of the module is uniformly fatal: the
PEM load (`:93`), key generation and the `OnceLock` set (`:102-105`), certificate
construction (`:134`, `:141`) and the host name in `into_tls_stream` (`:177`).

**Observed by execution** for the panic half, in
`tls_env::a_malformed_no_verify_override_panics`; the rest is source-established.

Note in favour of the current code: `server_config` is awaited in the caller's
own task (`start.rs:140`, `:226`), so a missing or malformed certificate file
panics out of `start_node_inner` rather than through F-040's detached-task route.
That is the better of the two outcomes and is written down nowhere. Same class as
F-009. `011` KD-2.

### F-043 `defect`, confidence `high`

**A verifying client has nothing to verify against.**
`hiqlite/src/tls.rs:152-170` (`build_tls_config`) builds
`RootCertStore::empty()` and adds certificates only under
`#[cfg(feature = "webpki-roots")]`, which is not in `default`
(`hiqlite/Cargo.toml:22`). `reqwest` is configured `default-features = false`
with `rustls-no-provider` and no roots feature (`Cargo.toml:80-85`), and
`hiqlite/src/http_client.rs:15-22` merges the webpki bundle under the same
`cfg`. `ServerTlsConfigCerts` has three fields and none is a CA, and no
environment variable or TOML key supplies one.

**Source-established**, with the material type's shape executed in
`tls::tests::the_tls_material_type_has_no_field_for_a_trust_anchor`. What
`reqwest` itself trusts under that feature set was not executed and is not
claimed.

Consequence: in a default build, `danger_tls_no_verify = false` with specific
certificates means verification against an empty trust store, which no peer
certificate satisfies; with the feature enabled it means verification against
the public web PKI, which an internally issued certificate does not satisfy
either. The only configuration in which specific certificates and a working
connection coexist is the one named `danger`. Fail-closed, so nothing is
weakened; the safe setting simply has no reachable use. `011` KD-3.

### F-044 `defect`, confidence `high`

**The API channel's no-verify flag is read from the raft configuration.** Four
consumers make REST requests to a peer's `addr_api` and use three different
pairings of scheme flag and verification flag:

| caller | scheme from | verification from | client |
|---|---|---|---|
| `store/mod.rs:98-109`, `:195-206` | `tls_api` | `tls_api` | `build_http_client` |
| `start.rs:254-284` to `become_cluster_member` | `tls_raft` | `tls_raft` | `build_http_client` |
| `start.rs:114-118` to `split_brain_check::spawn` | `tls_api` | neither | `reqwest::Client::new()` |
| `start.rs:291-295`, then `client/mgmt.rs:186-190` | `tls_api` | `tls_raft` | `build_http_client` |

`start.rs:38-43` derives `tls_no_verify` from `node_config.tls_raft`;
`init.rs:275-276` turns the paired flag into `https` or `http` against
`node.addr_api`; `split_brain_check.rs:135` builds a default `reqwest::Client`
that honours no override.

**Source-established.** `011`'s acceptance block pins each call site's text.

Consequences, most likely first. A cluster with TLS on one endpoint and not the
other has a join sequence that speaks the wrong scheme to `addr_api` and cannot
form. A cluster using auto-certificates has a split-brain checker whose every
request fails verification, so the observability path of `010` B-8 reports
connection errors on an interval instead of memberships. And the API side's own
`danger_tls_no_verify` is doubly dead: unreachable from TOML (F-031) and ignored
by two of the four consumers. `011` KD-4.

### F-045 `contradiction`, confidence `high`

**The stated reason for not verifying certificates does not cover the endpoints
that send a bearer secret.** `hiqlite/src/tls.rs:20-23` and `hiqlite.toml:173-175`
both justify unverified certificates with a 3-way handshake that validates both
parties "without the secret ever being sent over the network".

That is accurate for the WebSocket channels, which use `HandshakeSecret`
(`hiqlite/src/network/handshake.rs`, at `network/api.rs:488` and
`network/raft_server.rs`). It is not accurate for the REST endpoints:
`network/mod.rs:64-76` (`validate_secret`) compares the `X-API-SECRET` **request
header** against `secret_api`, and that header is set on every `/cluster/*`,
`/listen` and `/backup` request (`init.rs:186`, `:458`, `:583`, `:683`, `:742`,
`split_brain_check.rs:140`, `client/mgmt.rs:73`, `client/listen_notify.rs:57`).

Consequence: under auto-certificates or any `danger_tls_no_verify`, those
requests carry `secret_api` in cleartext inside a TLS session whose server
certificate is not checked, so an attacker able to intercept the connection can
present any certificate and read the credential for the whole management
surface. Classed as a contradiction and not a defect because the code does what
it was written to do; an authored sentence describes a narrower channel than the
one it is printed beside. Whether to narrow the sentence, verify the API
channel, or move the REST endpoints onto the challenge-response is a security
design decision with three different costs and no default here.
**Source-established.** `011` KD-5.

### F-046 `evidence`, confidence `high`

**`HQL_TLS_AUTO_CERTS` was documented in one reference file only.** It appeared
in `hiqlite.toml:170-182` and in neither `hiqlite.env` nor anywhere else an
operator reads, although it is the single switch that turns certificate
verification off on both endpoints at once.

**Found and closed on 2026-09-21 by `011` section 5**, which adds it to
`hiqlite.env` with what it does, how it interacts with the per-endpoint
variables, and its parse behavior. The entry deliberately omits the
justification `hiqlite.toml:173-175` gives, because F-045 records that the
justification is true of one channel and false of the others, and copying it
into a second reference file would propagate the claim. Recorded rather than
performed silently, and retained as the record of the gap.

### F-047 `defect`, confidence `high`

**`try_get_log_entries` panics on an empty store instead of returning nothing.**
`hiqlite/src/store/logs/memory.rs:76-78` calls
`logs.front().expect("to have at least 1 entry in logs as long as end > 0")`
after the `end < start` early return, so a request for any range with a non-zero
end bound against an empty deque panics rather than returning an empty `Vec`.
OpenRaft 0.9.24 states the opposite requirement on the trait method: "Entry that
is not found is allowed"
(`openraft-0.9.24/src/storage/mod.rs:162-167`, read from the locked crate in the
local registry checkout).

Configuration: `cache` with `cache_storage_disk = false`, which is the only
configuration that selects this store (`hiqlite/src/store/mod.rs:161`).

**Source-established. Reachability not established**, for the same reason
F-023's is not: whether OpenRaft asks this store for a range it does not hold
was not determined by execution. Distinct from F-023, which is the `*i - 1`
underflow at an exclusive end bound of zero; this entry is about the empty-store
path that runs after that subtraction succeeds.

**Recorded 2026-09-21 by the cache-log repair proposal**
(`standards/spec/cache-log-repair-proposal.md` section 2.5), ahead of its owning
spec in the same way F-029 was: `007-cache-log-store` records four known defects
and now omits two. Reconciling both into `007` is queued under W-04 and is not
done by this change.

**Reconciled 2026-09-21.** `007` now records this as its KD-6. Both omissions
are closed; `007` records six known defects.

**Repaired 2026-09-21 by `020-cache-log-store-contract-repair`.** An empty deque
answers every range with no entries, and a partly overlapping range is clamped
to the intersection rather than asserted to be fully held (`020` B-4).

### F-048 `evidence`, confidence `high`

**Fifteen integration guarantees share one test result.**
`hiqlite/tests/cluster/` builds one binary whose `test_cluster`
(`main.rs:39-71`) delegates to `exec_tests` (`:73-229`), a flat sequence of
fifteen `?`-propagating phases. There are no sub-tests and no way to select,
skip or reorder a phase. The order is load-bearing and undeclared: phase 4
inserts the rows phases 12 and 15 check for, phase 8 seeds the cache value
phase 15 reads, phase 13 drops `_metadata` so phase 14 exercises the validation
bypass, and `check.rs:38-44` hard-codes six row ids left by phases 5 and 6.

Consequence: phase *n* passing means phases 1 to *n* passed in that order on
that tree, and a failure or stall at phase *n* leaves phases *n+1* to 15 unrun
and unreported. Realised twice already: `002` recorded a non-completion for a
guarantee whose test never ran, and the 2026-09-21 run recorded in F-051 leaves
eleven of fifteen guarantees unestablished for a reason unrelated to any of
them. Not a fault in any phase; a property of the composition.
**Observed by execution.** `012` KD-1.

### F-049 `defect`, confidence `high`

**The suite's success path exits the process while a second test may still be
running.** `hiqlite/tests/cluster/main.rs:65-68` calls `process::exit(0)` from
inside `test_cluster`. The binary contains two `#[tokio::test]` functions,
`test_cluster` and `learner_only::learner_only_node_stays_non_voter_and_becomes_ready`
(`learner_only.rs:9-38`), and libtest runs them in parallel threads of one
process by default; an executed run prints `running 2 tests`.

Consequence: if `test_cluster` finishes first, the process ends immediately. The
other test is terminated wherever it is, libtest never prints its result or the
summary line, and cargo sees exit status 0 and reports success. The reverse
exposure is `set_panic_hook` (`:231-254`), whose `process::exit(1)` turns a panic
in either test into a whole-binary failure attributed to neither. The authored
`// TODO sometimes the test gets stuck here` immediately above the exit records
that this path has been unreliable before.

The concurrency is **observed** (both clusters initialise within the same
millisecond in the 2026-09-21 run); the truncation is source-established and did
not fire in that run, because `learner_only` happened to finish first. Nothing
enforces that order. `012` KD-2.

### F-050 `defect`, confidence `high`

**The cluster health wait is unbounded, and so is the public client call
underneath it.** `hiqlite/tests/cluster/start.rs:92-125`
(`wait_for_healthy_cluster`) is `for i in 1..=3 { loop { sleep(1s); ... } }` with
no iteration cap and no deadline. `check.rs:10-11` reaches the same shape through
the public API: `Client::wait_until_healthy_db` and `wait_until_healthy_cache`
(`hiqlite/src/client/mgmt.rs:139-153`) are unbounded `loop`s over `is_healthy_*`
with a 500 ms sleep.

Consequence: a regression that stops a cluster forming is reported as an
unbounded hang rather than a failure, in a harness that imposes no timeout of
its own; CI then sits until its job limit and is killed without a diagnosis. The
same binary already contains the bounded form of this wait
(`learner_only.rs:67-80`: thirty attempts, then a real error), so both idioms sit
side by side.

The unbounded shape is source-established; that the suite can hang indefinitely
is **observed** (F-051). The library half is in `003`'s unit. `012` KD-3.

### F-051 `defect`, confidence `high`

**`Client::remote` returns before its event subscription exists, so an event
published soon after construction is dropped and `listen()` waits forever.**
`hiqlite/src/client/create.rs:163` calls `RemoteListener::spawn`, which is
`task::spawn(Self::handler(...))` followed by an immediate return of the receiver
(`hiqlite/src/client/listen_notify.rs:29-37`). The handler then connects an SSE
stream to `/listen` (`:45-62`), and the server registers the subscriber only when
that connection is accepted. `Client::remote` awaits none of it, returns no
readiness signal, and the client exposes none afterwards. `Client::listen`
(`:104-110`) is `recv_async().await` on an unbounded `flume` receiver with no
timeout.

**Observed by execution**, on 2026-09-21, in the run `012` section 5 records.
Client 1's listener began connecting at `19:28:51.645559`; the test published at
`19:28:51.885931` (`remote_only.rs:42-48`); the server registered client 1's
subscription at `19:28:52.060964` and client 2's at `19:28:52.118574`. The
publish therefore reached an empty subscriber set, `Notify` has no buffering or
replay, and both `listen()` calls blocked permanently. The losing window was
175 ms.

Consequence, beyond the test: any consumer that constructs a remote client and
publishes an event soon afterwards can lose it silently and then block forever
waiting for it.

**This is the cause of F-017's recorded non-completion.** Self-healing is phase
15 of 15 and was not reached because phase 11 does not return; the stall says
nothing about `self_heal.rs`. Not a flake: the ordering in `remote_only.rs` is
deterministic, and whether the race is lost depends only on whether the SSE
connection completes inside the ~120 ms between `Client::remote` returning and
the publish. The unit is `003`'s; recorded here because `012` is where it was
diagnosed. `012` KD-4 and section 5.

### F-052 `evidence`, confidence `high`

**Two of the three bad-migration fixtures are on disk with their assertions
commented out.** `hiqlite/tests/cluster/migration.rs:8-14` comments out the
`rust_embed` derives for `bad_1` and `bad_2`, and `:33-41` and `:121-126` comment
out the assertions that used them. The authored reason is at `:29-31`:
`#[should_panic]` does not work in an async helper called from another test. The
fixtures are still in the tree
(`tests/cluster/migrations/bad_1/no_leading_index.sql`,
`tests/cluster/migrations/bad_2/2_bad_start_index.sql`).

Consequence: the two migration-naming rules those fixtures exist to test, that a
file needs a leading integer index and that the sequence must start at 1, have
fixtures, a disabled test and no evidence. Only `bad_3`, the SQL-syntax case, is
exercised. W-10 owns the closure. Source-established. `012` KD-5.

**Closed 2026-09-21 by `014-schema-migration-contract`.** Both fixtures are now
executed, as ordinary `#[test]` functions against `Migrations::build` rather
than through `Client::migrate`:
`a_name_without_a_numeric_index_panics_with_the_other_rules_message` and
`an_index_set_that_does_not_start_at_one_panics`. That is the layer where
`#[should_panic]` works, which is what `hiqlite/tests/cluster/migration.rs:29-31`
could not reach. Running `bad_1` surfaced F-064. The entry is retained as the
record of the gap; `014` section 4 states what is still unevidenced, which is
the `.sql` suffix rule, the gap rule and the duplicate-index case, none of which
ever had a fixture.

### F-053 `limit`, confidence `high`

**No cluster test has ever run with TLS enabled.**
`hiqlite/tests/cluster/start.rs:76-81` sets `config.tls_raft = None` and
`config.tls_api = None` for every node the suite starts, with an authored
reason: TLS routes through `axum_server`, which has no graceful shutdown, which
the suite needs because it runs three nodes in one process.

Consequence, stated once so other specs can cite it rather than re-derive it:
`011`'s entire wire behavior, `010` B-5's TLS-dependent shutdown path, and
F-044's scheme mismatch are all unreachable by the only integration surface this
repository has. Classed as a limit because the boundary is deliberate, authored
and explained; what was missing is any record of what it costs.
Source-established. `012` KD-6.

### F-054 `defect`, confidence `medium`

**A restart race in the WAL is papered over by a sleep in the test.**
`hiqlite/tests/cluster/main.rs:140-142`: `// TODO if this next action comes too
fast, there will be a WAL log ID mismatch -> find out why and fix it`, followed
by `time::sleep(Duration::from_millis(1000))` before the first post-restart cache
write. A second 250 ms sleep at `:131-132` waits for the log sync task to notice
a closed channel.

Consequence: the restart guarantee of phase 12 holds only for a caller that waits
a second, and nothing in the library documents the wait. Recorded as a defect in
the code under test rather than as a test smell: the sleep is the evidence, not
the fault. **Confidence medium** because the mismatch itself was not reproduced;
the authored admission and the workaround are what is established. `012` KD-7.

### F-055 `defect`, confidence `medium`

**Two concurrently running tests share process-wide environment mutation.**
`hiqlite/tests/cluster/backup_restore.rs:13-42` sets and removes
`HQL_BACKUP_RESTORE` and `HQL_BACKUP_SKIP_VALIDATION` through
`unsafe { env::set_var }`, and `main.rs:47` removes the first at startup. These
are process-wide, and per F-049 the process is also running `learner_only`,
which starts three nodes that read the environment at startup. Nothing sequences
the two tests.

Consequence: a learner-only node can be constructed while a restore variable
belonging to the other test is set, which would start it on a backup image. The
window is narrow in practice, because `learner_only` finishes early and the
restore phases are late. **Not observed**; recorded because the ordering that
makes it safe is incidental and undeclared. `012` KD-8.

### F-056 `defect`, confidence `high`

**The local backup retention guard is inverted and deletes files that are not
backups.** `hiqlite/src/backup.rs:221-223`:

```rust
if !s.starts_with("backup_node_") && !s.ends_with(".sqlite") {
    continue;
}
```

A file is skipped only when it matches **neither** half, where the intent is to
skip unless it matches both. Consequence: any file in the backups directory whose
name ends in `.sqlite` and whose text after the last `_` parses as a plausible
Unix timestamp reaches the deletion branch, and so does any file whose name
starts with `backup_node_` regardless of suffix. `Client::backup_list_local`
(`hiqlite/src/client/backup.rs:148-151`) filters the same directory on the prefix
alone, and `dt_from_backup_name` (`backup.rs:248-278`) requires prefix, a second
underscore, the suffix and a parsable `i64`. Three predicates over one naming
convention, no two the same.

**Observed by execution.** `backup::tests::local_cleanup_deletes_files_that_are_not_backups`
writes `someone_elses_1704153600.sqlite` into a temporary directory and the sweep
deletes it. Latent rather than an incident, because the directory is hiqlite's
own; nothing prevents an operator from putting a file there, and
`HQL_BACKUP_RESTORE=file:` invites it by naming a path the restore then copies
into `backups/`. `013` KD-1.

### F-057 `defect`, confidence `high`

**A restore removes the live database before the replacement is in place.**
`hiqlite/src/backup.rs:354-368`. `restore_backup` validates the staged backup,
then `remove_dir_all`s the database, snapshot, lock-file and log directories,
then recreates the database directory, then `fs::copy`s the backup into it. A
failure of the copy, or of the `create_dir_all` or `set_path_access` between
them, leaves the node with neither its previous state nor the backup, and there
is no rollback. The copied file is never `sync_data`ed before the node starts on
it, so a crash between the copy and the writeback can leave a short image that
the next start accepts.

Same family as F-003 and F-004, in a different file: `002` records the
non-atomic publication of snapshots, this is the non-atomic publication of a
restored database. Source-established. `013` KD-2.

### F-058 `defect`, confidence `high`

**Every node that is not node 1 deletes its data directory before any restore has
succeeded.** `hiqlite/src/backup.rs:287-293`. Whenever `HQL_BACKUP_RESTORE`
parses to a known prefix, `restore_backup_start` on nodes 2 and 3 runs
`let _ = fs::remove_dir_all(node_config.data_dir.as_ref()).await` and returns
`Ok(false)`, so the node proceeds into a normal cluster join. This happens before
node 1 has pulled, validated or copied anything, and the two are not coordinated.

Consequence: a restore that fails on node 1, for instance because the `file:`
path does not exist or the object is not in the bucket, has already destroyed the
other nodes' state, so the cluster cannot fall back to what it had. The discarded
`Result` also makes a failed deletion invisible, and the node then joins with a
half-removed directory. Source-established. `013` KD-3.

### F-059 `defect`, confidence `high`

**Backup validation is one query, and a corrupt payload panics rather than
failing validation.** `hiqlite/src/backup.rs:379-398`. `is_metadata_ok` opens the
candidate, selects `data FROM _metadata WHERE key = 'meta'` and runs
`deserialize(&bytes).unwrap()` at `:391`, inside `spawn_blocking`. A `_metadata`
row that exists but does not decode as `StateMachineData` therefore panics the
blocking task and the caller sees a join error rather than "this backup is not
valid".

The validation itself checks only that the row exists and decodes: not the
schema, not the row counts, and not which cluster produced the image, which the
authored `TODO` at `:393` acknowledges. `HQL_BACKUP_SKIP_VALIDATION` disables
even that, comparing against the exact string `"true"`, so `"TRUE"` validates;
that direction is fail-closed. Source-established. `013` KD-4.

### F-060 `defect`, confidence `high`

**The post-restore log purge retries forever with no delay.**
`hiqlite/src/backup.rs:477-479`:
`while let Err(err) = state.raft_db.raft.trigger().purge_log(last_log).await { error!(...) }`.
No sleep, no attempt bound, no exit. Consequence: a purge that keeps failing, for
example because the Raft is shutting down, spins the task at full CPU while
emitting an error line per iteration. Every other loop in the same function
sleeps between 50 ms and 100 ms, and the snapshot trigger twelve lines above was
deliberately changed to bail out rather than loop. Source-established.
`013` KD-5.

### F-061 `defect`, confidence `high`

**The S3 configuration panics on a missing variable and validates no
credential.** `hiqlite/src/s3.rs:45-76`. Reading `HQL_S3_URL` successfully
commits `try_from_env` to five further `expect`s, a `parse().expect` for
`HQL_S3_PATH_STYLE`, and `Bucket::new(...).unwrap()` at `:70`, on the authored
assumption at `:47` that all values exist together. A single missing or
misspelled variable ends the process at configuration time, while the same
`Bucket::new` failure in `S3Config::new` (`:37-38`) is a returned `Error::S3`.

Separately, the `TODO` at `:40` records that no path checks the credentials, so a
wrong key is first discovered by the detached upload task in `create_backup`
(`store/state_machine/sqlite/writer.rs:817-842`), after the backup has been
acknowledged, as an error log. Same class as F-009 and F-042.
Source-established. `013` KD-6.

### F-062 `contradiction`, confidence `high`

**The backup cron failure message counts retries that were not attempted.**
`hiqlite/src/backup.rs:121-155`. The loop is `for _ in 0..retries` with
`retries = 5`, but only a forward-to-leader error sleeps and retries; every other
error logs and `break`s on the first attempt with `success` still false. The
message that then runs is `"Backup task failed after {} retries"` with the
literal 5. Consequence: an operator reading the log believes five backup attempts
were made and all failed, when one was. Classed as a contradiction between
authored text and behavior, not a defect. Source-established. `013` KD-7.

### F-063 `contradiction`, confidence `high`

**The local retention floor constant is an hour earlier than its comment.**
`hiqlite/src/backup.rs:196-197`: `let ts_min = 1704063600;` annotated
`// 2024/01/01 00:00:00`. That value is `2023-12-31T23:00:00Z`, which is that
midnight in CET; `1704067200` is the UTC one. Every timestamp it is compared
against comes from `Utc::now()`.

Nothing observable follows, because the constant's purpose is to be far in the
past as a guard against a trailing token that happens to parse as a small
integer. Recorded because the constant guards a deletion and its stated meaning
is what a reader would check it against. Source-established. `013` KD-8.

### F-064 `contradiction`, confidence `high`

**The panic for a malformed migration name names the wrong rule.**
`hiqlite/src/migration.rs:11-17`. The `split_once('_')` expect says names must
start with `<integer>_<migration_name>`; the `parse::<u32>()` expect says they
must start with an increasing integer with no gaps starting at index 1. For any
name that contains an underscore but whose first token is not a number, which is
the ordinary malformed case and exactly what the repository's own
`tests/cluster/migrations/bad_1/no_leading_index.sql` fixture is, the **second**
message fires. An operator whose file is named `create_users.sql` is told about
gaps and start indices, and the message stating the actual rule is reachable only
for a name with no underscore at all.

**Observed by execution.**
`migration::tests::a_name_without_a_numeric_index_panics_with_the_other_rules_message`
asserts the message that fires. `014` KD-1.

### F-065 `defect`, confidence `high`

**A duplicate migration index is reported as a gap.**
`hiqlite/src/migration.rs:52-59`. The check is
`migration.id != res[len - 1].id + 1`, which a second file carrying an
already-seen id fails, so `1_a.sql` and `1_b.sql` panic with
`"Migration index has a gap: 1 does not follow 1"`. A gap and a duplicate are
different deployment mistakes with different fixes, and the message describes the
one that did not happen. Source-established; no fixture reaches it, and `014` D-2
records why none was added. `014` KD-2.

### F-066 `defect`, confidence `high`

**Every migration validation failure is a panic, and the signature cannot carry
an error.** `Migrations::build` (`hiqlite/src/migration.rs:8-65`) returns
`Vec<Migration>` and enforces all four file-name rules with `expect` and
`panic!` (`:13`, `:16`, `:30`, `:42`, `:54`). It is called from `Client::migrate`
(`hiqlite/src/client/migrate.rs:32`, `:81`), which returns `Result<(), Error>`,
so a caller that handles migration errors correctly still cannot handle a
malformed migration set: the process ends instead.

Consequence, and the reason this is recorded rather than filed as a style note:
it is why the repository's own bad-fixture assertions were commented out.
`hiqlite/tests/cluster/migration.rs:29-31` states that `#[should_panic]` does not
work in that context, which is true of an async helper, and the underlying cause
is that the failure is a panic rather than the `Err` the surrounding test already
knows how to assert. Same class as F-009 and F-042, and also a public-API
question, which is W-17's. Source-established, with three of the five panic sites
executed. `014` KD-3.

### F-067 `defect`, confidence `high`

**The proxy panics on its first route registration and never binds.**
`hiqlite/src/server/proxy/mod.rs:60` registers `"/metrics/:raft_type"`, which is
axum 0.7 path syntax. The pinned axum is 0.8.9 (`Cargo.toml:31`, `Cargo.lock`),
which rejects a segment beginning with `:` at `Router::route` with
"Path segments must not start with `:`. For capture groups, use `{capture}`."
`Router::route` panics rather than returning an error, so `start_proxy` ends the
process before `Client::remote`, before the notify tasks, and before the bind.

Consequence: `hiqlite proxy` does not run at all on this tree. The node
registers the same capture correctly (`hiqlite/src/start.rs:166-180`,
`{raft_type}`), so this is one call site left behind by the 0.8 upgrade rather
than an unmigrated codebase. It was not caught because the `server` feature is
linted but never enabled for a CI test run (F-019), and the failure is a runtime
panic that no compile check reaches.

**Observed by execution**, twice:
`server::proxy::tests::the_proxy_metrics_route_is_rejected_by_the_pinned_axum`
panics with the real handler and the same `nest`, and
`the_same_capture_in_zero_eight_syntax_is_accepted` shows the node's spelling is
accepted. `015` KD-1.

### F-068 `defect`, confidence `high`

**The proxy compares the API secret in non-constant time.**
`hiqlite/src/server/proxy/handlers.rs:84-96` is a private copy of
`hiqlite/src/network/mod.rs:63-76`, and the two differ in one line. The node's is
`!constant_time_eq(state.secret_api.as_bytes(), secret.as_bytes())`; the proxy's
is `state.secret_api.as_bytes() != secret.as_bytes()`, a byte-slice comparison
that returns on the first differing byte.

Consequence: on `/listen` and `/cluster/metrics/*` the proxy's rejection time
varies with how long a prefix of the supplied header matched, which is the
condition `constant_time_eq` exists to remove. `HEADER_NAME_SECRET` is duplicated
alongside it (`handlers.rs:21`), which is how the two copies came to diverge: the
hardening landed on one of them.

Source-established; no timing measurement was taken and none is claimed.
Reachable only once F-067 is fixed, which is the order a repair has to consider.
`015` KD-2.

### F-069 `defect`, confidence `high`

**A valid path value reaches an unconditional panic.** `RaftType`
(`hiqlite/src/app_state.rs:28-36`) derives `Deserialize` with
`#[serde(rename_all = "lowercase")]` and has an `Unknown` variant, so the path
segment `unknown` deserializes successfully.
`hiqlite/src/server/proxy/handlers.rs:68` then matches
``RaftType::Unknown => panic!("neither `sqlite` nor `cache` feature enabled")``.
The message describes a build configuration; the input that reaches it is a
request.

The same arm appears six further times in `hiqlite/src/helpers.rs` (`:33`, `:56`,
`:69`, `:94`, `:124`, `:156`), which no spec claims, and the node routes
`/cluster/add_learner/{raft_type}`, `/become_member/{raft_type}`,
`/membership/{raft_type}`, `/metrics/{raft_type}` and `/stream/{raft_type}`
(`start.rs:166-180`) into handlers taking `Path<RaftType>`, so the shape is not
confined to the proxy.

Mitigation, so the severity is not overstated: `validate_secret` runs before the
match on both surfaces, so the caller must already hold `secret_api`. Under
unwinding the consequence is a dropped connection; under `panic = abort` it is
the process, which is `010` B-7's split. Source-established. `015` KD-3.

### F-070 `defect`, confidence `high`

**The proxy's documented default configuration file can never be loaded.**
`hiqlite/src/server/args.rs:39-40` documents `$HOME/.hiqlite/hiqlite.env` as the
default `--config-file`, and `clap` does not expand shell variables.
`hiqlite/src/server/proxy/config.rs:20-22` passes the literal straight to
`dotenvy::from_filename_override`, which fails to find a relative path called
`$HOME/.hiqlite/hiqlite.env` and logs at debug.
`hiqlite/src/server/config.rs:9-13` special-cases the identical sentinel for the
`serve` path, so the mechanism exists and was applied to one of the two.

Consequence: an operator who follows the help text and puts a file at
`~/.hiqlite/hiqlite.env` gets a debug-level "config file not found", then a panic
from `HQL_SECRET_API not found` (`proxy/config.rs:45`), with no indication that
the file was looked for in the wrong place. Source-established. `015` KD-4.

### F-071 `evidence`, confidence `high`

**The generated configuration is a second reference file, and it has drifted.**
`hiqlite/src/server/config.rs:89-433` embeds a 344-line TOML template that
`hiqlite generate-config` writes, duplicating `hiqlite.toml`, which `009` claims.
Nothing compares them.

They currently differ by eight keys, all present in the reference and absent from
the generated file: `listen_addr_raft`, `listen_addr_api`,
`tls_auto_certificates`, `secrets_file`, and the four `rate_limit_*` keys.

Consequence: an operator who starts from `generate-config` never sees that the
listen addresses can be set separately from the advertised ones (`010` B-4), never
sees `tls_auto_certificates`, which is the switch F-046 was filed about and which
`011` had just finished documenting in the other reference file, and never sees
`secrets_file` or the rate limits.

**Observed by execution.**
`server::config::tests::the_generated_config_omits_keys_the_reference_file_documents`
pins the exact eight in both directions, so the drift can neither widen nor
silently close unnoticed. Classed as `evidence` for the same reason F-012 is: two
artifacts that must agree, with no check, is thin support rather than a mismatch
between a claim and behavior. `015` KD-5.

### F-072 `defect`, confidence `high`

**`start_proxy` panics where its signature promises an error.**
`hiqlite/src/server/proxy/mod.rs` returns `Result<(), Error>` and then `expect`s
the crypto provider installation (`:19-21`), `expect`s the socket address parse
(`:70`), and `unwrap`s the serve future in both the TLS and plaintext branches
(`:78`, `:83`). A port already in use, a malformed listen address or a TLS
material failure therefore ends the process instead of returning through
`server()` to `main`. Same class as F-038 and F-040, and part of W-22.
Source-established. `015` KD-6.

### F-073 `limit`, confidence `high`

**The proxy binds `0.0.0.0` with no way to change it.**
`hiqlite/src/server/proxy/mod.rs:68`: `format!("0.0.0.0:{}", config.listen_port)`.
The port is configurable through `LISTEN_PORT`; the interface is not. The node
the proxy fronts has `listen_addr_api` for exactly this (`009`, `010` B-4), so an
operator who binds the node to a private interface cannot do the same for the
proxy. Classed as a limit because nothing claims otherwise; recorded because the
asymmetry with the node is nowhere documented. Source-established. `015` KD-7.

### F-074 `contradiction`, confidence `high`

**The proxy's validation message names a secret the proxy does not have.**
`hiqlite/src/server/proxy/config.rs:55-59` rejects a `secret_api` shorter than 16
characters with `"'secret_raft' and 'secret_api' should be at least 16 characters
long"`. The proxy's `Config` has four fields and none is `secret_raft`; the
message is the node's, copied. Consequence: an operator is told to fix a setting
that does not exist in the file they are editing. **Observed by execution**
(`server::proxy::config::tests::proxy_validation_covers_two_fields_and_names_a_third`).
`015` KD-8.

### F-075 `contradiction`, confidence `high`

**A declared module contains nothing but commented-out code.**
`hiqlite/src/server/cache.rs` is 21 lines, every one a comment, and
`hiqlite/src/server/mod.rs:9` declares `mod cache;`. The declaration claims a
component of the server binary that does not exist, which is why this is classed
as a contradiction between two authored texts rather than as dead code.

Recorded rather than deleted: deleting it is a change and `015` is an adoption,
and the commented type is a two-variant cache enum that would answer what the
server binary's `Empty` cache (`server/mod.rs:24`) was meant to become.
Source-established. `015` KD-9.

### F-076 `limit`, confidence `high`

**The server binary ignores `RUST_LOG`.** `hiqlite/src/server/logging.rs:14-26`
calls `with_env_filter(level.as_str())`, which builds the filter from that string
rather than from the environment. Consequence: `--log-level` is the only control,
and the per-target directives an operator would reach for, including silencing
`openraft`, are unavailable. Nothing claims `RUST_LOG` works, so this is a limit
and not a contradiction; it is recorded because every other Rust service an
operator runs does read it. Source-established. `015` KD-10.

### F-077 `defect`, confidence `high`

**`CacheVariants` cannot be derived on a generic enum, or on one with
data-carrying variants.** `hiqlite-derive/src/into_cache_data.rs:25` emits
`impl ::hiqlite::CacheVariants for #impl_generics #name #ty_generics
#where_clause`, with the generics after `for` instead of after `impl`. For a
non-generic enum `#impl_generics` is empty and the header is accidentally
correct; for `enum Generic<T>` it expands to
`impl ::hiqlite::CacheVariants for <T> Generic<T>`, which is not Rust.

Separately, `Self::#id => #idx` (`:17`) is a unit-variant pattern, so a variant
carrying data is rejected by the generated code.

**Observed by execution**, both halves, by compiling probes against this tree:

```
error: expected `::`, found `Generic`
error: proc-macro derive produced unparsable tokens
```
```
error[E0533]: expected unit struct, unit variant or constant,
              found tuple variant `Self::One`
```

Consequence: a cache-variant enum must be a plain unit-variant, non-generic enum,
which every current consumer happens to be, and neither restriction is documented
or diagnosed. The first is a one-token fix; the second is a design choice that
should be a `compile_error!` rather than an E0533 pointing at the derive. The
probes are not committed, because a source file that fails to compile cannot live
in a crate CI builds; `016` D-2 records that and why no compile-fail harness was
added. `016` KD-1.

### F-078 `defect`, confidence `high`

**`core::option::Option` is treated as a non-optional type.**
`hiqlite-derive/src/from_row.rs:104-119` (`is_field_ty_opt`) strips a leading
`std` segment and an `option` segment, but not `core`, so
`core::option::Option<i64>` fails the check and the field takes the non-optional
branch: `row.get::<i64>("a").into()`.

Consequence: in a codebase that spells the path through `core`, a nullable column
mapped through `from_i32`, `from_i64`, `from_string` or `parse` asks the row for
a bare value and fails at runtime rather than yielding `None`. **Observed by
execution**, both spellings
(`from_row::tests::the_core_spelling_of_option_is_not_recognised`). `016` KD-2.

### F-079 `limit`, confidence `high`

**The derived row conversion cannot report a failure.**
`hiqlite-derive/src/from_row.rs:92` implements
`::std::convert::From<&mut ::hiqlite::Row<'_>>`, not `TryFrom`, so the generated
`from` has no error channel. Three of the seven `#[column]` attributes therefore
expand to a panic: `flatten` to `try_from(...).unwrap()` (`:28`), `parse` to
`parse().unwrap()` (`:60`, `:63`), and `from_i32` to
`try_from(i).expect("column value does not fit into i32")` (`:33-37`).

Consequence: a row whose value does not fit, does not parse, or whose nested type
rejects it panics inside a query result mapping, surfacing wherever `query_map`
was called rather than as an `Err` the caller can handle. Classed as a limit: the
trait choice is deliberate and `From` is what makes `query_map` ergonomic; what
is missing is any statement of the cost and a fallible counterpart. **Observed by
execution** for all three expansions
(`from_row::tests::the_fallible_attributes_expand_to_unwrap_or_expect`).
`016` KD-3.

### F-080 `limit`, confidence `high`

**A non-struct input panics instead of producing a diagnostic.**
`hiqlite-derive/src/from_row.rs:81-82` and
`hiqlite-derive/src/into_cache_data.rs:21` are `unimplemented!()`. Consequence:
`#[derive(FromRow)]` on an enum ends the compilation with `proc-macro derive
panicked: not implemented`, with the span on the derive and no statement of what
is supported, where a `syn::Error::to_compile_error` would point at the item and
name the supported shape. The same applies to `CacheVariants` on a struct or
union. `lib.rs:12` carries a `TODO` about returning a result, which is about the
macro's internal shape rather than about its diagnostics. **Observed by
execution** for the `FromRow` half
(`from_row::tests::an_enum_input_panics_instead_of_emitting_a_diagnostic`).
`016` KD-4.

### F-081 `evidence`, confidence `high`

**Fifty-four example assertions are compiled and never evaluated.** The six
example crates contain 54 `assert*` calls: 16 in `walkthrough`, 15 each in
`sqlite-only` and `derive-complex-types`, 5 in `cache-only`, 2 in `bench` and 1
in `external-state-machine`. They are self-checking programs, not illustrations
with printed output. The only thing CI does with them is `just clippy-examples`
(`.github/workflows/code_style.yaml`), which compiles.

Consequence: a library change that breaks what an example README promises
compiles, lints and merges; the examples' value as documentation is exactly the
part CI does not verify. Related to F-019, which is the same shape for the
`dashboard` and `server` features.

**Partly observed.** The assertion count is source-established. The four examples
that take no arguments were run by hand on 2026-09-21 and all four exited `0`:
`external-state-machine`, `derive-complex-types`, `cache-only` and `sqlite-only`.
`walkthrough` and `bench` take arguments and were not run. Those runs are a dated
observation, not a control, which is the gap this entry is about. `017` KD-1.

### F-082 `contradiction`, confidence `high`

**The examples are the one clippy step that does not deny warnings.**
`justfile:112-123`: `clippy-examples` runs bare `cargo clippy`, while
`cargo clippy -- -D warnings` and every line of `just clippy` deny them, and the
workflow step immediately above this one is named "Clippy (deny warnings)".

Consequence: a warning introduced in an example is reported and ignored, inside a
recipe that `just verify` runs beside two that fail on one. Source-established;
the 2026-09-21 local run of all six crates produced zero warnings, so nothing is
currently hidden by it. `017` KD-2.

### F-083 `defect`, confidence `high`

**The four tracked example lockfiles are stale, and the documented build rewrites
them.** Running `just clippy-examples` on this tree modified all four tracked
`Cargo.lock` files. Each gained `constant_time_eq`, which `hiqlite` now depends
on, and `examples/cache-only/Cargo.lock` gained thirteen packages including the
whole `rusqlite`, `libsqlite3-sys` and `serde_rusqlite` chain, so its lock
predates a change in what its feature set pulls in.

Consequence: the dependency set a reader, or a scanner, sees in a committed
example lockfile is not the set CI builds, and CI discards its own corrected
version on every run. The inconsistency extends to which crates have a lockfile
at all: neither `examples/derive-complex-types` nor
`examples/external-state-machine` has a tracked one, the first having an
untracked file on disk and the second none until it is built, while
`.gitignore:7` ignores `Cargo.lock` so the four tracked ones were force-added and
nothing in the repository records the rule.

**Observed by execution**: the rewrite was produced, its contents recorded, and
then reverted, because updating dependency locks is not something an adoption
does (`017` D-2). Same family as F-012 and F-071: artifacts that must agree, with
no check. `017` KD-3.

### F-084 `defect`, confidence `high`

**The dashboard's single-flight hashing lock is dropped before it guards
anything.** `hiqlite/src/dashboard/password.rs:13` is
`let _ = IS_HASHING.write().await;`. A `_` pattern drops its value at the end of
the statement, so the write guard is released immediately and the argon2 work
that follows is not serialized.

Two authored texts state the opposite. `password.rs:7-9`: "only a single password
hash at a time is allowed, the dashboard is just for debugging. prevents
brute-fore effectively". `dashboard/session.rs:36-37`, reasoning about the global
login cooldown: "the single-flight lock in `password::verify_password` still
serializes the actual hashing".

Consequence: concurrent login attempts each start their own argon2id at
`m=32768, t=2, p=2`, so N simultaneous requests cost N times 32 MiB and N hashing
threads. The control meant to bound that does nothing, and the cooldown F-087
describes engages only **after** a failure has been computed, so the first burst
is unbounded. The fix is `let _guard =`.

**Observed by execution.**
`dashboard::password::tests::the_single_flight_lock_is_released_before_any_hashing`
acquires the lock exactly as `verify_password` does, then acquires it again.
`018` KD-1.

### F-085 `defect`, confidence `high`

**The unauthenticated dashboard fallback panics on a multi-byte path.**
`hiqlite/src/dashboard/static_files.rs:28`:
`let path_ending = &path[path.len().saturating_sub(4)..];` slices at a byte index
without asking whether it is a character boundary. `http::Uri` accepts raw UTF-8
in a path, verified against the pinned `http` crate, so
`GET /dashboard/\u{20ac}abc` reaches `static_files::handler`, which is the
`/dashboard` **fallback** (`start.rs:215`) and therefore requires no session, and
panics with `byte index 2 is not a char boundary`.

Consequence: any client that can reach the API port of a node with the dashboard
enabled can panic a request task without authenticating. Under unwinding the
connection dies; under `panic = abort` the node does, which is `010` B-7's split.
The dashboard tree exists only when `password_dashboard.is_some()`
(`start.rs:189`), which bounds exposure to deployments that configured it.

**Observed by execution**
(`dashboard::static_files::tests::a_multibyte_path_panics_the_fallback`).
`018` KD-2.

### F-086 `defect`, confidence `high`

**The same character-boundary mistake in the dashboard query classifier.**
`hiqlite/src/dashboard/query.rs:24`: `let sql_start = sql[..7].to_lowercase();`,
guarded at `:14` by `sql.len() < 8`. The guard stops the out-of-range case and
not the boundary one: a statement whose seventh byte falls inside a multi-byte
character, for example `aaaaaa\u{20ac}`, panics. `post_query` builds the string
with `String::from_utf8_lossy` over the raw request body
(`dashboard/handlers.rs:112`), so the input is whatever was sent.

Behind the `Session` extractor, so the caller must be logged in, which is why it
is separated from F-085 rather than folded into it. Recorded because it is the
same defect in the same module, which makes it a pattern rather than a slip.
Source-established. `018` KD-3.

### F-087 `defect`, confidence `high`

**The global login cooldown is a denial of service against the operator.**
`hiqlite/src/dashboard/session.rs:33-54` and `:174-183`. After any failed
password, **every** login is rejected with `429` for five seconds, with no client
identity involved. The authored justification at `:33-37` is that "there is no
per-client state to spoof or exhaust".

That is true and it is not the exposure. The global lock is itself the
exhaustible resource: an unauthenticated client sending one wrong password every
five seconds keeps the dashboard permanently unloginable for the real operator,
at one request per five seconds. The cost asymmetry runs the wrong way, because
the attacker's request is rejected at `:175` before any hashing while the
operator is locked out.

Recorded as a defect rather than a design preference because the code states its
own threat model in a comment and the stated model does not cover this. The
answer is a policy choice with at least three forms (per-client cooldown,
exponential backoff, accepting the exposure), which is why `018` D-1 does not
pick one. Source-established; no availability test was written. `018` KD-4.

### F-088 `defect`, confidence `high`

**A dashboard read that does not begin with one of three keywords is replicated
as a write.** `hiqlite/src/dashboard/query.rs:24-27` lowercases the first seven
bytes and treats the statement as a read only if that prefix starts with
`select`, `explain` or `pragma`. So `WITH x AS (SELECT 1) SELECT * FROM x`,
`VALUES (1)`, and any statement preceded by a comment such as
`/* note */ SELECT 1` are classified as writes and sent through `client_write` to
be applied on every node.

Consequence: a read issued from the dashboard can take the write path, occupy the
Raft, and be rejected by the non-deterministic-function guard for containing a
function that would have been accepted on the read path. Nothing is corrupted; a
read is charged as a cluster-wide write. Source-established. `018` KD-5.

### F-089 `defect`, confidence `high`

**A malformed dashboard password ends the process at startup.**
`hiqlite/src/dashboard/mod.rs:41`:
`String::from_utf8(b64_decode(&b64).unwrap()).unwrap()`. A
`HQL_PASSWORD_DASHBOARD` that is not base64, or that decodes to non-UTF-8, panics
inside `DashboardState::from_env`, on the configuration path.

The `Err` arm four lines below handles the variable being **absent** gracefully,
disabling the dashboard with a warning, so two adjacent cases of the same
misconfiguration are handled in opposite ways. Same class as F-009, F-042 and
F-061, and part of W-22. Source-established. `018` KD-6.

### F-090 `evidence`, confidence `high`

**The dashboard UI has one test and nothing runs it.**
`dashboard/tests/smoke.spec.ts` loads the login page against a preview server
with no backend and asserts it hydrates without page errors, which is a
reasonable smoke test. `dashboard/package.json` exposes it as
`npm run test:smoke`, and that string appears **nowhere else**: no `just` recipe,
no workflow step. The `justfile` touches `dashboard` only to build it.

Consequence: the dashboard's single automated check is opt-in and, on the
evidence of the repository, opted out of. Same shape as F-081 for the examples,
and a further consequence of F-019. Source-established. `018` KD-7.

### F-091 `limit`, confidence `high`

**A dashboard session cannot be revoked.** The cookie is an encrypted
`{ created, expires }` (`hiqlite/src/dashboard/session.rs:76-80`, `:113-129`)
with no identity, no nonce, no server-side record, no rotation and no logout
route. Validity is `expires < now` and the lifetime is 3600 seconds (`:24`,
`:157-164`).

Consequences: changing `HQL_PASSWORD_DASHBOARD` does not invalidate a live
session; a leaked cookie is valid for up to an hour and cannot be withdrawn; and
the only revocation available is rotating the encryption keys, which invalidates
every encrypted value in the process and not only sessions. Classed as a limit: a
stateless one-hour session on an ops surface is a defensible design, and what is
missing is the statement of what it costs. Source-established. `018` KD-8.

### F-094 `defect`, confidence `high`

**A local dashboard build inflates the coverage denominator and stales the
committed index.** `npm run build` creates `dashboard/.svelte-kit`, which
`dashboard/.gitignore:5` ignores. The pinned indexer's npm package walk does not
consult `.gitignore`: with the directory present, `spec-spine index coverage`
reports a denominator of **292** instead of 236, having taken 56 generated `.js`
and `.d.ts` files as package source, and `spec-spine check` reports the committed
shards stale. Deleting the directory returns it to 236 exactly, on an otherwise
identical tree.

Consequence: the drift check W-13 asks for **must** build the dashboard, and on
this tool a build makes `spec-spine index coverage` disagree with the committed
shards, so the check cannot be added without either excluding `.svelte-kit` or
cleaning it afterwards. Today, a contributor who runs `just build ui` sees
`just spine-check` fail for a reason unrelated to anything they changed. CI never
builds the dashboard, so the committed index is unaffected in practice, which is
why this went unnoticed.

Same family as F-016, and blocked on the same missing capability: whether
`resolver_exclusions` or any other key reaches a path inside a declared npm
package was not probed here, and W-14 records that the equivalent question for
`hiqlite/static` has no answer at this pin. **Observed by execution.**
`019` KD-3.

### F-095 `defect`, confidence `high`

**The dashboard's three cooldown tests race each other.**
`hiqlite/src/dashboard/session.rs:197-231` are three pre-existing tests over one
process-global `NEXT_LOGIN_ALLOWED` (`:39`), which libtest runs in parallel
threads by default. `cooldown_locks_and_unlocks` unlocks and asserts unlocked
while `cooldown_response_reports_remaining_wait` locks, so their assertions
contradict each other whenever they interleave.

**Observed by execution, 2026-09-21**: eight consecutive runs of
`cargo test -p hiqlite --features dashboard --lib dashboard::session::tests`
produced three failures, once with two of the three failing.

Consequence: enabling the `dashboard` feature in CI, which F-019 and several
findings in `018` point towards, introduces a flaky test on the first day. `018`'s
acceptance block passes `--test-threads=1` for that group, which is correct for an
acceptance block and is **not** a fix: `cargo test` does not. Recorded rather than
repaired, because serializing them changes tests the adoption did not write and
the right fix is a per-test lock or a reset fixture, which is a small design
choice. `018` KD-9.

## Summary by class

Class is what a finding **is**. State is what has **happened** to it. They are
separate fields and neither is read off the other: a `defect` may be open or
repaired, and a `gap` may be closed at M1 while its entry is retained as a
record.

| class | ids | count |
|---|---|---|
| `defect` | F-001 to F-006, F-009, F-021 to F-024, F-027 to F-029, F-031, F-036 to F-044, F-047, F-049 to F-051, F-054 to F-061, F-065 to F-070, F-072, F-077, F-078, F-083 to F-089, F-092 to F-095 | 56 |
| `contradiction` | F-018, F-030, F-032 to F-034, F-045, F-062 to F-064, F-074, F-075, F-082 | 12 |
| `gap` | F-010, F-013 | 2 |
| `evidence` | F-011, F-012, F-017, F-019, F-046, F-048, F-052, F-071, F-081, F-090 | 10 |
| `limit` | F-007, F-008, F-015, F-016, F-026, F-035, F-053, F-073, F-076, F-079, F-080, F-091 | 12 |
| `decision` | F-014, F-020, F-025 | 3 |

### Summary by state (2026-09-21)

| state | ids | count |
|---|---|---|
| repaired | F-001, F-002, F-030 | 3 |
| closed, entry retained | F-013 (at M1, by wave 1), F-011 and F-046 (2026-09-21, by `010` and `011`), F-052 (by `014`) and F-015 (by `017`), both 2026-09-21 | 5 |
| open | everything else: F-003 to F-010, F-012, F-014, F-016 to F-029, F-031 to F-045, F-047 to F-051, F-053 to F-095 | 87 |

A finding's state answers whether the thing it records is still in the tree, and
nothing else. F-030 is repaired because its contradiction is gone; the optional
process decision it surfaced is W-24's, and a work item's being undecided has
never been a reason to hold a finding open.

Eighty-seven open, of which four carry a dated disposition recording what has
moved since they were written. Two were appended on 2026-09-20: F-016 (dashboard
now visible, the generated and vendored denominator problem unresolved) and
F-018 (the `AGENTS.md` half resolved, the three-way naming collision unchanged).
Two more were appended on 2026-09-21: F-017, whose unclaimed-files half is
closed at M1 by `012` while its recorded non-completion is diagnosed as F-051 and
still in the tree, and F-012, whose missing drift check was finally run and came
back negative twice (F-092, F-093). F-015 carried a fourth until `017` closed it outright on the same
day. A disposition narrows an entry; it does not close it.

Open **defects**, which is the subset a repair workstream draws from:
F-003, F-004, F-005, F-006, F-009, F-021, F-022, F-023, F-024, F-027, F-028,
F-029, F-031, F-036 to F-044, F-047, F-049 to F-051, F-054 to F-061, F-065 to
F-070, F-072, F-077, F-078, F-083 to F-089, F-092 to F-095. Fifty-four of the
fifty-six defects; F-001 and F-002 are the two repaired.

**Reclassified on 2026-09-19**, after each class test was applied rather than
assumed: F-007 and F-008 from `defect` to `limit`, because a stated contract with
a caller obligation is not a mismatch; F-010 and F-013 from `contradiction` and
`decision` to `gap`, because incomplete ownership and unclaimed territory are
migration state rather than faults; and the consequences of F-001, F-002, and
F-014 rewritten against traced source, with three earlier claims withdrawn in
place.

Thirty defects: six from the pilot specs, F-009 from the whole-project pass,
five (F-021 to F-024, F-027) found by wave 1, F-028 found while tracing the
`008` repair, F-029 found on 2026-09-20 while re-reading the memory log store
against the locked trait, F-031 and F-036 found by the configuration adoption on
2026-09-20, F-037 to F-040 found by the node-lifecycle adoption on 2026-09-21,
F-041 to F-044 found by the transport-security adoption on the same day,
F-047 found on 2026-09-21 while tracing the cache log store against the locked
trait for the W-04 proposal, F-049 to F-051, F-054 and F-055 found on
2026-09-21 by the cluster integration adoption, three of them by running the
suite rather than by reading it, F-056 to F-061 found on the same day by the
backup and object-storage adoption, F-065 and F-066 by the schema migration
adoption, F-067 to F-070 and F-072 by the server-binary and proxy adoption,
whose first finding is that the proxy subcommand panics before it binds, and
F-077 and F-078 by the derive-macro adoption, F-083 by the examples adoption,
which found it by running the documented build, and F-084 to F-089 and F-095 by the
dashboard adoption, two of them found by writing the first test their file had
ever had and one by running the ones it already had eight times, and F-092 to
F-094 by the dashboard build adoption, all three found by running the rebuild
W-13 asks for.
F-013 is closed at M1 by wave 1, F-011 by `010`, and F-046 by `011` in the same
change that recorded it; all three are retained as records rather than deleted. No finding in this register authorizes
a repair; each repair is a separate governed change with its own spec and
evidence.

**Repaired so far.** F-001 and F-002, by
`008-wal-append-completion-notification` on 2026-09-19, and F-030, by `005` D-7
on 2026-09-20. A repaired entry is annotated in place and keeps its identifier,
its class, and its original text, so the baseline a repair was reviewed against
stays readable.

**Fifty-four defects are open**: F-003 to F-006, F-009, F-021 to F-024,
F-027 to F-029, F-031, F-036 to F-044, F-047, F-049 to F-051, F-054 to F-061,
F-065 to F-070, F-072, F-077, F-078, F-083 to F-089, F-092 to F-095. Two earlier revisions of this paragraph were
stale: one said ten against a table of fourteen, and the next said twelve after
`009` had already added F-031 and F-036. The arithmetic is fifty-six recorded
minus the two repaired, and this paragraph is the one that has to be recomputed
whenever the class table changes. F-021 through F-024 and F-029 are a separate
cache-log repair that was deliberately not bundled into `008`.
