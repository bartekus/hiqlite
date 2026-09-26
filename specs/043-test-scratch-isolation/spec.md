---
id: "043-test-scratch-isolation"
title: "Give each lib test process its own scratch root, so concurrent runs from one checkout do not collide"
status: draft
created: "2026-09-25"
owner: "hiqlite maintainers"
risk: low
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "005-adoption-assessment-and-plan"
extends:
  # B-1: the scratch helpers of these units' lib tests. Test code only.
  - spec: "024-exclusive-storage-ownership"
    unit: { kind: file, path: "hiqlite/src/storage_lock.rs" }
    nature: additive
  - spec: "035-n1-upgrade-exclusion"
    unit: { kind: file, path: "hiqlite/src/upgrade_exclusion.rs" }
    nature: additive
  - spec: "007-cache-log-store"
    unit: { kind: directory, path: "hiqlite/src/store/logs/" }
    nature: additive
  - spec: "006-cache-state-machine"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/memory/" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
summary: >
  Repairs F-139 and F-140, two lib tests that failed once on a loaded host. The
  cause was not timing: the tests' scratch directories were fixed paths under
  `../target/test_data`, shared by every process running the lib tests from
  one checkout, so concurrent runs removed and locked each other's
  directories. Each test process now uses its own root. Test code only.
---

# 043: Give each lib test process its own scratch root

## 1. Purpose

F-139 (`storage_lock::tests::a_crash_releases_ownership`, `StorageInUse` after
the child aborted) and F-140
(`store::logs::tests::the_opt_in_moves_the_legacy_cache_aside_and_marks_the_new_one`,
a rename of a missing `.partial`) each failed once during the patched.4 suite
run on 2026-09-25 and passed alone and in the rerun.

## 2. Ownership

Test code in four units other specs own (`extends`, additive), plus a
`#[cfg(test)]` helper in `hiqlite/src/lib.rs`, which no spec claims. No
runtime behavior changes; OpenRaft and callers are untouched.

## 3. Behavior

**B-1.** `crate::test_scratch_root()` (`#[cfg(test)]`) returns
`../target/test_data/<process id>`. The scratch helpers of the `storage_lock`,
`upgrade_exclusion`, `store::logs` (cache format) and cache-compatibility lib
tests build their directories under it. A child process a `storage_lock` test
spawns receives its directory through `HQL_TEST_OWNER_DIR`, so it uses the
parent's directory, not its own process id.

## 4. Evidence and its limits

Measured on macOS arm64 from one checkout, with the lib test binary built with
`--features cache,dlock,listen_notify,macros,toml,external-state-machine`:

- Before (trunk `f4ce3a7`): two concurrent processes filtered to
  `storage_lock::` and `store::logs::`, 15 rounds: **30 of 30** processes
  failed, both F-139's and F-140's tests among the failures. One process at a
  time beside twelve `yes` CPU hogs, 30 rounds: **0 of 30** failed. The failure
  needs a second process, not load.
- After: two concurrent processes filtered to `storage_lock::`,
  `store::logs::`, `upgrade_exclusion::` and `cache_compat`, 15 rounds: **0 of
  30** failed (38 tests each).

The limit: the reproduction is two processes of the same binary; the original
failures were most likely a suite beside another run in the same worktree
(an acceptance block or a second feature set), which this models but was not
observed directly. The per-process directories are not removed after the run;
they live under `target/`.

## 5. Known defects

**KD-1.** The `hiqlite-wal` lib tests use cwd-relative `test_data/...` paths
(`wal.rs`, `reader.rs`, `writer.rs`, `lockfile.rs`, `metadata.rs`,
`log_store_impl.rs`), with the same exposure to a concurrent run from the same
crate directory. Not repaired here; the workaround is one run at a time per
checkout, or an isolated working directory.

## 6. Out of scope

Integration tests under `hiqlite/tests/`, which use their own directories, and
`hiqlite-wal` (KD-1).

## 7. Resolved decisions

**D-1 (2026-09-25).** The process id rather than a random or per-test unique
name, because the tests in one process already use distinct case names and
only cross-process sharing collided; a stable per-process root keeps paths
readable in failure messages.

## Verification

```verify:cli
grep -q 'fn test_scratch_root' hiqlite/src/lib.rs
cargo +1.95.0 test --locked -p hiqlite-patched --lib --features cache,dlock,listen_notify,macros,toml,external-state-machine -- storage_lock:: store::logs:: upgrade_exclusion:: cache_compat
```
