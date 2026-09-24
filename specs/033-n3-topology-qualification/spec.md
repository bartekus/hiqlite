---
id: "033-n3-topology-qualification"
title: "Qualify a three-voter topology on Kubernetes before anyone calls it supported"
status: draft
created: "2026-09-23"
owner: "hiqlite maintainers"
risk: high
implementation: in-progress
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
  # D-5: the reconciliation handoff to Rahi, a repository-backed record of decision status and
  # consumer obligations. It asserts nothing about Rahi's repository.
  - "standards/spec/n3-rahi-reconciliation-handoff.md"
  # D-6: the request to Rauthy's maintainer, with what each item blocks. It sends nothing
  # and asserts nothing about Rauthy's repository beyond what it cites.
  - "standards/spec/n3-rauthy-request.md"
  # D-2: the real-node harness, built by lane D (2026-09-23). A workspace of its own, so its
  # release builds and consumer feature sets never enter the library's graph.
  - { kind: directory, path: "qualification/n3/" }
extends:
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  # D-1, D-8: added by the lane B change that implements B-3 (the positive-evidence decision
  # for both raft groups) and B-4 (the pre-shutdown option), in the range that changes them.
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/init.rs" }
    nature: superseding
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/start.rs" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/app_state.rs" }
    nature: additive
  # D-8: the peer's explicit "not initialized" answer, served by `get_membership`.
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/network/" }
    nature: additive
  # D-9: `client/mgmt.rs` and `client/shutdown_handle.rs` are `003`'s, not `010`'s.
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/client/" }
    nature: additive
  # D-9: the two options are recorded in `009`'s configuration contract.
  - spec: "001-wal-durability-and-completion"
    unit: { kind: file, path: "hiqlite/src/config.rs" }
    nature: additive
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite/src/config_toml.rs" }
    nature: additive
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite.toml" }
    nature: additive
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite.env" }
    nature: additive
  # D-9: the server binary's generated configuration documents the same two keys, so F-071's
  # pinned drift between it and `hiqlite.toml` does not widen.
  - spec: "015-server-binary-and-proxy"
    unit: { kind: file, path: "hiqlite/src/server/config.rs" }
    nature: additive
references:
  - unit: { kind: file, path: "hiqlite/src/membership_gate.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/store/mod.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/error.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/Cargo.toml" }
    role: "context"
  - unit: { kind: file, path: "standards/spec/consumer-handoff.md" }
    role: "context"
summary: >
  Proposes, for owner decision, how this fork would qualify a three-voter
  topology for the Statecraft cell on Kubernetes while N=1 stays supported and
  stays the Aicortex local profile. Owns the architecture and migration
  proposal, the reconciliation handoff to Rahi and the request to Rauthy's
  maintainer, records nine findings (F-118 to F-125 from source, and F-132, a
  barrier's limit, by a bounded probe), and specifies the hiqlite-side repairs, the real-node
  harness and the deterministic acceptance that stand between today and any
  claim of N=3 support. Records the owner's decision D-14: at N=3 the cell is
  two StatefulSets, one hiqlite node per pod, and the single-container
  composition stays the N=1 profile. It changes no code, supports nothing, and
  claims no consumer territory. Lane B (2026-09-23) implements B-3 and B-4
  on a branch, each with a test observed failing without it; nothing of it is
  merged, released or run in a harness.
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

- **Establishes** `standards/spec/n3-topology-proposal.md`,
  `standards/spec/n3-rahi-reconciliation-handoff.md` and
  `standards/spec/n3-rauthy-request.md`, new documents, and
  `qualification/n3/`, the real-node harness (B-2), built by lane D.
- **Extends** `005`'s findings register, additively, with F-118 to F-125 and
  F-132. The N=1 upgrade-exclusion findings (F-126 to F-131, F-133) are `035`'s.
- **References**, without claiming, the files the planned repairs would touch.
  Those are owned by `010`/`027` (`init.rs`, `client/mgmt.rs`,
  `membership_gate.rs`) and `031` (`hiqlite/Cargo.toml`). A change that
  implements B-3 or B-4 adds the `extends` edge on the unit it changes, in the
  same range, so the coupling gate binds it then (D-1).
- **Extends, since lane B** (D-8, D-9): `010`'s `init.rs` (superseding: the
  node-1 decision is replaced), `start.rs` and `app_state.rs`; `003`'s
  `hiqlite/src/network/` (the explicit answer) and `hiqlite/src/client/` (the
  shutdown option and its outcome); `001`'s `config.rs` and `009`'s
  `config_toml.rs`, `hiqlite.toml` and `hiqlite.env`, which is how the two new
  options are recorded in `009`'s configuration contract; `015`'s
  `server/config.rs`, whose generated file lists the same keys. `store/mod.rs` and
  `error.rs`, which no spec owns, are referenced.

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
- **A-9. Shutdown budget.** Every `SIGTERM` in A-1 to A-8 records, per simulated
  pod (one hiqlite node in the split layout, two in the co-located one), the
  end-to-end duration and the **outcome**: confirmed graceful completion
  (`Ok(())`), unconfirmed completion (`Err(Timeout)`, the sequence possibly still
  running), or confirmed forced exit before completion. Reported separately as
  M-N1 (co-located, N=1 profile) and M-S3 (split N=3), with the distribution
  and the outcome counts. Only confirmed completions count as within budget; an
  unconfirmed completion fails graceful-within-budget acceptance. This is the
  measurement the consumers' graces are set from, not a threshold chosen here.
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

**The harness exists; nothing it ran is qualification** (lane D, 2026-09-23).
`qualification/n3/` implements B-2's mechanisms (own process per node, release
build with `panic = "abort"`, both feature sets, split and co-located layouts, a
harness-owned proxy, its own `SIGTERM`/`SIGKILL` with recorded outcomes, bounded
waits on observed state, stop at the first failure). Its one scenario is
`smoke`; A-1 to A-10 are registered and refuse to run. Three smoke runs were
allowed and used, on macOS arm64 with the Rahi feature set and
`LogSync::Immediate`: split and co-located each passed (four groups formed with
voters `{1,2,3}`, writes acknowledged and read back, full stops confirmed `Ok`
on six nodes, restart with the membership log id unchanged), and a run with an
injected `SIGKILL` failed as it must and kept its directory. What this does not
show: any A-scenario, any Linux run, the Rauthy node build under load, a
partition applied through the proxy (unit-tested only), or anything about N=3
support.

What already exists, and what it is worth for N=3 (F-122): the in-process
cluster suite starts three nodes in one debug process, TLS off, in-memory
cache, with sleeps. It is evidence that the join, restart, volume-loss and
N=3 restore paths **run** in that configuration. It is not evidence about
release builds, separate processes, a disk-backed cache at N=3, a real
partition, `SIGKILL`, or any consumer's configuration. `031`'s external
consumer checks ran at N=1. `027`'s gate tests drive the gate with a stand-in
for the raft.

**Lane B evidence (2026-09-23), unit level only.** B-3 and B-4 are
implemented on the lane B branch. Each repair's test was first run against the
unrepaired tree and observed failing: `init::tests::f118_unreachable_peers_are_not_evidence_of_a_fresh_cluster`,
`..._an_unauthenticated_answer_is_not_evidence` and
`..._a_stopped_peer_answer_is_not_evidence` each returned "initialize"; and
`config_toml::tests::the_lane_b_options_are_accepted_toml_keys` was refused
as an unknown key. The peers in those tests are stub HTTP/2 listeners on
ephemeral ports serving the production route shape, not hiqlite nodes, so they
show the decision's reading of each answer and its bound, not that a real peer
gives those answers at the right moments; the server half is exercised only by
the in-process cluster suite, whose every N=3 bootstrap now depends on it. No
harness scenario (A-1, A-6) has run. After review (D-10),
`init::tests::f118_a_black_holed_peer_does_not_hold_the_decision` took 20 s on
the first lane B commit, where the peers were asked one after another, and
passes within the 5 s cap now.

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

**KD-7. F-124 and F-125.** A stopped data directory records neither its
committed log id nor, after an unclean stop, an accurate applied log id. What an
offline export can prove is therefore conditional; the proposal's section 13
states the conditions and the alternatives, and `034` B-2 was rewritten.

## 6. Resolved decisions

The D-numbers below are this spec's own. A decision of the proposal's section 11
is always cited as "the proposal's D-n"; the two numberings are unrelated.

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

**D-5 (2026-09-23, reconciliation with Rahi decision packet 1).** A
governance-only pass against Rahi's packet (D1 to D5, Rahi `b815b18`) and a
source read of Rauthy `ccf2250`. It separated the five controls the migration
had conflated (proposal 7.2), defined activation by authoritative mutations
(7.3) and the tombstone as a proposed downstream feature, replaced the export's
committed-index comparison with a proposed rule, its safety argument and two
alternatives (section 13, F-124, F-125), split cache-replacement security from
stale-backup security (section 14), stated three shutdown outcomes (section 4),
and separated Track N1 from Track S7 (section 15). It added the handoff document
this spec now establishes. It took **no** decision: D-8a to D-8e, D-15 and D-16
were added as pending proposals, and D-4 above (the proposal's D-14) is
unchanged.

**D-6 (2026-09-23, second reconciliation pass, against Rahi 043 at
`5707f60`).** Governance, bounded probes and handoffs only; no runtime change and
no decision. The proposal's 13.7 barrier claim was overstated and is narrowed: a
barrier proves an export faithful to the cluster at barrier time, not that
nothing acknowledged was lost before it, which a probe on real 0.15 builds
showed with a SQL row standing in for the barrier (F-132); 13.5 is restated
against the fields a stopped directory holds, 13.10 defines the clean-stop
marker and 13.11 separates two barriers from a cross-store transaction. Section
14.3's controls became five separate decisions, and signing-key rotation is no
longer offered as immediate invalidation. Section 11 is now the one decision
packet (adding D-17 and D-18 for `035`); section 16 is the dependency graph and
the lane authorizations; section 9 separates candidate integration from
adoption and support. The handoff to Rahi was rewritten against 043, and the
request to Rauthy's maintainer added. The N=1 upgrade hazard Rahi reported is
`035`'s, a separate producer item that waits for none of this.

**D-7 (2026-09-23, third reconciliation pass).** Against Rahi 043 revision 3
(`c2c7c72`, `abd66fd`, `fc3f339`), its producer requests version 2, and Rauthy
`d7d087aa` (unpublished). The proposal's section 11 restates D-8b for Rahi's
permanent floor and lifetime ceiling, changes D-17's recommendation to "when a
consumer asks" (Rahi no longer holds hiqlite's locks), makes D-18's label
conditional on its availability at publication, and adds D-19 with section 17's
evaluation of a producer-side downgrade fence; 13.7 (b) is corrected and 13.13
states what recurring barriers prove; section 16 records A1 as an unreleased
candidate, prepares lane B and the lane C contracts, and counts the launches
inside each qualification run, restore scenarios included. The Rahi handoff is
rewritten with the answer to Rahi's H-7, and the Rauthy request gains a producer
notice. Nothing in this pass decides anything or supports N=3.

**D-8 (2026-09-23, lane B: how B-3 is implemented).** Owner authorization of
lane B, 2026-09-23. B-3 is imprecise on four points, decided here:

- *What "answered not initialized" is.* An authenticated `200` from `GET
  /cluster/membership/{raft_type}` whose body decodes to an **empty**
  membership. Before this change no peer could give that answer: a peer
  returned one `400 Config("Raft node has not been initialized")` both when its
  group was pristine and when it was initialized but its raft was still stopped
  (a start before `set_raft_running`, or a shutdown), so the only explicit
  signal was ambiguous and counting it would let an initialized peer vote
  "fresh". `get_membership` now answers `200` with an empty membership when, and
  only when, the group is not initialized, and a different `400` when it is
  initialized and stopped. That is why the change extends `003`'s
  `hiqlite/src/network/`, which the proposal's section 16 table did not list.
- *Who counts.* Only distinct peers other than node 1; node 1 is never seeded.
  The peers are asked in full rounds, and an initialized answer from any of them
  ends the decision as "join" before the count is looked at, so an initialized
  peer that answers is never outvoted by fresh ones in the same round.
- *The bound.* `NodeConfig::init_peer_wait_secs`, code, TOML
  (`init_peer_wait_secs`) and environment (`HQL_INIT_PEER_WAIT_SECS`), default
  120 s, which is also the per-request bound's ceiling. The node holds no
  listener while it waits (the decision runs inside `store::start_raft_db`,
  before the listeners are bound), so "reports not-ready" is met by `/ready`
  being unreachable, and `/health` is unreachable too: a liveness probe shorter
  than the bound restarts node 1 before it can report the error. Recorded, not
  changed: moving the decision after the listeners is a larger reordering than
  B-3 asks for.
- *What the rule costs.* A new cluster can only bootstrap when its peers run
  alongside node 1; with a StatefulSet's `OrderedReady` policy node 1 waits for
  peers that are not started until it is ready, and fails at the bound. And the
  answer changed shape: an unrepaired node 1 panics ("initialized but has no
  configured members") on a repaired pristine peer's `200`, and a repaired node 1
  never counts an unrepaired pristine peer's ambiguous `400`, so a fresh
  bootstrap needs every node on a repaired build. *Corrected by D-10:* the
  first sentence above also holds for a full restart of an **existing** cluster
  whose cache group is in memory (`cache_storage_disk = false`), whose cache
  group is pristine on every start.

The rule is B-3's and is not changed here. Its limit, stated rather than
repaired: at N=3 one pristine peer is enough, so a cell that lost two of its
three volumes forms a new cluster beside the survivor; the survivor alone holds
no majority of the old membership and cannot accept writes.

**D-9 (2026-09-23, lane B: how B-4 is implemented).** The option is
`NodeConfig::pre_shutdown_delay_ms` (TOML `pre_shutdown_delay_ms`, environment
`HQL_PRE_SHUTDOWN_DELAY_MS`), default 9500, and is still skipped when the node
is the only member. B-4 does not say what happens to the caller's 15 s wait
(`SHUTDOWN_WAIT`) when the delay is changed; decided: it becomes 15 s plus
however much the delay exceeds 9.5 s, and never less than 15 s, so the default
keeps the bound Rahi 043 B-9 composed its grace from, and a longer delay does
not take time from the stops. The three outcomes of the proposal's section 4 are
now distinguishable in what shutdown returns: `Ok(())` is confirmed completion;
an error for which the new `Error::is_shutdown_unconfirmed()` is `true` is an
unconfirmed completion; any other error is a shutdown that returned without
stopping everything, including a membership drain that timed out and stopped
nothing, which is also an `Error::Timeout` and was indistinguishable before. The
method is an addition to the public surface `027` B-7 describes. *Corrected by
D-10:* it is not the only one. Corrected here, not in the proposal: `client/mgmt.rs` is owned by `003`'s
`hiqlite/src/client/` directory, not by `010` as section 16's table says. The
two options are recorded in `009`'s contract through the reference files it
establishes, `hiqlite.toml` and `hiqlite.env`; `009`'s own text is not edited.
The server binary's generated configuration (`015`'s
`hiqlite/src/server/config.rs`) lists both keys as well, so the
drift `011`, `029` and `030` pin between it and `hiqlite.toml` stays as it was.

**D-10 (2026-09-23, lane B: an independent review's corrections to D-8 and D-9).**

- *Bootstrap under `OrderedReady`, and the in-memory cache.* The node-1 decision
  runs before the listeners are bound, so while node 1 waits it serves neither
  `/ready` nor `/health`. A StatefulSet with the default `OrderedReady` policy
  creates pods 1 and 2 only once pod 0 is ready, so node 1 waits out
  `init_peer_wait_secs`, fails and restarts, for ever: a new cluster never forms.
  D-8 said that cannot happen to an existing cluster; it can, when the cache group
  is in memory, because that group is pristine after every start, so a full
  restart of such a cluster under `OrderedReady` crashloops node 1 in the cache
  decision. Before lane B, `/ready` answered ready for a pristine node 1 after ten
  seconds, and at N=3 node 1 had already initialized itself on no evidence
  (F-118), so neither showed. **`podManagementPolicy: Parallel` is required**,
  and is now said where the option is documented and in the README's StatefulSet
  example. The review's preferred repair, taking the decision after the listeners
  and answering ready meanwhile, is **not** made: B-3 says node 1 "reports
  not-ready" while its evidence is missing, and answering ready is the opposite.
  Which of the two B-3 should say is the owner's decision, reported with lane B
  and not taken here.
- *A second cache cluster at N=3 with an in-memory cache.* A peer's in-memory
  cache group is pristine after every restart, so when node 1 and node 2 restart
  together and node 3 is silent, node 2's "not initialized" is true and is enough
  evidence at N=3: node 1 forms a new cache cluster while node 3 still holds the
  old one. The old one cannot commit on its own, having one voter of three, but
  its votes and the new cluster's may mix (inferred from openraft's protocol, not
  observed), which is the in-memory hazard
  `NodeConfig::cache_storage_disk` already warns of. The rule cannot tell this
  apart: the peer does not know it was a member, because nothing of its cache
  group survives its restart. Not repaired; recorded here and in the F-118
  register note. The proposal's D-3 (`cache_storage_disk = true` at N=3) removes
  the case.
- *The public surface.* Besides `Error::is_shutdown_unconfirmed()`, `NodeConfig`
  gains two public fields, `pre_shutdown_delay_ms` and `init_peer_wait_secs`.
  `NodeConfig` is not `#[non_exhaustive]`, so a consumer that builds it with a
  struct literal and no `..Default::default()` no longer compiles; one built from
  `Default`, `from_env` or `from_toml` is unaffected.
- *The HTTP API.* `GET /cluster/membership/{raft_type}` answers `200` with an
  empty membership for a group that is not initialized, where it answered `400`;
  for an initialized group that is not running it answers a `400` with new text.
  The route is internal to the cluster, but its answer changed.
- *The peers are asked concurrently.* Each round asks every peer still without an
  explicit answer at once, each request capped at 5 s and at what is left of the
  wait, so a peer that never answers no longer takes the whole wait from the
  peers after it.

**Owner decisions pending.** D-1 to D-13, D-8a to D-8e and D-15 to D-19 of the
proposal's section 11 are not decided by this spec. Each will be dated here
when the owner records it.

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
grep -q '^### F-125 ' standards/spec/findings-register.md
grep -q '^## 13. Export currency' standards/spec/n3-topology-proposal.md
grep -q '^## 14. Security state across migration, restore and upgrade' standards/spec/n3-topology-proposal.md
grep -q '^\*\*7.2 Five controls, one authority' standards/spec/n3-topology-proposal.md
test -f standards/spec/n3-rahi-reconciliation-handoff.md
grep -q 'Decision status' standards/spec/n3-rahi-reconciliation-handoff.md
test -f standards/spec/n3-rauthy-request.md
grep -q '^## 11. Owner decisions: the one packet' standards/spec/n3-topology-proposal.md
grep -q '^## 16. Dependency graph, critical path and lane authorizations' standards/spec/n3-topology-proposal.md
grep -q '^### F-132 ' standards/spec/findings-register.md
grep -q '^\*\*13.13 What recurring external barriers prove' standards/spec/n3-topology-proposal.md
grep -q '^## 17. A producer-side downgrade fence' standards/spec/n3-topology-proposal.md
grep -q '^## 3. H-7: an adversarial reading' standards/spec/n3-rahi-reconciliation-handoff.md
grep -q '^## Third pass (2026-09-23): producer notice' standards/spec/n3-rauthy-request.md
sh -c '! grep -rl "$(printf "\342\200\224")" specs/033-n3-topology-qualification standards/spec/n3-topology-proposal.md standards/spec/n3-rahi-reconciliation-handoff.md standards/spec/n3-rauthy-request.md'
sh -c 'spec-spine index owner hiqlite/src/init.rs | grep -q 033-n3-topology-qualification'
grep -q 'pub(crate) async fn peer_init_evidence' hiqlite/src/init.rs
sh -c '! grep -q "let mut skip_nodes = vec!\[1\];" hiqlite/src/init.rs'
grep -q 'return fmt_ok(headers, Membership::<NodeId, Node>::default());' hiqlite/src/network/management.rs
grep -q 'HQL_PRE_SHUTDOWN_DELAY_MS' hiqlite.env
grep -q 'HQL_INIT_PEER_WAIT_SECS' hiqlite.env
grep -q 'pre_shutdown_delay_ms = 9500' hiqlite.toml
grep -q 'init_peer_wait_secs = 120' hiqlite.toml
grep -q 'podManagementPolicy: Parallel' README.md
grep -q 'const PEER_REQUEST_CAP' hiqlite/src/init.rs
sh -c '! grep -q "Duration::from_millis(9500)" hiqlite/src/client/mgmt.rs'
cargo test -p hiqlite-patched --lib init::tests::f118
cargo test -p hiqlite-patched --lib --no-default-features --features cache init::tests::f118
cargo test -p hiqlite-patched --lib config_toml::tests::the_lane_b_options
cargo test -p hiqlite-patched --lib client::mgmt::tests
```
