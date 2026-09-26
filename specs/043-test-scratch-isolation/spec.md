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
  # B-2: the fixed ports of the fork's integration tests.
  - spec: "035-n1-upgrade-exclusion"
    unit: { kind: file, path: "hiqlite/tests/upgrade_exclusion.rs" }
    nature: additive
  - spec: "037-startup-recovery-readiness"
    unit: { kind: file, path: "hiqlite/tests/recovery_readiness.rs" }
    nature: additive
  - spec: "039-lock-after-restart-evidence"
    unit: { kind: file, path: "hiqlite/tests/lease_wake.rs" }
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

**B-2.** The fork's integration tests bind fixed ports below Linux's default
ephemeral range (32768 to 60999): `upgrade_exclusion` 28611 and 28612,
`recovery_readiness` 28711 to 28722, `lease_wake` 28741 to 28752 (each test
uses its API port and the next one for Raft). Before, they were the same
numbers plus 10000, inside that range (F-141).

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

For B-2 (F-141): the failure was seen once, on CI Linux amd64 (run
36215829784, both failing tests at `0.0.0.0:38612`, "Address already in use"),
and not reproduced: the same tree passed 9 of 9 runs in a Linux arm64
container and 20 of 20 on macOS. That the port was held by an ephemeral
connection on the runner is an inference from the port ranges, not an
observation. Ports below 32768 are outside the range the kernel hands out by
default; they can still be taken by a listener another process chose.

## 5. Known defects

**KD-1.** The `hiqlite-wal` lib tests use cwd-relative `test_data/...` paths
(`wal.rs`, `reader.rs`, `writer.rs`, `lockfile.rs`, `metadata.rs`,
`log_store_impl.rs`), with the same exposure to a concurrent run from the same
crate directory. Not repaired here; the workaround is one run at a time per
checkout, or an isolated working directory.

**KD-2.** The upstream-derived `cluster` suite binds 35001 to 35003 and 36001
to 36003, also inside Linux's ephemeral range. Not moved here: those tests are
upstream's, and no failure of theirs has been attributed to it.

## 6. Out of scope

Integration tests under `hiqlite/tests/`, which use their own directories, and
`hiqlite-wal` (KD-1).

## 7. Resolved decisions

**D-1 (2026-09-25).** The process id rather than a random or per-test unique
name, because the tests in one process already use distinct case names and
only cross-process sharing collided; a stable per-process root keeps paths
readable in failure messages.

**D-2 (2026-09-26).** Fixed ports moved below the ephemeral range rather than
ports chosen at run time: the tests restart a node on the same address, so a
port the kernel picked for the first start could be taken by the time of the
second; and `lease_wake` is `039`'s evidence, whose text names its setup.

## Verification

```verify:cli
grep -q 'fn test_scratch_root' hiqlite/src/lib.rs
cargo +1.95.0 test --locked -p hiqlite-patched --lib --features cache,dlock,listen_notify,macros,toml,external-state-machine -- storage_lock:: store::logs:: upgrade_exclusion:: cache_compat
```
