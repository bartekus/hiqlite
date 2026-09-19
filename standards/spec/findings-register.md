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

**Classes.** `defect` is behavior confirmed at source that the corpus would not
have chosen. `contradiction` is two authored texts, or a text and the code,
that disagree. `evidence` is a claim whose support is thinner than a reader
would assume. `limit` is a deliberate boundary with a stated consequence.
`decision` is an open design or ownership question that no document answers.

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

Configurations: all `LogSync` modes. Consequence: the storage method reports an
error while the completion callback conveys success for the same entries, so the
two events disagree about whether the log advanced. Evidence and its limits:
`hiqlite-wal/src/writer.rs:483-496`
(`append_failure_is_returned_but_completion_still_fires`) asserts this behavior,
so it is pinned as current, not incidental. No test establishes what OpenRaft
does with the conflicting pair. Recorded at `001` section 8, bullet 1.

Next action: repair under an amending spec that decides the callback error
contract. See the proposed first implementation task.

### F-002 `defect`, confidence `high`

**A blocking persistence failure after the acknowledgement drops the callback
without reporting an error to OpenRaft.** `hiqlite-wal/src/writer.rs:307-315`.
The `complete_append(...)?` call propagates a flush error out of the writer
loop, so the callback is never invoked and no explicit `log_io_completed(Err)`
is sent.

Configuration: `LogSync::Immediate`, where the persistence step is
`flush_blocking`. Consequence: the caller has already been told the append was
accepted, and the completion event that would report the failure is lost with
the loop. Evidence and its limits:
`writer.rs:465-479` (`persistence_failure_suppresses_completion_callback`)
proves the helper suppresses the callback; it does not exercise loop exit, nor
what OpenRaft observes afterwards. Recorded at `001` section 8, bullet 2.

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

### F-007 `defect`, confidence `high`

**Ordinary cluster writes have no durable operation id or response receipt.**
Lost-response retries may repeat effects. `003` section 7. The external
state-machine engine implements a bounded receipt window; ordinary cluster mode
does not. Consequence: at-least-once effects on retry after a lost response,
with no duplicate suppression.

### F-008 `defect`, confidence `high`

**The reconnect buffer window and the public timeout are hardcoded and
unrelated.** The reconnect buffer is ten seconds, fixed in code, and starts only
after a successful connection; the public timeout is separately fixed at 120
seconds. `003` section 7. Consequence: the buffer cannot be tuned to a
deployment's reconnect profile, and the two constants can disagree about how
long an outcome remains recoverable.

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

### F-010 `contradiction`, confidence `high`

**The configuration contract is dispersed, and the claimed unit does not bound
it.** `001` claims `hiqlite/src/config.rs` as a `file` unit, which invites a
reader to treat that file as the configuration surface. It is not: the tree
holds 46 `env::var` reads, 20 of them in `config.rs`. The remaining 26 live in
`backup.rs`, `s3.rs`, `tls.rs`, `init.rs`, `split_brain_check.rs`,
`server/proxy/config.rs`, `dashboard/mod.rs`, and `dashboard/session.rs`, and
include the `HQL_DANGER_RAFT_STATE_RESET`, `HQL_BACKUP_SKIP_VALIDATION`,
`HQL_INSECURE_COOKIE`, and `HQL_TLS_*_DANGER_TLS_NO_VERIFY` escape hatches.
`hiqlite/src/config_toml.rs` is unclaimed although it loads the same contract
from `hiqlite.toml`.

Consequence: no single authored text states what configures hiqlite, and the
ownership ledger implies a boundary the code does not respect. Evidence and its
limits: counted by grep across `hiqlite/src` and `hiqlite-wal/src`; the
semantics of each variable were not individually verified.

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

### F-013 `decision`, confidence `high`

**A stated ownership territory has no spec.** Constitution VII and `000`
section 7 both name "the SQLite **and cache** state machines, their snapshots,
and recovery integration" as hiqlite-owned and specifiable. `002` claims
`hiqlite/src/store/state_machine/sqlite/`. Nothing claims
`hiqlite/src/store/state_machine/memory/` (6 files, 2192 lines: the cache state
machine, the KV, dlock, TTL, and notify handlers) or `hiqlite/src/store/logs/`
(2 files, 238 lines, the OpenRaft log-store adapter and its in-memory variant).

Consequence: the boundary statement promises coverage the ledger does not
carry, which is exactly the confusion constitution XII now forbids. Next action:
wave 1.

### F-014 `decision`, confidence `medium`

**A watchdog converts a checker failure into process termination, by design and
without a stated contract.** `hiqlite/src/split_brain_check.rs:15-21` spawns a
task that every 600 seconds asserts the split-brain checker task has not
finished, with the comment "TODO just a safety net until everything runs super
smooth and stable". `check_split_brain` loops forever, so the assertion can only
fire after the checker has already panicked; under `panic = "abort"` the
assertion then aborts the process.

Consequence: a fault in an observability path escalates to node termination up
to ten minutes later, which may be intended fail-fast behavior or may be
leftover scaffolding. The comment says the latter. Evidence and its limits: read
at source; not executed. This is recorded as a decision rather than a defect
because the intent is genuinely unclear and the owner may want fail-fast.

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

## Summary by class

| class | ids | count |
|---|---|---|
| `defect` | F-001 to F-009 | 9 |
| `contradiction` | F-010, F-018 | 2 |
| `evidence` | F-011, F-012, F-017, F-019 | 4 |
| `limit` | F-015, F-016 | 2 |
| `decision` | F-013, F-014, F-020 | 3 |

Nine defects, of which eight were already recorded by the pilot specs and one
(F-009) is new. No finding in this register authorizes a repair; each repair is
a separate governed change with its own spec and evidence.
