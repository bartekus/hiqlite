---
id: "042-ci-code-debt"
title: "Make the workspace pass the CI profile's code job: rustfmt, clippy on all targets, and a featureless workspace test"
status: draft
created: "2026-09-25"
owner: "hiqlite maintainers"
risk: low
implementation: in-progress
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "004-governance-harness"
extends:
  # A1: the rustfmt pass touched a file in each of these units. Nature is additive: the
  # change is whitespace and line breaking, and no behavior any owner describes moves.
  - spec: "001-wal-durability-and-completion"
    unit: { kind: directory, path: "hiqlite-wal/src/" }
    nature: additive
  - spec: "001-wal-durability-and-completion"
    unit: { kind: file, path: "hiqlite/src/config.rs" }
    nature: additive
  - spec: "002-snapshot-publication-and-recovery"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/sqlite/" }
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
  - spec: "007-cache-log-store"
    unit: { kind: directory, path: "hiqlite/src/store/logs/" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/app_state.rs" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/split_brain_check.rs" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/start.rs" }
    nature: additive
  - spec: "011-transport-security-material"
    unit: { kind: file, path: "hiqlite/src/tls.rs" }
    nature: additive
  - spec: "011-transport-security-material"
    unit: { kind: file, path: "hiqlite/tests/tls_env.rs" }
    nature: additive
  - spec: "012-cluster-integration-evidence"
    unit: { kind: file, path: "hiqlite/tests/cluster/cache.rs" }
    nature: additive
  - spec: "012-cluster-integration-evidence"
    unit: { kind: file, path: "hiqlite/tests/cluster/check.rs" }
    nature: additive
  - spec: "012-cluster-integration-evidence"
    unit: { kind: file, path: "hiqlite/tests/cluster/learner_only.rs" }
    nature: additive
  - spec: "012-cluster-integration-evidence"
    unit: { kind: file, path: "hiqlite/tests/cluster/remote_only.rs" }
    nature: additive
  - spec: "013-backup-retention-and-object-storage"
    unit: { kind: file, path: "hiqlite/src/backup.rs" }
    nature: additive
  - spec: "013-backup-retention-and-object-storage"
    unit: { kind: file, path: "hiqlite/src/s3.rs" }
    nature: additive
  - spec: "014-schema-migration-contract"
    unit: { kind: file, path: "hiqlite/src/migration.rs" }
    nature: additive
  - spec: "015-server-binary-and-proxy"
    unit: { kind: file, path: "hiqlite/src/server/config.rs" }
    nature: additive
  - spec: "015-server-binary-and-proxy"
    unit: { kind: file, path: "hiqlite/src/server/password.rs" }
    nature: additive
  - spec: "015-server-binary-and-proxy"
    unit: { kind: file, path: "hiqlite/src/server/proxy/handlers.rs" }
    nature: additive
  - spec: "015-server-binary-and-proxy"
    unit: { kind: file, path: "hiqlite/src/server/proxy/state.rs" }
    nature: additive
  - spec: "016-derive-macros"
    unit: { kind: crate, id: "hiqlite-derive-patched" }
    nature: additive
  - spec: "018-dashboard-service-and-ui"
    unit: { kind: file, path: "hiqlite/src/dashboard/handlers.rs" }
    nature: additive
  - spec: "018-dashboard-service-and-ui"
    unit: { kind: file, path: "hiqlite/src/dashboard/middleware.rs" }
    nature: additive
  - spec: "018-dashboard-service-and-ui"
    unit: { kind: file, path: "hiqlite/src/dashboard/mod.rs" }
    nature: additive
  - spec: "018-dashboard-service-and-ui"
    unit: { kind: file, path: "hiqlite/src/dashboard/query.rs" }
    nature: additive
  - spec: "018-dashboard-service-and-ui"
    unit: { kind: file, path: "hiqlite/src/dashboard/session.rs" }
    nature: additive
  - spec: "018-dashboard-service-and-ui"
    unit: { kind: file, path: "hiqlite/src/dashboard/static_files.rs" }
    nature: additive
  - spec: "024-exclusive-storage-ownership"
    unit: { kind: file, path: "hiqlite/src/storage_lock.rs" }
    nature: additive
  - spec: "027-node-lifecycle-and-startup-errors"
    unit: { kind: file, path: "hiqlite/src/membership_gate.rs" }
    nature: additive
  - spec: "035-n1-upgrade-exclusion"
    unit: { kind: file, path: "hiqlite/src/upgrade_exclusion.rs" }
    nature: additive
  - spec: "035-n1-upgrade-exclusion"
    unit: { kind: file, path: "hiqlite/tests/upgrade_exclusion.rs" }
    nature: additive
  - spec: "037-startup-recovery-readiness"
    unit: { kind: file, path: "hiqlite/tests/recovery_readiness.rs" }
    nature: additive
summary: >
  The Statecraft CI profile's `code` job (`004` D-8, PR #47) runs `cargo fmt
  --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test
  --workspace --locked` with default features, and the trunk failed all three.
  This spec records why and removes the debt in three separate changes, so
  `ci-gate` can become the required check. It changes no runtime behavior.
---

# 042: Make the workspace pass the CI profile's code job

## 1. Purpose

`004` D-8 adopted the Statecraft CI profile (revision 7). Its `code` job and the
per-commit `gate_each_commit` step failed on the trunk for three reasons the
patched.4 handoff measured on 2026-09-25: 55 files differed from `cargo fmt`;
`cargo clippy --all-targets -- -D warnings` reports 6 errors in `hiqlite-wal`'s
lib tests; and `cargo test --workspace --locked` does not compile the `cluster`
test target without features. Until all three pass, branch protection cannot
move from `Check`/`govern` to `ci-gate`. The owner put this debt in scope on
2026-09-25 night.

## 2. Ownership

This spec owns no behavior. It `extends` each unit a debt change touches, with
`nature: additive`, so the owning specs keep what they describe. Formatting,
lint fixes in test code, and a test target's feature gate are the only edits.
OpenRaft and callers are untouched.

## 3. Behavior

**A1 (rustfmt).** `cargo +1.95.0 fmt --all -- --check` exits 0 on the trunk.
The reformat is one mechanical commit with no other change except the
regenerated `.derived/` shards it forces; its SHA is listed in
`.git-blame-ignore-revs`, so `git blame` with that file configured skips it.

**A3 (featureless workspace test).** `cargo test --workspace --locked`, with
default features only, compiles and runs every test target whose features are
enabled and skips the `cluster` integration target cleanly: `hiqlite/Cargo.toml`
declares it as a `[[test]]` with `required-features = ["backup", "cache",
"dlock", "listen_notify", "macros", "sqlite"]`, the features its modules call.
Cargo then leaves it out of a run that lacks them, and `clippy --all-targets`
does the same. A run with those features (the `Check` job's `just test-no-s3`,
`--features cache,counters,dlock,listen_notify,macros,toml,external-state-machine`
on top of the defaults) still builds and executes it. The other integration
targets already gate themselves with `#![cfg(...)]` on their features.

## 4. Evidence and its limits

Why the files differed (measured 2026-09-25, rustfmt from toolchain 1.95.0, no
`rustfmt.toml`, edition 2024 from the workspace):

- It is not a toolchain or configuration drift: upstream `sebadob/hiqlite`
  `main` (`e0a6a8e`) is clean under the same rustfmt.
- 24 files were already unformatted at the fork point `52122ae` ("improve WAL
  robustness (#367)"); upstream formatted them afterwards (in `9824bcf`, #368),
  which the fork did not take.
- 31 files became unformatted through fork commits, because no fork job checked
  formatting. After lane B's revert (#52) one of them, `hiqlite/src/init.rs`, is
  formatted again, so the reformat touches 54 files: 24 inherited and 30 from
  fork commits.
- Of the 54, 47 also exist upstream (every one of them is modified by the fork)
  and 7 are fork-only (`membership_gate.rs`, `storage_lock.rs`,
  `cache_ttl_handler.rs`, `upgrade_exclusion.rs`, and the tests
  `recovery_readiness.rs`, `tls_env.rs`, `upgrade_exclusion.rs`).

The limit: formatting is checked by rustfmt, not by a test. That the reformat
changed no behavior rests on rustfmt's own guarantee and on the full suite
passing before and after, not on a semantic diff.

## 5. Known defects

None recorded.

## 6. Out of scope

Coverage debt (`index coverage --fail-on-untraced`, `004` D-8), the profile's
other jobs, and any runtime change.

## 7. Resolved decisions

**D-1 (2026-09-25).** One spec for the three debt items, each landing in its
own pull request, because they share one purpose and one ownership shape; the
edges each later item needs are added by that item's change.

**D-2 (2026-09-25).** The reformat commit carries the regenerated `.derived/`
shards, because the content hashes in the index change with every formatted
file and a commit without them fails `spine-check` on its own
(`gate_each_commit`). No other file changes in it.

**D-3 (2026-09-25).** `required-features` rather than a `#![cfg(...)]` at the
top of `tests/cluster/main.rs`, because it leaves the upstream-derived test
source untouched and makes cargo report the skip instead of building an empty
binary. `hiqlite/Cargo.toml` is claimed by no spec, so no `extends` edge is
needed for it. The feature list is what the modules use (SQLite, the cache,
`lock`, listen/notify, the `CacheVariants` derive, and the backup tests), not
the whole `Check` set; `counters`, `toml` and `external-state-machine` are not
needed to build the target.

## Verification

```verify:cli
cargo +1.95.0 fmt --all -- --check
grep -q 'required-features = \["backup", "cache", "dlock", "listen_notify", "macros", "sqlite"\]' hiqlite/Cargo.toml
```
