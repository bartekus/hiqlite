---
id: "037-startup-recovery-readiness"
title: "Neither healthy nor ready, and serving nothing, until startup recovery has applied the log"
status: draft
created: "2026-09-24"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "003-client-consistency-and-retry-outcomes"
  - "010-node-lifecycle-and-split-brain"
  - "027-node-lifecycle-and-startup-errors"
origin:
  retroactive: false
establishes:
  # B-1 to B-3: the per-group recovery record and its completion rule.
  - "hiqlite/src/recovery.rs"
  # Section 4: the regression tests, public API only, so they run against the published source.
  - "hiqlite/tests/recovery_readiness.rs"
extends:
  # B-4: every client operation and the health checks refuse while a group recovers.
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/client/" }
    nature: additive
  # B-4: `/health`, `/ready`, the client stream and the event stream refuse while recovering.
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/network/" }
    nature: superseding
  # The recovery record is held with each Raft group's state.
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/app_state.rs" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  # The two new files' freshness hash (lint L-008), as `035` did for its own.
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
references:
  - unit: { kind: file, path: "hiqlite/src/store/mod.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/lib.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/error.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/store/state_machine/sqlite/state_machine.rs" }
    role: "context"
summary: >
  Repairs F-134. hiqlite persists no committed index, so after every start a
  Raft group's state machine trails its own log until a leader of the new term
  commits, and after an unclean stop under auto-heal the SQLite state machine
  starts empty. Health and readiness only asked whether a leader was known, so
  a node reported healthy with nothing applied and Rauthy, reading at once,
  found its database empty and bootstrapped it again. Each group now records
  the last log index its log held at start; until its state machine has
  applied it, /health, /ready and the client health checks are false, every
  client operation and client stream is refused with the new, distinguishable
  Error::Recovering, and Client::recovery_state reports the progress.
---

# 037: Neither healthy nor ready, and serving nothing, until startup recovery has applied the log

## 1. Purpose

F-134 records the defect and its consequence. A node started, elected itself
or heard from a leader, and answered healthy and ready while its state machine
still held less than the node had acknowledged: nothing at all after an unclean
stop under `auto-heal`, whose rebuild deletes the SQLite database and replays
the Raft log. Rauthy waited for health, read at once, found `jwks` empty and
generated a second signing-key set; it shipped a consumer-side wait in Rauthy
`0.36.2-patched.3` (`bartekus/rauthy` #7, #8). The guarantee belongs here, so
that no consumer has to know how hiqlite rebuilds itself.

One responsibility: **a node that says it can serve has applied everything its
log held when it started, and until then it serves nothing and says why.**

The owner's decision of 2026-09-24 (D-1) states the requirement: health and
readiness stay false, and client writes and streams are refused, until startup
recovery completes on every path (the unclean-stop marker, the deep WAL
integrity check, the `auto-heal` rebuild, restore), with a recovery state that
lets a consumer report "recovering" rather than "down".

## 2. Territory

- **Establishes** `hiqlite/src/recovery.rs` (B-1 to B-3) and
  `hiqlite/tests/recovery_readiness.rs` (section 4).
- **Extends** `003`'s `hiqlite/src/client/` directory, additively (B-4 adds a
  check beside each existing one and a public method), and `003`'s
  `hiqlite/src/network/` directory, superseding what `/health` and `/ready`
  answered during recovery; `010`'s `hiqlite/src/app_state.rs`, additively (the
  record lives with each group's state); `005`'s findings register (F-134); and
  `000`'s `spec-spine.toml` for the two new files' freshness hash.
- **References** without claiming: `hiqlite/src/store/mod.rs` (where each
  record is created), `hiqlite/src/error.rs` (the new variant), `hiqlite/src/lib.rs`
  (the export) and the SQLite state machine, whose rebuild is unchanged.

Every existing acceptance command still holds: B-4 adds its checks beside the
`ensure_node_available` calls `027` and `035` count, and removes none.

## 3. Behavior

### B-1. What recovery is, per group

Each Raft group records, when it starts, the last log index its log holds (the
**target**), read from the log store before the state machine is built. The
group has **recovered** once its state machine has applied the target. An empty
log has no target and is recovered at once. The rule is the same on every path
named in section 1, because each of them ends in the same state: a log, and a
state machine that has applied less of it. A restore, the deep WAL integrity
check and `HQL_DANGER_RAFT_STATE_RESET` all run before the log state is read, so
the target is the log as they left it.

- **The SQLite group.** Its target is its WAL's last index. After an unclean
  stop under `auto-heal` it applies from nothing; without `auto-heal` the
  marker is still a refusal (`035` B-1, `027`), so the rebuild does not arise.
- **The cache group with `cache_storage_disk = true`.** Its state machine is in
  memory, so every start, clean or not, replays its WAL; its target is the
  WAL's last index.
- **The cache group with an in-memory log.** Its log starts empty, so it has no
  target of its own, and is recovered at once. What it receives from the
  cluster afterwards is replication, not recovery.

### B-2. A replaced tail

A follower may hold entries at the end of its log that were never committed. A
new leader replaces them, so the target may no longer exist. The group also
counts as recovered once a leader is known, its log has been cut below the
target, and everything its log now holds is applied. A follower that simply
lags, still holding the target, is not recovered by this route.

### B-3. Completion is permanent and watched, not polled

The record follows the group's Raft metrics and is marked complete the first
time B-1 or B-2 holds; it is never cleared for the life of the node. It stops
watching once complete, or when the group shuts down. Completion is logged with
what was applied and what the target was.

### B-4. Nothing is served before recovery

Until a group has recovered:

1. `/health` answers `503` with `Error::Recovering`, **before** the
   `health_check_delay_secs` grace period, which otherwise answers healthy
   unconditionally. `/ready` answers the same, after its shutdown check.
2. `Client::is_healthy_db` and `Client::is_healthy_cache` return
   `Error::Recovering` for their own group, so `wait_until_healthy_db` and
   `wait_until_healthy_cache` (and their bounded forms) wait through recovery.
   The bounded forms still stop early only for a terminal failure.
3. Every operation of an embedded client refuses with `Error::Recovering` for
   the group it uses: each rate-limit gate (every write, consistent query,
   cache and counter operation) and each local read, beside the existing
   `ensure_node_available` check.
4. A new client stream (`/stream/{raft_type}`), which carries remote and
   forwarded reads and writes, and a new event stream (`/listen`) are refused
   with `Error::Recovering` while **any** group of the node recovers. The remote
   clients reconnect, as they do for any refusal.

Replication between nodes and cluster management are not gated: they are how
recovery completes.

### B-5. The recovery state is distinguishable

`Error::Recovering(message)` is a new variant, added last so no earlier
variant's position changes, mapped to `503 Service Unavailable`, with
`Error::is_recovering()`. The message names the group, what has been applied,
the target, and whether the state machine is being rebuilt after an unclean
stop. `Client::recovery_state()` returns `Some(RecoveryState::Complete)` or
`Some(RecoveryState::Recovering(progress))`, one `RecoveryProgress` per group
still recovering, and `None` for a remote client, which has no local node. A
consumer reports "recovering" on `Error::Recovering` and "down" on
`Error::NodeFailed` or a connection failure.

## 4. Evidence and its limits

`hiqlite/tests/recovery_readiness.rs` uses only the public start and client API,
so the same file compiled against the published `0.15.0-patched.2` source
(`37910d7`, whose `hiqlite/` and `hiqlite-wal/` equal `5c2cdef`) and against the
repair. Two tests:

- `f134_unclean_stop_rebuild_is_not_healthy_before_the_replay` (`auto-heal`):
  3000 acknowledged single-row writes, a clean stop, the unclean-stop marker
  recreated, a restart; the first moment the client or `/health` says healthy,
  a read must see 3000 rows.
- `f134_cache_replay_is_not_healthy_before_the_replay` (disk-backed cache): 3000
  puts, a clean stop, a restart; the first moment the cache says healthy, the
  last key must read back.

Runs, each one launch, no retry:

| host | build | feature set | unrepaired (`37910d7`) | repaired |
|---|---|---|---|---|
| macOS arm64 | debug | Rauthy's | both fail: 37 of 3000 rows; last key `None` | both pass |
| macOS arm64 | debug | Rahi's | cache fails: last key `None` | passes |
| Linux arm64, Docker Desktop, native | release | Rauthy's | both fail: 18 of 3000 rows; last key `None` | both pass |
| Linux arm64, Docker Desktop, native | release | Rahi's | cache fails | passes |
| Linux amd64, GitHub-hosted, native (run 36084685834) | release | Rauthy's | both fail: 33 of 3000 rows; last key `None` | both pass (run 36085716425, on `56ce51a`) |
| Linux amd64, GitHub-hosted, native (run 36084685834) | release | Rahi's | cache fails | passes (run 36085716425) |

Rahi's set has no `auto-heal`, so the SQLite rebuild test is compiled out
there; the cache test is the one that applies. The unit tests in
`recovery.rs` cover B-1's and B-2's rule directly, including that a leader
alone is not enough and that a lagging follower is not taken for a replaced
tail. With the repair, on macOS arm64: the library suites pass under the default
(103), Rauthy's (159) and Rahi's (150) feature sets, `hiqlite-wal`'s passes
(39), and the three-node cluster suite runs to "All tests successful"
(`TEST_SKIP_S3_RESTORE=true`).

What this does **not** establish:

- **The unclean stop is simulated.** A clean stop followed by recreating
  `state_machine/lock` is exactly what the state machine's check sees, but it
  is not a `SIGKILL`; a `SIGKILL` may also leave a WAL whose tail the deep
  integrity check repairs. Rauthy's leg Q is the `SIGKILL` evidence, and it is a
  consumer's.
- **B-2 is not executed.** No test cuts a follower's uncommitted tail; the rule
  is unit tested only.
- **No three-node restart asserts B-4.** The cluster suite restarts nodes and
  passes, which shows the gate does not wedge a follower, not that a follower
  refuses during its own replay.
- **The race is a race.** The tests fail on the unrepaired source because 3000
  entries take longer to apply than an N=1 election; the repaired source has no
  race to lose, because health waits on applied state rather than on time.

## 5. Known defects

**KD-1. The local event receiver is not gated.** `Client::listen` with
`listen_notify_local` reads a channel the cache state machine feeds while it
applies, so a replay re-delivers notifications to local listeners, before and
after this repair. Refusing the call would not stop the channel being fed, and
would turn a consumer's listen loop into an error loop at every start.

**KD-2. A consumer that never waits is refused rather than served.** Before
this repair a write issued straight after `start_node` succeeded and a read saw
partial state; now both fail with `Error::Recovering` until recovery completes.
That is the requirement, but it is a behavior change for a consumer that uses
one group without waiting for its health (Rauthy waits only for the database
group before using the cache).

**KD-3. `health_check_delay_secs` no longer covers recovery.** A liveness probe
on `/health` that is shorter than a long replay now fails where it used to pass,
and may restart the node before recovery completes. The consumer handoff says
to size the probe, or use a startup probe, accordingly.

## 6. Resolved decisions

**D-1 (2026-09-24, owner decision: the requirement, and this spec's approval).**
The owner escalated F-134 as a high-severity defect, directed this spec to be
drafted, approved and implemented, and stated its requirement (section 1). The
approval is the owner's; the `status` flip to `approved` is the owner's act and
is not made by this change (`000`).

**D-2 (2026-09-24, the target is local).** Recovery is measured against the log
this node held at start, not against a leader's commit index. It needs no peer,
it is what this node may have acknowledged, and hiqlite does not persist a
committed index to measure against. B-2 covers the one case where the local log
overstates it.

**D-3 (2026-09-24, refusal is per group, readiness per node).** A client
operation is refused only while the group it uses recovers, so a consumer that
waited for its database is not refused because the cache is still replaying.
`/health`, `/ready` and the client and event streams answer for the node, which
recovers when every group has.

**D-4 (2026-09-24, `start_node` does not wait).** A start that blocked until
recovery would block a follower whose cluster has no quorum, which today returns
and waits for health. Refusal plus the existing health waits give the same
guarantee to a consumer that waits, and a visible error to one that does not.

**D-5 (2026-09-24, the new checks sit beside the old ones).** `027`'s and
`035`'s acceptance count the `ensure_node_available` calls in `rate_limit.rs`
and `query.rs`. The recovery check is a separate line after each, so their
acceptance is unchanged and not amended.

## 7. Out of scope

- **The release that carries this.** A separate release spec.
- **Persisting a committed index**, which would let OpenRaft apply at start
  instead of after the first commit; an OpenRaft storage contract change.
- **The consumers' own waits.** Whether Rauthy's F22 can be dropped is Rauthy's
  decision, verified against a release that carries this.
- **KD-1.**

## Verification

Run with `just spine-verify 037`.

```verify:cli
# the regression tests, public API only; they fail on the published source (section 4)
cargo test -p hiqlite-patched --features cache,cast_ints,counters,dashboard,listen_notify_local,macros --test recovery_readiness
cargo test -p hiqlite-patched --no-default-features --features sqlite,cache,counters,dlock,listen_notify_local,backup,s3 --test recovery_readiness
# B-1 to B-3: the rule, unit tested
cargo test -p hiqlite-patched --lib --features cache recovery::tests
# B-1: the target is the log state read before the state machine is built, for both groups
sh -c 'grep -q "StartupRecovery::new(\"db\", recovery_target, unclean_stop)" hiqlite/src/store/mod.rs'
sh -c 'grep -q "StartupRecovery::new(\"cache\", recovery_target, false)" hiqlite/src/store/mod.rs'
sh -c 'test "$(grep -c "recovery.watch(raft.metrics());" hiqlite/src/store/mod.rs)" -eq 2'
# B-4.1: /health refuses before its grace period, /ready after its shutdown check
sh -c 'a=$(grep -n "state.ensure_recovered()?;" hiqlite/src/network/api.rs | head -1 | cut -d: -f1); b=$(grep -n "if check_health(&state).await.is_err()" hiqlite/src/network/api.rs | cut -d: -f1); test -n "$a" && test -n "$b" && test "$a" -lt "$b"'
sh -c 'test "$(grep -c "state.ensure_recovered()?;" hiqlite/src/network/api.rs)" -eq 3'
# B-4.2 and B-4.3: the health checks and every gate, beside the existing check
sh -c 'test "$(grep -c "self.ensure_db_recovered()?;" hiqlite/src/client/query.rs)" -eq 7'
sh -c 'test "$(grep -c "self.ensure_node_available()?;" hiqlite/src/client/query.rs)" -eq 7'
sh -c 'grep -q "self.ensure_db_recovered()?;" hiqlite/src/client/rate_limit.rs'
sh -c 'grep -q "self.ensure_cache_recovered()?;" hiqlite/src/client/rate_limit.rs'
sh -c 'test "$(grep -c "self.ensure_cache_recovered()?;" hiqlite/src/client/cache.rs)" -eq 3'
sh -c 'grep -A3 "pub async fn is_healthy_db" hiqlite/src/client/mgmt.rs | grep -q "self.ensure_db_recovered()?;"'
sh -c 'grep -A3 "pub async fn is_healthy_cache" hiqlite/src/client/mgmt.rs | grep -q "self.ensure_cache_recovered()?;"'
# B-4.4: the client stream refuses while recovering
sh -c 'grep -A6 "debug!(\"New Raft Stream for" hiqlite/src/network/api.rs | grep -q "state.ensure_recovered()"'
# B-5: the distinguishable state
sh -c 'grep -q "Recovering(Cow<.static, str>)," hiqlite/src/error.rs'
sh -c 'grep -q "Error::Recovering(_) => StatusCode::SERVICE_UNAVAILABLE" hiqlite/src/error.rs'
sh -c 'grep -q "pub fn is_recovering(&self) -> bool" hiqlite/src/error.rs'
sh -c 'grep -q "pub fn recovery_state(&self) -> Option<crate::RecoveryState>" hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "pub use recovery::{RecoveryProgress, RecoveryState};" hiqlite/src/lib.rs'
# the register records the finding against this spec
sh -c 'grep -q "^### F-134 .defect., confidence .high." standards/spec/findings-register.md'
sh -c 'grep -q "037-startup-recovery-readiness" standards/spec/findings-register.md'
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
sh -c '! grep -rl "$(printf "\342\200\224")" specs/037-startup-recovery-readiness hiqlite/src/recovery.rs hiqlite/tests/recovery_readiness.rs'
```
