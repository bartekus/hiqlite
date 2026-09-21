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

**Reconciled on 2026-09-20** at integration head `58ee7fa`, against the same
pinned revision, after waves and repairs that this document had recorded as
proposed were delivered. Updated again the same day when W-01 was delivered as
`009-configuration-contract`; coverage moved to 71/228 (31.1%), the denominator
growing by the two configuration reference files that entered
`coverage.governed_scope` with them. Measurements from 2026-09-19 are kept
where they are the baseline a decision was made against, and every restatement
is dated. The
current assignment table is section 8; read that, not the wave list, for what is
actually outstanding.

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

**Rung 0's first half is done; its second half is blocked.** The two parts are
separate and were collapsed by an earlier revision of this line.

- **Inventory expansion: done, 2026-09-19.** The configuration was corrected
  after probing the pinned tool in an isolated checkout, so the examples and the
  dashboard are declared and discovered; `000` section 12.1 records what each
  key actually does. Numbers below are after that change.
- **Exclusion semantics: unresolved.** Removing generated and vendored output
  from the denominator without also exempting it from the coupling gate is not
  achievable with the keys this document names, on the pinned revision. Section
  5's "Rung 0 is blocked on the pinned tool" records the probe and the blocker.
  Nothing in this section closes it.

| | before | after | now (2026-09-20) |
|---|---|---|---|
| denominator | 134 | **226** | 226 |
| specifically claimed | 60 | **60** | **68** |
| reported share | 44.8% | **26.5%** | **30.1%** |
| packages discovered | 3 | **10** | 10 |

The third column is a later measurement, not part of the configuration change
the first two columns describe. It moved for a different reason: wave 1 claimed
the cache state machine and the cache log store, so the numerator rose by eight
while the denominator stayed put. The two causes are kept in separate columns
because conflating them is exactly the error this table exists to prevent.

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

The denominator, reconciled on 2026-09-19 against `git ls-files`, which
reported 339 tracked files that day. Tracked counts move with every change and
are quoted here only as the baseline of this reconciliation; `git ls-files | wc
-l` reported 355 on 2026-09-20. Neither number is a coverage figure, and the
denominator itself did not move between the two dates.

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
| A5 | `hiqlite/src/config.rs`, `config_toml.rs`, `hiqlite.toml`, `hiqlite.env` | auth | configuration surface | `001` (file unit) + **`009`** (the other three, plus `extends` on `config.rs`), 2026-09-20 | 20 of 46 `env::var` reads in `config.rs`; the TOML loader carries the rest of the contract | 4 characterization tests (`009`) | F-010 narrowed; F-031 to F-036 recorded | delivered (W-01); repairs are separate |
| A6 | `hiqlite/src/store/state_machine/memory/` (6, 2192 lines) | auth | cache state machine, KV, dlock, TTL, notify | **`006`** (directory unit, 2026-09-19) | `cache`, `dlock`, `counters`, `listen_notify_local`, `in-memory-snapshots` | in-crate tests only | M1 reached; F-025 to F-027 recorded, evidence gaps in `006` section 4 | delivered; evidence and repairs outstanding (W-05, W-08) |
| A7 | `hiqlite/src/store/logs/` (2, 238 lines) | auth | OpenRaft log-store adapter, memory variant | **`007`** (directory unit, 2026-09-19) | `__cluster` | 2 characterization tests | M1 reached; F-021 to F-024 and F-029 recorded | delivered; contract repair outstanding (W-04) |
| A8 | `hiqlite/src/init.rs` (907), `start.rs` (334), `app_state.rs`, `split_brain_check.rs` (164) | auth | node lifecycle, join, split-brain observation | **none** | `HQL_DANGER_RAFT_STATE_RESET`, `HQL_SPLIT_BRAIN_INTERVAL` | none focused | F-009, F-011, F-014 | W-02 |
| A9 | `hiqlite/src/tls.rs` (231) | auth | transport security material | **none** | `HQL_TLS_*`, incl. `DANGER_TLS_NO_VERIFY` | none focused | unspecified security surface | W-03 |
| A10 | `hiqlite/src/backup.rs` (482), `s3.rs` (128) | auth | scheduled backup, restore, object storage | **none** | `backup`, `s3`, `HQL_BACKUP_*` | `backup.rs`, `backup_restore.rs` cluster tests (unclaimed); S3 skipped in CI | F-009, F-019 | W-09 |
| A11 | `hiqlite/src/migration.rs` (98) + 6 test `.sql` fixtures | auth | schema migration ordering and validation | **none** | `sqlite` | `migration.rs` cluster test (unclaimed) | fixtures outside denominator | W-10 |
| A12 | `hiqlite/src/server/` minus `proxy/stream.rs` (11) | auth | server binary, args, proxy, logging, password | **none** | `server`, `dashboard` | not tested in CI (F-019) | F-009 (`HQL_SECRET_API`) | W-11 |
| A13 | `hiqlite/src/dashboard/` (8, 1022 lines) | auth | dashboard HTTP, session, password, query | **none** | `dashboard`, `HQL_PASSWORD_DASHBOARD`, `HQL_INSECURE_COOKIE` | not tested in CI (F-019) | security surface unspecified | W-12 |
| A14 | `dashboard/src/` (69) + configs | auth | Svelte dashboard source | **none**; visible since 2026-09-19 | `vite`, `svelte.config.js`, CSP directives | `tests/smoke.spec.ts` (Playwright), not in CI | F-016, narrowed | W-12, W-13 |
| A15 | `dashboard/src/spow/` (5) + `.wasm` | 3p | proof-of-work client | **none** | bundled | none | third-party, not authored here | declare third-party; reference, never claim |
| A16 | `hiqlite/static/` (54: 12 `.js`, 18 `.gz`, 18 `.br`, 4 `.css`, `.html`, `.json`, `.png`) | gen | built dashboard, embedded by `rust-embed` | **none**, 12 counted as source | `adapter-static`, `precompress: true`, `#[folder = "static"]` | no drift check (F-012) | F-012, F-016 | W-13 for the build contract; the denominator exclusion stays blocked on W-14 |
| A17 | `hiqlite-derive/src/` (3) | auth | `FromRow`, `IntoCacheData` derive macros | **none** | `macros` | `derive-complex-types` example | proc-macro contract unstated | W-18 |
| A18 | `hiqlite/src/error.rs` (408), `lib.rs`, `macros.rs`, `helpers.rs`, `http_client.rs` | auth | public API surface and error taxonomy | **none** | all features | compile-time only | the error contract callers match on is unspecified | W-17 |
| A19 | `hiqlite/tests/cluster/` (16 files, 2295 lines) | auth | integration evidence for A1 to A4 | `002` claims `self_heal.rs` only | `cache_storage_disk=false`; `test-no-s3` | F-017 | W-15 |
| A20 | `examples/` (6 crates, 35 files) | auth | executable user documentation | **none**; visible since 2026-09-19 | own manifests; `just clippy-examples` | compiled in CI, never claimed | F-015, narrowed | W-16 |
| A21 | `Cargo.toml`, `Dockerfile`, `.cargo/config`, `.github/workflows/code_style.yaml`, `justfile` release recipes | auth | build, release, packaging, CI | `000` owns `justfile` and the spec-spine workflow | MSRV, feature matrix, `panic = "abort"` | CI is the evidence | the other workflow and the image are unclaimed | W-19 |
| A22 | `README.md`, `CHANGELOG.md` (`hiqlite.toml` and `hiqlite.env` moved to A5) | auth | user-facing documentation | **none** | narrative docs | F-011 | the config reference half is delivered under W-01; `README.md` and `CHANGELOG.md` stay on the bypass floor |
| A23 | `LICENSE`, `.gitignore`, `.dockerignore`, `.npmrc`, lockfiles | excl | repository hygiene | `000` owns `.gitignore` | none | none | permanently excluded, except `.gitignore` |
| A24 | `.derived/` (13) | gen | compiler output | `000` by the authored/derived boundary | determinism | `check` | none | permanently excluded from claims |

---

## 3. Waves

Dependency-ordered. Each wave is one or more draft specs, each retroactive
unless stated, each with its own acceptance block and review.

**The proposed spec ordinals below are vacated (2026-09-20).** They were written
when the corpus ended at `005`, and the corpus now runs to `008`: wave 2's
proposed `008-configuration-contract` collides with the delivered
`008-wal-append-completion-notification`, and every later wave's numbering
inherited the same assumption. Ordinals are identity, not schedule (`000`
section 3), so an ordinal is allocated when the work starts, from the next free
number at that moment, and never reserved in advance here. Read the wave entries
below for **scope, dependencies, and review obligations**; read the names as
descriptions rather than as assignments, and take the work identifiers from
section 8. Names shown as `NNN-...` mean "a spec of this shape", not that
number.

### Wave 1: the cache state machine and its log store. **Executed 2026-09-19**

Delivered as `006-cache-state-machine` (directory claim on
`hiqlite/src/store/state_machine/memory/`) and `007-cache-log-store` (directory
claim on `hiqlite/src/store/logs/`). Both retroactive, both `draft`, both
`implementation: complete` against their own stated obligations. M1 reached for
both; M2 reached with the evidence limits each spec states in its own section 4.
Coverage moved 60 to 68 of 226 as a result, this time because territory was
actually claimed.

Four new defects were found and recorded, none repaired: `007` KD-1, a confirmed
off-by-one in `get_log_state` that always reports `last_log_id: None`, observed
by an added characterization test; KD-2, a `debug_assert!` comparing an offset to
an absolute index; KD-3, an underflow on an exclusive end bound of zero; KD-4,
`purge` never updating `last_purged`. `006` records three: panic-on-dead-handler
escalating to process abort under this repository's release profile, a
compile-time lock validity constant, and an unvalidated `cache_idx`.

Distributed-lock behavior is described as found, per the owner's direction: the
queue, the ten-second constant, and the wall-clock expiry with its own source
comment about clock skew. No lease design, no fencing, no membership
integration.

**Added 2026-09-20.** A fifth defect in `007`'s territory was confirmed at
source and recorded as F-029: `purge` drains an exclusive range while OpenRaft
0.9.24 documents `RaftLogStorage::purge` as inclusive, and the characterization
test wave 1 added pins the exclusive behavior as expected. `007` itself still
records four known defects, so reconciling a KD-5 into that spec is outstanding
work (W-04 carries it). No runtime consequence was executed for F-029.

The original wave-1 plan follows, for reference.

### Wave 1 as originally planned: the storage territory the boundary already promises

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

**Status on 2026-09-21: two thirds delivered.** W-01 as `009` on 2026-09-20 and
W-02 as `010` on 2026-09-21. W-03 is the remaining third. The scope and review
text below is kept as written, because it is the standard each delivered spec
was reviewed against; section 8 is the queue.

**Scope.** A5, A8, A9, A22 (the config reference half).
**Proposed specs:** one for the configuration contract (W-01), one for node
lifecycle and split-brain (W-02), one for transport security material (W-03).
Ordinals are allocated when each starts; the numbers this entry originally
proposed are vacated, and `008` in particular is taken.
**Existing ownership to extend.** The configuration spec `extends` `001`'s
`hiqlite/src/config.rs` unit rather than re-establishing it, and claims
`config_toml.rs`, `hiqlite.toml`, and `hiqlite.env` so the contract is owned as
a contract (F-010, F-020).
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
**Known defects retained.** F-009 is recorded, not fixed. F-011 was closed by
`010` section 5 instead of retained: documenting a variable is an addition to a
reference file, not a change to behavior, and `009` section 7 had already
assigned it here by name.
**Owner decisions.** OD-3 (F-014 intent), honoured by `010` D-1: the watchdog is
described in full and left exactly as found.

### Wave 3: durability services

**Scope.** A10, A11. **Proposed specs:** one for backup and object storage
(W-09), one for schema migrations (W-10). Ordinals allocated at start.
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
**Proposed specs.** One for the server binary and proxy (W-11), one for the
dashboard service and UI (W-12), one for the dashboard build contract (W-13).
Ordinals allocated at start.
**Existing ownership to extend.** The server spec `extends` `003` on
`hiqlite/src/server/proxy/stream.rs`, which `003` already claims.
**Retroactive or forward-looking.** Retroactive for W-11 and W-12. W-13 is
**forward-looking**: the drift check it specifies does not exist. It declares
its units with `planned: true` and `origin.retroactive: false`. It is no longer
the first spec in this corpus to declare a non-retroactive origin: `005` did,
and its D-2 records the probe.
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
**Known defects retained.** F-012 closes only when W-13 reaches M2.
**Owner decisions.** OD-2 (generated assets), and the layout change of section 5
for `dashboard/` to be claimable at all.

### Wave 5: the public surface

**Scope.** A17, A18. **Proposed specs.** One for the public API and error
taxonomy (W-17), one for the derive macros (W-18). Ordinals allocated at start.
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

**Scope.** A19, A20, A21. **Proposed specs.** One for the integration evidence
surface (W-15), one for examples as documentation (W-16), one for build, release
and CI (W-19). Ordinals allocated at start.
**Existing ownership to extend.** The evidence spec `extends` `002`'s claim on
`self_heal.rs`; the build spec `extends` `000`'s `justfile` and workflow
units.
**Retroactive or forward-looking.** Retroactive, except any new harness.
**Behavioral and evidence review required.** F-017's recorded cluster-test
non-completion has to be resolved or restated honestly before `018` can claim
the suite. That may need one bounded cluster diagnostic, which is the only
expensive run this plan anticipates.
**Acceptance boundary.** W-16 can assert that every example builds, which CI
already does. W-15 must not claim the suite passes if it does not.
**Known defects retained.** F-017 until the run is resolved.
**Owner decisions.** OD-1 (examples and dashboard in scope), and the layout
change of section 5.

---

## 4. What "whole-project adoption complete" means

All five must hold, and each is separately checkable.

1. **Every authored source file in the corrected denominator has an owning
   spec.** `spec-spine index coverage` reports zero unclaimed, with the
   denominator corrected per section 5 so that it includes `dashboard/` and
   `examples/` and excludes generated output. **Blocked**: the pinned tool
   cannot exclude generated output or third-party source from the denominator
   without also exempting it from the coupling gate, so 16 files stay unclaimed
   no matter how much first-party adoption is done. See "Rung 0 is blocked on
   the pinned tool" in section 5 for the exact sets and the recommended
   resolution.
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

#### Rung 0 is blocked on the pinned tool (probed 2026-09-19)

The first two bullets are done and are in `spec-spine.toml` today. The third is
**not achievable with the keys this document names**, and the blocker is a
prerequisite for rung 2, not a detail of it.

**The affected sets, exactly.** Sixteen tracked files are in the coverage
denominator and can never be claimed, because milestones 3 and 4 of section 4
forbid claiming either set:

- **Generated JavaScript, 12 files**, all under `hiqlite/static/_app/immutable/`:
  `chunks/Bjy-W4x2.js`, `chunks/DYl5dUZ5.js`, `chunks/HS78R7GZ.js`,
  `chunks/Rzvk3oo4.js`, `chunks/ucR_hd6Y.js`, `chunks/xihTtKlq.js`,
  `chunks/yhtOcEv9.js`, `entry/app.BN4hQcvT.js`, `entry/start.BSvfSFwO.js`,
  `nodes/0.CCzH6Rmr.js`, `nodes/1.DxhH6ylA.js`, `nodes/2.BxpRrFU6.js`. They
  enter through the `hiqlite` cargo package walk (109 files = 97 `.rs` + these
  12), which is why they are counted as that package's unclaimed source. A16
  and milestone 4 say generated output is governed by its build contract and
  its bytes are never claimed.
- **Third-party vendored source, 4 files**, under `dashboard/src/spow/`:
  `spow-wasm.d.ts`, `spow-wasm.js`, `spow-wasm_bg.js`, `spow-wasm_bg.wasm.d.ts`.
  They enter through the `dashboard` npm package declaration. The `.wasm`
  binary is not counted. A15 and milestone 3 say this set is referenced and
  never claimed.

**What was probed, on revision `aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f`.**
Each candidate was set in `spec-spine.toml`, the index regenerated, and
`spec-spine index coverage` re-read. Baseline: `68/226 claimed, 158 unclaimed`.

| Candidate | Result |
|---|---|
| `coverage.governed_scope_exclusions = ["hiqlite/static/**", "dashboard/src/spow/**"]` | **No effect.** Still `68/226`, all 16 still listed unclaimed. The key filters only the declared-scope set; it cannot reach files a package walk contributed. |
| `index.resolver_exclusions += ["hiqlite/static", "dashboard/src/spow"]` | **No effect.** Still `68/226`, all 16 still listed. |
| `coupling.bypass_prefixes = ["hiqlite/static/", "dashboard/src/spow/"]` | **Works.** `68/210 claimed, 142 unclaimed`; none of the 16 is listed. |

**The blocker.** The only key that removes these files from the denominator is
`coupling.bypass_prefixes`, and on the pinned revision that key does two things
at once: it exempts the paths from the coupling gate *and* it removes them from
the coverage denominator. There is no key that does the second alone. Rung 3 of
this ladder states that source paths are never added to the bypass floor, and
`dashboard/src/spow/` is source-shaped even though it is third-party, so
applying the one working lever to it contradicts this document's own rule and
would silently exempt vendored bindings from `C-001` for as long as it stands.

**Consequence for the plan.** Rung 2,
`spec-spine index coverage --fail-on-untraced`, **cannot pass under the current
configuration even after first-party adoption is complete**. Sixteen files
would remain unclaimed, and the gate does not distinguish "unclaimed because
nobody got to it" from "unclaimed on purpose". Section 4 milestone 1 is
therefore not reachable as written, and any plan that schedules rung 2 after
the last adoption wave is scheduling a gate that will fail.

**Recommended resolution**, in preference order, and **none of them is done
here**:

1. **A later pin upgrade** that separates the two concerns: a
   `coverage.denominator_exclusions` key, or making
   `governed_scope_exclusions` apply to package-walk output as well as to the
   declared scope. This is the only resolution that keeps `C-001` on
   `dashboard/src/spow/` while taking it out of the denominator, and it is the
   one to ask the tool for.
2. **If and only if 1 is unavailable**, add `hiqlite/static/` alone to
   `coupling.bypass_prefixes`, once wave 4's build contract exists, which rung 3
   already contemplates for generated output. That clears 12 of the 16 and
   leaves the 4 third-party files as a known, documented rung-2 exception
   rather than a hidden one. It does not make rung 2 passable on its own.

**Explicitly rejected.** Claiming the generated bytes or the vendored bindings
under some spec to make the number go up: that inflates coverage with files no
spec can meaningfully describe, and milestones 3 and 4 exist to prevent it.
Also rejected here: changing the tool pin, and enabling any rung. This entry
records the prerequisite; it does not resolve it.

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

They were 116 files, counted on 2026-09-19, and were invisible to the ledger by
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

**Status on 2026-09-20.** The trigger this decision names has occurred: wave 1
was delivered on 2026-09-19 (`006` and `007`, merged as PR #6), so the
reassessment it defers to is now due rather than pending. Two further facts
belong on the record here, and neither is a ratification:

- The owner **decided the per-document transition** on 2026-09-20, which
  alternative (b) below had left open. It is recorded in the constitution's
  Amendment section, `000` section 1.1, and `004` D-5. Deciding the policy that
  governs what ratifying `000` would close is not ratifying `000`.
- **No spec has been approved.** Every `status` in the corpus is still read from
  its own frontmatter, and `spec-spine registry list` prints the current values.
  This document does not assert them.

The alternatives below are kept as written on 2026-09-19, because they are the
record of what the decision weighed.

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


## 7. The first implementation task this plan recommended: delivered

**Delivered 2026-09-19 as `008-wal-append-completion-notification`**, merged as
PR #7. F-001 and F-002 are repaired and annotated in place in the register. An
earlier revision of this section said "Not started", which was true when it was
written and is no longer; the recommendation text below is kept because it is
the proposal the delivered repair was reviewed against.

**The survival question this section left open was answered.** `008` section 6
records the owner's decision: option (b), a surviving writer with a defined
poisoned state, was declined, and the existing termination is preserved. A
persistence failure notifies and then ends the writer thread, which `008`
section 3.6 states as the failure policy and pins with
`persistence_failure_notifies_then_terminates_the_writer`. That is a decision,
not a deferral, and nothing further is required for it.

What the delivery did **not** settle: whether to add **supervision** on top of
that termination, which `008` section 6 lists as out of scope and section 3.6
bounds by naming what the report does not cover (a panic, an abort, a signal);
and **F-028**, a truncated entry stream acknowledged as a successful append,
found while tracing this repair and deliberately left unrepaired. They are
carried in section 8 as W-06 and W-07, and neither is an unfinished part of the
delivered repair: W-06 is optional work on a settled policy, and W-07 is a
different defect.

**The recommendation as written on 2026-09-19 follows.** It is traced rather
than sketched, and the full proposal is
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

---

## 8. Current assignment table (2026-09-21)

**This is the queue.** Sections 3 and 7 are the reasoning and the history;
this table is what is actually outstanding. A work identifier `W-nn` is stable
and is not a spec ordinal: an ordinal is allocated when the work starts, from
the next free number at that moment (`000` section 3).

**Implementation state** and **evidence state** are separate columns on purpose,
and so is **owner decision**: a row can be fully owned with no evidence, or
fully evidenced and still blocked on a decision. Nothing in this table is
scheduled, authorized, or approved by this document.

| id | subject | owning spec today | implementation | evidence | owner decision | depends on | closes when |
|---|---|---|---|---|---|---|---|
| W-01 | configuration contract: `config.rs`, `config_toml.rs`, `hiqlite.toml`, `hiqlite.env` (A5, A22) | **`009-configuration-contract`**, 2026-09-20 | **delivered at M1**: `009` establishes `config_toml.rs`, `hiqlite.toml` and `hiqlite.env` and `extends` `001` on `config.rs`; `spec-spine.toml` gained the freshness and denominator declarations | M2 partly: four characterization tests pin the contract, and `009` section 4 states what they do not reach, notably every environment-route claim, which is source-read only | none outstanding | none | closed at M1. The remaining evidence gap is the environment route, which cannot be tested without process-wide mutation (`009` D-3), and the per-variable validation assertions for the variables other specs will claim |
| W-02 | node lifecycle and split-brain: `init.rs`, `start.rs`, `app_state.rs`, `split_brain_check.rs` (A8) | **`010-node-lifecycle-and-split-brain`**, 2026-09-21 | **delivered at M1**: `010` establishes all four files; `spec-spine.toml` gained their freshness declarations | M2 partly: six characterization tests pin the listen-address and node-identity contracts, and `010` section 4 states what they do not reach, which is everything needing a running node or cluster (B-1, B-4 to B-8) and both source-established defects | OD-3 honoured: the watchdog is described, not changed (`010` D-1) | none | closed at M1. Four new defects recorded (F-037 to F-040), F-009 and F-014 retained, F-011 closed by documenting `HQL_SPLIT_BRAIN_INTERVAL`. The remaining evidence gap is the join and shutdown sequences, which need the cluster surface W-15 owns |
| W-03 | transport security material: `tls.rs` (A9) | none | not started | none focused | none outstanding | W-01 | material loading, trust validation and each `DANGER_*` override are specified and tested in every claimed configuration |
| W-04 | cache log store contract repair: F-021 to F-024 and F-029, and reconciling F-029 into `007` as a KD entry | `007` | not started | two characterization tests, which pin the current behavior including the exclusive purge | none outstanding for the trait mismatch itself | none | the contract matches the locked trait, the replaced acceptance is carried through the governed mechanism rather than left asserting the old outcome, and `007` records or closes the fifth defect |
| W-05 | `006` evidence gaps: counter and Notify semantics, dead-handler behavior, cache-index validation, cross-node convergence, clock-dependent lock limits | `006` | delivered at M1 | narrow; `006` section 4 states the limits | some gaps need W-08 and a lock policy decision first | W-08 | each gap is either evidenced or restated as a declared limit with its consequence |
| W-06 | **optional** supervision of the WAL writer thread, on top of the termination policy `008` preserved | `001`, amended by `008` | the policy is **decided and in force**, not deferred: a persistence failure notifies, then ends the writer thread (`008` section 3.6, owner decision at `008` section 6) | the `run`-returns-`Err` termination is reported by a single ERROR log and tested by `writer_termination_is_reported`; `008` section 3.6 states what the report does not cover (panic, abort, signal), and `008` KD-3 records that the report has no consumer | **optional**: whether to add supervision at all (retain and join the `JoinHandle`, catch panics, wire a health check). Existing behavior stands unless the owner asks for a change; no decision is outstanding | none | supervision is either specified with evidence, or the corpus records that the reported termination is the whole of the contract and this row closes unchanged |
| W-07 | F-028: a truncated entry stream acknowledged as a successful append | `001`, amended by `008` | not started | untested | **required, and specific to this defect**: how the writer tells a `recv` error from the `None` that ends a healthy stream; what becomes of the prefix already written; what the acknowledgement and the completion notification report for a partial append, and with what cardinality; whether the writer stays usable afterwards or ends as a persistence failure does | none | sender disconnect before the first entry and after a prefix produce no false success, with the old behavior demonstrated failing rather than merely untested, in every supported `LogSync` mode |
| W-08 | F-027: unvalidated `cache_idx` and incompatible cache variant sets | `006` | not started | untested | **required**: what a replicated invalid command means | none | incompatible inputs produce the agreed deterministic result and no node silently skips committed work |
| W-09 | backup and object storage: `backup.rs`, `s3.rs` (A10) | none | not started | cluster tests exist but are unclaimed; S3 is skipped in CI (F-019) | none outstanding | W-01 | cron, naming, retention, validation bypass and restore ordering are specified, with skipped service tests reported as skipped |
| W-10 | schema migrations: `migration.rs` and its fixtures (A11) | none | not started | one unclaimed cluster test; three `bad_*` fixtures | none outstanding | W-01 | ordering, gaps, malformed names and migration-failure restart behavior are asserted against the existing fixtures |
| W-11 | server binary and proxy (A12) | none | not started | not exercised by CI (F-019) | none outstanding | W-01 | authentication, forwarding, errors, reconnect and shutdown are tested with `server` enabled |
| W-12 | dashboard service and UI (A13, A14) | none | not started | not exercised by CI (F-019) | none outstanding | W-01 | session, password, cookie and query contracts, invalid and expired sessions, and the authorization boundary are tested rather than asserted |
| W-13 | dashboard build contract and drift check (F-012, A16) | none; forward-looking, `planned: true` | not started | none | OD-2 decided: keep the bytes committed | W-12 | a rebuild under recorded toolchain and lockfile inputs compares deterministically and drift fails the check |
| W-14 | denominator exclusion for the 12 generated `.js` and 4 vendored files (F-016) | `000` owns `spec-spine.toml` | **blocked on the pinned tool** | probed 2026-09-19; results in section 5 | **required**: whether to pursue a pin upgrade | a tool capability that does not exist at the pin | generated and vendored files leave the denominator without losing `C-001` on explicitly claimed paths |
| W-15 | integration evidence surface: `hiqlite/tests/cluster/` (A19, F-017) | `002` claims `self_heal.rs` only | not started | one claimed test with a recorded non-completion | none outstanding | none | each test is mapped to the guarantee it establishes and the remote-client stall is diagnosed or restated honestly |
| W-16 | examples as executable documentation (A20, F-015) | none; visible since 2026-09-19 | not started | compiled by CI, never claimed | OD-1 decided: in scope | none | every example builds under documented configurations and representative behavior is exercised where promised |
| W-17 | public API and error taxonomy (A18) | none | not started | compile-time only | **required**: is the public API frozen for this fork | none | feature availability and error semantics are specified with consumer-facing behavioral assertions |
| W-18 | derive macros (A17) | none | not started | one example crate | none outstanding | none | supported types, attributes, diagnostics and rejections are covered by compile-pass and compile-fail tests |
| W-19 | build, release, packaging and CI (A21) | `000` owns the `justfile` and the spec-spine workflow | not started | CI is the only evidence | none outstanding | none | the supported feature and MSRV matrix, packaging contents and an external-consumer install are validated |
| W-20 | internal SQLite snapshot publication and installation: F-003, F-004, F-006 | `002` | not started | focused storage tests | **required**: atomicity and durability, failed-install rollback versus poison, corrupt-newest handling | none | deterministic failure injection shows incomplete staging is never selectable and a failed install never becomes a restart candidate |
| W-21 | internal exclusive access: F-005 | `002` | not started | none | **required**: supported systems, lock lifetime, storage identity | none | two real processes cannot own the same storage, a rejected contender does not mutate it, and exit or crash releases ownership |
| W-22 | startup-error and background-task lifecycle policy: F-009, F-014, F-025, and F-039 and F-040 added by `010` | none; `009` and `010` now describe the behavior | not started | none focused; `010` B-7 and section 4 state the abort-versus-unwind split and why no test reaches it | **required**: what failed background work does, and whether a listener that cannot bind is a startup error | W-01 and W-02, both delivered | the selected policy is externally observable and characterized under both abort and unwind profiles |
| W-23 | enforcement rungs 1 to 4 | `000` owns the configuration | not enabled | rung-0 probe recorded | **required**: the distinct enforcement decision | W-14, and M1 across the adopted scope | each enabled refusal is demonstrated against the exact pin and candidate workflow |
| W-24 | whether acceptance blocks should be executed by an automated control, given that CI deliberately abstains. Surfaced by F-030, which is itself repaired | `004` states the CI trust boundary; `000` section 15 states the rule | no process changed, and none is required to change | `just spine-verify` is a documented manual step and was run by hand for `000`, `004` and `005` in this pass, all passing; nothing runs it automatically | **optional**: whether the manual run stays the accepted control, or an automated one is added on a trusted tree. The manual control stands unless the owner asks otherwise | none | the corpus records the chosen control, and any control added is demonstrated; closing it unchanged is a valid outcome |

**Not in this table, and deliberately.** Publication, release and upstream
acceptance. This document is an adoption plan; a release candidate is qualified
against a chosen release scope, which nothing here selects.

**How to keep this table honest.** A row moves only on delivered evidence, and a
delivered row is rewritten as history rather than deleted, the way section 7
was. If a completed piece of work still reads as the recommended next task
anywhere in this document, that is the defect this section exists to prevent.
