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
   next to the claim, and carries an acceptance block that **characterizes
   accurately and detects meaningfully**: every line asserts a specific claimed
   behavior and would fail if that behavior changed. These are retroactive
   adoption specs over working code, so their acceptance is expected to pass the
   moment it is written; a manufactured before-state failure proves nothing and
   is not required. The fail-then-pass standard belongs to behavioral repairs,
   which must demonstrate a regression test failing against the implementation
   being repaired and passing after, and for which a command that merely errors
   on the base because the test is absent there is not that demonstration
   (`standards/spec/templates/spec-template.md`). Behavior the spec would not
   have chosen goes under a recognized `known-defects` heading, and the test for
   that heading is a mismatch between what the code promises and what it does,
   not the availability of a different design.
3. **M3, enforcement enabled.** A configuration change makes the absence of M1 a
   refusal rather than a report. This is the only milestone that changes what
   CI rejects, it is an owner decision, and it is reached last.

A wave at M1 with no M2 is honest migration progress. A wave declared at M2
without an acceptance block that can fail is not.

---

## 2. Reconciled inventory

### 2.1 The denominator, reconciled

**Rung 0 is done.** The inventory configuration was corrected on 2026-09-19 after
probing the pinned tool in an isolated checkout; `000` section 12.1 records what
each key actually does. Numbers below are after that change.

| | before | after |
|---|---|---|
| denominator | 134 | **226** |
| specifically claimed | 60 | **60** |
| reported share | 44.8% | **26.5%** |
| packages discovered | 3 | **10** |

**The share fell because the denominator grew, not because anything was
unclaimed that was claimed before.** The numerator is unchanged at 60. This is
the clearest available demonstration that a coverage percentage measures a
configured set, not progress.

Where the 92 new denominator files come from:

| added | files | mechanism |
|---|---|---|
| `dashboard` `.ts` and `.js` | 24 | `standalone_npm_packages = ["dashboard"]` |
| example crate `.rs` | 7 | `standalone_rust_workspaces`, six crates |
| authored dashboard `.svelte`, `.css`, `.html`, build configs, and example `.sql` and `Cargo.toml` | 61 | `coverage.governed_scope` globs |

The 45 `.svelte` files reach the denominator only through the third row: the
pinned npm walk counts `.ts` and `.js` and nothing else. All of the above are
also in `index.extra_hashed_inputs`, which is a **separate** mechanism that
affects freshness and no denominator; without it a byte change in newly governed
source would leave the committed index reporting `fresh`.

The denominator, reconciled against `git ls-files` (339 tracked):

| in the 226 | files |
|---|---|
| `hiqlite` package: 97 `.rs` + 12 generated `.js` | 109 |
| `hiqlite-wal` `.rs` | 11 |
| `hiqlite-derive` `.rs` | 3 |
| `dashboard` package `.ts` + `.js` | 24 |
| six example crates `.rs` | 7 |
| declared scope (`coverage.governed_scope`) | 72 |
| **total** | **226** |

The 113 tracked files outside it:

| omitted | files | why |
|---|---|---|
| `hiqlite/static` compressed and binary output | 43 | generated; `.gz`, `.br`, `.css`, `.png`, `.json`, `.html` |
| `examples/` non-source | 17 | lockfiles, configs, `.gitignore`, README |
| `.derived/` | 15 | compiler output, correctly excluded |
| root files outside `governed_scope` | 9 | `Cargo.toml`, `Dockerfile`, `hiqlite.toml`, `hiqlite.env`, `README.md`, `CHANGELOG.md`, `LICENSE`, `.gitignore`, `.dockerignore` |
| `hiqlite/tests/cluster/migrations` `.sql` fixtures | 6 | test data |
| `specs/` | 6 | the corpus itself, not its subject |
| `dashboard/` other and binary assets | 7 | `.npmrc`, lockfile, `.gitignore`, `.wasm`, `.png` |
| `.github/` | 3 | two workflows plus `FUNDING.yml` |
| crate manifests, READMEs, `.cargo/config.toml` | 7 | manifests and docs |
| **total** | **113** |

**Known distortion, not fixable by configuration.** 12 of the 109 files in the
`hiqlite` package row are generated `.js` under `hiqlite/static`. No probed key
removes package-discovered source from the denominator, so those 12 are
permanently unclaimed and the reported figure is understated by exactly that
much. Quote the number with this sentence attached.

**What the percentage means.** 26.5% is the share of a configured denominator
that some spec names. It is not test coverage and not behavioral completeness.

An earlier revision also quoted a repository-wide "about 17%". That figure is
**withdrawn**: its numerator was resolved inside the package denominator and was
never reconciled against the other tracked paths, several of which are claimed
while others can never be. No repository-wide ownership percentage is quoted
here until every tracked path is independently resolved to an owner or to a
recorded exclusion.

**Third-party, generated, and authored are kept apart.** `dashboard/src/spow/`
(5 files: wasm bindings and their declarations) is vendored third-party and is
hashed, so drift is detected, but no spec will claim it. `hiqlite/static` is
generated. Everything else under `dashboard/src` and `examples/*/src` is
first-party authored source and none of it is hidden by an exclusion: there are
no entries in `governed_scope_exclusions`, and `resolver_exclusions` is
unchanged from its defaults.

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
   acceptance block that fails if the behavior it names changes, names the
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

Turns unclaimed source into a `C-002` refusal. Safe only once every
non-excluded path has reached M1, which is the end state of the waves rather
than any single one of them; this is the one control that genuinely requires all
of them, and it is unrelated to the ratification question of OD-4. Verification: not by reading the config, but by
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

## 6. Owner decisions

OD-1 through OD-3 were **decided by the owner on 2026-09-19** and are recorded
here as settled direction for this plan. They are direction, not ratification:
nothing about them approves a spec.

### OD-1: are `examples/` and `dashboard/` in scope? **Decided: yes, both.**

They are 116 of 333 tracked files and were invisible to the ledger by
configuration rather than by decision. Both are in the adoption target: examples
as governed executable documentation, the dashboard as a specified product
surface, which also places the fork's only authentication surface under a spec.
This sets rung 0's shape and waves 4 and 6, and it is implemented by the layout
changes of section 5 rather than by claiming anything here.

### OD-2: how are generated assets governed? **Decided: keep them committed.**

`hiqlite/static` stays committed, and packaging does not change: `cargo install`
continues to work without Node. Governance runs through the **authored source**
(`dashboard/src`), the **build configuration** (`dashboard/svelte.config.js`,
`vite.config.ts`, `package.json`), and a **reproducibility and drift check** that
rebuilds and compares. The generated bytes are referenced, never claimed as
authored units. This is wave 4's `015-dashboard-build-contract`, and F-012 closes
when that reaches M2.

### OD-3: the split-brain watchdog. **Decided: preserve behavior; investigate first.**

Runtime behavior is preserved during retroactive adoption, and no policy is
proposed until the actual behavior is established. That investigation is now
done and F-014 is rewritten against it, with its earlier claim withdrawn: under
`panic = "abort"` the checker's own panic already terminates the process, so the
watchdog is unreachable for its stated purpose; under unwinding, which is the
default for dev and test profiles and for any downstream consumer that does not
set abort, both the checker and the watchdog panic into `JoinHandle`s nobody
awaits, so split-brain checking stops silently and nothing terminates. Wave 2
describes this as found. A future policy, repair, remove, or report, is a
separate decision with no default here.

### OD-4: ratification sequencing, and when to reassess

Carried from PR #3, restated because it gates everything above. The owner has
directed that ratification be **deferred**, with readiness reassessed after the
first substantive adoption wave, and has stated that completing all six waves is
**not** a prerequisite for reconsidering it.

An earlier revision of this document contradicted itself on this point, saying
in one place that `000` should be ratified after waves 1 to 6 and in another
that wave 1 was enough. The single rule is the one above: **reassess after wave
1 completes**, and treat any later wave as additional evidence rather than as a
gate.

**Alternatives.** (a) Defer ratification of `000`; reassess when wave 1 reaches
M2. (b) Ratify `000` now to lock the freeze surface, and accept that further
corrections to the constitution, contract, and templates then require a
drafted-then-approved amending spec under the proposed per-document rule.
**Implications.** This assessment alone produced corrections to tier-1 and
tier-2 text in two successive passes, including reclassifications of its own
earlier findings. Under (b) each would have needed its own amending spec and an
owner ratification. The cost of (a) is that the freeze surface is not yet final.
**Recommendation: (a)**, which is also the owner's stated direction. Ratifying
`001` through `005` is a separate per-spec question and is not blocked by this
one; there is no aggregate corpus ratification to wait for.


## 7. The first implementation task this plan recommends

Not started, and not authorized by this document. It is now traced rather than
sketched, and the full proposal is
`standards/spec/wal-repair-proposal.md`.

**Repair the WAL append and completion error contract (F-001 and F-002).**

Chosen because it is a confirmed mismatch at an already-claimed unit with an
existing test that pins the defective behavior, so the repair is measurable and
bounded.

The proposal traces the locked OpenRaft (0.9.24, from a `"0.9.21"` caret
requirement) and establishes that the error channel hiqlite needs already
exists: `LogFlushed::log_io_completed` takes `Result<(), io::Error>`, consumes
`self` so cardinality is exactly one, and forwards an error to `RaftCore`.
hiqlite discards it at `hiqlite-wal/src/log_store_impl.rs:214`, which hardcodes
`Ok(())` into a `Box<dyn FnOnce() + Send>` that cannot carry a result.

An earlier revision of this section recommended deciding "the callback error
contract" as one question. That was too coarse. The repair separates **success
completion** (the entries reached the durability the `LogSync` mode promises;
the only thing that may produce `Ok(())`) from **error notification** (the append
or its persistence failed, with the cause). Both belong to the adapter, which
holds the `LogFlushed`; the writer's job is to report which occurred, which its
current callback type makes impossible.

One design decision is deliberately left open and does not block the repair:
whether the writer keeps exiting its loop after a persistence failure, keeps
running and fails subsequent appends, or exits with its thread error surfaced.
The proposal recommends notifying correctly and surfacing the thread error while
deferring the survival question, so a durability policy decision does not hold up
a notification defect.

Amendment and acceptance relationships, ownership justification, the three
regression tests, and the smallest sufficient integration boundary (the writer
loop, not the OpenRaft adapter) are in the proposal. One consequence to note
here: `001`'s acceptance block names the test that pins the defect, so retiring
it makes `001`'s block fail, and the amending spec must carry the replacement
acceptance and say that it supersedes that line.
