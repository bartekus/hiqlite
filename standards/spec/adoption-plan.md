# hiqlite whole-project adoption plan (proposed)

A dependency-ordered proposal for extending the specification corpus from the
bounded pilot to the whole project, with a reconciled inventory of what exists
and measurable completion criteria.

**Where this is referenced.** Deliberately nowhere in tier 1 or tier 2. A
pointer was drafted into constitution XII and removed: the coupling gate refused
it (`C-001`), because `005` does not own `standards/spec/constitution.md` and
`000` does. Clearing it would have meant either declaring an `extends` edge from
a proposal spec onto the constitution, or editing `000`, and neither is
warranted to add a cross-reference to a document that is not adopted. If the
owner wants the constitution to point here, that is a foundation change under
`000`, made after this plan is accepted.

**Status: proposed, not adopted.** Nothing here is scheduled, ratified, or in
progress. Every wave below becomes real only when its spec is written, reviewed,
and merged under its own authorization, and several depend on owner decisions
listed at the end. This document changes no behavior, enables no gate, and
claims no territory beyond itself.

Measured on 2026-09-19 against `spec-spine` revision
`aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f` at integration head `2d58a32`.

---

## 1. Three milestones, never collapsed

Constitution XII separates current coverage, intended adoption scope, and
enforcement settings. This plan adds the same separation over time. A wave
reaches these in order, and a later milestone never implies an earlier one is
better than it is.

1. **M1, ownership recorded.** A spec claims the units and the ledger resolves
   them. `spec-spine index owner <path>` names an owning spec. This says who is
   responsible. It says nothing about whether the behavior is described.
2. **M2, behavior specified with evidence.** The spec states the contract, names
   the configuration each claim holds under, states the limit of its evidence
   next to the claim, and carries an acceptance block that fails before the work
   and passes after. Behavior it would not have chosen is under a recognized
   `known-defects` heading.
3. **M3, enforcement enabled.** A configuration change makes the absence of M1 a
   refusal rather than a report. This is the only milestone that changes what
   CI rejects, it is an owner decision, and it is reached last.

A wave at M1 with no M2 is honest migration progress. A wave declared at M2
without an acceptance block that can fail is not.

---

## 2. Reconciled inventory

### 2.1 The denominator, reconciled

`spec-spine index coverage` reports **57 of 131 source files specifically
claimed (43.5%)**. The repository tracks **333 files**. The two numbers describe
different sets, and the gap is configuration, not neglect.

| in the 131 | files | claimed |
|---|---|---|
| `hiqlite` package: 97 `.rs` + 12 generated `.js` under `hiqlite/static` | 109 | 38 |
| `hiqlite-wal` package `.rs` | 11 | 11 |
| `hiqlite-derive` package `.rs` | 3 | 0 |
| declared `coverage.governed_scope`: `ARCHITECTURE.md`, `justfile`, `spec-spine.toml`, `AGENTS.md`, `standards/spec/**/*.md` | 8 | 8 |
| **total** | **131** | **57** |

The 202 tracked files the denominator omits:

| omitted | files | why |
|---|---|---|
| `dashboard/` | 81 | `npm_workspaces` names root `package.json` and `pnpm-workspace.yaml`; neither exists, the manifest is `dashboard/package.json`, and `standalone_npm_packages` is empty (F-016) |
| `examples/` | 35 | workspace-excluded in `Cargo.toml:4` and `standalone_rust_workspaces` is empty (F-015) |
| `hiqlite/` non-source | 51 | 18 `.gz`, 18 `.br`, 6 `.sql`, 4 `.css`, and one each `.toml`, `.png`, `.md`, `.json`, `.html` |
| `.derived/` | 13 | compiler output, correctly excluded |
| root files outside `governed_scope` | 9 | `Cargo.toml`, `Dockerfile`, `hiqlite.toml`, `hiqlite.env`, `README.md`, `CHANGELOG.md`, `LICENSE`, `.gitignore`, `.dockerignore` |
| `specs/` | 5 | the corpus itself, not its subject |
| `.github/`, `.cargo/`, non-`.rs` in the two smaller crates | 8 | outside every declared scope |
| **total omitted** | **202** | |

These figures describe integration head `2d58a32`, before this document and the
findings register existed. Adding the two of them, both claimed by
`005-adoption-assessment-and-plan`, moves the reported figure to **59 of 133
(44.4%)**, with the declared governance scope at 10 of 10 claimed. The rise is
two new governance documents that arrived with an owner, not two source files
that became governed, which is the distinction constitution XII draws and a
worked example of why a coverage percentage is not a progress metric.

**What the percentage means.** 43.5% is the share of a configured denominator
that some spec names. It is not test coverage, not behavioral completeness, and
not a share of the repository: as a fraction of tracked files, claimed territory
is 57 of 333, about 17%. The denominator currently **includes** minified build
output and **excludes** more than a thousand lines of authored Svelte and
TypeScript, so correcting it (section 5) must precede quoting any coverage
figure as progress.

### 2.2 Areas

`auth` = authored source; `gen` = generated output; `3p` = third-party; `excl` =
intentionally excluded.

| # | area | kind | responsibility | current owner | contracts and configuration | evidence today | gap | proposed treatment |
|---|---|---|---|---|---|---|---|---|
| A1 | `hiqlite-wal/src/` (11 `.rs`) | auth | WAL append, sync, vote, truncate, purge, recovery | `001` (directory unit) | `LogSync::Immediate` / `ImmediateAsync`, `auto-heal` | focused writer unit tests | no power-cut harness; F-001, F-002 | keep; repair under an amending spec |
| A2 | `hiqlite/src/store/state_machine/sqlite/` (7) | auth | internal SQLite state machine, snapshots, restore | `002` | `sqlite`, `auto-heal`, `backup` | focused storage tests + `self_heal.rs` | F-003 to F-006; cluster test non-completion (F-017) | keep; repairs separately |
| A3 | `hiqlite/src/client/` (16), `query/` (3), `network/` (8), `server/proxy/stream.rs` | auth | client consistency, retries, transport | `003` | `sqlite`, `cache`, `listen_notify` | focused lib tests | F-007, F-008; no transport-fault test | keep; extend for the proxy in wave 4 |
| A4 | `hiqlite/src/external_state_machine.rs` | auth | externally committed engine, receipts | `002`, `003` (co-authority) | `external-state-machine` | 5 focused tests | caller boundary held by review | keep |
| A5 | `hiqlite/src/config.rs` | auth | configuration surface | `001` (file unit) | 20 of 46 `env::var` reads | none focused | F-010, F-020: contract is wider than the file | wave 2 reclaims as a contract |
| A6 | `hiqlite/src/store/state_machine/memory/` (6, 2192 lines) | auth | cache state machine, KV, dlock, TTL, notify | **none** | `cache`, `dlock`, `counters`, `listen_notify_local`, `in-memory-snapshots` | in-crate tests only | F-013: named as owned, never claimed | wave 1 |
| A7 | `hiqlite/src/store/logs/` (2, 238 lines) | auth | OpenRaft log-store adapter, memory variant | **none** | `__cluster` | none focused | F-013 | wave 1 |
| A8 | `hiqlite/src/init.rs` (907), `start.rs` (334), `app_state.rs`, `split_brain_check.rs` (164) | auth | node lifecycle, join, split-brain observation | **none** | `HQL_DANGER_RAFT_STATE_RESET`, `HQL_SPLIT_BRAIN_INTERVAL` | none focused | F-009, F-011, F-014 | wave 2 |
| A9 | `hiqlite/src/tls.rs` (231) | auth | transport security material | **none** | `HQL_TLS_*`, incl. `DANGER_TLS_NO_VERIFY` | none focused | unspecified security surface | wave 2 |
| A10 | `hiqlite/src/backup.rs` (482), `s3.rs` (128) | auth | scheduled backup, restore, object storage | **none** | `backup`, `s3`, `HQL_BACKUP_*` | `backup.rs`, `backup_restore.rs` cluster tests (unclaimed); S3 skipped in CI | F-009, F-019 | wave 3 |
| A11 | `hiqlite/src/migration.rs` (98) + 6 test `.sql` fixtures | auth | schema migration ordering and validation | **none** | `sqlite` | `migration.rs` cluster test (unclaimed) | fixtures outside denominator | wave 3 |
| A12 | `hiqlite/src/server/` minus `proxy/stream.rs` (11) | auth | server binary, args, proxy, logging, password | **none** | `server`, `dashboard` | not tested in CI (F-019) | F-009 (`HQL_SECRET_API`) | wave 4 |
| A13 | `hiqlite/src/dashboard/` (8, 1022 lines) | auth | dashboard HTTP, session, password, query | **none** | `dashboard`, `HQL_PASSWORD_DASHBOARD`, `HQL_INSECURE_COOKIE` | not tested in CI (F-019) | security surface unspecified | wave 4 |
| A14 | `dashboard/src/` (69) + configs | auth | Svelte dashboard source | **none**, invisible | `vite`, `svelte.config.js`, CSP directives | `tests/smoke.spec.ts` (Playwright), not in CI | F-016 | wave 4, after layout fix |
| A15 | `dashboard/src/spow/` (5) + `.wasm` | 3p | proof-of-work client | **none** | bundled | none | third-party, not authored here | declare third-party; reference, never claim |
| A16 | `hiqlite/static/` (54: 12 `.js`, 18 `.gz`, 18 `.br`, 4 `.css`, `.html`, `.json`, `.png`) | gen | built dashboard, embedded by `rust-embed` | **none**, 12 counted as source | `adapter-static`, `precompress: true`, `#[folder = "static"]` | no drift check (F-012) | F-012, F-016 | wave 4: a build contract, then exclude from the source denominator |
| A17 | `hiqlite-derive/src/` (3) | auth | `FromRow`, `IntoCacheData` derive macros | **none** | `macros` | `derive-complex-types` example | proc-macro contract unstated | wave 5 |
| A18 | `hiqlite/src/error.rs` (408), `lib.rs`, `macros.rs`, `helpers.rs`, `http_client.rs` | auth | public API surface and error taxonomy | **none** | all features | compile-time only | the error contract callers match on is unspecified | wave 5 |
| A19 | `hiqlite/tests/cluster/` (16 files, 2295 lines) | auth | integration evidence for A1 to A4 | `002` claims `self_heal.rs` only | `cache_storage_disk=false`; `test-no-s3` | F-017 | wave 6 |
| A20 | `examples/` (6 crates, 35 files) | auth | executable user documentation | **none**, invisible | own manifests; `just clippy-examples` | compiled in CI, never claimed | wave 6, after layout fix |
| A21 | `Cargo.toml`, `Dockerfile`, `.cargo/config`, `.github/workflows/code_style.yaml`, `justfile` release recipes | auth | build, release, packaging, CI | `000` owns `justfile` and the spec-spine workflow | MSRV, feature matrix, `panic = "abort"` | CI is the evidence | the other workflow and the image are unclaimed | wave 6 |
| A22 | `README.md`, `CHANGELOG.md`, `hiqlite.toml`, `hiqlite.env` | auth | user-facing documentation and config reference | **none** | documents `HQL_*` | F-011 | wave 2 for the config reference; docs stay on the bypass floor |
| A23 | `LICENSE`, `.gitignore`, `.dockerignore`, `.npmrc`, lockfiles | excl | repository hygiene | `000` owns `.gitignore` | none | none | permanently excluded, except `.gitignore` |
| A24 | `.derived/` (13) | gen | compiler output | `000` by the authored/derived boundary | determinism | `check` | none | permanently excluded from claims |

---

## 3. Waves

Dependency-ordered. Each wave is one or more draft specs, each retroactive
unless stated, each with its own acceptance block and review.

### Wave 1: the storage territory the boundary already promises

**Scope.** A6, A7. **Proposed specs:** `006-cache-state-machine-and-handlers`,
`007-log-store-adapter`.
**Existing ownership to extend.** `extends` on `002` for the shared snapshot
path; `depends_on` `001` for the WAL boundary. No re-`establishes`.
**Retroactive or forward-looking.** Retroactive; the code is years old.
**Behavioral and evidence review required.** Cache TTL eviction and dlock
expiry under `cache` and `dlock`; whether `in-memory-snapshots` changes the
snapshot contract; what the memory log store is for and whether it is
production-reachable.
**Acceptance boundary.** Focused lib tests per handler under the exact features
named; no cluster run.
**Known defects retained.** Any found; F-013 closes on M1.
**Owner decisions or external dependencies.** None. This wave is unblocked and
is the natural first expansion.

### Wave 2: the configuration contract, node lifecycle, and transport security

**Scope.** A5, A8, A9, A22 (the config reference half).
**Proposed specs:** `008-configuration-contract`, `009-node-lifecycle-and-split-brain`,
`010-transport-security-material`.
**Existing ownership to extend.** `008` `extends` `001`'s `hiqlite/src/config.rs`
unit rather than re-establishing it, and claims `config_toml.rs`, `hiqlite.toml`,
and `hiqlite.env` so the contract is owned as a contract (F-010, F-020).
**Retroactive or forward-looking.** Retroactive, with one forward-looking
decision carried into wave 2's review: F-009's panic-versus-startup-error
question. The decision is recorded; the change is a separate repair.
**Behavioral and evidence review required.** Every `HQL_*` variable: default,
validation, failure mode, and whether it is documented. The `DANGER_*` escape
hatches need explicit statements of what they disable. F-014's watchdog needs an
intent ruling.
**Acceptance boundary.** A test per validation rule for the variables the spec
claims, plus a documentation assertion that every claimed variable appears in
`hiqlite.toml` or `hiqlite.env`.
**Known defects retained.** F-009 and F-011 are recorded, not fixed here.
**Owner decisions.** OD-3 (F-014 intent).

### Wave 3: durability services

**Scope.** A10, A11. **Proposed specs:** `011-backup-and-object-storage`,
`012-schema-migrations`.
**Existing ownership to extend.** `extends` `002` for the snapshot and restore
path they share; `depends_on` wave 2 for the `HQL_BACKUP_*` and `HQL_S3_*`
contract.
**Retroactive or forward-looking.** Retroactive.
**Behavioral and evidence review required.** What `HQL_BACKUP_SKIP_VALIDATION`
disables and when it is safe; the cron contract; restore ordering against A2's
recorded defects; migration index rules against the three `bad_*` fixtures.
**Acceptance boundary.** Migration validation is testable cheaply from the
existing fixtures. Backup and restore evidence must state that CI skips S3
(F-019) rather than implying coverage it does not have.
**Known defects retained.** F-019's S3 gap.
**Owner decisions.** None.

### Wave 4: the product surfaces, and the generated-asset contract

**Scope.** A12, A13, A14, A15, A16.
**Proposed specs.** `013-server-binary-and-proxy`, `014-dashboard-service`,
`015-dashboard-build-contract`.
**Existing ownership to extend.** `013` `extends` `003` on
`hiqlite/src/server/proxy/stream.rs`, which `003` already claims.
**Retroactive or forward-looking.** Retroactive for `013` and `014`.
`015` is **forward-looking**: the drift check it specifies does not exist. It
declares its units with `planned: true` and `origin.retroactive: false`, and it
is the first spec in this corpus that does.
**Behavioral and evidence review required.** Dashboard session, cookie, and
password handling is the fork's only authentication surface and is not exercised
by CI tests (F-019): it needs a real behavioral review, not a claim. The
provenance chain for A16 must be written down: `dashboard/src` to
`@sveltejs/adapter-static` (`pages` and `assets` at `../hiqlite/static`,
`precompress: true`) to committed bytes to `rust-embed #[folder = "static"]` to
the served response.
**Acceptance boundary.** For `015`, a command that rebuilds the dashboard and
fails when the committed output differs; it must fail on today's tree if the
output is stale, which is the point.
**Known defects retained.** F-012 closes only when `015` reaches M2.
**Owner decisions.** OD-2 (generated assets), and the layout change of section 5
for `dashboard/` to be claimable at all.

### Wave 5: the public surface

**Scope.** A17, A18. **Proposed specs.** `016-public-api-and-error-taxonomy`,
`017-derive-macros`.
**Existing ownership to extend.** None; these are new territory. `constrains`
is the right edge for the API freeze aspect if the owner wants one.
**Retroactive or forward-looking.** Retroactive.
**Behavioral and evidence review required.** Which `Error` variants are public
contract and which are internal; what a caller may match on; what the derive
macros guarantee about column mapping and type conversion.
**Acceptance boundary.** Compile-fail and trybuild-style tests for the macros;
a test that pins the public error variants a caller matches on.
**Known defects retained.** Any found.
**Owner decisions.** Whether the public API is frozen for this fork.

### Wave 6: the evidence surface and the build

**Scope.** A19, A20, A21. **Proposed specs.** `018-integration-evidence-surface`,
`019-examples-as-documentation`, `020-build-release-and-ci`.
**Existing ownership to extend.** `018` `extends` `002`'s claim on
`self_heal.rs`; `020` `extends` `000`'s `justfile` and workflow units.
**Retroactive or forward-looking.** Retroactive, except any new harness.
**Behavioral and evidence review required.** F-017's recorded cluster-test
non-completion has to be resolved or restated honestly before `018` can claim
the suite. That may need one bounded cluster diagnostic, which is the only
expensive run this plan anticipates.
**Acceptance boundary.** `019` can assert that every example builds, which CI
already does. `018` must not claim the suite passes if it does not.
**Known defects retained.** F-017 until the run is resolved.
**Owner decisions.** OD-1 (examples and dashboard in scope), and the layout
change of section 5.

---

## 4. What "whole-project adoption complete" means

All five must hold, and each is separately checkable.

1. **Every authored source file in the corrected denominator has an owning
   spec.** `spec-spine index coverage` reports zero unclaimed, with the
   denominator corrected per section 5 so that it includes `dashboard/` and
   `examples/` and excludes generated output.
2. **Every claimed unit is specified, not merely owned.** Each spec carries an
   acceptance block that fails before its work and passes after, names the
   configuration each claim holds under, and states its evidence limits beside
   its claims. This is the milestone that cannot be measured by a percentage
   and is confirmed by review.
3. **Justified exclusions are written down, not implied.** The permanent
   exclusion list is A23 (`LICENSE`, `.dockerignore`, `.npmrc`, lockfiles),
   A24 (`.derived/`, compiler output under the authored/derived boundary),
   A15 (`dashboard/src/spow/`, third-party, referenced and never claimed), and
   build output under A16, which is governed through its build contract rather
   than by claiming its bytes. Each exclusion names its reason in a spec.
4. **Generated output is governed by its source and its build, never by claiming
   the artifact.** A spec claims the generator, the configuration, and the
   verification command; the artifact is `references`-only. This is how A16 is
   completed, and it is the general rule for anything else generated later.
5. **Enforcement is on and demonstrated.** Section 5's ladder is fully climbed
   and each rung was verified by a probe, not by reading the configuration.

Milestones 1 and 2 are independent: reaching 1 alone is ownership without
description, and the corpus must not report it as adoption complete.

---

## 5. The enforcement ladder

Four controls, evaluated separately. **None is enabled by this document.**

### Rung 0 (prerequisite): correct the denominator

Until this is done, every coverage number is measured over a set that includes
minified build output and excludes the dashboard source (F-012, F-015, F-016).

- Declare the six example crates through `layout.standalone_rust_workspaces`.
- Declare `dashboard/package.json` through `layout.standalone_npm_packages`.
- Remove generated output from the source denominator, either by
  `coverage.governed_scope_exclusions` or by an index exclusion for
  `hiqlite/static/`.

The exact key semantics must be **probed against the pinned revision** before
adoption, the way `004` D-3 probed the bypass floor. Expect the reported
percentage to move sharply, and in both directions: the dashboard adds authored
files to the denominator, and removing build output takes 12 unclaimed files out.

### Rung 1: `coupling.require_ownership = true`

Turns unclaimed source into a `C-002` refusal. Safe only after waves 1 to 6 reach
M1 for every non-excluded path. Verification: not by reading the config, but by
a probe pull request that edits a path deliberately left unclaimed and confirming
the gate refuses, plus one that edits a claimed path with its owning spec and
confirming it passes.

### Rung 2: `spec-spine index coverage --fail-on-untraced` in CI

Turns the coverage report into a gate. Depends on rung 0 and rung 1. Verification:
a probe branch that adds an unclaimed file and confirms CI fails on it.

### Rung 3: `coupling.bypass_prefixes`

Currently `[]`, and the built-in floor (13 entries, confirmed by `spec-spine
config show`) is non-removable. The only justified addition is generated output
once its build contract exists (wave 4). **Source paths are never added.**
Verification: `spec-spine config show` attributes each entry as built-in or
adopter-added, so the diff is auditable.

### Rung 4: explicit-claim overrides

Already active and already verified: an explicit, ownership-bearing unit claim
overrides the floor, which is why `000`'s claims on
`standards/spec/constitution.md`, `.github/workflows/spec-spine.yaml`, and
`.gitignore` are enforced. This rung needs no change; it needs protection.
`004` KD-1 records that dropping one of those claims silently moves the path
from enforced to exempt with no diagnostic. A coverage gate does not catch it,
because an unclaimed floor path is what the floor is for.

### Forward-looking specs: the gates already support them

Probed on 2026-09-19 against the pinned revision, then reverted:

- A unit declared `{ kind: file, path: "...", planned: true }` whose file does
  not exist compiles with **0 warnings**, lints clean, and passes
  `spec-spine check --fail-on-unresolved --fail-on-warn` (exit 0).
- The same unit **without** `planned: true` raises `W-001` and makes that same
  check **refuse with exit 1**.
- `origin.retroactive: false` compiles cleanly.

So a forward-looking spec can declare territory it has not written without
weakening any gate, and no workflow conflict exists. One limit to know:
`spec-spine index owner <planned path>` reports no owner while the file is
absent, so a planned claim records intent and enforces nothing until the file
exists, at which point ownership and coupling attach normally.

---

## 6. Owner decisions that block progress

### OD-1: are `examples/` and `dashboard/` in scope?

They are 116 of 333 tracked files and are currently invisible to the ledger by
configuration, not by decision.
**Alternatives.** (a) Both in scope: examples as governed executable
documentation, dashboard as a specified product surface. (b) Examples in scope,
dashboard declared permanently excluded as a separate product. (c) Both
excluded, and "whole-project" is redefined to mean the Rust libraries.
**Implications.** (a) is the only reading under which "whole-project adoption"
is true as stated, and it is the only one that puts the fork's sole
authentication surface under a spec. (c) leaves 35% of the repository ungoverned
while the corpus claims completeness.
**Recommendation: (a).** It sets rung 0's shape and waves 4 and 6.

### OD-2: how are generated assets governed?

`hiqlite/static` is committed build output with no drift check (F-012).
**Alternatives.** (a) Keep it committed and add the wave 4 build contract plus a
CI drift check. (b) Stop committing it and build the dashboard in CI and in the
release image. (c) Leave it as is.
**Implications.** (a) preserves the current workflow, in which `cargo install`
works without Node, and closes the drift hole; it costs one CI job that needs
Node. (b) is cleaner but changes how the crate is consumed and is a runtime and
packaging change, which this task does not authorize. (c) leaves the shipped
dashboard unverifiable against its source.
**Recommendation: (a).**

### OD-3: what is the split-brain watchdog for?

F-014: a task that aborts the process up to ten minutes after the checker dies,
labeled a temporary safety net in the code.
**Alternatives.** (a) Intended fail-fast: specify it, document the abort, keep
it. (b) Leftover scaffolding: record it as a defect and repair it under wave 2.
**Implications.** Only the owner knows the intent; guessing here would put a
speculative claim in a spec, which constitution IX forbids.
**Recommendation: ask before wave 2 is written.** No default.

### OD-4: ratification sequencing

Carried from PR #3, restated because it gates everything above. The in-place
correction route for the constitution, contract, and templates closes when `000`
is approved.
**Alternatives.** (a) Ratify `000` last, after waves 1 to 6 have exercised it.
(b) Ratify `000` now to lock the freeze surface, and accept that further
constitutional corrections then require a drafted-then-approved amending spec.
**Implications.** This assessment found four corrections to tier-1 and tier-2
text in one pass. Under (b) each would have needed its own amending spec and an
owner ratification. Under (a) the freeze surface is not yet final, which is the
cost.
**Recommendation: (a).** Ratify `001` through `004` whenever they are ready;
hold `000` until the corpus has been exercised by at least wave 1.

---

## 7. The first implementation task this plan recommends

Not started, and not authorized by this document.

**Repair the WAL append and completion error contract (F-001 and F-002).**

Chosen over a demonstration because it is a confirmed defect at a claimed unit
with an existing test that pins the defective behavior, so the repair is
measurable, bounded, and already inside governed territory.

- **Intended behavior.** A failed append must not report completion. When
  `append_result` is `Err`, `complete_append` acknowledges the error and returns
  without invoking the completion callback. When the blocking persistence step
  fails after a successful append acknowledgement, the writer reports the
  failure to OpenRaft explicitly rather than exiting the loop and dropping the
  callback. Which explicit form that takes is the decision the spec must make
  and record; `001` section 8 already says the callback error contract is
  undecided.
- **Regression tests.** `append_failure_is_returned_but_completion_still_fires`
  currently asserts the defect and must be replaced, not deleted, by a test
  asserting the new contract, with the old name retired in the amending spec so
  the change is visible. Add a test that drives a blocking persistence failure
  through the writer loop rather than the helper, closing the evidence gap
  F-002 names.
- **Amendment relationships.** A new spec `amends: ["001-wal-durability-and-completion"]`
  with `amends_sections` naming the append and completion anchors, and
  `co_authority` over `hiqlite-wal/src/` for the duration. `001` section 8
  already requires that future work on this "MUST amend this contract rather
  than silently rewriting its baseline", so the edge is prescribed by the spec
  being amended, not chosen here.
- **Acceptance boundary.** `cargo test -p hiqlite-wal --lib` for the writer
  tests, old and new, under both `LogSync` modes the spec names. No cluster run.
  The acceptance must fail on today's tree and pass after.
- **Out of scope for that task.** F-003 to F-008, the power-cut harness, and
  anything in `002` or `003`.
