---
id: "027-node-lifecycle-and-startup-errors"
title: "Return startup failures as errors, observe background failures, and define the node lifecycle API"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "008-wal-append-completion-notification"
  - "010-node-lifecycle-and-split-brain"
  - "022-replicated-cache-command-compatibility"
  - "024-exclusive-storage-ownership"
amends: ["010-node-lifecycle-and-split-brain"]
# D-7: this spec's `## Verification` block IS 010's acceptance from now on, and 010's own file
# is not edited. Whole-block replacement is the mechanism's unit.
amends_verification: ["010-node-lifecycle-and-split-brain"]
amends_sections:
  - "3-behavior"
  - "5-known-defects"
establishes:
  - "hiqlite/src/lifecycle.rs"
  - "hiqlite/src/bin/abort_probe.rs"
  - "hiqlite/src/membership_gate.rs"
extends:
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/start.rs" }
    nature: superseding
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/app_state.rs" }
    nature: superseding
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/split_brain_check.rs" }
    nature: superseding
  - spec: "001-wal-durability-and-completion"
    unit: { kind: directory, path: "hiqlite-wal/src/" }
    nature: additive
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/client/" }
    nature: additive
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/network/" }
    nature: additive
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Repairs F-009, F-014, F-025, F-039 and F-040 as one policy. Expected
  configuration and startup failures are returned as named errors instead of
  depending on a panic profile that belongs to the consumer; both listeners are
  bound before a node is reported started and shut down gracefully on both the
  TLS and plaintext paths; a partial startup tears down what it started; and a
  WAL writer that ends, for any reason including a panic inside it, takes the
  node out of service instead of leaving it answering as though it were
  healthy. Defines the minimum public error and lifecycle API and runs the
  expected failure paths under an abort profile no test can use.
---

# 027: Return startup failures as errors, observe background failures, and define the node lifecycle API

## 1. Purpose

`010` adopted the node lifecycle as found and recorded four defects. Three of
them, and F-025 in `006`, are the same defect wearing different clothes:

**hiqlite expressed expected failures as panics inside detached tasks, and that
makes the outcome depend on a panic profile that, for an embedded node, belongs
to the consumer.** Under `panic = "abort"` a malformed environment variable ends
the whole application; under unwinding it kills one task silently and the node
carries on looking healthy. Neither is a failure hiqlite reported, and neither is
hiqlite's decision to make.

`008` KD-3 is the other half. The WAL writer's termination report has no
consumer, so a node whose log storage has died goes on answering `/health` and
`/ready` on the strength of its Raft metrics.

One responsibility: **what hiqlite tells its caller when something it depends on
cannot start or has stopped, and who decides what happens next.**

## 2. Territory

**Establishes** `hiqlite/src/lifecycle.rs` and
`hiqlite/src/bin/abort_probe.rs`, neither of which existed.

**Extends**, as `superseding`, `010`'s `start.rs`, `app_state.rs` and
`split_brain_check.rs`; and additively `001`'s `hiqlite-wal/src/` (which gains
the writer-failure watch), `003`'s client and network directories, and `000`'s
`spec-spine.toml`.

**Amends** `010` sections 3 and 5 and carries its acceptance (D-7).

**Ownership boundary, and it is the whole point of this spec.** hiqlite owns
**reporting**. It does not own the decision to end the process: an embedded node
is a library inside somebody else's application, and taking that application
down is not a storage library's call. Every failure below is recorded, made
visible, and handed to the caller.

## 3. Behavior

### B-1. Expected failures are returned, not panicked

An expected failure is one hiqlite can describe: a value that does not parse, a
variable that is missing, an address that cannot be bound, a component that
could not be constructed. All of them now produce `Error::Startup` or an
existing named error, returned from the constructor.

What changed, by site:

| site | was | is |
|---|---|---|
| `HQL_SPLIT_BRAIN_INTERVAL` (F-009) | `.expect(..)` inside a spawned task | validated **before** the task is spawned, returned as `Error::Startup` |
| the listen-address parse (F-040) | `expect` in a detached task | `Error::Startup` naming the endpoint and the address |
| the TCP bind (F-040) | `expect` in a detached task | `Error::Startup` naming the endpoint, the address and the OS reason |
| `Raft::new` for either group | `.expect("Raft create failed")` | `Error::Startup` |
| opening the SQLite state machine | `.unwrap()` | `Error::Startup` |
| the S3 configuration (F-061) | five `expect`s and an `unwrap` | named `Error::Config`, repaired by `026` |

This is not a claim that hiqlite no longer panics anywhere. Section 5 says what
is left and why.

### B-2. Both listeners are bound before the node is reported started

`bind_listener` parses the address, binds it, and hands back an already-bound
socket, for **both** endpoints, before `AppState` is constructed and before any
server task is spawned. A port already in use is therefore an error the caller
receives, where before it was a panic in a task whose `JoinHandle` was dropped
at the moment `start_node_inner` was returning `Ok`.

The socket is handed to the server as a bound listener on both paths, which is
what removes the gap: nothing re-resolves or re-binds the address later.

### B-3. Both paths shut down gracefully

The TLS branches had no shutdown future at all (F-039), which the `TODO`s they
carried acknowledged. With TLS on both endpoints neither plaintext branch ran,
both watch receivers dropped when startup returned, and the shutdown sequence
then `expect`ed a send on a channel with no receivers: **a fully TLS-configured
node could not be shut down without a panic, and no TLS listener was ever shut
down gracefully.**

Each listener now takes its own receiver. The plaintext path keeps
`with_graceful_shutdown`; the TLS path uses `axum_server::Handle` and a ten
second grace period, which is that server's equivalent.

### B-4. A partial startup tears down what it started

If the cache group fails to start after the SQLite group has, or if either
listener cannot bind, the groups that are already running are shut down before
the error is returned: the raft cores, the WAL writer threads through their
shutdown handles, and the SQLite writer thread.

A constructor that returns `Err` and leaves threads running against the data
directory it was told to open is worse than one that never started: the storage
stays busy behind a node that reported it had not started, and the exclusive
ownership lock is released only when a local guard happens to drop.

### B-5. A failed component takes the node out of service, and nothing restarts it

`NodeLifecycle` is a `OnceLock` holding the **first** terminal failure, shared by
everything that has to honour it. A later failure does not replace it, because
the first one is the one that explains the node's state and a later one is
usually its consequence.

Who sets it:

- the **WAL writer** for either raft group, through the watch `hiqlite-wal` now
  exposes (B-6);
- a **listener** whose server task returns an error rather than a requested
  shutdown;
- the **cache state machine**, through `022`'s incompatibility record, which
  `/health` and `/ready` now also consult.

What it does: `/health` and `/ready` fail **before** any Raft metric is
consulted, `Client::is_healthy_db` and `is_healthy_cache` refuse, and
`Client::node_failure` reports it.

**Nothing restarts the failed component and nothing clears the record.** An
automatic restart of a writer whose durability guarantees have already been
broken is how a node comes back looking healthy with a hole in its log. The
recovery path is restarting the node, and the error message says so.

**Nothing ends the process.** The message says that too, because the caller has
to know that the decision is theirs.

### B-6. The WAL writer's termination has a consumer

`008` D-2 deliberately did not retain the writer thread's `JoinHandle`, and
`008` KD-3 recorded that the resulting ERROR log had no consumer. `LogStore`
now exposes a `watch` receiver with **three** states, and the third is the one
that matters:

- `None`: the writer is running.
- `Some(reason)`: it ended and said why.
- **closed**: it ended without saying why, which is what a panic unwinding past
  the reporting closure looks like from outside it.

All three are handled. The third is still a terminated writer and still not a
healthy node, and hiqlite does not invent a cause for it. This is an
observation, not supervision: nothing joins the thread and nothing restarts it.

**A shutdown closes that channel too**, because ending the writer thread is
exactly what a shutdown does, and the third state cannot tell the two apart on
its own. `NodeLifecycle::begin_shutdown` is therefore called by the shutdown
path before anything is asked to stop, and `fail` records nothing after it. The
third state then means "ended unexpectedly" rather than "ended", which is what
it was always supposed to mean.

A failure recorded **before** the shutdown is kept: the node did fail, that is
still why it is going away, and `ensure_available` still says so. This is F-101,
and it was this spec's own defect: three nodes shut down on purpose each logged
`this node is out of service because the Raft log WAL writer failed` and
recorded a terminal failure that was not one.

### B-7. The minimum public API this release freezes

Four things, and no more:

- `Error::Startup(Cow<'static, str>)`, for a node that could not start.
- `Error::NodeFailed(Cow<'static, str>)`, for a node that is out of service,
  mapped to `503`.
- `Client::node_failure() -> Option<NodeFailure>`, and
  `Client::ensure_node_available() -> Result<(), Error>`.
- `hiqlite::lifecycle::{NodeFailure, FailedComponent, NodeLifecycle}`, where
  `NodeLifecycle`'s mutating methods are crate-private: a caller can read the
  state and cannot set it.

Deliberately **not** frozen here: any readiness enum, any health-check trait,
any restart policy, any supervision hook. W-17 asks whether the public API is
frozen for this fork and the answer this spec gives is narrow: these four are
the lifecycle surface a consumer needs to tell a working node from a failed one,
and the rest of the API is whatever `0.14.0` had plus the error variants this
release adds.

### B-8. The split-brain watchdog is removed

`010` D-1 preserved it under OD-3, which directed that behavior be preserved
during retroactive adoption and the actual behavior established first. That
investigation is done and F-014 records it: under `panic = "abort"` the
checker's own panic already terminates the process, so the watchdog is
unreachable for its stated purpose; under unwinding both the checker and the
watchdog panic into `JoinHandle`s nobody awaits, so split-brain checking stops
silently and nothing terminates.

An `assert!` in a task nobody joins is not a safety net under either profile. It
is removed, and what replaces it is the checker no longer having a panic to be a
net for: its one unvalidated read is validated before the task is spawned (B-1).

### B-9. A node leaving its own cluster may not change anyone else's membership

`leave_cluster`'s guard asked only whether this node reports itself leader. A
node that removes **itself** from the voter set keeps reporting leadership for a
window, so the guard authorized a membership change that openraft then refused,
because openraft's own invariant is about being a **voter** and not about the
reported leader id.

The decision is now a pure function with four refusals: the node is shutting
down; there is no leader; the node is not the leader; or the node is the leader
and **not a voter**. The last one is the hole that was there.

Every refusal is `Error::LeaderChange`, which the HTTP layer maps to `409`, and
`leave_remote_cluster` already walks to the next node on a non-success. A
refusal is therefore a redirect, not a failed leave, and the self-removal path
that a shutting-down leader legitimately performs is untouched.

F-107. The sequence that exposed it spans two milliseconds and cannot be
scheduled by a test, so section 4 says how it is tested instead.

**The decision alone did not close F-107** (D-8). Tracing every membership
mutation and both shutdown paths found four more gaps, none of them observed
failing, all of them read from source:

1. the decision ran in `are_we_leader`, **before** `raft_lock` was taken, so a
   request authorized against one membership could act on another after waiting
   for the lock;
2. `post_membership` changed membership with neither the decision nor the lock;
3. the raft stream's `RemoveMembershipCache` took the lock but never asked
   whether this node was leader, voter, or shutting down;
4. shutdown stopped both raft groups without the lock, so a stop could run while
   a membership change admitted earlier was still in flight on the same node.

`membership_gate` is now the only way to change membership, and the ordering it
enforces is the same for both raft groups, which share one gate:

- **Admission closes first.** Shutdown closes the gate before its pre-shutdown
  delay. A request arriving afterwards is refused **before** it waits for the
  lock, with `LeaderChange` (`409`, "ask another node").
- **Every change is decided under the lock.** The decision reads the metrics the
  change will act on, and requires the reported leader to be this node, its raft
  to be in the `Leader` state, and this node to be a voter. A request that queued
  before the gate closed and is served after it is refused.
- **Shutdown drains, bounded.** It waits at most five seconds for an admitted
  change to finish. On timeout it returns `Error::Timeout` and **stops nothing**:
  the gate stays closed, the node keeps serving everything except membership
  changes, and the caller may shut down again or end the process. A process that
  ends is a crash, which Raft tolerates; a raft group stopped under a running
  membership change is what this ordering prevents. It does not fall through to
  the unsynchronized stop.
- **Every stop runs under the gate.** The shutdown holds it until both raft
  groups, the WAL writer and the SQLite writer have been asked to stop. A stopped
  gate stays closed, and a second shutdown is a no-op.
- **A caller that stops waiting does not cut the stop short, while the runtime
  lives.** The sequence runs in its own task. `Client::shutdown` and
  `ShutdownHandle::wait` still wait at most fifteen seconds, and when that passes
  they now return `Error::Timeout` saying the sequence continues. **A process that
  exits then ends the task wherever it is**, which is a crash for every component
  not yet stopped; independent review corrected an earlier claim here that
  cancellation could not split the stop at all. On a single node the pre-shutdown
  delay is skipped, so the drain and the stops fit the fifteen seconds; with peers
  the 9.5-second delay leaves little of it, which is one reason N=3 is not a
  supported topology for this release.
- **The result reaches the caller.** `Client::shutdown` and
  `ShutdownHandle::wait` used to discard the sequence's own result and return
  `Ok` after a drain timeout that had stopped nothing, or a component that had not
  stopped. Both now return it, and a later shutdown after an unclean one says so
  rather than reporting the node already shut down.
- **A failing stop no longer skips the rest.** Each component's stop is
  attempted; the first failure is returned, and storage ownership is then kept
  rather than released, because a component that did not stop may still hold the
  data directory.
- **Everything under the gate is bounded.** The loops that waited for local
  metrics to show a committed change give up after ten seconds; every openraft
  membership call (`add_learner(.., true)`, which waits for the learner to catch
  up, and `change_membership`) after thirty; and leaving an in-memory cache
  cluster through a peer during shutdown after ten, after which the stop goes on.
  Cancelling an openraft membership call is safe because openraft serializes
  membership changes itself and refuses a second while one is in progress.
- **An out-of-service node refuses its embedded client.** `ensure_node_available`
  said every local operation went through it; only the health checks and the
  network API did. Found by the Rauthy integration's fault injection (F-110):
  after a WAL writer failure, writes failed inside openraft with "sending on a
  closed channel" and reads were still served. Both rate-limit gates, which every
  write, consistent query and cache operation passes, and every local read and
  listen now refuse with `Error::NodeFailed` and the lifecycle's account.

The membership helpers in `helpers.rs` take a `&MembershipHeld`, which only the
gate produces, so a new call site that bypasses the gate does not compile.

**Deadlock.** While shutdown holds the gate it waits on the cache self-leave (a
local change, or an HTTP call to another node's leader), `Raft::shutdown` for
both groups, the WAL writer and the SQLite writer. None of them takes the gate.
The only inbound path that does is a membership request, and every one checks
the closed gate before it waits, so none can queue behind the drain and stall the
raft stream that carries the replication a remote leave needs. `Raft::initialize`
is outside the gate: it runs only on a pristine node during startup, before the
API serves.

### B-10. An upgrade from hiqlite 0.14.x carries the database and not the cache

Added 2026-09-22, after the Rauthy integration observed the patched build abort
with exit `134` on a data directory Rauthy v0.36.2 (hiqlite 0.14.0) had written.
Three defects stacked, and this is the contract that replaces them.

**The cache raft's log format changed upstream.** PR #362 (`1a4e345`, inside
this fork's upstream baseline) inserted `GetRemove` and `Replace` at indices 2
and 3 of `CacheRequest` and made every variant feature-independent. hiqlite
0.14.0 wrote the old, feature-dependent layout. A 0.14.0 cache log is therefore
unreadable by any build from upstream's post-0.14.0 tree, and worse than
unreadable: some entries decode **as a different command** (a legacy `Delete`
as `GetRemove`), so decoding it at all risks silent divergence (F-111). The
SQLite raft's `QueryWrite` layout is unchanged for builds with `backup`, which
Rauthy and Rahi both enable.

**Stop every node, then upgrade.** The marker guards a node's own disk; it does
not guard the network. A node of this release in a cluster whose leader still
runs 0.14.x would receive legacy cache entries and snapshots over replication and
decode them in the new layout, which is F-111's divergence by another route. A
mixed-version cluster is not supported in either direction. This release
supports N=1, where the question does not arise; at N>1 every node is stopped
before any is upgraded. Found in independent review of `34641b0`.

**The contract.** The SQLite database and its raft log are carried across an
upgrade from 0.14.x, and across a downgrade to it. The cache raft's log and
snapshots are not, in either direction. A disk-backed cache (`cache_storage_disk
= true`, the default) marks the log directory it creates with its format. On
start, a `logs_cache` holding WAL files **without** that marker is refused with
`Error::Startup`,
before anything decodes it, naming both directories, the opt-in and the manual
procedure. With `HQL_CACHE_LEGACY_MOVE_ASIDE=true` (exported as
`hiqlite::CACHE_LEGACY_MOVE_ASIDE_ENV`) the start instead moves both directories
into `{data_dir}/pre-upgrade-<unix seconds>/`, logs what it moved, and starts
with an empty cache. The opt-in does nothing when the marker is present or the
cache is in memory. A downgrade to 0.14.x must move `logs_cache` and
`state_machine_cache` aside first, by hand: 0.14.0 has no marker check, and this
release's cache log would decode there as the wrong commands.

**The abort that hid it (F-112).** The WAL reader thread `unwrap`ped every send.
The failed start dropped its receiver, the next send panicked, and under a
consumer's `panic = "abort"` the process ended before the startup error could be
returned. A requester that stops reading now abandons its request, and the
reader keeps serving.

**Review of the first form, 2026-09-22.** Snapshots alone were first treated as
legacy too, which refused a node of this very build whose only change was
switching `cache_storage_disk` on: a memory-only run leaves snapshots under
`state_machine_cache`. Only WAL files are evidence now, and the snapshot
directory still moves with a legacy log. The move-aside now syncs the destination
as well as the source directory. A downgrade to 0.14.x that skipped the manual
move-aside leaves the marker beside legacy entries, and the next upgrade accepts
them; that is the operator's step and the guard cannot see it.

**Where the guard runs.** Immediately after storage ownership is taken, before
the restore path, the reset path and either raft group. It first ran inside the
cache raft's construction, after the SQLite raft had opened the database, and a
refused start then left the database checkpointed and with fresh optimizer
statistics: application data was identical, but "Nothing was changed" was not
literally true. Observed by the Rauthy integration on 34641b0. Only the owner
lock file is created before a refusal now.

It runs a **second** time after the reset path and before the cache raft is
built. Moving it first broke `HQL_DANGER_RAFT_STATE_RESET`: the reset deletes
`logs_cache`, marker and all, the cache raft then wrote WAL files into an
unmarked directory, and the next ordinary start was refused as legacy (found in
review of `e1e9135`; the reset, restart sequence now starts and restarts). The
check is idempotent. A legacy cache and a reset requested together are refused at
the first check, before the reset would have wiped the cache; the opt-in covers
that case.

**The failure that was not one (F-113).** The failed start's log store, dropped
on the error path, ended its writer, and the watch recorded that as "the Raft log
WAL writer failed". A raft that fails to construct, and every teardown after a
partial start, now marks the lifecycle as shutting down first. Review found the
same class one step later: a failure in `init_pristine_node_1_*`, after the raft
had been built, returned with the raft and its WAL writer still running, so the
writer's lock outlived the failed start and the next start in the same process
panicked on it. Both groups now stop what they started before returning.

## 4. Evidence and its limits

**The abort-profile half is real and is the unusual part.**

No test in this repository can run under `panic = "abort"`, because Rust's test
harness requires unwinding: `cargo test` builds with unwind whatever the profile
says. An expected failure that is a panic therefore looks **identical** to one
that is a returned error when a test runs it, and the difference only appears in
a consumer's build. That is precisely the failure mode F-009, F-014, F-025 and
F-040 all share, so testing it under unwind alone would be testing the wrong
thing.

`hiqlite-abort-probe` is a binary, built `--release` so it inherits the
workspace's `panic = "abort"`, that calls five expected failure paths and reports
whether each returned an error. Run and observed: all five returned errors, exit
`0`. **And it was observed catching the defect**: with the
`HQL_SPLIT_BRAIN_INTERVAL` parse restored to its `.expect(..)`, the same binary
aborted with exit `134`, which is `SIGABRT`.

Under unwind, four unit tests:

- a malformed and a zero split-brain interval are startup errors, and the
  documented default of sixty seconds is unchanged;
- a listener that cannot start is a startup error, for an unparsable address and
  for one already in use, and the same address binds once it is free;
- the lifecycle record keeps the first failure, refuses with an account naming
  the component and both the recovery path and the fact that the process is not
  ended, and fails the node both for a reported writer failure and for a writer
  thread that ended **without** one;
- a deliberate shutdown is not a failure, and a failure recorded before one
  survives it. Added 2026-09-22 for F-101, and observed failing against this
  spec's first implementation, which recorded every clean shutdown as a WAL
  writer that had ended without reporting a reason.
- the membership gate, six tests with tokio's clock paused and every interleaving
  driven explicitly, so the schedule is the same on every run. Added 2026-09-22
  for D-8. They show: the original check-then-lock ordering acting as a non-voter
  on an authorization given to a voter, and the same schedule refused through the
  gate; shutdown waiting for an admitted change and admitting nothing after,
  including a request that queued before it; a drain that times out after exactly
  its bound, stops nothing, keeps admission closed and succeeds when retried; a
  cancelled drain that leaves nothing held; a bounded admission wait shared by
  both raft groups; and a refused decision that holds nothing. **Each of four
  mutations was observed failing a test**: deciding before the lock; dropping the
  closed check under the lock; letting a drain timeout fall through to a stop;
  and draining without closing admission.
- a node leaving its own cluster may not commit a membership change, and every
  refusal is one the caller can retry elsewhere. Added 2026-09-22 for F-107.
  All five input combinations are enumerated, because the sequence that produced
  the defect spans two milliseconds between two tasks and no test can schedule
  it. **Both tests were observed failing with the voter check removed.** The
  panic itself was reproduced twice, once in CI and once locally, and three
  further full cluster runs after the repair are clean; three clean runs are
  reported as what they are, which is not proof for a defect that appeared once
  in roughly seven.

What the acceptance does **not** establish:

- **No node is started in any test.** B-2's ordering, B-3's shutdown handles and
  B-4's teardown are source changes. Every one of them needs a running node,
  which is `012`'s surface, and B-3's is the one `010` KD-3 already said needs
  real TLS material and a real shutdown.
- **The teardown is not observed.** Nothing asserts that the threads a partial
  startup started have actually stopped.
- **The probe covers five paths, not every panic.** Section 5 lists what is left.
- **`022`'s cache failure is wired into readiness but not tested there**, for the
  same reason `022` section 4 gives: no served request is refused in a test.
- **Nothing measures what a consumer's profile does.** The probe establishes
  hiqlite's behavior under abort; it says nothing about how an application that
  embeds it handles the error it now gets.

## 5. Known defects

**KD-1. hiqlite still panics in plenty of places.** This spec repairs the
**expected** failure paths named by F-009, F-025, F-039 and F-040, plus the
constructor `unwrap`s beside them. It does not sweep the crate: `.expect(..)` on
internal channels that "can never be closed", the `unreachable!` arms, the
forbidden-function panics in the SQLite writer, and `init::get_this_node`'s
`expect` are all still there. Several are defensible and none has been argued
through here.

**KD-2. The teardown is best effort and unobserved.** B-4 sends the shutdowns and
ignores their results, because a startup that is already failing has nothing
better to do with a second failure. Nothing confirms the threads stopped.

**KD-3. A failed node still holds its storage.** B-5 takes it out of service and
does not release the exclusive ownership lock, because the failed component may
still hold file handles. So a failed node keeps its data directory until the
process ends, and a replacement node cannot take it over in the meantime.

**KD-4. There is no health surface for a failure that is not terminal.** The
record is a `OnceLock`: either the node is serving or it is out of service. A
degraded state, for example one raft group healthy and the other not, has no
representation.

**KD-5. The watch cannot distinguish a panic from a `SIGKILL`.** B-6's closed
channel means "the thread is gone without a reason", and a panic is one way to
get there. `008` section 3.6 already stated that an abort or a signal is reported
by nothing, and that is unchanged.

**KD-7. The gate is tested as a gate, not on a running node.** The six D-8
tests drive the real `MembershipGate` with a stand-in for the raft state. No test
starts a node and interleaves an HTTP membership request with `Client::shutdown`,
so the wiring of every path through the gate rests on the compile-time token and
on review, not on execution. The cluster suite exercises shutdown and leave, but
does not schedule the race.

**KD-8. A membership call can hold the gate for thirty seconds.** A learner
catching up, or a change waiting for a quorum, keeps its openraft call running
under the gate up to `MEMBERSHIP_OP_BOUND`, so a shutdown in that window returns
its drain timeout and stops nothing. That is the designed outcome, and the caller
now sees it, but the node cannot shut down cleanly until the call ends.

**KD-10. F-110's refusal is read from source, not injected.** No test fails a
component and then calls a client operation; the Rauthy injection is the
evidence the defect existed, and the refusal is wired at the gates named above.

**KD-9. The release-build consequence of F-107 is not established.** In
openraft 0.9.25 both checks in `append_membership` are `debug_assert!`. Read from
source, a release build appends the membership to the effective state and
rebuilds replication streams on a node whose server state is no longer `Leader`.
What that does to commit and to the cluster's membership was never executed.

**KD-11. B-10's cross-version evidence is one directory and no snapshot.** The
upgrade, downgrade and re-upgrade cycle was run once, on the directory Rauthy
v0.36.2 wrote, with builds from this tree and from upstream `v0.14.0` on the same
machine: the database carried in both directions, with rows written on each side
of each transition read back on the other. No SQLite snapshot existed at any
point, so snapshot readability across versions is read from source (the naming
is unchanged), not executed. A 0.14.x node built **without** `backup` has a
different `QueryWrite` layout and is not covered by the contract at all.

**KD-6. `010` KD-1 and KD-2 are untouched.** The bracketed IPv6 listen address
and the two meanings of `node_id` are not repaired here. The IPv6 one is now a
**returned** error rather than a panic in a detached task, which is a smaller
problem and still a defect.

## 6. Resolved decisions

**D-1 (2026-09-21, report and refuse; never exit).** The alternative was a
configurable "terminate the process on fatal error" policy, which several
databases offer. Declined for an embedded library: the process belongs to the
embedding application, and a storage dependency that can end it is a dependency
that has to be sandboxed. Saying so in the error message is part of the
decision, not decoration.

**D-2 (2026-09-21, terminal, with no automatic restart).** Owner direction, and
the reason is B-5's: a writer whose durability guarantees have been broken and
then silently restarted is a node that looks healthy with a hole in its log.

**D-3 (2026-09-21, a `OnceLock` rather than a settable state).** The first
failure explains the node; later ones are usually its consequence. KD-4 records
the cost.

**D-4 (2026-09-21, a watch with three states rather than a `JoinHandle`).**
Retaining the handle would be lifecycle management, which `008` D-2 declined and
which this spec does not reopen. The watch observes, and the closed-channel case
is what makes it cover a panic without joining anything.

**D-5 (2026-09-21, an abort-profile binary rather than an untested claim).** The
alternative was to state that these paths no longer panic and leave it to a
grep. Declined: the claim is precisely about a profile no test can use, so the
evidence had to come from outside the test harness. That the probe was observed
**aborting** against the restored defect is what makes it evidence rather than a
tautology.

**D-6 (2026-09-21, remove the watchdog rather than repair it).** OD-3 directed
that the behavior be preserved until it was established. It is established, and
F-014 says it cannot work under either profile. Keeping an unreachable safety net
is worse than not having one, because it reads as coverage.

**D-7 (2026-09-21, this block is `010`'s acceptance).** Eleven of `010`'s
thirty-six commands asserted the defective expressions this spec removes. Each is
replaced below and marked with what it was; the rest are carried forward.

**D-8 (2026-09-22, one gate, decided under its lock, and a shutdown that
stops nothing it cannot synchronize).** B-9's first form repaired the decision and
left where it ran. The alternatives were to keep the pre-lock check and add a
second check under the lock, or to take the lock around the raft shutdowns only.
Declined: the first keeps two decisions that can disagree, and the second was
already tried and reverted for F-107 on a false premise, and would still let
`post_membership` and `RemoveMembershipCache` bypass everything. A drain timeout
returns an error rather than stopping anyway, because stopping under a running
change is exactly the unsynchronized shutdown, and a caller that needs the
process gone can end it, which Raft already treats as a crash.

**D-9 (2026-09-25, B-7's "the rest of the API" misses three `hiqlite::tls`
changes; F-136).** B-7 says the API outside its four frozen items is whatever
`0.14.0` had plus this release's error variants. That is not so for
`hiqlite::tls`: `ServerTlsConfig::from_env` returns `Result<Option<Self>, Error>`
instead of `Option<Self>`, `build_tls_config` takes a second parameter
(`ca_path`), and `ServerTlsConfigCerts` has a new public field `ca`, which
breaks struct-literal construction. B-7's text is left as written; this entry
is the correction a reader needs, and `CHANGELOG.md` now lists the three under a
`0.15.0-patched` section. The release ledger and the consumer handoffs already
disclosed them.

## 7. Out of scope

- **A general panic audit of the crate.** KD-1.
- **Supervision or automatic restart.** W-06, and D-2 refuses it.
- **Releasing storage ownership from a failed node.** KD-3.
- **The IPv6 listen address and the two meanings of `node_id`.** `010` KD-1,
  KD-2.
- **Cluster-level failure behavior.** `012`.
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 027`. **This block is `010`'s acceptance as well as
this spec's** (D-7). `010`'s own file is not edited.

The two probe commands are a pair and must stay together: the build is what puts
the binary under `panic = "abort"`, and running a stale one would prove nothing.

```verify:cli
# Package names, not library names: the downstream release renamed the three packages
# (`031` B-2), and `-p` takes a package name. `use hiqlite::..` is unaffected.
# --- 010's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/start.rs
test -f hiqlite/src/init.rs
test -f hiqlite/src/app_state.rs
test -f hiqlite/src/split_brain_check.rs
sh -c 'spec-spine index owner hiqlite/src/start.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine index owner hiqlite/src/init.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine index owner hiqlite/src/app_state.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine index owner hiqlite/src/split_brain_check.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine registry relationships 010-node-lifecycle-and-split-brain | grep -q 009-configuration-contract'
cargo test -p hiqlite-patched --lib start::tests::listen_port_comes_from_the_advertised_address -- --exact
cargo test -p hiqlite-patched --lib start::tests::missing_advertised_port_falls_back_to_the_scheme_default -- --exact
cargo test -p hiqlite-patched --lib start::tests::ipv6_advertised_address_produces_an_unparsable_listen_address -- --exact
cargo test -p hiqlite-patched --lib init::tests::node_identity_is_resolved_by_id_here_and_by_position_in_start -- --exact
cargo test -p hiqlite-patched --lib init::tests::is_valid_accepts_a_nodes_list_whose_ids_are_not_positions -- --exact
cargo test -p hiqlite-patched --lib init::tests::get_this_node_panics_when_the_id_is_absent -- --exact
grep -q '.get(node_config.node_id as usize - 1)' hiqlite/src/start.rs
grep -q 'expect("this node to always exist in all nodes")' hiqlite/src/init.rs
grep -q 'let (tx_shutdown, rx_shutdown) = tokio::sync::watch::channel(false);' hiqlite/src/start.rs
# was two `with_graceful_shutdown` and two `axum_server::bind_rustls`, of which only the
# plaintext pair ever received a shutdown future. Both endpoints now go through one server
# helper that takes an already-bound listener and a receiver of its own.
sh -c 'grep -q "async fn serve_router" hiqlite/src/start.rs'
sh -c 'grep -q "axum_server::Handle::new()" hiqlite/src/start.rs'
sh -c 'grep -q "graceful_shutdown(Some(Duration::from_secs(10)))" hiqlite/src/start.rs'
sh -c '! grep -q "axum_server::bind_rustls" hiqlite/src/start.rs'
grep -q 'expect("The global Hiqlite shutdown handler to always listen")' hiqlite/src/client/mgmt.rs
# was `assert!(!handle.is_finished())`, the watchdog F-014 showed is unreachable under abort
# and silent under unwind
sh -c '! grep -q "assert!(!handle.is_finished())" hiqlite/src/split_brain_check.rs'
# was `expect("Cannot parse HQL_SPLIT_BRAIN_INTERVAL as u64")`. The string survives in the doc
# comment that records what it used to be, so this asserts the absence of the call rather than
# of the words.
sh -c '! grep -q "^ *\.expect(\"Cannot parse HQL_SPLIT_BRAIN_INTERVAL as u64\")" hiqlite/src/split_brain_check.rs'
sh -c 'grep -q "fn split_brain_interval_from" hiqlite/src/split_brain_check.rs'
grep -q 'HQL_SPLIT_BRAIN_INTERVAL' hiqlite.env
sh -c '! grep -q "split_brain" hiqlite.toml'
grep -q 'panic = "abort"' Cargo.toml
grep -q 'hiqlite/src/start.rs' spec-spine.toml
grep -q 'hiqlite/src/init.rs' spec-spine.toml
grep -q 'hiqlite/src/app_state.rs' spec-spine.toml
grep -q 'hiqlite/src/split_brain_check.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/010-node-lifecycle-and-split-brain'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/027-node-lifecycle-and-startup-errors'
# --- what this repair adds ---
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache split_brain_check::tests::a_malformed_split_brain_interval_is_a_startup_error -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache start::tests::a_listener_that_cannot_start_is_a_startup_error -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::the_first_failure_is_the_one_that_is_kept -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_failed_node_refuses_with_an_account_of_why -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_writer_thread_that_ends_without_a_reason_still_fails_the_node -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_reported_writer_failure_names_its_cause -- --exact
# F-101: a deliberate shutdown is not a failure, and a real one still survives it
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_deliberate_shutdown_is_not_a_failure -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_failure_recorded_before_the_shutdown_survives_it -- --exact
sh -c 'grep -q "state.lifecycle.begin_shutdown();" hiqlite/src/client/mgmt.rs'
sh -c 'grep -A3 "fn fail(&self" hiqlite/src/lifecycle.rs | grep -q "self.shutting_down.load"'
# B-9 / F-107: a node that is not a voter may not commit a membership change
cargo test -p hiqlite-patched --lib --no-default-features --features cache network::management::tests::a_node_leaving_its_own_cluster_may_not_commit_a_membership_change -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache network::management::tests::every_refusal_tells_the_caller_to_try_elsewhere -- --exact
sh -c 'grep -q "fn membership_change_allowed" hiqlite/src/network/management.rs'
sh -c 'grep -q "Some(_) if !this_node_is_voter" hiqlite/src/network/management.rs'
sh -c 'grep -q "membership_change_allowed(" hiqlite/src/network/management.rs'
# D-8: one gate, decided under its lock, and a bounded shutdown drain that stops nothing
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::a_decision_taken_before_the_lock_acts_on_a_membership_it_did_not_see -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::shutdown_waits_for_an_admitted_change_and_admits_nothing_after -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::a_shutdown_that_cannot_drain_stops_nothing_and_can_be_retried -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::a_cancelled_shutdown_wait_holds_nothing_and_keeps_admission_closed -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::admission_waits_for_a_bounded_time_across_both_raft_groups -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::a_refused_decision_holds_nothing -- --exact
sh -c '! grep -rnE "\.raft_lock|raft_lock:" hiqlite/src'
sh -c 'test "$(grep -c "_held: &crate::membership_gate::MembershipHeld" hiqlite/src/helpers.rs)" -eq 4'
sh -c 'grep -q "state.membership.drain(SHUTDOWN_DRAIN)" hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "state.membership.close();" hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "admit_membership_change(&state, &raft_type)" hiqlite/src/network/management.rs'
sh -c 'grep -q "admit_membership_change(" hiqlite/src/network/raft_server.rs'
# review of the candidate: results reach the caller, everything under the gate is bounded
sh -c 'test "$(grep -c "bounded_membership_op(" hiqlite/src/helpers.rs)" -eq 8'
sh -c 'grep -q "Ok(res) => res," hiqlite/src/client/shutdown_handle.rs'
sh -c 'grep -q "Ok(res) => res," hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "REMOTE_LEAVE_BOUND," hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "mark_stopped(&held, first_err.is_none())" hiqlite/src/client/mgmt.rs'
# B-10 / F-111 to F-113: the upgrade guard, the reader that no longer aborts
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::tests::a_legacy_cache_log_is_refused_and_left_untouched -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::tests::the_opt_in_moves_the_legacy_cache_aside_and_marks_the_new_one -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::tests::a_fresh_directory_is_marked_and_a_foreign_marker_is_refused -- --exact
cargo test -p hiqlite-wal-patched --lib reader::tests::a_requester_that_stops_reading_does_not_end_the_reader -- --exact
sh -c '! grep -q "ack.send(.*).unwrap()" hiqlite-wal/src/reader.rs'
# the guard runs before the restore and before either raft group, so a refusal opens nothing
sh -c 'a=$(grep -n "store::logs::ensure_cache_log_format" hiqlite/src/start.rs | head -1 | cut -d: -f1); b=$(grep -n "backup::restore_backup_start(&node_config)" hiqlite/src/start.rs | head -1 | cut -d: -f1); c=$(grep -n "store::start_raft_db(" hiqlite/src/start.rs | head -1 | cut -d: -f1); test -n "$a" && test -n "$b" && test -n "$c" && test "$a" -lt "$b" && test "$a" -lt "$c"'
sh -c '! grep -q "ensure_cache_log_format" hiqlite/src/store/mod.rs'
sh -c 'test "$(grep -c "store::logs::ensure_cache_log_format(&node_config.data_dir)" hiqlite/src/start.rs)" -eq 2'
sh -c 'test "$(grep -c "lifecycle.begin_shutdown();" hiqlite/src/store/mod.rs)" -ge 4'
sh -c '! grep -q "holds_files(&dir_snapshots" hiqlite/src/store/logs/mod.rs'
sh -c 'grep -q "if let Err(err) = init::init_pristine_node_1_db(" hiqlite/src/store/mod.rs'
sh -c 'grep -q "if let Err(err) = init::init_pristine_node_1_cache(" hiqlite/src/store/mod.rs'
# F-110: an out-of-service node refuses its embedded client
sh -c 'test "$(grep -c "self.ensure_node_available()?;" hiqlite/src/client/rate_limit.rs)" -eq 2'
sh -c 'test "$(grep -c "self.ensure_node_available()?;" hiqlite/src/client/query.rs)" -eq 7'
# the abort-profile half. The build is what puts it under `panic = "abort"`; these two are a
# pair and running a stale binary would prove nothing.
cargo build --release -p hiqlite-patched --features __abort-probe,s3 --bin hiqlite-abort-probe
./target/release/hiqlite-abort-probe
# the lifecycle is consulted before any raft metric, on both endpoints and in the client
sh -c 'grep -q "state.lifecycle.ensure_available()?" hiqlite/src/network/api.rs'
sh -c 'grep -q "pub fn node_failure" hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "pub fn writer_failure" hiqlite-wal/src/log_store.rs'
sh -c 'grep -q "NodeFailed" hiqlite/src/error.rs'
sh -c 'grep -q "Startup" hiqlite/src/error.rs'
```
