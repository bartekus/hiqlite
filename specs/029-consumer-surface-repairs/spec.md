---
id: "029-consumer-surface-repairs"
title: "Repair the defects that reach a consumer of this release, and say which ones do not"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "005-adoption-assessment-and-plan"
  - "009-configuration-contract"
  - "014-schema-migration-contract"
  - "015-server-binary-and-proxy"
  - "016-derive-macros"
  - "027-node-lifecycle-and-startup-errors"
amends:
  - "005-adoption-assessment-and-plan"
  - "009-configuration-contract"
  - "014-schema-migration-contract"
  - "015-server-binary-and-proxy"
  - "016-derive-macros"
# D-8: this spec's `## Verification` block is the acceptance for all five. Two of them were
# already failing before this spec touched anything, which section 5 records rather than
# quietly repairs.
amends_verification:
  - "005-adoption-assessment-and-plan"
  - "009-configuration-contract"
  - "014-schema-migration-contract"
  - "015-server-binary-and-proxy"
  - "016-derive-macros"
amends_sections:
  - "3-behavior"
  - "5-known-defects"
extends:
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite/src/config_toml.rs" }
    nature: superseding
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite.env" }
    nature: superseding
  - spec: "014-schema-migration-contract"
    unit: { kind: file, path: "hiqlite/src/migration.rs" }
    nature: superseding
  - spec: "015-server-binary-and-proxy"
    unit: { kind: directory, path: "hiqlite/src/server/" }
    nature: superseding
  - spec: "016-derive-macros"
    unit: { kind: file, path: "hiqlite-derive/src/from_row.rs" }
    nature: superseding
  - spec: "016-derive-macros"
    unit: { kind: file, path: "hiqlite-derive/src/into_cache_data.rs" }
    nature: superseding
  - spec: "001-wal-durability-and-completion"
    unit: { kind: directory, path: "hiqlite-wal/src/" }
    nature: superseding
  - spec: "001-wal-durability-and-completion"
    unit: { kind: file, path: "hiqlite/src/config.rs" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/app_state.rs" }
    nature: additive
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/client/" }
    nature: additive
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/network/" }
    nature: additive
  - spec: "006-cache-state-machine"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/memory/" }
    nature: additive
  - spec: "014-schema-migration-contract"
    unit: { kind: directory, path: "hiqlite/tests/cluster/migrations/" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Triages the whole findings register against the feature sets the two named
  consumers actually enable, repairs the defects that reach them, and records a
  source-backed reason for every one that does not. The largest is unrecorded
  until now: an entry larger than the WAL panicked the writer by default, which
  under an aborting profile ends the embedding application because one of its
  own writes was too big.
---

# 029: Repair the defects that reach a consumer of this release, and say which ones do not

## 1. Purpose

The register carries ninety-seven findings. A release has to say which of them
reach the people who will consume it, repair those, and give a reason for the
rest that a reader can check.

The two consumers are known and so are their feature sets. Rauthy takes the
default features plus `cache`, `cast_ints`, `counters`, `dashboard`,
`listen_notify_local` and `macros`. Rahi takes `default-features = false` plus
`sqlite`, `cache`, `counters`, `dlock`, `listen_notify_local`, `backup` and
`s3`. Neither enables `server`, `listen_notify`, `in-memory-snapshots`,
`external-state-machine`, `shutdown-handle` or `webpki-roots`.

One responsibility: **what this release fixes for those two, and what it does
not, each with its reason.**

This spec covers the node and library surface. Transport security and the
dashboard are `030`'s.

## 2. Territory

Ten units are extended, most of them additively. Five specs are amended and
this spec carries the acceptance for all five, which is the largest such
replacement in this corpus and is why section 5 opens with what it found.

**Ownership boundary.** A feature this release does not fix is not thereby
unreachable: it is unreachable **for these two consumers**, on the evidence of
the `#[cfg]` that gates it, and section 4 names that gate for every exclusion.
A third consumer with a different feature set gets a different answer, and this
spec does not claim otherwise.

## 3. Behavior

### B-1. An entry larger than the WAL is a rejected append, not a dead writer

**This had no finding identifier before this spec.** A single raft entry cannot
span WAL files, so an entry larger than `wal_size` cannot be written. That was a
`panic!` by default, on the reasoning that an oversized entry is a
non-recoverable setup issue, with an opt-in `oversized-entry-error` feature that
turned it into a returned error.

The comment beside the panic said what is wrong with that reasoning: "With the
default `wal_size` of 2MB this is easily reached by a single large INSERT,
transaction or batch." An application's own data ending its storage thread, and
under an aborting profile its whole process, is a large write and not a setup
issue. Neither consumer enables the feature.

The error is now the only behavior. `Error::WalSizeExceeded` on the
acknowledgement and on the completion, and the writer keeps serving. The feature
is kept as a no-op so a consumer that enables it still builds.

`wal_size` also had no environment route at all, so the ceiling could not be
raised by the constructor most deployments use. It reads `HQL_WAL_SIZE` now, and
`hiqlite.env` documents both the variable and what it bounds.

Recorded as F-098.

### B-2. An unknown raft type is a bad request

`RaftType` derives `Deserialize` and the routes are `/{raft_type}`, so `unknown`
in a path deserializes to `RaftType::Unknown`, which six helpers answered with
`panic!("neither `sqlite` nor `cache` feature enabled")`: a message about a
build configuration, for a value that arrived over the network (F-069).

It is rejected at the boundary. `RaftType::selected()` returns a `BadRequest`
naming the raft groups this build **does** serve, and all six management
handlers and the raft stream call it before doing anything else. The panicking
arms stay where they are and are now unreachable from a request, which is the
same shape `022` B-1 used for the cache index.

### B-3. Migration validation returns named errors

Five rules, five errors, where there were five `expect`s and two `panic!`s
inside a function returning `Vec<Migration>` whose caller returns `Result`
(F-066). Two of the messages also named the wrong rule:

- a name with no `_` reported the **gap** rule, because the id parse's message
  is what a reader sees for `create_users.sql` (F-064);
- a **duplicate** index reported "Migration index has a gap: 1 does not follow
  1", which is neither true nor actionable (F-065). It now says the index is
  used twice and names both files.

`Migrations::try_build` is the new entry point and the client's migration path
calls it. `Migrations::build` is kept as a panicking wrapper, because it is the
published signature and the `migrate!` macro expands to it.

### B-4. The proxy's router can be built, and its secret is compared in constant time

F-067: the proxy registered `"/metrics/:raft_type"`, axum 0.7's spelling,
against the pinned axum 0.8, which panics at router construction on a `:`
segment. **`hiqlite proxy` has not started at all since that upgrade.**

Neither consumer enables `server`, so this is not release-blocking for them. It
is release-blocking for the **published crate**, which offers that binary. The
route table is also split out of `start_proxy` so a test can construct it
without a live upstream client, which is why a router that could not be built at
all went unnoticed through a whole major version: nothing ever built it.

F-068: the proxy compared the API secret with `!=` over the bytes, which returns
as soon as they differ, while the node's own `validate_secret` four files away
uses a constant-time compare for the same secret. It does now too.

F-074: the proxy's validation message named `secret_raft`, which the proxy has
no concept of.

### B-5. Four configuration keys mean what they say

- **F-031.** The API TLS block read `tls_raft_danger_tls_no_verify` a **second
  time**, so the documented `tls_api_danger_tls_no_verify` was consumed by
  nothing, survived to the unknown-key check, and **the whole configuration file
  was rejected**. This is the one configuration defect that is release-blocking
  on its own: a Rauthy deployment that sets the documented key cannot start.
- **F-032.** `health_check_delay_secs` was parsed with an empty `env_var`, so
  `HQL_HEALTH_CHECK_DELAY_SECS` was documented in `hiqlite.env` and read by
  nothing on either route. Both routes read it now.
- **F-033.** `HQL_ENC_KEYS_FROM` was documented and read by nothing, and there
  was never a value of it that changed anything. It is removed from the
  reference file rather than implemented: adding a second key source is a
  feature, and a variable in a reference file that does nothing is how an
  operator comes to believe they configured something.
- **F-034.** The TOML route defaulted the prepared-statement cache to 1000 while
  `Default` and the environment route both used 1024. One value.
- **F-036.** A `bool` was parsed with a message saying `u64`.

### B-6. The derive accepts what it claims to

- **F-078.** `core::option::Option<T>` took the non-optional branch, so a
  nullable column spelled that way failed at runtime on a NULL. One missing
  alternative in one comparison.
- **F-077.** The `CacheVariants` impl emitted its generics **after** `for`,
  which is not where they go, so any generic cache enum failed to compile; and a
  data-carrying variant produced a unit-variant pattern, so the caller got E0533
  pointing at the derive. The generics are in the right place and a non-unit
  variant gets a `compile_error!` naming the rule. A non-enum input gets one too,
  where it used to get `unimplemented!()`.

### B-7. Two things a caller can now bound

- **F-050.** `wait_until_healthy_db` and `wait_until_healthy_cache` are loops
  with no deadline, so a consumer that calls them at boot hangs forever instead
  of failing. They keep their signatures, because they are published, and gain
  `_timeout` siblings that return the last health error and stop early on a
  terminal node failure.
- **F-025's remaining reachable cause.** `027` KD-1 recorded that a dead cache
  handler thread still panics the applying task. The realistic way a handler
  dies is a panic inside it, and the TTL handler had three: a send to the kv
  handler and two snapshot acknowledgements. All three report instead. The
  thirty-two `expect`s on the state machine's side are untouched and are now
  that much harder to reach; section 5 says so plainly rather than claiming the
  finding is closed.

## 4. What is excluded, and why

Every exclusion below names the gate that makes it unreachable for these two
consumers, read from source.

**Behind `server`** (`hiqlite/src/lib.rs`: `#[cfg(feature = "server")] pub mod
server;`, and the proxy is `mod proxy;` inside it, constructed only from the
`Args::Proxy` arm): F-070, F-072, F-075, and the non-defect F-071, F-073, F-076.
B-4 repairs three of this set anyway, because they are defects in an artifact
this release publishes.

**Behind `listen_notify`** (`client/listen_notify.rs`: `#[cfg(feature =
"listen_notify")] pub(crate) mod remote;`, and `client/create.rs` sets
`rx_notify = None` with `listen_notify_local` alone): **F-051**, the
connect-versus-publish race in `Client::remote`. Excluded twice over, because an
embedded node gets its client from `Client::new_local` and never constructs the
remote one.

**Test targets and examples**, which are not linked into a consumer's build:
F-049 and F-055 (`hiqlite/tests/cluster/`), F-082, F-083, F-094, F-095.

**`toml`-only**, so Rahi is unaffected: the TOML half of F-031 and F-034. Both
are repaired anyway.

**`dashboard`-only**, so Rahi is unaffected: F-084 to F-091. Those are `030`'s.

**`macros`-only**, so Rahi is unaffected: F-077 to F-080. F-077 and F-078 are
repaired; F-079 and F-080 are limits, not defects.

## 5. Known defects

**KD-1. Two acceptance blocks were already failing before this spec, both from
this release's own repairs.** `026` KD-1 recorded the first, where `024` changed
a unit `013` owns without carrying its acceptance. This spec found the second:
`020` rewrote the status line of `standards/spec/cache-log-repair-proposal.md`,
which `005`'s acceptance greps verbatim, and did not carry `005`'s block.

Two occurrences of one rule being broken, by two different specs, in one release.
That is not a coincidence and it is recorded as such: the rule is only checked by
a command nobody runs on a pull request, which is exactly what `028`'s
post-merge job exists to detect and exactly what `028` KD-1 says it cannot
prevent. F-096 carries both.

**KD-2. F-025 is narrowed, not closed.** Thirty-two `expect`s on handler channels
remain in the cache state machine's apply path. B-7 removed the reachable causes
in the TTL handler and `023` removed them in the lock handler; a panic in the kv
handler, or a handler task cancelled some other way, still ends the applying
task.

**KD-3. Six defects in the node's own surface are recorded and not repaired.**
F-037 (a bracketed IPv6 advertised address yields an unparsable listen address,
which `027` turned from a silent missing listener into a returned startup error,
so an IPv6 deployment cannot start rather than starting wrong); F-038 (`node_id`
is a position in `start.rs` and an id in `init.rs`); F-041, F-042 and F-043,
which are `030`'s; and F-054, an unreproduced WAL restart log-id mismatch whose
only record is a `TODO` and a one-second sleep in a cluster test.

**KD-4. F-035 is repaired for the ceiling and not for the mode.** `HQL_WAL_SIZE`
is readable now; `wal_sync` still is not, so the environment route cannot select
a WAL durability mode.

**KD-5. The exclusions are a feature-gate argument, not a test.** Nothing here
builds a consumer's exact feature set and demonstrates that an excluded path is
absent. The gates were read from source and named; that is weaker than a build
and stronger than an assumption.

**KD-6. F-092 and F-093 have no register entries.** They are referenced in F-012's
disposition and counted in the register's own class tables, and they have no
`###` heading of their own. The register therefore counts two defects it never
defines. Found while triaging; not repaired here, because the register's
structure is `005`'s.

## 6. Resolved decisions

**D-1 (2026-09-21, the oversized entry becomes an error by default rather than
staying a feature).** The alternative was to tell both consumers to enable
`oversized-entry-error`. Declined: a default that ends the embedding
application on a large write is not a default to leave in place and document
around, and the feature that fixes it is not discoverable from the failure.

**D-2 (2026-09-21, reject the unknown raft type at the boundary rather than
change six signatures).** Two of the six helpers return values that are not
`Result`, so making them fallible ripples through the call graph for a value
that should never have reached them. The gate goes where the value enters.

**D-3 (2026-09-21, repair the proxy although no consumer uses it).** It is not
release-blocking for Rauthy or Rahi and it is release-blocking for the artifact:
publishing a crate whose advertised binary panics at startup, when the repair is
a route literal, is not a defensible release.

**D-4 (2026-09-21, `try_build` beside `build` rather than instead of it).**
`build`'s signature is published and the `migrate!` macro expands to it.
Changing it would be a breaking change to buy a rename.

**D-5 (2026-09-21, `HQL_ENC_KEYS_FROM` is removed rather than implemented).**
Implementing it means adding a file-backed key source, which is a feature. The
smallest honest action is to stop documenting something that does nothing.

**D-6 (2026-09-21, the health waits keep their unbounded signatures).** Changing
them to return `Result` would break every caller. The siblings are additive and
the unbounded ones now say in their own documentation that they never return if
the node never becomes healthy.

**D-7 (2026-09-21, exclusions are argued from gates, not from tests).** Building
each consumer's exact feature set and asserting absence is the stronger evidence
and is a matrix this release does not have. KD-5 records the gap rather than
letting the exclusions read as demonstrated.

**D-8 (2026-09-21, this block is the acceptance for five specs).** Five is
unusual and is what the sweep required: one spec's repairs touched five owned
units. Twenty-three of their commands asserted defective expressions or named
replaced tests; each is replaced below and marked with what it was.

## 7. Out of scope

- **Transport security and the dashboard.** `030`.
- **The cluster-test findings.** `012`.
- **The dashboard build contract.** `019`, and KD-6's missing entries.
- **A full public API taxonomy.** `027` B-7 froze the lifecycle surface only.
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 029`. **This block is the acceptance for `005`,
`009`, `014`, `015` and `016` as well as this spec's** (D-8). None of their files
is edited.

```verify:cli
# Package names, not library names: the downstream release renamed the three packages
# (`031` B-2), and `-p` takes a package name. `use hiqlite::..` is unaffected.
# --- 005-adoption-assessment-and-plan's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f standards/spec/adoption-plan.md
test -f standards/spec/findings-register.md
test -f standards/spec/wal-repair-proposal.md
test -f standards/spec/cache-log-repair-proposal.md
# was: grep -q 'changes no runtime behavior' ... - stale since PR #7 rewrote the
# header this asserted. Replaced per D-7 with the statement the document must
# carry now: which spec governs it. See F-030.
grep -q 'which is the contract' standards/spec/wal-repair-proposal.md
grep -q '008-wal-append-completion-notification' standards/spec/wal-repair-proposal.md
grep -q 'log_io_completed' standards/spec/wal-repair-proposal.md
grep -q 'Status: proposed, not adopted' standards/spec/adoption-plan.md
grep -q 'M1, ownership recorded' standards/spec/adoption-plan.md
grep -q 'M2, behavior specified with evidence' standards/spec/adoption-plan.md
grep -q 'M3, enforcement enabled' standards/spec/adoption-plan.md
grep -q 'F-001' standards/spec/findings-register.md
grep -q 'F-020' standards/spec/findings-register.md
grep -q 'F-029' standards/spec/findings-register.md
grep -q 'F-047' standards/spec/findings-register.md
grep -q 'record, not a mandate' standards/spec/findings-register.md
grep -q '## 8. Current assignment table' standards/spec/adoption-plan.md
sh -c '! grep -q "^Not started, and not authorized by this document" standards/spec/adoption-plan.md'
grep -q 'Delivered 2026-09-19 as' standards/spec/adoption-plan.md
grep -q 'The proposed spec ordinals below are vacated' standards/spec/adoption-plan.md
grep -q 'are separate columns on purpose' standards/spec/adoption-plan.md
# was `Status, 2026-09-21: proposed, not authorized`, which `020` replaced when it delivered
# the repair that document proposed. F-096's second occurrence.
grep -q 'Status, 2026-09-21: delivered' standards/spec/cache-log-repair-proposal.md
grep -q 'openraft-0.9.24' standards/spec/cache-log-repair-proposal.md
grep -q 'drain(..=purge_until)' standards/spec/cache-log-repair-proposal.md
sh -c '! grep -n "origin/main" standards/spec/adoption-plan.md standards/spec/findings-register.md standards/spec/wal-repair-proposal.md standards/spec/cache-log-repair-proposal.md'
sh -c '! grep -rl "$(printf "\342\200\224")" standards/spec/adoption-plan.md standards/spec/findings-register.md standards/spec/wal-repair-proposal.md standards/spec/cache-log-repair-proposal.md'
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c 'spec-spine index owner standards/spec/adoption-plan.md | grep -q 005-adoption-assessment-and-plan'
sh -c 'spec-spine index owner standards/spec/findings-register.md | grep -q 005-adoption-assessment-and-plan'
sh -c 'spec-spine index owner standards/spec/wal-repair-proposal.md | grep -q 005-adoption-assessment-and-plan'
sh -c 'spec-spine index owner standards/spec/cache-log-repair-proposal.md | grep -q 005-adoption-assessment-and-plan'

# --- 009-configuration-contract's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/config_toml.rs
test -f hiqlite.toml
test -f hiqlite.env
sh -c 'spec-spine index owner hiqlite/src/config_toml.rs | grep -q 009-configuration-contract'
sh -c 'spec-spine index owner hiqlite.toml | grep -q 009-configuration-contract'
sh -c 'spec-spine index owner hiqlite.env | grep -q 009-configuration-contract'
sh -c 'spec-spine registry relationships 009-configuration-contract | grep -q 001-wal-durability-and-completion'
# was documented_tls_api_no_verify_key_is_rejected_as_unknown, which pinned F-031
cargo test -p hiqlite-patched --lib --features toml config_toml::tests::the_documented_tls_api_no_verify_key_is_consumed_and_honoured -- --exact
cargo test -p hiqlite-patched --lib --features toml config_toml::tests::tls_api_no_verify_stays_false_when_the_raft_key_is_set -- --exact
# was prepared_statement_cache_capacity_default_differs_from_the_env_path, which pinned F-034
cargo test -p hiqlite-patched --lib --features toml config_toml::tests::the_prepared_statement_cache_default_is_the_same_on_every_route -- --exact
# was health_check_delay_secs_is_settable_from_toml_only, which pinned F-032
cargo test -p hiqlite-patched --lib --features toml config_toml::tests::health_check_delay_secs_is_settable_from_toml_and_from_the_environment -- --exact
grep -q 'tls_api_danger_tls_no_verify' hiqlite.toml
# was an assertion that the parser never reads the key `hiqlite.toml` documents
sh -c 'grep -q "\"tls_api_danger_tls_no_verify\"" hiqlite/src/config_toml.rs'
grep -q 'HQL_HEALTH_CHECK_DELAY_SECS' hiqlite.env
# was an assertion that the documented variable is read nowhere
sh -c 'grep -rq "env::var(\"HQL_HEALTH_CHECK_DELAY_SECS\")" hiqlite/src'
# was the empty `env_var` argument that made the variable a documentation-only fiction
grep -q 'HEALTH_CHECK_DELAY_ENV' hiqlite/src/config_toml.rs
grep -q 'HQL_ENC_KEYS_FROM' hiqlite.env
sh -c '! grep -rq "HQL_ENC_KEYS_FROM" hiqlite/src'
grep -q 'hiqlite/src/config_toml.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/009-configuration-contract'

# --- 014-schema-migration-contract's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/migration.rs
test -f hiqlite/tests/cluster/migrations/bad_1/no_leading_index.sql
test -f hiqlite/tests/cluster/migrations/bad_2/2_bad_start_index.sql
test -f hiqlite/tests/cluster/migrations/bad_3/1_filename_ok.sql
test -f hiqlite/tests/cluster/migrations/good/1_init.sql
sh -c 'spec-spine index owner hiqlite/src/migration.rs | grep -q 014-schema-migration-contract'
sh -c 'spec-spine index owner hiqlite/tests/cluster/migrations/good/1_init.sql | grep -q 014-schema-migration-contract'
sh -c 'spec-spine index owner hiqlite/tests/cluster/migration.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine registry relationships 014-schema-migration-contract | grep -q 012-cluster-integration-evidence'
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::a_valid_set_is_ordered_by_index_and_hashed_by_content -- --exact
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::a_syntactically_invalid_migration_still_builds -- --exact
# was a_name_without_a_numeric_index_panics_with_the_other_rules_message, which pinned F-064
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::a_name_without_a_numeric_index_names_that_rule -- --exact
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::an_index_set_that_does_not_start_at_one_is_an_error -- --exact
# was the `expect` whose message fired for the wrong rule
grep -q 'no `_` separating the index from the name' hiqlite/src/migration.rs
grep -q 'must start with an integer index' hiqlite/src/migration.rs
grep -q 'migrations must start at index 1' hiqlite/src/migration.rs
grep -q 'must end with `.sql`' hiqlite/src/migration.rs
grep -q 'migration index has a gap: {} does not follow {}' hiqlite/src/migration.rs
grep -q 'Migration index has a gap between {} and {}' hiqlite/src/store/state_machine/sqlite/writer.rs
sh -c 'grep -q "pub fn build<T: RustEmbed>() -> Vec<Migration>" hiqlite/src/migration.rs'
sh -c '! grep -q "pub use migration::Migrations" hiqlite/src/lib.rs'
grep -q 'pub use migration::AppliedMigration;' hiqlite/src/lib.rs
# was `Migrations::build`, which panics, inside a function that returns `Result`
grep -q 'let mut migrations = Migrations::try_build::<T>()?;' hiqlite/src/client/migrate.rs
sh -c 'grep -q "if applied.hash != migration.hash {" hiqlite/src/store/state_machine/sqlite/writer.rs'
grep -q '46a52cfa9b2532439423fe769a3a75aa17e8690ee98b2c1b7c5c21560702e2aa' hiqlite/src/migration.rs
grep -q '46a52cfa9b2532439423fe769a3a75aa17e8690ee98b2c1b7c5c21560702e2aa' hiqlite/tests/cluster/migration.rs
grep -q 'hiqlite/src/migration.rs' spec-spine.toml
grep -q 'hiqlite/tests/cluster/migrations/\*\*/\*.sql' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/014-schema-migration-contract'

# --- 015-server-binary-and-proxy's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/main.rs
test -f hiqlite/src/server/proxy/mod.rs
sh -c 'spec-spine index owner hiqlite/src/main.rs | grep -q 015-server-binary-and-proxy'
sh -c 'spec-spine index owner hiqlite/src/server/proxy/handlers.rs | grep -q 015-server-binary-and-proxy'
sh -c 'spec-spine index owner hiqlite/src/server/config.rs | grep -q 015-server-binary-and-proxy'
sh -c 'spec-spine index owner hiqlite/src/server/proxy/stream.rs | grep -q 003-client-consistency-and-retry-outcomes'
sh -c 'spec-spine registry relationships 015-server-binary-and-proxy | grep -q 011-transport-security-material'
# was the_proxy_metrics_route_is_rejected_by_the_pinned_axum, which pinned F-067 by asserting
# that the literal the proxy used panics. It does; the proxy no longer uses it.
cargo test -p hiqlite-patched --features server --lib server::proxy::tests::the_proxy_route_table_can_be_constructed -- --exact
cargo test -p hiqlite-patched --features server --lib server::proxy::tests::the_zero_seven_spelling_is_still_rejected -- --exact
cargo test -p hiqlite-patched --features server --lib server::proxy::tests::the_same_capture_in_zero_eight_syntax_is_accepted -- --exact
cargo test -p hiqlite-patched --features server --lib server::config::tests::the_generated_config_omits_keys_the_reference_file_documents -- --exact
# was proxy_validation_covers_two_fields_and_names_a_third, which pinned F-074
cargo test -p hiqlite-patched --features server --lib server::proxy::config::tests::proxy_validation_covers_two_fields_and_names_the_right_one -- --exact
# was the axum 0.7 spelling that panics at router construction under the pinned axum 0.8
grep -q '.route("/metrics/{raft_type}", get(handlers::metrics)),' hiqlite/src/server/proxy/mod.rs
grep -q '"/metrics/{raft_type}", get(management::metrics)' hiqlite/src/start.rs
grep -q 'axum = { version = "0.8' Cargo.toml
sh -c 'grep -q "constant_time_eq(state.secret_api.as_bytes(), secret.as_bytes())" hiqlite/src/network/mod.rs'
# was the plain `!=` over the secret bytes, which returns as soon as they differ
sh -c '! grep -q "if state.secret_api.as_bytes() != secret.as_bytes() {" hiqlite/src/server/proxy/handlers.rs'
sh -c 'grep -q "constant_time_eq" hiqlite/src/server/proxy/handlers.rs'
grep -q 'static HEADER_NAME_SECRET: &str = "X-API-SECRET";' hiqlite/src/server/proxy/handlers.rs
grep -q 'HandshakeSecret::server(&mut ws, state.secret_api.as_bytes())' hiqlite/src/server/proxy/stream.rs
sh -c 'grep -q "serde(rename_all = \"lowercase\")" hiqlite/src/app_state.rs'
grep -q 'RaftType::Unknown => panic!("neither `sqlite` nor `cache` feature enabled"),' hiqlite/src/server/proxy/handlers.rs
sh -c 'test "$(grep -c "RaftType::Unknown => panic!" hiqlite/src/helpers.rs)" -ge 6'
sh -c 'grep -q "if args.config_file == \"\$HOME/.hiqlite/hiqlite.toml\"" hiqlite/src/server/config.rs'
sh -c 'grep -q "default_value = \"\$HOME/.hiqlite/hiqlite.env\"" hiqlite/src/server/args.rs'
sh -c '! grep -q "HOME" hiqlite/src/server/proxy/config.rs'
# was a message naming a secret the proxy has no concept of
grep -q "'secret_api' should be at least 16 characters long" hiqlite/src/server/proxy/config.rs
sh -c '! grep -q "secret_raft:" hiqlite/src/server/proxy/config.rs'
grep -q 'let addr_str = format!("0.0.0.0:{}", config.listen_port);' hiqlite/src/server/proxy/mod.rs
grep -q 'with_env_filter(level.as_str())' hiqlite/src/server/logging.rs
sh -c '! grep -q "RUST_LOG" hiqlite/src/server/logging.rs'
sh -c '! grep -vq "^\s*//" hiqlite/src/server/cache.rs || test "$(grep -cv "^\s*//\|^\s*$" hiqlite/src/server/cache.rs)" = "0"'
grep -q 'mod cache;' hiqlite/src/server/mod.rs
grep -q 'hiqlite/src/main.rs' spec-spine.toml
grep -q 'hiqlite/src/server/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/015-server-binary-and-proxy'

# --- 016-derive-macros's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite-derive/src/lib.rs
test -f hiqlite-derive/src/from_row.rs
test -f hiqlite-derive/src/into_cache_data.rs
sh -c 'spec-spine index owner hiqlite-derive/src/from_row.rs | grep -q 016-derive-macros'
sh -c 'spec-spine index owner hiqlite-derive/src/into_cache_data.rs | grep -q 016-derive-macros'
sh -c 'spec-spine registry relationships 016-derive-macros | grep -q 006-cache-state-machine'
# was the_core_spelling_of_option_is_not_recognised, which pinned F-078
cargo test -p hiqlite-derive-patched --lib from_row::tests::both_spellings_of_option_take_the_optional_branch -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::the_fallible_attributes_expand_to_unwrap_or_expect -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::rename_combines_with_a_conversion_in_either_order -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::an_enum_input_panics_instead_of_emitting_a_diagnostic -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::from_i32_uses_try_from_and_never_clamps -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::basic_mapping_uses_row_get_by_column_name -- --exact
grep -q 'proc_macro_derive(FromRow, attributes(column))' hiqlite-derive/src/lib.rs
grep -q 'proc_macro_derive(CacheVariants)' hiqlite-derive/src/lib.rs
sh -c 'grep -q "impl #impl_generics ::std::convert::From<&mut ::hiqlite::Row" hiqlite-derive/src/from_row.rs'
# was the generics emitted after `for`, which is not where they go
sh -c 'grep -q "impl #impl_generics ::hiqlite::CacheVariants for #name #ty_generics #where_clause" hiqlite-derive/src/into_cache_data.rs'
sh -c 'grep -q "index_matches.push(quote! {Self::#id => #idx,});" hiqlite-derive/src/into_cache_data.rs'
# was `unimplemented!()` for a non-enum input, which is a panic with no diagnostic
sh -c 'grep -q "can only be applied to an enum" hiqlite-derive/src/into_cache_data.rs'
grep -q 'Data::Enum(_) => unimplemented!(),' hiqlite-derive/src/from_row.rs
sh -c 'grep -q "if s == \"std\" || s == \"core\" {" hiqlite-derive/src/from_row.rs'
sh -c 'grep -q "s == \"core\"" hiqlite-derive/src/from_row.rs'
grep -q 'column value does not fit into i32' hiqlite-derive/src/from_row.rs
sh -c 'grep -q "TryFrom::try_from(&mut \*row).unwrap()" hiqlite-derive/src/from_row.rs'
grep -q 'use ::std::str::FromStr;' hiqlite-derive/src/from_row.rs
grep -q 'ColumnAttr::Skip => quote! {#id: ::std::default::Default::default(),},' hiqlite-derive/src/from_row.rs
grep -q 'hiqlite-derive/src/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/016-derive-macros'

# --- what this repair adds ---
# an oversized entry is a rejected append, not a dead writer
cargo test -p hiqlite-wal-patched --lib writer::tests::oversized_entry_errors_without_killing_writer -- --exact
cargo test -p hiqlite-wal-patched --lib log_store_impl::tests::append_adapter_reports_a_rejected_append_as_an_error -- --exact
sh -c '! grep -q "`data` length must not exceed `wal_size` -> data length" hiqlite-wal/src/writer.rs || ! grep -q "panic!" hiqlite-wal/src/writer.rs'
sh -c 'grep -q "No-op, kept so a consumer that enables it still builds" hiqlite-wal/Cargo.toml'
# and the ceiling it implies is selectable from the environment
sh -c 'grep -q "HQL_WAL_SIZE" hiqlite/src/config.rs'
sh -c 'grep -q "HQL_WAL_SIZE" hiqlite.env'
# an unknown raft type is a bad request, not a panic about a build configuration
sh -c 'grep -q "pub fn selected" hiqlite/src/app_state.rs'
sh -c 'test "$(grep -c "raft_type.selected()?;" hiqlite/src/network/management.rs)" -eq 6'
sh -c 'grep -q "raft_type.selected()?;" hiqlite/src/network/api.rs'
# the migration rules are named errors, and the duplicate case says so
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::a_duplicate_index_says_so -- --exact
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::the_panicking_wrapper_still_panics_for_source_compatibility -- --exact
test -f hiqlite/tests/cluster/migrations/duplicate/1_first.sql
test -f hiqlite/tests/cluster/migrations/duplicate/1_second.sql
sh -c 'grep -q "pub fn try_build<T: RustEmbed>() -> Result<Vec<Migration>, crate::Error>" hiqlite/src/migration.rs'
# the derive accepts both spellings of Option and both shapes of generics
cargo test -p hiqlite-derive-patched --lib from_row::tests::both_spellings_of_option_take_the_optional_branch -- --exact
sh -c 'grep -q "needs unit variants" hiqlite-derive/src/into_cache_data.rs'
# the public health waits can be bounded
sh -c 'grep -q "pub async fn wait_until_healthy_db_timeout" hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "pub async fn wait_until_healthy_cache_timeout" hiqlite/src/client/mgmt.rs'
# the cache TTL handler does not panic its own task
sh -c '! grep -q "expect(\"kv handler to always be running\")" hiqlite/src/store/state_machine/memory/cache_ttl_handler.rs'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/029-consumer-surface-repairs'
```
