---
id: "033-n3-topology-qualification"
title: "Qualify a three-voter topology on Kubernetes before anyone calls it supported"
status: draft
created: "2026-09-23"
owner: "hiqlite maintainers"
risk: high
implementation: pending
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "012-cluster-integration-evidence"
  - "026-backup-and-restore-integrity"
  - "027-node-lifecycle-and-startup-errors"
  - "031-downstream-release-qualification"
origin:
  retroactive: false
establishes:
  - "standards/spec/n3-topology-proposal.md"
  # D-2: the real-node harness does not exist yet. It is a workspace-excluded crate so its
  # release builds and consumer feature sets never enter the library's own graph.
  - { kind: directory, path: "qualification/n3/", planned: true }
extends:
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
references:
  - unit: { kind: file, path: "hiqlite/src/init.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/client/mgmt.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/membership_gate.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/Cargo.toml" }
    role: "context"
  - unit: { kind: file, path: "standards/spec/consumer-handoff.md" }
    role: "context"
summary: >
  Proposes, for owner decision, how this fork would qualify a three-voter
  topology for the Statecraft cell on Kubernetes while N=1 stays supported and
  stays the Aicortex local profile. Owns the architecture and migration
  proposal, records six findings the assessment established from source
  (F-118 to F-123), and specifies the hiqlite-side repairs, the real-node
  harness and the deterministic acceptance that stand between today and any
  claim of N=3 support. Records the owner's decision D-14: at N=3 the cell is
  two StatefulSets, one hiqlite node per pod, and the single-container
  composition stays the N=1 profile. It changes no code, supports nothing, and
  claims no consumer territory.
---

# 033: Qualify a three-voter topology on Kubernetes before anyone calls it supported

## 1. Purpose

The supported topology of this fork is N=1 (`026` B-7, the consumer handoff
section 8). The owner's target for Statecraft is N=3 on Kubernetes, with new
production cells bootstrapped at N=3 and existing N=1 cells migrated through a
planned window into a fresh N=3 cell by coordinated backup and restore.

That target sits on three kinds of ground this corpus has not covered: what
hiqlite itself does when three voters live in separate pods (its initialization
decision, its shutdown, its restore), what evidence would show it, and which
parts of the result belong to openraft or to the consumers instead.
`standards/spec/n3-topology-proposal.md` works through all of it. This spec owns
that document, states the hiqlite obligations that follow from it as planned
behavior, and defines the acceptance that would discharge them.

**What this spec does not do.** It does not mark N=3 supported, and no change
under it may, before stage 10 of the proposal's section 9. It does not approve
itself or anything else. It edits no consumer. It does not repair the defects
it records: each repair in section 3 lands in a later change that carries its
own test and adds its own edges (D-1).

## 2. Territory

- **Establishes** `standards/spec/n3-topology-proposal.md`, a new document, and
  `qualification/n3/`, a planned directory for the real-node harness (B-2).
- **Extends** `005`'s findings register, additively, with F-118 to F-123.
- **References**, without claiming, the files the planned repairs would touch.
  Those are owned by `010`/`027` (`init.rs`, `client/mgmt.rs`,
  `membership_gate.rs`) and `031` (`hiqlite/Cargo.toml`). A change that
  implements B-3 or B-4 adds the `extends` edge on the unit it changes, in the
  same range, so the coupling gate binds it then (D-1).

**Boundaries.** OpenRaft owns elections, quorum, log matching, the
joint-consensus membership protocol and the meaning of its feature flags.
hiqlite owns how a node decides to initialize or join, the membership gate, the
shutdown sequence, persistence under a named `LogSync` mode, the state
machines, the per-cluster backup image and restore, readiness and transport.
Rahi, Rauthy and Statecraft own the cell composition, application consistency,
fencing, quiescence, the cell archive, and every Kubernetes object. Nothing in
section 3 claims a consumer-owned or openraft-owned guarantee; section 8 of the
proposal lists what is asked of each consumer, as proposals to their
repositories.

## 3. Behavior

Planned. Every statement below describes what a later implementing change
MUST do; none describes the tree at `72e09a6`.

### B-1. N=3 stays unsupported until its evidence exists

No document, release note, handoff or log message of this fork may describe
N=3 as supported until stages 0 to 9 of the proposal's section 9 have passed on
a named release, and the owner has recorded the decision. N=1 behavior MUST NOT
change as a side effect of any change under this spec, except where a repair
removes a hazard at N=1 as well (F-119 via `034`) and says so.

### B-2. A real-node harness, separate from the in-process suite

`qualification/n3/` MUST provide a harness that:

- starts each node as its **own process**, from a **release** build with the
  workspace's `panic = "abort"`, under each consumer's exact hiqlite feature set
  (Rahi: `sqlite, cache, counters, dlock, listen_notify_local, backup, s3`;
  Rauthy: defaults plus `cache, cast_ints, counters, dashboard,
  listen_notify_local, macros`), with `cache_storage_disk = true` and the
  `LogSync` mode the owner chose (proposal D-4);
- runs two layouts: **split**, one node per simulated pod, which is the N=3
  layout D-14 decided and the one every A-scenario must pass; and
  **co-located**, two clusters per simulated pod with one kill applying to a
  voter of each, which is the N=1 profile's shape and is run so a difference
  between the layouts is seen rather than assumed;
- routes every raft and API link through a harness-owned TCP proxy, so a
  partition is a deterministic proxy state, not a firewall rule or a timing
  accident;
- delivers `SIGTERM` and `SIGKILL` itself and records each node's shutdown
  duration;
- synchronizes on observed state (metrics, membership log id, applied index)
  with a **fixed bound per wait**, and never on a fixed sleep;
- runs each scenario a fixed number of times, stops at the first failure, and
  keeps the logs of the failing run.

It MUST NOT be part of `cargo test` for the library, and it MUST NOT run
open-ended: every run has a bound on its duration and its repetitions.

### B-3. Node 1 initializes only on positive evidence that the cluster is fresh

Repairs F-118. A pristine node 1 in a configuration with peers MUST call
`Raft::initialize` only when at least `⌊N/2⌋` distinct peers have **answered**
that their own group is not initialized, so that together with itself a majority
has positively said so. A connection error, a timeout or an unauthenticated
answer is not such an answer. While that evidence is missing, node 1 waits,
bounded by a configured limit, reports not-ready, and on expiry returns a
startup error naming the peers it could not hear from. It never initializes by
default.

At N=1 the decision is unchanged. The cache group takes the same decision.

### B-4. The pre-shutdown delay is a node option

The fixed 9.5 s delay before a multi-node shutdown (`client/mgmt.rs:386`) MUST
become a `NodeConfig` option, readable from code, TOML and the environment,
**defaulting to 9.5 s** so no current caller changes behavior. The option MUST be
recorded in `009`'s configuration contract by the implementing change.

### B-5. The log-revert relaxation is a recorded decision

The implementing change for F-121 MUST record the owner's decision (proposal
D-13) and either remove `openraft/loosen-follower-log-revert` from the `cache`
feature, with the in-memory cache's rejoin shown correct without it, or keep it
and state in the handoff that N=3 durable groups require `LogSync::Immediate`
(proposal D-4).

### B-6. The acceptance is deterministic and named by scenario

The harness MUST implement the scenarios below. Each passes only if it passes
its full run count consecutively with no retry, under the configuration B-2
names.

- **A-1. Bootstrap.** Three pristine nodes per cluster, started in every order
  including simultaneously; four groups reach voters `{1,2,3}` within the bound.
  Negative: node 1 pristine, node 2's link held down, node 3 in an existing
  cluster; node 1 MUST NOT initialize (B-3).
- **A-2. Immediate restart.** Each node in turn stopped and restarted on its
  data, and written to through each group the moment `start_node` returns, with
  no sleep. F-054: pass means no log-id mismatch and no failed write.
- **A-3. Leader loss.** `SIGKILL` of each group's leader under a write load;
  writes resume within the bound; every write acknowledged before the kill is
  present after it. Under `ImmediateAsync` the result is reported, and a loss is
  recorded against F-121, not called a pass.
- **A-4. Partition and rejoin.** The leader isolated by the proxy: the minority
  refuses writes within the bound rather than hanging; the majority elects and
  serves; after healing, every node converges to the same membership log id and
  the same content digest.
- **A-5. Lagging rejoin.** A follower partitioned past the snapshot threshold
  rejoins by snapshot installation, and converges.
- **A-6. Permanent replacement, same id.** A follower's data directory emptied
  and the node restarted: it leaves, rejoins and converges. Repeated for node 1,
  with and without its first peer reachable (F-118).
- **A-7. Membership and shutdown interleavings on real nodes.** `SIGTERM` to the
  leader while a learner is being added; to a follower during its promotion; to
  two nodes at once; and a leave request racing a shutdown. Pass: no stop under a
  running change, every refusal is a `409`, and the group converges. This is
  `027` KD-7, and in the release build `027` KD-9.
- **A-8. Rolling restart under load.** Each node restarted in turn, one at a
  time, while writes continue: no acknowledged write lost, membership unchanged
  throughout.
- **A-9. Shutdown budget.** Every `SIGTERM` in A-1 to A-8 records the
  end-to-end duration of the shutdown of each simulated pod (one hiqlite node
  in the split layout, two in the co-located one); the maximum and the
  distribution are reported. This is the measurement the
  consumers' graces are set from, not a threshold chosen here.
- **A-10. Full stop and start.** All nodes stopped, then all started, which is
  the supported upgrade shape (proposal D-11): the cluster reforms with its
  membership and data.
- **A-11. Debug-build pass.** A-1 to A-8 repeated in a debug build, so openraft's
  `debug_assert!`s can fire. Reported separately; a debug-only failure is a
  finding, not a pass of the release run.
- **A-12. Restore.** Delegated to `034` section 3; not passed by this spec.

## 4. Evidence and its limits

**Nothing in this spec has been executed.** The proposal and the six findings
were read from source at `72e09a6`, with the pinned `spec-spine` revision
confirmed before any gate was trusted.

What already exists, and what it is worth for N=3 (F-122): the in-process
cluster suite starts three nodes in one debug process, TLS off, in-memory
cache, with sleeps. It is evidence that the join, restart, volume-loss and
N=3 restore paths **run** in that configuration. It is not evidence about
release builds, separate processes, a disk-backed cache at N=3, a real
partition, `SIGKILL`, or any consumer's configuration. `031`'s external
consumer checks ran at N=1. `027`'s gate tests drive the gate with a stand-in
for the raft.

What B-6 would add, when implemented, and what it still would not: one host,
so no kernel crash and no real disk loss, which is where `ImmediateAsync` and
F-121 matter most; run counts bound an observed failure rate and do not prove
absence (F-107 appeared about once in seven runs); and nothing on Kubernetes,
which is stage 8 and consumer territory.

## 5. Known defects

Recorded, left unfixed here.

**KD-1. F-118.** A pristine node 1 treats an unreachable first peer as proof of
a fresh cluster. B-3 is the proposed repair.

**KD-2. F-119 and F-120.** A restore instruction re-applies on every start, and
a node-1 restore that skipped initialization waits forever before binding a
listener. `034` owns the repair.

**KD-3. F-121.** The `cache` feature relaxes openraft's follower-revert check
for the durable group too. B-5 records the decision it needs.

**KD-4. The shutdown budget does not fit the cell's graces at N>1.** Section 4
of the proposal. D-4 removes the double shutdown at N=3; B-4 and the consumers'
graces remain the remedy for the single shutdown that is left, and A-9 is the
measurement.

**KD-5. Every N=3 cluster is formed by membership change.** There is no path to
initialize three voters at once. Not proposed for change: the join path is the
one every other lifecycle event also uses, and qualifying it once is cheaper
than qualifying two. Recorded so nobody assumes "bootstrap at N=3" avoids
`027`'s territory.

**KD-6. F-123.** The release ledger contradicts itself about the cluster suite.
`031`'s to resolve.

## 6. Resolved decisions

**D-1 (2026-09-23, territory is declared by the change that moves it).** This
spec references the source files its planned repairs would touch instead of
extending them now. An `extends` edge on a unit is a claim that this spec
governs changes to it; declaring it before any change exists would make this
draft an owning spec of `init.rs` and `client/mgmt.rs` for every unrelated
range. The change that implements B-3, B-4 or B-5 adds the edge in the same
range.

**D-2 (2026-09-23, the harness is a separate, planned directory).**
`qualification/n3/` is claimed `planned: true` because it does not exist. It is
kept out of the workspace so that release builds under two consumer feature
sets never enter the library's graph or `just test`.

**D-3 (2026-09-23, one proposal document, two specs).** The architecture
comparison, migration protocol, owner decisions and qualification plan are one
document because they are one argument. The restore coordination is a separate
spec, `034`, because it changes `026`'s territory and carries its own
acceptance; the rest of the N=3 work does not.

**D-4 (2026-09-23, owner decision: the N=3 cell is two StatefulSets).** The
owner decided the proposal's D-14. At N=3 the cell is one namespace with a
`rahi` and a `rauthy` StatefulSet, each pod running one hiqlite node of one
cluster; the single-container, supervised composition stays the N=1 local
profile. The proposal's section 12 codifies what replaces the single-container
guarantees: routing to Rauthy through an internal Service over an encrypted,
NetworkPolicy-restricted path; liveness independent of Rauthy and readiness
that fails without it, with peer discovery kept independent of readiness; and
coherent exports by stopping both StatefulSets before exporting both volumes.
Those are obligations for new Rahi and Statecraft specs, which amend Rahi 031
and 032 and Statecraft 002 for N=3; nothing in this repository enforces them.
For this spec the decision changes B-2 (both layouts, split as the one that
must pass) and A-9 (one shutdown per pod), and removes KD-4's cause at N=3.

**Owner decisions pending.** D-1 to D-13 of the proposal's section 11 are not
decided by this spec. Each will be dated here when the owner records it.

## 7. Out of scope

- Live N=1 to N=3 expansion (proposal D-1).
- Replacement under a new node id (proposal D-2).
- Leader transfer, which openraft 0.9 does not offer.
- Any Kubernetes manifest, which belongs to the deploying repository.
- Consumer acceptance: Rahi, Rauthy, Statecraft and Aicortex qualify their own
  composition; a hiqlite stage never stands in for theirs.

## Verification

```verify:cli
test -f standards/spec/n3-topology-proposal.md
grep -q 'N=3 is not a' standards/spec/n3-topology-proposal.md
grep -q '033-n3-topology-qualification' standards/spec/n3-topology-proposal.md
grep -q '^### F-118 ' standards/spec/findings-register.md
grep -q '^### F-123 ' standards/spec/findings-register.md
grep -q '^## 12. D-14: the N=3 cell is two StatefulSets' standards/spec/n3-topology-proposal.md
```
