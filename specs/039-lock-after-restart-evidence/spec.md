---
id: "039-lock-after-restart-evidence"
title: "A lock left held by a restart is granted to the next caller within one lease and a bounded margin"
status: draft
created: "2026-09-25"
owner: "hiqlite maintainers"
risk: medium
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "006-cache-state-machine"
  - "023-distributed-lock-lease-liveness"
origin:
  retroactive: false
establishes:
  # B-1 and B-2: the end-to-end regression tests, public API only.
  - "hiqlite/tests/lease_wake.rs"
extends:
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  # The new test file's freshness hash (lint L-008), as `037` did for its own.
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
summary: >
  rahi reported (its 045 D-22) that a distributed lock left held by a restart
  strands the next caller until its request times out. This spec records F-138,
  shows with an end-to-end restart test that the stall is the upstream F-102
  behavior `023` B-6 already repaired, and holds the bound the fork gives: the
  next caller is granted within one lease plus a bounded margin. No source
  changes.
---

# 039: A lock left held by a restart is granted to the next caller within one lease and a bounded margin

## 1. Purpose

rahi's `045` D-22 (2026-09-25) measured that a lease released just before a
stop, or held at a crash, reads as held after the restart, and that a `lock()`
queued before that lease runs out is never woken and fails after about 60
seconds. The work order that reported it asked for a repair spec in this
repository.

Measured here, the stall is real on the build rahi runs and absent from this
fork. rahi's `Cargo.lock` resolves upstream `sebadob/hiqlite` at
`8f3b9bde9454d563d604f49c527e1527e193c4ab` (0.14.0), which predates `023`. The
cause is F-102, repaired by `023` B-6 and shipped in every `0.15.0-patched.*`
release. This spec records the finding (F-138), adds the end-to-end evidence
`023` section 4 says B-6 lacks, and fixes the bound as a regression test. It
changes no source.

## 2. Ownership

This spec establishes `hiqlite/tests/lease_wake.rs` and adds F-138 to the
findings register. The lock handler and the client's bounded await stay `023`'s
(`hiqlite/src/store/state_machine/memory/` through `006`, and
`hiqlite/src/client/dlock.rs`). OpenRaft owns the log replay that rebuilds the
lock state on a start; hiqlite owns what the handler does with the replayed
entries and how long a client waits.

## 3. Behavior

Configuration: one node, `cache_storage_disk: true` (the cache log on disk, so
lock state is replayed on every start), features `cache` and `dlock`. The lease
is `LOCK_VALID_SECONDS` (10 s, not configurable).

### B-1. The next caller is granted within one lease and a bounded margin

After a restart that replays a `Lock` entry with no `LockRelease` for it, a
`lock()` made on the restarted node MUST be granted within one lease plus five
seconds of being made. The fork's bound is one lease plus two seconds
(`AWAIT_BOUND`, `023` B-6): the queued caller's await ends there and its
re-request with the same ticket takes over the expired lease.

### B-2. Every queued caller is served

Three callers queued on the same key behind the replayed holder MUST each be
granted, in turn, each within the B-1 bound of its own request.

## 4. Evidence and its limits

`hiqlite/tests/lease_wake.rs`, public API only, so the same file runs against
upstream. A held lock is leaked (`std::mem::forget`, so no release is ever
sent), the node is stopped cleanly and started again on the same directory,
and the test waits for `wait_until_healthy_cache` before asking.

- **Fork trunk** (`18386b4`, macOS arm64, `cargo +1.95.0 test -p
  hiqlite-patched --features cache,dlock --test lease_wake`): both pass; the
  caller was granted 12.0 s after asking, the three queued callers were all
  served within 12.6 s.
- **Upstream `8f3b9bde`** (the same file, `-p hiqlite`): both fail; no caller
  was granted within 15 s.

Limits. A clean stop stands in for a crash: what the test needs is a replayed
`Lock` with no `LockRelease`, which the leaked guard guarantees, and a real
crash reaches the same replay. It is not proved that the 60-second figure rahi
measured has no second cause; the upstream run shows only that the bound is
missed. The run is N=1; a follower-local await is `023` B-6's unit-tested
territory. A caller is served at one lease plus two seconds after it asks, not
at the moment the replayed lease expires: nothing wakes a parked waiter at
expiry (`023` B-6 item 1), so a caller that asks just after the restart waits
the full bound.

## 5. Known defects

None new. `023` KD-3 (a replayed lease lasts one more lease window, measured
from the restart) is what makes the replayed lock read as held at all, and it
stands as recorded there.

## 6. Out of scope

Waking parked waiters at lease expiry, a shorter or configurable lease, and any
change to rahi. rahi reaches this behavior by adopting a `0.15.0-patched.*`
release through its own governed change.

## 7. Resolved decisions

**D-1 (2026-09-25, no repair, because the fork already has it).** The work
order asked for a regression test observed failing on current code and a fix.
On this fork's trunk the test passes with no change, so there is nothing to fix
here and no fix is invented. It was observed failing on the build the defect
was reported from (upstream `8f3b9bde`), which is the before of `023` B-6's
repair. Recorded as a finding against upstream and as evidence for B-6, not as
a fork defect.

## Verification

Run with `just spine-verify 039`.

```verify:cli
# B-1 and B-2: the end-to-end regression tests (they fail on upstream 8f3b9bde, section 4)
cargo test -p hiqlite-patched --features cache,dlock --test lease_wake
# the bound they rely on is the client's bounded await from 023 B-6
sh -c 'grep -q "const AWAIT_BOUND: Duration = Duration::from_secs(LOCK_VALID_SECONDS as u64 + 2);" hiqlite/src/client/dlock.rs'
sh -c 'grep -q "^### F-138 " standards/spec/findings-register.md'
```
