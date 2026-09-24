# N=3 topology on Kubernetes: architecture qualification and migration proposal

A proposal, owned by `specs/033-n3-topology-qualification/spec.md` and written
for owner decision. It changes no behavior and supports nothing. **N=3 is not a
supported topology of this fork**, and nothing here makes it one: that follows
only from the evidence section 9 plans, recorded by a later change. N=1 stays
supported unchanged, and stays the local profile Aicortex runs.

**Decided by the owner on 2026-09-23: D-14.** At N=3 the cell is two
StatefulSets, one per application, each with one hiqlite node per pod; the
single-container composition stays the N=1 local profile. Section 12 states the
replacements for what the split removes. Sections 1 to 5 describe the
composition as it is deployed today, and say where D-14 changes the
consequence. **Every other decision is pending**: D-1 to D-13, the D-8
subdivisions D-8a to D-8e, and the proposed D-15 to D-18. Section 11 is the one
decision packet.

**Revised 2026-09-23, second reconciliation pass** (against Rahi 043 at
`5707f60`): section 13's barrier and clean-stop claims are corrected and
narrowed (F-132); section 14's security controls are separate decisions;
section 11 consolidates every pending decision; section 16 is the dependency
graph and the lane authorizations. The N=1 upgrade hazard Rahi reported is a
separate producer item with its own repair contract,
`specs/035-n1-upgrade-exclusion/spec.md`, and does not wait for anything here.

Established against: this repository at `72e09a6` (`spec-spine`, clean);
`openraft =0.9.25` from the committed `Cargo.lock`; `spec-spine` built from the
pinned revision `aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f`, reporting `0.20.0`,
with `check` and `lint` exiting `0` on that tree. The globally installed
binary on the authoring machine reported `0.22.0` and was **not** used for any
gate result here. Consumer trees were read, not edited: Rahi `cf66e31`
(`corpus/040-binding-schema`, 47 dirty paths, so a statement about Rahi is about
that working tree), Statecraft `10d830c` (2 dirty), Aicortex `a6118b8`
(6 dirty), Rauthy `rauthy-hq015-worktree` `ccf2250` (the only tree that consumes
`hiqlite-patched`).

Nothing here was executed on a Kubernetes cluster, and no test was run to
produce it. Every claim is read from source or from a cited document, and says
so where the difference matters.

---

## 1. What is actually deployed, and what that means for this proposal

**The Statecraft cell does not run this fork.** A cell is one pod, one
container, supervised by Rahi (Rahi 031; `statecraft/deploy/k8s/statefulset.yaml`):

| hiqlite node | process | API / raft ports | data | hiqlite version | groups |
|---|---|---|---|---|---|
| Rahi app node | `rahi serve` | 8300 / 8400 | `/data/hiqlite` | upstream `0.14` at Git rev `8f3b9bde` (`rahi/Cargo.toml:30,146-147`) | SQLite, cache |
| Rauthy node | Rauthy child of `rahi supervise` | 8200 / 8100 | `/data/rauthy` | upstream `0.14.x`, from the upstream `ghcr.io/sebadob/rauthy:0.36.2` image (`rahi/docker/Dockerfile:12`) | SQLite, cache |

So a cell runs **two independent hiqlite clusters and four raft groups** per
replica, and **none of them is `hiqlite-patched`**. Statecraft and Aicortex
reach hiqlite `0.14.0` through Rahi's crates pinned at `=0.1.0` from the registry. Only
`rauthy-hq015-worktree` builds against `hiqlite-patched =0.15.0-patched.1`,
published as `ghcr.io/bartekus/rauthy-patched:0.36.2-patched.2`.

Consequence: every repair this fork has made (the WAL, snapshot, restore,
startup and membership-gate work of `008` to `032`) is absent from the cell
today, at N=1 and at N=3 alike. **No hiqlite N=3 qualification applies to the
cell until both halves of it run a qualified `hiqlite-patched`.** That is the
first downstream obligation, and it is Rahi's and the Rauthy image's, not this
repository's. It comes in two tracks that must not be confused (section 15):
**Track N1**, adopting the already published patched builds at N=1 now, which
does not depend on this work; and **Track S7**, adopting a release that passed
this proposal's N=3 stages, which does.

Consumer configuration that matters below (read from source; Rahi and Rauthy
agree on each): `cache_storage_disk = true` (the default); `LogSync::ImmediateAsync`
(never set, so the default); no TLS on either hiqlite endpoint; `secret_raft`
and `secret_api` from Secrets; the same `node_id` for both clusters in a pod,
from the pod ordinal plus one; peers from a ConfigMap and stable pod DNS;
`podManagementPolicy: Parallel`; `terminationGracePeriodSeconds: 30`; no
`preStop`; no PodDisruptionBudget or NetworkPolicy in Rahi or Statecraft.
Rahi's `deploy/n3` overlay (three replicas, required hostname anti-affinity,
three peers per cluster) exists and, by Rahi 032's own status note, has never
run as a real three-pod cluster.

## 2. Ownership boundaries for this topology

Constitution VII and VIII, applied to what N=3 adds.

**OpenRaft owns consensus**: elections, terms, quorum, log matching, the
joint-consensus membership protocol, snapshot transfer. It also owns the
meaning of `loosen-follower-log-revert` (F-121). No hiqlite spec claims any of
it, and an openraft guarantee is not evidence of hiqlite behavior.

**hiqlite owns its integration**: how a node decides to initialize or join
(F-118), the membership gate and its shutdown ordering (`027` B-9), the
self-leave of an in-memory cache, WAL and vote persistence under the named
`LogSync` mode, the SQLite and cache state machines, the per-cluster backup
image and its restore (`026`), readiness, the transport and its secrets, and the
exclusive storage lock (`024`).

**The consumers own application consistency, fencing and orchestration**:
Rahi owns the cell composition (two processes, die-together, shutdown order,
graces), the cell backup archive that combines both clusters with the keys
(Rahi 030 B-5), its restore verb and marker, its fencing tokens over `dlock`
(Rahi 012; F-026 says the lock is a lease, not a fence), and write quiescence.
Rauthy owns its identity data and signing keys. Statecraft owns its per-scope
serialization, which is **in-process only** today
(`statecraft/docs/design/04-scope-serialization-investigation.md`), and so is
not a cross-replica guarantee at N=3. The Kubernetes objects (StatefulSet, PDB,
NetworkPolicy, probes, grace) belong to whichever repository deploys them.

A proposal that crosses a boundary is written here as a **proposed obligation
for the owning repository**, not as a change.

## 3. Lifecycle events are different operations

Each row is a different operation with a different mechanism and a different
risk. Treating one as another is the most likely operational error at N=3.

| event | identity and volume | membership change? | mechanism today | state at `72e09a6` |
|---|---|---|---|---|
| **Ordinary restart** (rollout, eviction, crash) | same ordinal, same PVC | **none** with `cache_storage_disk = true`; the cache self-leaves and rejoins when `false` | node reopens its log and state machine and catches up by log or snapshot | F-054 (immediate restart race at N=3, behind a 1 s sleep); budget overrun (section 4) |
| **Permanent replacement, same id** | same ordinal, **new empty PVC** | yes: leave then rejoin, automatic | `become_cluster_member`: "leave before proceed", then learner, then voter | exercised in-process (`self_heal.rs`); **F-118 on node 1** |
| **Permanent replacement, new id** | new ordinal or new id | yes, operator-driven | remove the old id, add the new; every node's static `nodes` list must change | not supported; `nodes` is read at first start (Rahi README) |
| **Membership expansion** (N=1 to N=3 live) | existing cell grows | yes, on the production copy | add learners to the running node 1, promote | not supported; no procedure recorded anywhere |
| **Disaster recovery** | fresh cell, fresh PVCs | yes, on an empty cluster: node 1 alone, then two joins | node-1 restore, then the ordinary join | `026` B-7 supports N=1 only; F-058 residual, F-119, F-120 |
| **Version upgrade** | same ids and PVCs | none | rolling restart | a mixed-version cluster is **not supported** (handoff section 5) |

The row that matters most for the migration: **in hiqlite, every N=3 cluster is
born by membership change.** Node 1 initializes itself as the only voter
(`init.rs:71-100`) and nodes 2 and 3 join through `add_learner` and
`change_membership`. There is no path that initializes three voters at once,
although openraft's `initialize` accepts a set. Fresh bootstrap, disaster
recovery and live expansion therefore share the `{1} → {1,2,3}` membership path;
they differ in what is at stake while it runs.

## 4. Shutdown and self-leave

**Does routine termination change membership?** For the consumers' configuration,
**no**. The SQLite group never self-leaves. The cache group leaves only when
`cache_storage_disk = false` (`client/mgmt.rs:428-487`), and both consumers run
`true`. A pod rollout therefore stops and restarts voters without touching the
membership of any of the four groups.

**Is that right for stable StatefulSet identities?** Yes, and the other
behavior is right for its own case:

- With a retained PVC the node keeps its vote and its log. Removing it on every
  restart would drop the group to two voters for the duration (one more failure
  then halts writes), force a snapshot transfer on rejoin, and make every
  rollout a sequence of membership changes, which is exactly the `027` KD-7 /
  KD-9 territory that is least evidenced.
- With an in-memory cache the node loses its vote and log on exit, and
  rejoining under the same id without leaving would let it vote from an empty
  log: the openraft FAQ sequence. Leaving is required there, and hiqlite's
  "leave before proceed" on start covers the case where the shutdown never ran
  (`SIGKILL`). The stream gate refuses raft traffic until that leave is done.

**Recommendation (D-3):** require `cache_storage_disk = true` for every N=3
production group, so no routine operation changes membership. Keep the
in-memory self-leave as it is for other deployments.

**The shutdown budget does not fit.** Read from source, per hiqlite node, with
peers present:

| step | bound | source |
|---|---|---|
| fixed pre-shutdown delay | 9.5 s, whenever membership has more than one node | `mgmt.rs:386` |
| membership drain | up to 5 s | `SHUTDOWN_DRAIN` |
| wait for a cache leader | up to 5 s, only if none is known | `mgmt.rs:419-426` |
| in-memory cache leave through a peer | up to 10 s (not taken by the consumers) | `REMOTE_LEAVE_BOUND` |
| wait for a SQLite leader | up to 5 s, only if none is known | `mgmt.rs:520-531` |
| raft, WAL and SQLite writer stops | unbounded individually | |
| caller's wait | **15 s**, then `Error::Timeout`: completion unconfirmed, the sequence may still be running | `SHUTDOWN_WAIT` |

Typical is about 10 s; the worst case exceeds 20 s for the consumers'
configuration. A cell has **two** of these, sequential: Rahi stops serve (HTTP
drain up to 10 s, denials 5 s, then `Client::shutdown`) with a 15 s serve grace,
then SIGTERMs Rauthy (actix `shutdown_timeout(10)`, then its hiqlite shutdown)
with a 10 s grace (`supervise.rs:476,486,604-614`), inside a 30 s pod grace.
Inferred, not measured: at N=3 both hiqlite shutdowns are cut short by the
supervisor's graces, and the pod's routine termination ends in `SIGKILL`. Raft
recovers from a crash, so this is not a safety failure; it makes every rollout a
crash recovery, which is the path F-054 sits on and the one least tested.

The pre-delay exists "so services can stop sending requests"
(`mgmt.rs:380-384`). Kubernetes removes a terminating pod from Service endpoints
on its own, and Rahi's `/readyz` does not consult hiqlite at all, so for the
cell the delay buys nothing a Service needs. openraft 0.9 has no leader
transfer (`mgmt.rs:489-490`), so a terminating leader still costs its groups an
election: 1.5 to 3 s with the default `election_timeout_min/max`.

**At N=1 the sequence is shorter**, read from source: membership has one node,
so the 9.5 s pre-delay is skipped; the drain returns at once unless a membership
change is in flight (`membership_gate.rs:130-145`); the leader waits end at once
because a leader exists. What remains is the raft, WAL and SQLite-writer stops,
which are **unmeasured**.

**Three outcomes, not two.** A shutdown ends in exactly one of:

- **confirmed graceful completion:** `Client::shutdown` (or `ShutdownHandle::wait`)
  returned `Ok(())`. Every component stopped, the WAL writer flushed, the SQLite
  writer persisted its metadata (F-125), and hiqlite released storage ownership.
- **unconfirmed completion:** it returned `Err(Timeout)` after 15 s. The sequence
  runs in its own task and **may still complete** while the runtime lives, but
  nothing reports whether it did (`mgmt.rs:601-606`). It is not a failure of
  the node and not a success of the shutdown.
- **confirmed forced exit before completion:** the process ended, by its own exit
  after an unconfirmed or failed stop or by `SIGKILL` at the grace, while the
  sequence had not returned `Ok`. Every component not yet stopped was crashed;
  the OS released the advisory storage lock (`024`).

Only the first **passes** graceful-within-budget acceptance. An unconfirmed
completion fails it however soon the process then exits, and a forced exit is
recorded as a crash. Section 13's export requires the first (R-b).

**Reconciliation with Rahi 043** (draft, `5707f60`, superseding decision
packet 1's D4). The packet's ten-second store allowance is gone. 043 B-9 composes
the stop from configured bounds: stream drain S 10 s, connection drain C 10 s,
denial drain D 5 s, store shutdown H 15 s (hiqlite's own caller-side wait,
`SHUTDOWN_WAIT`, after which `Client::shutdown` returns `Error::Timeout`), and
Rauthy's stop R 10 s; `SERVE_GRACE >= S + C + D + H` = 40 s, and the pod grace and
`docker stop -t` `>= SERVE_GRACE + R` = 50 s, both proposed and neither measured.
It adds a workload check (measured maximum × 1.5 within the grace), records each
stop as confirmed completion, unconfirmed completion or forced exit (B-10), and
never wraps `Client::shutdown` in a shorter timeout. That matches this section.
What remains open is evidence: at N=1 the 9.5 s pre-delay is skipped, so H is a
bound hiqlite enforces, not a duration anyone has measured under 043's workload
(Rahi's D-P4 measured 0.37 to 0.73 s end to end without streams or Rauthy, and
says it bounds nothing). In a split N=3 pod, H = 15 s cannot contain the default
9.5 s pre-delay plus the stops with margin; `033` B-4's option set well below it,
or a larger H with a matching grace, is still required.

**Measurements, recorded separately, not yet run.** **M-N1:** the single
container, both hiqlite nodes, Rahi's stop phases as Rahi 043 B-9 composes them:
end-to-end duration and each node's outcome. **M-S3:** the split N=3 layout, one
node per pod, per role, leader and follower, with and without a membership
change in flight: duration and outcome. A consumer's grace is set from these,
and only confirmed completions count toward "within budget".

**Recommendations:** a hiqlite option for the pre-shutdown delay, defaulting to
today's 9.5 s so nothing changes for current callers (`033` B-4, planned), and
graces derived from M-N1 and M-S3 with margin (D-6; consumer obligation).

**Under D-14** a pod holds one hiqlite node, so its termination is one hiqlite
shutdown behind one HTTP drain, with no supervisor grace in between. The
sequential double shutdown above remains a property of the N=1 local profile
only, where the pre-delay is skipped because membership has one node.

## 5. The four raft groups and their shared lifecycle

| group | holds | durable in the cell? | leader location |
|---|---|---|---|
| Rahi SQLite | ledger, application state | yes | independent |
| Rahi cache | `dlock` leases behind Rahi's fencing tokens, counters, notify | yes on disk (`cache_storage_disk = true`), **not in any backup** | independent |
| Rauthy SQLite | identities, clients, signing keys (encrypted under `ENC_KEYS`) | yes | independent |
| Rauthy cache | Rauthy's cached and ephemeral state | yes on disk, **not in any backup** | independent |

Each group elects independently, so the four leaders can sit on any mix of pods.
The shared pod lifecycle couples them:

- **Pod loss removes one voter from all four groups at once.** With required
  anti-affinity that is one voter per group per node, so a single node failure
  is survivable by every group, and two node failures halt writes in every
  group. Four elections may run concurrently when the lost pod led them all.
- **Die-together is correlated failure by contract** (Rahi 031): a Rauthy exit
  ends the container, so a Rauthy-only fault costs the Rahi groups a voter too.
  This is accepted by the composition contract and is not proposed for change;
  it does mean the four groups' failures are never independent within a pod.
- **One pod's shutdown is two hiqlite shutdowns** (section 4), and each is at
  least 9.5 s at N>1 today.
- **Readiness is asymmetric.** Rahi's `/readyz` checks the app store and ledger
  head, not Rauthy's hiqlite. A pod can be Ready with its Rauthy node still
  joining.
- **Same `node_id` in both clusters** is sound (they are separate clusters with
  separate secrets and ports), and makes node 1 the same pod in both, so F-118
  applies to both clusters on that pod together.
- **The cache groups are not "per node".** `rahi/deploy/README.md:265` says so;
  in hiqlite the cache is a replicated raft group, and Rahi's `dlock` fencing
  relies on exactly that. A documentation obligation for Rahi.

**Under D-14** the four groups stay, but a pod carries the voters of two of them
(one application's SQLite and cache groups), not four. Pod loss, a rollout, an
OOM kill or a crash costs one application's groups a voter and leaves the other
application's untouched; die-together no longer applies at N=3; each pod has one
shutdown; and `node_id` is each StatefulSet's own ordinal plus one. Node-level
failure is unchanged: losing one worker still removes at most one voter from each
cluster, and losing two still halts both, because splitting separates process
faults, not hosts.

Changes to the composition are proposed to Rahi and Statecraft (sections 8 and
12), not made here.

## 6. Architecture comparison

Three ways to reach a production N=3 cell.

**A. Live expansion.** Keep the N=1 cell serving; add two pods as learners to
its node 1; promote them.

**B. Fresh-cell restore.** In a maintenance window: quiesce the N=1 cell, take a
coherent backup, restore it into an isolated new N=3 cell, validate, cut over.

**C. Fresh N=3 bootstrap.** A new cell with no prior data starts at N=3.

| | A. live expansion | B. fresh-cell restore | C. fresh bootstrap |
|---|---|---|---|
| **write availability** | no planned outage, but a membership change runs against the only copy; node 1's config must also change, which is a restart of it | planned write outage: quiesce to cutover | n/a (no users yet) |
| **latency after** | every commit waits for 2 of 3 acks | same | same |
| **resources during** | 3 pods, one live snapshot transfer from the production node | old cell (stopped, PVC retained) plus the new cell: about 4 volumes' worth | 3 pods |
| **what is at risk while membership changes** | production data, on one voter | an empty cluster that can be destroyed and redone | nothing |
| **rollback** | remove the learners; after promotion, a membership change back | before the first new write: stop the new cell, restart the old, no reconciliation | redeploy |
| **rehearsable on real data first** | no: the rehearsal is the operation | yes: restore the same backup into a scratch cell as often as needed | yes |
| **data loss** | none if it works; a failure mid-change risks the only copy | none for quiesced writes; **cache groups start empty**, so whatever lives only in a cache is lost (D-8) | n/a |
| **recovery time objective** | continuous | the window: export, transfer, restore, join, validate | minutes |
| **also qualifies** | nothing else | **disaster recovery**: the same procedure is the DR runbook | the join path B also uses |
| **hiqlite state** | no procedure; `{1} → {1,2,3}` under load; `027` KD-7, KD-9; config churn on a live node | F-058 residual does not bite when followers are empty; F-119, F-120, `026` KD-5, KD-8 do | F-118 on later node-1 restarts; F-054; F-122 |

**Recommendation.** C for new production cells, B for existing N=1 cells, as
the owner proposed, and A **not** built now. The evidence supports that
direction and adds two refinements:

1. B and C are not separable from membership change. Both form the cluster by
   `{1} → {1,2,3}`, so qualifying them **is** qualifying F-107's gate on real
   nodes; A would use the same path against production data. F-058 and F-107
   are one workstream at the point where they meet: the restore produces node 1,
   the gate admits nodes 2 and 3.
2. B, done as specified below, is also the disaster-recovery procedure. Choosing
   it means every migration rehearses DR, and DR is qualified by the migration's
   own acceptance.

Cons of the recommendation, stated: a planned write outage per existing cell;
loss of cache-only state; a second cell's worth of resources during the window;
and Rahi's and Rauthy's adoption of `hiqlite-patched` before any of it applies.

## 7. Migration protocol: an existing N=1 cell into a fresh N=3 cell

States are named so an interrupted run can be resumed from its last completed
state. The **source** is the N=1 cell; the **target** is the new cell, split per
D-14. Revised 2026-09-23 by the reconciliation pass (section 15): the controls
are separated by what each one does (7.2), the export's currency rule is a
proposal with its own safety argument (section 13), and activation is defined
by authoritative mutations (7.3). Every decision this section depends on,
other than D-14, is pending.

**Prerequisites (P).**

- P-1. Both consumers run a `hiqlite-patched` release that passed section 9's
  hiqlite stages, at N=1 in the source **and** at N=3 in the target (Track S7,
  section 8). Migrating across hiqlite versions at the same time is a second
  change in one window and is refused (D-10).
- P-2. The target is isolated: its own namespace, its own Service and pod DNS
  names, NetworkPolicy admitting no traffic between the two namespaces, and
  **distinct `secret_raft` and `secret_api` for each of its two clusters**.
  These are **replication isolation** (7.2, control 4), not a write fence.
- P-3. The target's pods carry **no** restore instruction in their template.
  Restore is a one-shot act on node 1 of each StatefulSet (R-3), never an
  environment variable a restart can repeat (F-119).
- P-4. `033` and `034` acceptance passed on the release in use; the procedure
  was rehearsed end to end against a copy of the source's archive in a scratch
  cell, with its timings recorded.
- P-5. The downstream tombstone feature of 7.2 control 5 exists and is enforced
  on every startup path listed there.

**Steps.**

| state | action | done when | on failure |
|---|---|---|---|
| **Q-1 quiesce** | the source enters maintenance: Rahi's edge refuses every mutating request, its own and those it proxies to Rauthy | a probe write is refused | leave maintenance; nothing changed |
| **Q-2 barrier** | after Q-1, once every request admitted before it has completed (13.11), and if the owner adopts section 13's barrier (alternative B): write and record one barrier per cluster; it proves the export faithful to each cluster at barrier time, not that nothing was lost before (13.7) | both barrier receipts recorded outside the cell | leave maintenance; retry |
| **Q-3 exclude workloads** | control 2 of 7.2: scale to 0 and delete every source workload that can open a source data directory; suspend reconciliation that could re-create one | the objects are gone and cannot be re-created by automation | the source PVC is intact; re-create the source |
| **Q-4 stopped** | every source pod object gone, every VolumeAttachment released, **and** each hiqlite shutdown confirmed complete (section 4); an unconfirmed stop sends the node through one confirmed start and stop | evidence recorded per node | as Q-3 |
| **Q-5 volume snapshot** | a CSI VolumeSnapshot of the source PVC, **before** any write to it | snapshot `readyToUse` | retry; this is the byte-for-byte rollback artifact |
| **B-1 lock and export** | the export job takes every source storage lock (control 3) and holds them until B-2 ends; it produces the two cluster images by section 13's rule, then the keys and the manifest (D-7) | both images pass section 13's checks, or the job refuses | refuse; the source is unchanged; remedy per section 13 |
| **B-2 tombstone** | before releasing the locks, the job writes the tombstone (control 5) into each source data directory | tombstones present, locks released | the job retries; a half-written tombstone is not a tombstone (7.2) |
| **B-3 verify provenance** | every part's hash against the manifest; the manifest names the source cell, both clusters, each image's applied log id, the barrier receipts if any, the hiqlite version and the key ids | all match | stop; do not upload a mismatched archive |
| **B-4 upload and prove remote** | upload; then **independently** list, fetch back and re-hash, because `Client::backup` reports only the local copy (`026` KD-8) | remote hash equals local | retry the upload |
| **R-1 provision** | create the target as D-14's two StatefulSets in one namespace, with empty PVCs, the internal Rauthy Service, section 12's NetworkPolicies, and no restore instruction | objects exist | delete and recreate |
| **R-2 keys** | install the source's keys (Rahi keys Secret, Rauthy `ENC_KEYS`); refuse if key ids differ from the manifest | ids match | fix the Secret |
| **R-3 restore node 1** | on ordinal 0 of **each** StatefulSet, apply that cluster's image through `034`'s checked, at-most-once restore | node 1 of each cluster is a single-voter leader on the restored state | destroy the target's PVCs; back to R-1 |
| **R-4 join** | start ordinals 1 and 2 of each StatefulSet; each joins its own cluster | four groups report voters `{1,2,3}` | stop joins; if not resolvable, back to R-1 |
| **V-1 validate** | 7.1, with the target still in maintenance | every check passes | back to R-1 |
| **A-1 activate** | 7.3 | the first authoritative mutation is admitted | **point of no return** once A-1 completes |

**7.1 Validation before activation (V-1).** On the target, in maintenance, with
no external traffic:

- every group of both clusters: three voters, one leader, no learner, the same
  membership log id on every node;
- each SQLite group's applied log id on every node at or past the manifest's,
  and a deterministic content digest equal on all three nodes **and** to the
  manifest's (D-7); if a barrier was used, the barrier row is present;
- a leader kill in each cluster, writes resuming inside the stage-4 bound, the
  digest unchanged afterwards;
- Rahi reaches Rauthy only through the internal Service over the encrypted
  path, and a pod outside Rahi's labels is refused (section 12);
- with Rauthy scaled to 0, every Rahi pod goes not-ready and none restarts;
- consumer checks: Rahi `preflight` and `ledger verify` at the manifest's ledger
  head; Rauthy's discovery document with the unchanged issuer and a token signed
  by the expected key id; Statecraft's own acceptance;
- section 14's security acceptance;
- no pod has restarted since R-4, or each restart is explained.

Every write V-1 makes (probe writes, a leader-kill test's writes) happens on the
disposable target and is discarded by any return to R-1.

**7.2 Five controls, one authority.** The earlier text listed isolation
controls as independent write fences. They are not. Five different things are
in play; only the combination of 2, 3 and 5 keeps the source from writing, and
only 7.3 gives the target authority.

1. **Source quiescence** (Q-1). Maintenance refuses mutating client requests.
   Application-level and cooperative: it does nothing about Rauthy's background
   jobs, a Rahi cron path, or a pod started later without maintenance. Its job is
   to stop new user writes before the barrier and the stop, not to fence.
2. **Workload exclusion** (Q-3, Q-4). No controller object that could start a
   source writer exists, nothing can re-create one, and no source pod runs. This
   is what actually stops source writes, and it lasts only as long as nobody
   re-creates a workload.
3. **Storage locking** (B-1). `024`'s advisory lock on each data directory,
   held by the export job. It proves that no cooperating hiqlite process on the
   same mount holds the directory **while the job runs**; it does not stop a
   non-hiqlite writer, a process on another mount, or anything after the job
   ends.
4. **Replication isolation** (P-2). Distinct secrets, DNS names and
   NetworkPolicy keep the two cells' Raft groups from joining or replicating into
   each other. It does not stop either cell from accepting application writes.
5. **Application authority** (B-2 tombstone, 7.3 activation). The tombstone
   makes a re-created source refuse to serve; activation is the single recorded
   instant at which the target starts accepting authoritative mutations.

**The tombstone, a proposed downstream feature** (Rahi obligation, with Rauthy
enforcement; nothing in hiqlite):

- *Content.* A file at the root of each source data directory
  (`/data/hiqlite`, `/data/rauthy`) naming the migration id, the target cell,
  the time, and the manifest digest, written to a temporary name, synced,
  renamed, and the directory synced. A tombstone that is not at its final name
  is not a tombstone.
- *Enforcement: every startup path.* `rahi serve`; `rahi supervise` before it
  spawns Rauthy; every Rahi verb that opens a node (`preflight`, `migrate`,
  `backup`, `restore`); the Rahi backup CronJob and migrate Job, which run
  those verbs; and **Rauthy started on its own**, which does not pass through
  Rahi at all: an independent Rauthy container (a split cell), a manual
  `rauthy` invocation or a debugging pod on the volume. Rauthy has no such check
  today, so the proposal is an init container or entrypoint check in the Rauthy
  image used by the cell, plus a Rauthy-side refusal if its maintainer accepts
  one. Any path not listed here is a gap, not an exemption.
- *Effect.* Refuse to open the node, exit non-zero, name the migration id.
  Never delete or move data.
- *Interrupted fencing.* If the job dies after the export and before both
  tombstones are final, the source is still stopped (control 2) and unchanged
  except for any tombstone already written; the job reruns B-1 to B-2 from the
  start, re-verifying the images. Activation (7.3) requires both tombstones
  recorded, so a half-fenced source never coexists with an active target.
- *Deliberate rollback.* Removing a tombstone is an explicit, logged operator
  act, allowed only before A-1, or after a reverse migration that makes the
  source the active cell again. It restores the source exactly: the tombstone is
  the only byte B-2 added.
- *The unchanged-source claim.* The source's **databases and logs** are
  unchanged from Q-4 onwards; the tombstone file is the one addition, outside
  both hiqlite directories' database and log files, and Q-5's VolumeSnapshot is
  taken before it, so the byte-identical source image is the snapshot, and the
  live volume is "unchanged plus a tombstone". The earlier wording ("the source
  volume is unchanged") is corrected to that.

**7.3 Activation and the point of no return.** Defined by **authoritative
mutations**: a write to either target cluster that a client, a user, a relying
party or a background job of the application could observe or build on.

Before activation, the target is in maintenance and admits only **preparatory
mutations**, each disposable because the target is destroyed by any return to
R-1 and the source is untouched by all of them:

- the restore itself, the joins, and V-1's probe and leader-kill writes;
- the Rahi 041 **epoch record** for the target, appended with the migration id
  and the source's final ledger head;
- the **bearer floor** (section 14), set to the planned activation instant;
- the **security resets** section 14 selects (for example re-applied manual IP
  bans, or invalidated sessions for a restore from a stale image);
- background writers stay disabled: Rauthy's scheduled jobs and any Rahi
  periodic task are held off until activation, so nothing authoritative is
  written while in maintenance. Where a consumer cannot hold its background
  writers, that consumer must say which writes they make and why they are
  disposable; until then, the target is not in a disposable state.

Activation is one recorded step: lift maintenance, enable background writers,
switch routing. **The point of no return is the first authoritative mutation
the target admits after that step**, not the routing change itself: until one
is admitted, stopping the target and removing the tombstones returns to the
source with nothing lost. After it, returning is a reverse migration by this
same protocol; re-starting the source instead would discard that mutation and
need application-level reconciliation that no consumer has specified. If the
epoch record was appended, a later reverse migration carries it; it is not
retracted.

**7.4 Interrupted operation.** Before A-1 every state is recoverable without
reconciliation: the source's databases and logs are unchanged and the target is
disposable. **A failure anywhere in R-1 to V-1 destroys the target's PVCs and
resumes at R-1**; nothing is repaired in place. Within R-3, `026` B-8's
roll-forward finishes a committed restore, and `034`'s at-most-once record makes
a repeated instruction a no-op. A failure in Q-1 to B-4 leaves the source
stopped (or re-creatable), snapshotted, and at most tombstoned.

**7.5 Proposed recovery objectives, for owner decision (D-9).** None of these is
an accepted requirement.

| scenario | proposed RPO | proposed RTO | depends on |
|---|---|---|---|
| migration (B) | 0 for writes acknowledged before Q-1, **only** under the conditions section 13 establishes for the chosen export rule; cache-only state lost by design (section 14) | write outage at most 30 min per cell, set from the rehearsal | P-4, D-8, section 13 |
| one node lost at N=3 | 0 for committed writes under `LogSync::Immediate` only | writes resume within 10 s | D-4, F-121 |
| cell lost (DR from the last archive) | the archive interval; on hot archives **only** with section 14's restore validator | 60 min | D-9, D-12 |

## 8. Consumer follow-up, planned alongside

Proposed obligations, each for its owning repository. None is written there by
this change.

**Rahi.**
- R-a1. **Track N1**, now and independent of this work: adopt the published
  `hiqlite-patched 0.15.0-patched.1` and `rauthy-patched:0.36.2-patched.2` at
  N=1 (Rahi 043, draft at `5707f60`), with its durable SQL revocation and
  transition floor (043 B-6, B-6b; section 14's D-8b). Claims N=1 only. The
  producer repair of `035` reaches Rahi only if Rahi separately decides to move
  its exact pin to a release that carries it, and Rauthy's image only by a Rauthy
  rebuild (section 16, node F).
- R-a2. **Track S7**, later: adopt a release that passed stages 4 to 6. Only this
  track makes a hiqlite N=3 result apply to the cell.
- R-b. Replace the file-level restore of Rahi 030 D-2 with hiqlite's repaired
  restore once `034` makes it callable. D-2 re-implements the 0.14 sequence that
  F-057 and F-100 record, without staging, sync or roll-forward.
- R-c. Shutdown: graces from M-N1 and M-S3 (section 4); record each stop's
  outcome as confirmed completion, unconfirmed completion or forced exit, and
  never treat the second as the first; set the hiqlite pre-delay option once it
  exists; a `preStop` only if the measurement shows it is needed. Under D-14 this
  is one shutdown per pod.
- R-d. N=3 manifests, per StatefulSet: PodDisruptionBudget
  `maxUnavailable: 1`; required anti-affinity within the application; zone
  spread where the cluster has zones; the section 12 NetworkPolicies; restore
  the `--adopt-manifest` flag the n3 overlay drops.
- R-e. The D-14 composition: a new Rahi spec that amends 031 (die-together,
  supervision) and 032 (topology) for N=3 only, stating section 12's routing,
  trust, readiness and liveness rules, and keeping the single-container
  supervisor as the N=1 profile.
- R-f. Correct `deploy/README.md:265` (the cache group is replicated, and holds
  the leases behind Rahi's fencing tokens).
- R-g. The archive manifest gains what D-7 lists, so B-2 and V-1 can check it.
- R-h. Maintenance mode for Q-1 that also covers requests proxied to Rauthy.
- R-i. A two-StatefulSet archive: the backup verb addresses Rauthy through the
  internal Service instead of loopback, and its offline form exports both
  clusters after both StatefulSets are at 0, by section 13's rule (section 12).
- R-j. The tombstone (7.2 control 5): written by the export job, enforced on
  every Rahi startup path, removed only by a logged rollback.
- R-k. The barrier, if the owner adopts section 13.7: one per cluster after
  quiescence, recorded outside the cell.
- R-l. The bearer floor (D-8b) and the restore validator (section 14.4).
- R-m. Background writers held off until activation, or each one's writes
  stated and shown disposable (7.3).

**Rauthy** (proposals to its maintainer; the request, with what each item
blocks, is `standards/spec/n3-rauthy-request.md`). Confirm the cache inventory
of 14.1. Enforce the tombstone when Rauthy starts on its own. A way to hold
background writers until activation (7.3). Barrier access for Rauthy's cluster,
if D-12 adopts the barrier. A restore-time invalidation of sessions and refresh
tokens (D-8c). An admin-API export and re-import of manual IP bans (D-8d). A
rebuild on a release carrying `035`'s repair. Confirm `ENC_KEYS` and key-id
handling across a restore into a new cell. Separately, the DPoP nonce observation
of 14.5, a source finding only.

**Statecraft.** Its manifest's rule that Rauthy is never a second container
or a separate workload (Statecraft 002, `deploy/k8s/statefulset.yaml`) holds
for N=1 and must be amended for N=3 by a Statecraft spec that adopts D-14.
Its per-scope serialization is in-process only. At N=3, with
more than one replica serving, it is not a guarantee: a cross-replica mechanism
(for example Rahi's `dlock` with its fencing tokens) or single-writer routing is
required before Statecraft acceptance can pass. Its manifest carries
`replicas: 1` and no peer lists.

**Aicortex.** Keep N=1 as the local profile (044 B-8, D-1; `docker compose`,
the single-voter tests). 044's three-replica requirements (FR-003, FR-005) wait
on the stages below and on the Statecraft item above.

## 9. Qualification plan

Dependency-ordered. Each stage has pass criteria and a stated evidence limit. A
stage is **passed** only by recorded evidence on a named commit and release; no
stage is passed by this document. Stages 1 to 6 are hiqlite's; 7 onward are
consumer and cell acceptance and are never recorded as hiqlite qualification.

| # | stage | depends on | pass criteria | evidence limit |
|---|---|---|---|---|
| 0 | tool and graph | none | pinned `spec-spine` revision reports `0.20.0`; `cargo metadata --locked`; `openraft =0.9.25` | proves the gate and graph, nothing about behavior |
| 1 | the owner decisions **each later piece of work names** as blocking it (section 11's "blocks" column; section 16), recorded; not every decision before any work (D-13 follows stage 4) | 0 | each decision dated in `033` section 6 | a decision is not evidence |
| 2 | repairs for F-118, F-119, F-120; decision on F-121; pre-delay option; `034`'s export, restore and (if adopted) barrier entry points | the decisions section 16 names for each repair only | each has a test observed failing without it | unit and in-process tests only |
| 3 | real-node harness (`033` B-2) | 2 | three **release-build** node processes per cluster, the consumers' exact feature sets and settings, a per-link TCP proxy for partitions, `SIGKILL`/`SIGTERM` by the harness | one host; no kernel crash; no real disk loss |
| 4 | hiqlite N=3 acceptance (`033` section 3, A-1 to A-12) | 3 | every scenario passes 20 of 20 consecutive runs with no retry, no masking sleep, fixed bounds | 20 runs bound the observed rate, they do not prove absence (F-107 appeared once in about seven) |
| 5 | restore acceptance (`034` section 3) | 3, and `034` implemented | every restore scenario, including wrong-backup rejection and interruption at each state | one host; object storage by a local S3 double unless stated |
| 6 | TLS or mesh boundary (D-5) | 3 | the chosen boundary carries raft and API traffic; a capture shows no plaintext; shutdown under the boundary is still within budget | proves the configured path, not every certificate lifecycle |
| N1 | Track N1 (parallel, Rahi-owned, not a stage of this plan) | none here | Rahi 043 AC-1 to AC-9 at N=1 | N=1 consumer evidence; no N=3 claim |
| 7c | Track S7 candidate integration | a stage-4 **candidate** (stage 2 merged, stage 3 built, stage 4's first bounded tranche passed) | Rahi and the Rauthy image build and run their suites against the candidate, unpublished and unpinned in any release | integration findings only; no adoption, no support, no N=3 claim |
| 7 | Track S7 consumer adoption | 4, 5 and 6 passed on **the exact release** adopted, and that release published | Rahi and the Rauthy image pinned to it (R-a2) by their own governed decisions; both consumers' suites green | consumer evidence, not hiqlite's |
| 8 | Kubernetes cell acceptance | 5, 6, 7 | on a real three-node cluster: section 10 prerequisites in place; stage 4's scenarios re-run as pod, node and network faults; measured shutdown budget within grace with margin; PDB-respecting drain of each node | one cluster, one provider |
| 9 | migration rehearsal | 8 | section 7 end to end on a copy of a real archive, twice, timings recorded, V-1 green | a rehearsal on a copy |
| 10 | owner: mark N=3 supported | 9 | a later change updates `026` B-7, the handoff and the ledger with the evidence | ratification and support are the owner's acts |

**States, kept apart** (reconciled 2026-09-23, second pass; the earlier
sentence here let consumer adoption start on a stage-4 candidate while the table
made it depend on stage 4 passing). A build is, in order: a **candidate** (built
from merged repairs, unpublished; integration testing only); **qualified** (stages
4, 5 and 6 passed on that exact build); **published** (released under `031`'s
procedure with its own authorization); **adopted** (a consumer pinned it by its
own governed decision); and **supported at N=3** (stage 10, the owner's act after
stages 8 and 9). Candidate integration (7c) may start before every qualification
stage is complete, so composition problems surface early. Adoption (7) may not,
and published support may not start before stage 9. Nothing a candidate shows is
qualification evidence, and a candidate is never pinned by a consumer release.

**Lanes are separate authorizations.** Governance drafting, runtime repairs
(stage 2), harness construction (3), bounded harness execution (4 to 6, fixed
run counts, stop at the first failure), Kubernetes acceptance (8), the
rehearsal (9), publication and the owner's support decision (10) each start
only on their own authorization. That a run count appears in this table does
not authorize running it.

## 10. Kubernetes prerequisites

- **Stable ids and discovery.** One StatefulSet per application (D-14);
  `node_id` = that StatefulSet's ordinal + 1; peers by the application's own
  headless-Service pod DNS, with `publishNotReadyAddresses: true` so a not-ready
  pod stays addressable to its peers; peer lists fixed before first start.
  Replacement keeps the ordinal (D-2).
- **Bootstrap and readiness ordering.** `Parallel` pod management is acceptable
  only after F-118 is repaired; until then, `OrderedReady` for first bootstrap.
  Readiness and liveness follow section 12.
- **Independent failure domains and storage.** Required anti-affinity across
  nodes; zone spread when zones exist; one `ReadWriteOnce` PVC per replica on
  storage that is not shared between replicas; no network filesystem (the
  handoff's supported-platform statement). Two replicas on one node or one
  volume are one failure domain and void every N=3 claim.
- **Disruption.** A PDB `maxUnavailable: 1` per StatefulSet; node drains
  serialized; a drain that would take a second voter of either cluster waits.
- **Shutdown budget.** `terminationGracePeriodSeconds` set from stage 8's
  measurement plus margin, never below the measured maximum.
- **Upgrades.** Mixed-version clusters are not supported (handoff section 5).
  The supported upgrade is **full stop**: all replicas down, then all up on the
  new version, which is a planned outage. A rolling upgrade is supported only
  for a pair of versions qualified together (D-11).
- **Management endpoints.** The hiqlite API ports carry `/cluster/*` membership
  endpoints guarded by `secret_api`, and Rauthy enables `dashboard` (F-087,
  F-091). NetworkPolicy admits each application's hiqlite ports only from that
  application's own pods; they are never exposed through ingress.
- **Transport encryption.** NetworkPolicy is access control, not encryption.
  hiqlite's secrets authenticate a peer; they do not encrypt raft or API
  traffic, which is plaintext in the cell today. D-5 chooses the boundary.

## 11. Owner decisions: the one packet

Consolidated 2026-09-23 (second pass). Every pending decision of this proposal,
of `034` and of `035` is in this table and nowhere else; the other documents
cite it. **D-14 is decided and is not asked again.** A recommendation is not a
decision. "Blocks" names the section 16 nodes that cannot proceed without the
decision; a node not named proceeds without it. Owners: **H** the owner of this
fork; **R** Rahi's owner; **A** Rauthy's maintainer; **S** Statecraft's owner. The
same person may hold several roles; each role decides only its own repository.

| id | exact wording for the record | options | recommendation | consequence | owner | blocks |
|---|---|---|---|---|---|---|
| D-1 | "Existing N=1 cells reach N=3 only by fresh-cell restore (B); new cells bootstrap at N=3 (C); live expansion (A) is not built." | A; B; C; B+C | **B+C, A deferred** | a planned write outage per existing cell; the migration is also the DR drill | H, S | G |
| D-2 | "A permanently replaced node keeps its ordinal and node id with a new empty volume; a new node id is unsupported." | same id; new id | **same id**, after F-118's repair | static peer lists never change; replacement is leave-and-rejoin | H | E (A-6's pass criterion) |
| D-3 | "Every N=3 production group runs `cache_storage_disk = true`." | require; allow in-memory | **require** | no routine operation changes membership | H, R, A | E, G |
| D-4 | "The SQLite groups run `LogSync::Immediate` wherever zero loss of committed writes is the objective, including the migration source's last run." | `Immediate`; `ImmediateAsync` with the loss window stated | **`Immediate`** | an fsync per append batch, to be measured in stage 4 before production; not settable from the environment (F-035), so set in code or TOML; with `ImmediateAsync`, every RPO statement carries 13.4 (ii) | H, R, A | E (the configuration under test), the RPO wording of D-9 |
| D-5 | "hiqlite raft and API traffic between pods is protected by: ..." | CNI encryption; hiqlite TLS; mesh | **CNI encryption where available**, else hiqlite TLS after stage 6 | a mesh needs a qualified shutdown order (section 4) | H, S | E (stage 6), G |
| D-6 | "The pre-shutdown delay becomes a node option with today's 9.5 s default; consumer graces come from M-N1 and M-S3." | adopt; keep fixed | **adopt** | no current caller changes; only confirmed completions count toward a budget | H | nothing in B (the default is unchanged); consumer grace values in F and G |
| D-7 | "A backup image records: cluster id, source node, applied log id, content digest, barrier nonce if any, hiqlite version, key ids." | these fields; fewer | **these fields**, image fields in `034`, archive fields in Rahi | provenance only; no cross-cluster compatibility inferred | H, R | C1 |
| D-8a | "Functional cache-only state (in-flight flows, assertions, performance caches) is accepted as lost at every cache replacement." | accept; drain first | **accept** | in-flight flows fail closed | R, A | release notes of F |
| D-8b | "Rahi refuses every bearer token issued at or before a permanent, only-rising floor instant, raised at every cache replacement and every restore; admission separately refuses a token without `iat` or whose `exp - iat` exceeds the manifest's L; revocation rows are pruned only after `V(L*)`." | every event; upgrade and migration only (restore per Rahi P-7); accept replay | **every event** | every access token issued before the event is refused from then on, whatever its lifetime; native clients refresh. The floor needs no lifetime, historical or read back (Rahi 043 revision 2, B-6, B-6b, P-9) | R | F (Rahi), G |
| D-8c | "After a restore from anything but the final offline export, every Rauthy session and refresh token is invalidated before activation; signing-key rotation for other relying parties is decided per incident." | invalidate; invalidate and replay; invalidate and rotate keys; accept | **invalidate; rotation per incident**, never described as immediate (14.3) | users log in again; other relying parties stay exposed until their JWKS refresh or token expiry | R, A | DR restore acceptance in G; not N1 |
| D-8d | "Active manual IP bans are exported after quiescence and re-applied before activation, at migration, restore and the Track N1 upgrade." | export and re-apply; accept loss | **export and re-apply** | needs a Rauthy capability (section 8); until it exists the default is loss, stated | H, R, A | F (N1) only if chosen for the upgrade; G |
| D-8e | "Automatic abuse state (bans, escalation counters, stuffing windows) is accepted as lost, with the exposure stated." | accept; ask Rauthy to persist | **accept**, revisit if restores become routine | about 25 extra guesses per IP until the 24 h tier returns | R, A | release notes of F and G |
| D-9 | "Recovery objectives are targets: migration RPO 0 for writes acknowledged before quiescence, under the conditions section 13 records as accepted; node loss RPO 0 under `Immediate`; DR RPO the archive interval." | adopt as targets; set other numbers | **adopt as targets**, revisit after stage 9 | the zero-loss wording carries 13.7's historical assumptions unless receipts exist | H, S | G |
| D-10 | "No hiqlite version change inside a migration window." | adopt; allow | **adopt** | two variables never share one outage | H, S | G |
| D-11 | "The supported upgrade is a full stop unless a version pair is qualified together." | adopt; rolling by default | **adopt** | every upgrade is a planned outage by default | H | G |
| D-12 | "The migration export uses rule A's refusals and the clean-stop marker, plus a committed barrier per cluster after quiescence and drain; durable commit metadata is optional; hot archives are DR inputs only with the restore validator." | A only; A + B; A + B + C; hot backup | **A + B, C optional**, with 13.7's limits stated: B proves the export faithful to the cluster at barrier time, not that nothing was lost before it | needs `034` B-6, a Rauthy route to it, and a consumer step | H, R, A | C2, C3, G |
| D-13 | "Keep or remove `openraft/loosen-follower-log-revert` from `cache`." | keep; remove | **measure in stage 4, then decide** | removing restores openraft's panic on a reverted follower | H | B's F-121 item, after E's stage 4 |
| D-14 | Decided 2026-09-23: two StatefulSets at N=3, one hiqlite node per pod; supervised single container at N=1. | | | | H | |
| D-15 | "Migration safety rests on five separate controls (quiescence, workload exclusion, storage locking, replication isolation, application authority), and the target becomes authoritative at its first authoritative mutation, not at the routing switch." | adopt; the earlier single-fence model | **adopt** | the tombstone becomes a downstream feature on every startup path (7.2) | H, R, A, S | F (tombstone, background writers), G |
| D-16 | "Two tracks: N1 adopts published patched builds at N=1 now; S7 adopts only a release that passed stages 4 to 6." | adopt; one track | **adopt** | Track N1 never counts as N=3 evidence and never waits for it | H, R | nothing technical; it confirms that A and F-N1 do not wait for B to E |
| D-17 | "hiqlite adds a public storage-exclusion handle that a consumer acquires, moves the legacy cache through, and hands to start (`035` B-6)." | add it now; add it when a consumer asks; never | **when a consumer asks** (changed 2026-09-23, third pass) | adds one public module to `027` B-7's frozen surface. Rahi 043 revision 2 no longer needs it: it holds no hiqlite lock and fences its app store's old path (the H-7 answer in the Rahi handoff); no other consumer has asked | H | A2 only; A1 is implemented without it |
| D-18 | "The `035` repair ships alone, as `0.15.0-patched.2` of `hiqlite-patched`, `hiqlite-wal-patched` and `hiqlite-derive-patched`, under `031`'s qualification." | this label and alone; bundled with N=3 repairs; a minor bump | **alone**, the label once its availability on crates.io is confirmed at publication time (the release preparation in the handoff checks it) | consumers keep `=0.15.0-patched.1` until each decides to move; Rauthy's image needs its own rebuild | H | A's publication, not its implementation |
| D-19 | "hiqlite builds a producer-side layout fence so that an unmodified hiqlite 0.14 refuses a 0.15 directory before it writes (section 17)." | build it, qualified against the real 0.14 binary; state the unsupported downgrade only | **state the unsupported downgrade only**, for now | a layout change every consumer's paths and backups must follow, and 0.14's pre-lock phase (restore, reset) stays destructive under any layout; the supported way back stays the pre-upgrade archive in a fresh volume | H | nothing in A; a later lane if adopted |

## 12. D-14: the N=3 cell is two StatefulSets

**Decision.** Recorded 2026-09-23, owner. At N=3 the cell is one namespace
holding two StatefulSets, `rahi` and `rauthy`, each of three replicas, each pod
running exactly one hiqlite node of one cluster. N=1 keeps the single-container,
supervised composition of Rahi 031 unchanged, as the local profile Aicortex and
`docker compose` run.

**Why.** It removes the two sequential hiqlite shutdowns inside one grace
(section 4), a Rauthy fault costing Rahi a voter (section 5), coupled rollouts
and upgrades, and one memory limit shared by two processes. Rauthy's own HA
shape is a separate StatefulSet (`rauthy-helm-worktree`). What it does **not**
change is the node-failure arithmetic; section 5 says why.

**What it replaces.** The single-container design gave four things for free.
Each needs an explicit replacement, codified here so the downstream specs carry
all of it. They are obligations of the owning repositories (Rahi, Statecraft,
and whichever repository deploys the cell), proposed from here, not made here.

**12.1 Routing and trust boundary** (replaces loopback).

- Rahi MUST address Rauthy through an internal ClusterIP Service,
  `rauthy-internal.<namespace>.svc.cluster.local`, never a pod address and
  never the public ingress. The Service is internal only: no ingress, no
  `LoadBalancer`, no `NodePort`.
- That traffic MUST be encrypted, by Rauthy's native TLS or by a service mesh's
  mTLS where one is present. A mesh qualifies only with a shutdown order that
  keeps its proxy up until the application's hiqlite shutdown has finished
  (section 4). The mechanism is recorded in the downstream spec, and a capture
  showing no plaintext on the path is part of stage 8.
- A NetworkPolicy MUST admit ingress to Rauthy's HTTP port **only** from pods
  carrying Rahi's workload labels, plus the public path if Rauthy serves users
  directly. Rauthy's hiqlite ports (8100, 8200) admit only Rauthy's own pods;
  Rahi's (8300, 8400) only Rahi's own pods.
- Rahi's administrative calls to Rauthy (the backup verb's `POST /backup` with
  the admin token, Rahi 030 B-5) cross the same encrypted, policy-restricted
  path. The admin token is not sent to any other address.
- NetworkPolicy is access control, not encryption. hiqlite's own raft and API
  traffic inside each StatefulSet still needs D-5, which stays pending.

**12.2 Readiness and liveness** (replaces die-together).

- **Liveness** MUST NOT depend on Rauthy. An unreachable Rauthy never restarts a
  Rahi pod, so a Rauthy outage never becomes a restart storm and a round of
  elections in Rahi's groups.
- **Readiness** MUST fail while Rahi cannot reach Rauthy through the internal
  Service, which takes Rahi out of the client-facing Service instead of serving
  errors, and returns when Rauthy is reachable again.
- **Startup** MUST NOT wait on Rauthy either: today's supervisor waits up to
  60 s for Rauthy's health before serving (Rahi 031); at N=3 a startup probe
  gated on Rauthy would turn the same outage into restarts.
- Peer discovery MUST NOT depend on readiness. Each application's headless
  Service sets `publishNotReadyAddresses: true`, as Rahi's `deploy/k8s/service.yaml`
  already does, so a Rahi pod that is not ready because Rauthy is away stays
  addressable to its Raft peers, and a bootstrapping pod is addressable before
  it is ready. The client-facing Service is a separate object that honors
  readiness.
- Rauthy's own readiness stays its hiqlite `/ready`.

**12.3 Backup consistency** (replaces the single-volume cut).

Corrected 2026-09-23 (section 15): the earlier text overstated what a stop and
a lock prove.

- An export always produces **two logical cluster images**. From a split cell
  their sources are **six PVCs**, three per StatefulSet; section 13.5 says which
  replicas are read and which one is selected.
- An export that must be coherent across both clusters (a reverse migration, an
  offline DR archive from a split cell) MUST scale **both** StatefulSets to 0,
  exclude every workload that could restart them (7.2 control 2), confirm every
  pod gone and every stop's outcome (section 4), and hold every relevant storage
  lock from before the first read until both images and both tombstones are
  written (7.2 control 3, B-1, B-2).
- What that establishes: no write reached either raft log during the export
  window. What it does **not** establish: that each image is its cluster's
  committed state (that is section 13's rule, or its barrier alternative), or
  that the two images are consistent with each other at the application level
  (that is quiescence before the stop, and the consumers' cross-store check).
  The Statecraft and Rahi specs cite exactly this, not "the single-volume
  guarantee".
- The cell archive (Rahi 030 B-5) becomes an archive of two cluster images plus
  the keys, with one manifest naming both images, both applied log ids, both
  digests and any barrier receipts (D-7).
- A **hot** archive of a split cell, taken while serving, is two images at two
  instants. Their log ids cannot be compared; the archive is a qualified restore
  input only with the restore validator of section 14.4 (D-12).

**12.4 What stays.** The N=1 profile: one container, `rahi supervise`, loopback,
die-together, one volume, one archive. The migration protocol of section 7,
whose source is always an N=1 cell: only its target side changes (R-1, R-3, R-4,
V-1). The hiqlite plan: stages 2 to 6 are layout-independent, and the harness
runs both layouts (`033` B-2).

**12.5 Fallback, not chosen.** Rauthy as a native sidecar container in the Rahi
pod keeps loopback and one volume, gains per-container limits and an ordered
shutdown, and keeps coupled rollouts. Recorded so a later reader sees it was
weighed.

## 13. Export currency: a proposed rule, its safety argument, and alternatives

Proposed, not settled (D-12 pending). It replaces `034` B-2's comparison of the
applied log id with "the last committed entry in the raft log", which cannot
be implemented as written (F-124).

**Corrected 2026-09-23 (second pass).** The first version of 13.7 said a barrier
"detects storage rollback, asynchronous-sync loss and a wrong source". It detects
them only after the barrier commits; a loss before it is invisible to it, which a
bounded probe demonstrated (F-132). 13.5 is restated against the fields a stopped
directory actually holds, 13.10 defines the clean-stop marker, 13.11 separates
two barriers from a cross-store transaction, and 13.12 summarizes what each
combination proves.

**13.1 What must be preserved.** An **acknowledged write** is a client write for
which hiqlite returned success. hiqlite returns it after openraft's
`client_write` resolves, which is after the entry is committed (a quorum of the
membership in effect reported the append complete) and applied on the leader.
What "reported complete" means is the `LogSync` mode's: under
`LogSync::Immediate` the entry is on stable storage; under
`LogSync::ImmediateAsync` and `IntervalMillis` writeback was requested or
deferred, and the entry is not known to be durable (`001` section 3). The
migration's zero-loss objective (D-9, pending) is: every write acknowledged
before Q-1 is in the exported image.

**13.2 Facts the rule rests on** (read at source, `72e09a6`, unchanged at
`ec8fc6d`; F-132 executed):

- **F-124.** The committed log id is not persisted: `hiqlite-wal`'s log store
  does not implement `save_committed` / `read_committed`, so openraft's no-op
  defaults apply. There is **no persisted committed position** in any directory
  today, and no rule below compares against one.
- **F-125.** The SQLite state machine's persisted applied log id (`_metadata`)
  is written when a snapshot is built and when the SQLite writer exits, not per
  applied entry. It is evidence only together with evidence that the stop which
  wrote it was the clean stop of the run that made the directory's last writes
  (13.10). Without that, it can be behind what the database holds.
- A clean WAL writer exit performs a blocking flush of the active WAL file and
  writes its metadata (`hiqlite-wal/src/writer.rs:719-723`), so a **confirmed**
  clean stop (section 4) makes every appended entry durable, whatever the mode.
- **What a stopped directory holds, exactly.** From the WAL: the vote (term,
  node, committed flag), the last purged log id, the last log id, and the
  entries between them, membership entries included. From the SQLite
  `_metadata`: `last_applied_log_id`, `last_membership` with its log id, and
  `last_snapshot_id`. Nothing else: no committed id (F-124), no cluster identity
  (`034` KD-3), and no record of how the last stop ended until 13.10 exists.
- There is no persisted cluster identity (`034` KD-3). Membership node ids and
  addresses are the only identity a data directory carries today.

**13.3 Refusal cases common to every directory read.** The export refuses,
naming the case, unless all of these hold:

- *R-a identity:* the directory's latest membership (log and state machine)
  names exactly the expected node ids and addresses of the source cell, and,
  once `034` B-1 exists, its cluster id matches. Until then identity is weak and
  the manifest says so.
- *R-b confirmed clean stop:* a clean-stop marker that satisfies 13.10. Until
  hiqlite writes one, the evidence is the consumer's recorded `Ok(())` for that
  stop, which is weaker: it is outside the directory and bound to nothing in it.
  An unconfirmed or forced stop is refused; the remedy is one confirmed start and
  stop.
- *R-c complete log ids:* vote, last purged log id, last log id and the applied
  log id are all readable, and the WAL needs no repair (`021` B-8's torn-header
  case refuses rather than auto-heals).
- *R-d frontiers:* last purged ≤ applied ≤ last log id, in openraft's `LogId`
  order. An applied id beyond the last log id, or below the purge frontier, is
  refused.
- *R-e stable membership:* the latest membership entry in the log equals the
  state machine's last applied membership, and it is a uniform (non-joint)
  configuration. A joint configuration, or a membership entry in the
  uncommitted tail, is a change in flight and is refused.

**13.4 The N=1 rule.** The configuration is `{1}`; R-a to R-e hold; and
`applied == last log id`, else refuse (an uncommitted tail); the remedy is one
confirmed start and stop, after which the single leader has committed and
applied its whole log.

*Safety argument.* A single voter commits an entry when its own append
completes, and hiqlite acknowledges only committed, applied entries. Every write
acknowledged before Q-1 was therefore appended to this node's log before the
stop. The confirmed clean stop's blocking flush made it durable (13.2), R-b makes
the persisted applied id accurate (F-125), and `applied == last log id` means the
state machine reflects every entry in the log. So the image contains every
acknowledged write, and possibly also unacknowledged tail writes committed by
the remedy's restart, which the application issued.

*What it preserves, precisely:* every write acknowledged before Q-1, **provided**
(i) storage honoured the flushes and was not rolled back (a restored disk
snapshot or a lying write cache is invisible to local evidence), and (ii) under
`ImmediateAsync` or `IntervalMillis`, no host crash or power loss hit the node
between an acknowledgment and the clean stop's flush. Under (ii)'s failure an
acknowledged write can be absent, the node restarts consistent but shorter, and
nothing local detects it. (i) and (ii) are assumptions no local evidence tests,
and a barrier does not test them for the time before it was written (13.7).

**13.5 The N=3 rule** (for exports **from** a split cell: a reverse migration or
an offline DR archive; the migration of section 7 exports from N=1). Restated
against 13.2's fields.

- *Read set.* At least a majority of the voters of C, where C is the uniform
  configuration that every read member reports both as its latest log
  membership and as its applied membership (R-e). Learners do not count. Every
  member passes R-a to R-d and carries a 13.10 marker.
- *Order.* Log ids compare in openraft's `LogId` order (the leader id's term
  first, then index), never by index alone: a member that led a stale term can
  hold an uncommitted tail with a higher index and a lower log id.
- *Selection.* M is the greatest last log id in the read set. Select a member
  whose applied log id equals M. Refuse if none exists or if fewer than a
  majority of C are readable. The remedy is a confirmed start and stop of the
  cluster, so a leader commits or truncates every tail, then a new export.

*Safety argument.* The state machine applies only committed entries, so a
member's applied id, accurate by R-b, is committed, and M, applied by the
selected member, is committed. Every entry committed while C was in effect was
appended by a majority of C; the read set intersects that majority, so the entry
is at or below some read member's last log id, and so at or below M. Committed
entries form one prefix (openraft's log matching and leader completeness), so an
entry at or below M in that prefix is in the selected member's log up to M, and
the selected member applied all of it. Entries committed before C precede C's
entry in the same prefix. A purged prefix counts: an entry at or below a member's
purge frontier is in its snapshot and below its last log id. A later
configuration cannot have committed unseen, because joint consensus needs a
majority of C too, which the read set intersects; a member reporting it fails
R-e. The committed flag of a vote says a leader was granted a term, not what it
committed, so it is not used.

*What it preserves:* every write acknowledged before the stop, provided (i)
storage was not rolled back on any replica, and (ii) under `ImmediateAsync` or
`IntervalMillis`, no replica suffered a host crash or power loss during the
cluster's last run. A replica that lost an acknowledged tail can rejoin silently
(F-121), so under (ii)'s failure the majority argument does not hold and nothing
local detects it. Under `LogSync::Immediate`, (ii) is not needed. `034` B-2 lets
the implementing change either apply this rule or refuse multi-voter directories
and leave selection to an operator procedure.

**13.6 Why rule A alone does not establish zero loss.** Storage rollback and
asynchronous-sync loss are invisible to any local evidence, and identity is weak
until `034` B-1. So the zero-loss objective holds under rule A only if the owner
accepts those conditions (for the source's last run: `LogSync::Immediate`, or no
host crash, and no storage rollback). This is **not** silently weakened: D-9's
migration RPO stays "0 for acknowledged writes" only together with either those
recorded conditions or the independent evidence of 13.7.

**13.7 Alternative B: a consumer-coordinated committed barrier.** After Q-1 and
the in-flight drain of 13.11, and before Q-3, one barrier write per cluster
through the ordinary client path, carrying a fresh random nonce; the
acknowledgment and nonce are recorded outside the cell (the migration record)
before any workload is excluded. The export refuses an image whose state does
not contain the nonce.

*What inclusion proves.* openraft appends in order, and the image's log up to the
barrier is, by log matching, the log the cluster held when the barrier
committed. An image containing the nonce is therefore faithful to the cluster
**as it stood at barrier time**: nothing in that prefix was lost, rolled back or
truncated afterwards, during the stop, or on the way to the image; the image is
not another cluster's, nor an older copy of this one (neither can contain a
fresh nonce); and the export did not stop short of the barrier.

*What it does not prove.* That the cluster at barrier time still held every write
acknowledged before it. A write lost earlier, by a storage rollback, by an
asynchronous-sync loss on the single voter, or at N=3 by a reverted follower
elected leader (F-121), leaves a shorter log, and the barrier is appended to it.
**Demonstrated** (F-132): rolled back before the barrier, the barrier check
passed with an acknowledged write missing; rolled back after it, the check
refused.

*Historical assumptions that remain*, for writes acknowledged before the
barrier: (i) no storage rollback between their acknowledgement and the
barrier's commit; (ii) under `ImmediateAsync` or `IntervalMillis`, no host crash
or power loss in that interval on a voter that then counted toward a later
commit (at N=1, the node); (iii) at N=3, no reverted follower elected in that
interval. Under `LogSync::Immediate`, (ii) and (iii) reduce to the storage
honouring its flushes, and (i) remains.

*Independent evidence a stronger RPO claim would need*, recorded outside the
cell's failure domain while the cell runs, not reconstructed at export
(corrected 2026-09-23, third pass: (b) as first written required only the last
recorded barrier, which by F-132's argument detects nothing before it):
(a) **per-write receipts**: before a consumer reports success to its own caller,
it records off-cell the log id of the write (term and index) or an application
sequence with a hash-chain head, and the export requires the image to contain
the last receipt's entry at the same term; hiqlite's write path returns a row
count, not a log id (`client/execute.rs:22`), so (a) needs a producer change or a
consumer-side sequence; (b) **recurring recorded barriers**, whose exact claim is
13.13; (c) `LogSync::Immediate` on storage attested not to roll back, which
replaces (ii) by an infrastructure assumption, not by evidence.

*Cost:* a hiqlite entry point that records the nonce in the replicated state
machine (`034` B-6, proposed), because Rahi does not own Rauthy's schema; a
Rauthy-side way to call it for Rauthy's cluster (section 8); and a consumer step.

**13.8 Alternative C: durable commit metadata.** Implement `save_committed` in
`hiqlite-wal`, with a defined ordering: a committed id is persisted only after
every entry up to it is durable in the local WAL, and never ahead of it,
flushed with the same mode as appends. Offline it is a lower bound on the
cluster's commit (a follower learns commit late). It makes a corrected `034` B-2
check (`applied ≥ persisted committed`) implementable as a **necessary**
condition and strengthens R-d. **It does not detect storage rollback**: a
rolled-back directory carries a committed id consistent with its rolled-back
log. Nor does it detect a loss before the committed id was persisted, and it
does not replace the majority rule.

**13.9 Recommendation, pending the owner.** Rule A's refusal cases (13.3) and
rules (13.4, 13.5) as mandatory checks; the 13.10 marker as R-b's evidence;
alternative B as evidence that the export is faithful to the cluster at barrier
time; alternative C optional. A migration RPO of 0 for acknowledged writes is
stated only together with 13.7's historical assumptions, recorded as accepted by
the owner (D-9), or with receipts (13.7 (a)). For the zero-loss committed-write
objective, the source's SQLite groups run `LogSync::Immediate` (D-4) at least for
the source's last run before the window, at the cost of an fsync per append
batch, to be measured before it is required in production.

**13.10 The clean-stop marker** (`034` B-7, proposed).

- *Identity.* Each start generates a random run id after exclusion (`035` B-1).
  The marker records the run id, node id, raft group, hiqlite version, and the
  end state the stop left: vote, last purged, last log id, the state machine's
  applied log id and last membership log id, a digest of the WAL metadata and
  the active WAL file's id and length, and a digest of the SQLite `_metadata`
  row; plus a digest over the marker itself. The data directory path is
  diagnostic only.
- *Durability.* Written as the stop's last act, after the WAL writer's flush and
  metadata, the SQLite writer's metadata persist and the database's close: a
  temporary name, fsync, rename, fsync of the directory. The stop reports
  `Ok(())` only after the marker is durable; a failed marker write makes the stop
  unconfirmed (section 4).
- *Invalidation.* The next start, after exclusion and **before its first write**
  to the WAL or the database, removes the marker (or renames it to a consumed
  name) and fsyncs the directory. So a marker never coexists with a later run's
  writes, and a run that ends without a clean stop leaves no marker, which R-b
  refuses.
- *Verification.* The export requires the marker, its digest, and equality
  between every recorded field and what it reads from the directory. Any
  difference refuses R-b.
- *Limit.* The marker proves that the directory's durable state is the state the
  named run's confirmed stop left, unchanged since. It does **not** prove absence
  of rollback: a restored copy of the whole directory carries a marker that
  describes the copy correctly. It does not prove the run lost nothing before the
  stop (13.7's assumptions). Detecting a whole-directory rollback needs a record
  outside it: the run id and end state recorded by the consumer at the stop and
  compared at export, which is 13.7 (b) applied to the stop.

**13.11 Two barriers are not a cross-store transaction.** A cell has two
clusters, and a barrier covers one. The pair gives each image every write
acknowledged in its own cluster before its own barrier committed, under 13.7's
assumptions, and **no atomicity across the two**: a request that writes to both
stores can be in one image and not the other if it straddles a barrier. A
coherent archive needs, in this order:

1. *Quiescence* (Q-1): every entry point that can mutate either store refuses
   new mutating requests, including requests Rahi proxies to Rauthy and any path
   by which Rauthy serves users directly.
2. *In-flight drain:* every request admitted before quiescence has completed or
   failed, observed by the consumer's own counters and not by elapsed time,
   before the first barrier is issued. A request still in flight when a barrier
   is issued may land on either side of it.
3. *Background writers* held off (7.3) before the first barrier, or each one's
   writes stated as independent of the other store; a background write after a
   barrier is outside that image.
4. *Both barriers*, then both receipts recorded, before Q-3.
5. *Consumer validation:* the cross-store invariants validator (14.4) runs on the
   pair before activation, because only the consumers know which facts must be
   in both images.

hiqlite claims nothing about the pair; its evidence is per cluster.

**13.13 What recurring external barriers prove between checkpoints** (third
pass). Barriers `b_1 .. b_k` committed through the ordinary write path during
the cell's life, each nonce and commit acknowledgment recorded off-cell before
the next is issued, the barrier rows never deleted from the replicated state,
and the export requiring **every** recorded nonce to be present in the image (or
a hash chain over them whose head is recorded off-cell).

- *Proved.* No rollback, truncation or substitution of the log **crossed a
  recorded barrier**: a loss that removed `b_i` from the log leaves every later
  barrier appended to a log without it, and the image then lacks `b_i`. So a
  whole-directory restore of an older copy, a reverted follower elected over a
  barrier, or asynchronous-sync loss reaching back past a barrier is detected,
  and the image is faithful to the cluster as it stood at each `b_i`.
- *Not proved.* That every write acknowledged **between** two consecutive
  barriers survived. A loss confined to the interval between `b_(i-1)` and the
  loss event, with no barrier inside the lost range, leaves every recorded nonce
  present (F-132's shape, repeated per interval). Undetected loss is therefore
  bounded by one inter-barrier interval **per loss event**, not by the time
  since the last barrier before the export, and only if every nonce is checked.
- *Not proved either.* Anything about the other cluster (13.11), or about a
  barrier whose off-cell record was lost (then that interval is merged with the
  next).
- *What the interval buys.* The owner can state migration or DR RPO as "no loss
  crossing a recorded barrier; at most one barrier interval of acknowledged
  writes under 13.7's assumptions (i) to (iii)". A per-write claim still needs
  per-write evidence, 13.7 (a); periodic barriers never become it.

**13.12 What an export proves, by combination.**

| evidence | faithful to the cluster at export | faithful at barrier time | every acknowledged write present |
|---|---|---|---|
| rule A with the consumer's recorded `Ok(())` | only if nothing changed the directory after that stop, which nothing checks | no | only under 13.4/13.5 (i) and (ii) |
| rule A with the 13.10 marker | yes, for the directory as the marked stop left it; not against a whole-directory rollback | no | only under (i) and (ii) |
| plus the barrier (B) | yes | yes | only under 13.7's historical assumptions |
| plus durable commit metadata (C) | strengthens R-d | as B | unchanged |
| plus recurring recorded barriers (13.13), every nonce checked | yes | yes, at every recorded barrier | no loss crossing a barrier is undetected; within one interval only under (i) to (iii) |
| plus receipts (13.7 (a)) | yes | yes | yes, up to the last receipt, independently of (i) to (iii) |

## 14. Security state across migration, restore and upgrade

Two different threats, often conflated.

- **Cache replacement.** The cache groups start empty: after a migration or a
  restore (images hold only SQLite), and after Track N1's 0.14 to 0.15 upgrade
  move-aside. It removes state that lives **only** in a cache.
- **Stale backup.** The restored SQLite image is older than the source's last
  state: any hot archive, or any archive other than the final offline export. It
  **rolls back** SQLite state, including revocations.

The section 7 migration from a final offline export has the first and not the
second. A hot DR restore has both. The Track N1 upgrade has the first only.

**14.1 Cache replacement.** Read from source (Rahi at `b815b18`; Rauthy at
`ccf2250`, verified at three points):

| consumer | state lost | consequence |
|---|---|---|
| Rahi | bearer deny-lists by `jti` and by subject instant (Rahi 038 B-5, 025 B-5), as of `b815b18` | a revoked bearer token is accepted again until it expires. Rahi 043 (draft, `5707f60`) proposes durable SQL revocation for future revocations (B-6) and a transition floor for those only in the 0.14 cache (B-6b), each for V = L + 120 s with L read back from Rauthy (a manifest default L = 600 gives V = 720 s) |
| Rahi | rate-limit counters, session assertions | counters reset (not fail-closed); renewal round trip |
| Rauthy | IP blacklist: manual admin bans, brute-force bans (60 s to 24 h), credential-stuffing and scan bans | all lifted at once; manual bans have no default bound |
| Rauthy | failed-login escalation counter per IP (no TTL) | about 25 extra guesses per IP before the 24 h tier returns |
| Rauthy | credential-stuffing window (3 h), device and dynamic-client IP limits, email throttle | minor, bounded by their windows |
| Rauthy | auth codes, device codes, PoW, WebAuthn challenges, PAM tokens | vanish; in-flight flows fail closed; nothing becomes replayable |

Rauthy's sessions, refresh-token deletions, `issued_tokens.revoked` and disabled
users and clients live in SQLite. They are **protected from cache replacement,
not from database rollback.** Release builds clear only the `Html` and `App`
caches on start (Rauthy `src/bin/src/server.rs:239-241`), so bans survive
ordinary restarts and are lost only by these events.

**14.2 Stale backup.** Everything written to SQLite after the capture is undone,
including Rauthy session deletions, refresh-token deletions,
`issued_tokens.revoked`, disabled users and rotated client secrets, and Rahi's
SQL-side state.

**Outstanding access tokens** are a separate case in both threats. Relying
parties that validate a JWT locally, Rahi included, accept it until `exp`;
Rauthy's `issued_tokens.revoked` is consulted only at Rauthy's own token-info,
userinfo and token-exchange endpoints. After a stale restore, tokens issued
after the capture are unknown to the restored Rauthy yet carry valid signatures
until they expire, and tokens revoked after the capture read as unrevoked.

**14.3 Proposed controls: five separate decisions.** Corrected 2026-09-23
(second pass): each item below is its own owner decision with its own options.
Accepting one accepts nothing else, and in particular **accepting empty caches
(D-8a) accepts no security loss** (D-8b to D-8e). Six things are kept apart:

| | what | threat | covered by |
|---|---|---|---|
| 1 | functional cache loss: in-flight flows, assertions, performance caches | cache replacement | D-8a |
| 2 | Rahi's bearer floor: every access token presented **to Rahi** | both | D-8b |
| 3 | revived Rauthy sessions and refresh tokens, and `issued_tokens.revoked` rolled back | stale backup only | D-8c |
| 4 | other relying parties' outstanding access tokens | both | D-8c, option (iii) only, with its limit below |
| 5 | manual IP bans | cache replacement | D-8d |
| 6 | automatic abuse controls: bans, escalation counters, stuffing windows | cache replacement | D-8e |

- **D-8a functional loss.** Options: (i) accept, stated in release notes; (ii)
  require a drain of in-flight flows before the window. Recommend (i): every
  item fails closed (14.1).
- **D-8b Rahi bearer floor.** Refuse every bearer token whose `iat` is at or
  before the floor instant, and any token without `iat`, for V after it, with V
  from the lifetime Rahi reads back from Rauthy (Rahi 043's V = L + 120 s), fail
  closed until that read succeeds. Set as a preparatory mutation (7.3) at the
  planned activation, and at the Track N1 transition (Rahi 043 B-6b). Options:
  (i) the floor at every cache replacement and every restore; (ii) the floor at
  upgrade and migration only, and at a restore only when Rahi 043's P-7 is
  accepted; (iii) accept the replay window. Recommend (i). It covers tokens
  presented to Rahi only.
- **D-8c Rauthy stale-backup revocation.** For a restore from anything but the
  final offline export. Options: (i) invalidate every Rauthy session and refresh
  token before activation (a Rauthy capability; section 8); (ii) (i) plus a
  replay of revocations recorded outside the archive, if Rauthy offers one; (iii)
  (i) plus signing-key rotation for other relying parties; (iv) accept, with the
  exposure stated. Recommend (i), with (iii) as an owner call per incident.
  **Key rotation is not immediate invalidation.** A relying party that caches
  Rauthy's JWKS keeps validating tokens signed by the old key until it refreshes
  that cache, and it will refetch only when it sees an unknown `kid` or its cache
  expires; if Rauthy keeps publishing the retired public key for a grace period,
  old tokens stay valid for that period too. The bound is the smaller of a
  token's remaining lifetime and each relying party's JWKS refresh behavior,
  which this fork does not know and does not control. A claim of immediate
  invalidation needs, per relying party, a stated cache policy or an explicit
  refresh, and Rauthy's removal of the old key from JWKS at rotation.
- **D-8d manual IP bans.** Options: (i) export the active manual bans through
  Rauthy's admin API after Q-1 and re-apply them as a preparatory mutation; (ii)
  accept their loss, stated. Recommend (i). For Track N1 this applies to the 0.14
  to 0.15 move-aside too, where it needs a Rauthy capability (section 8) or
  becomes (ii) by default.
- **D-8e automatic abuse state.** Options: (i) accept with the exposure of 14.1
  stated; (ii) ask Rauthy to persist it. Recommend (i) for migration and the
  Track N1 upgrade; revisit if restores become routine.

**14.4 Acceptance, unexecuted.** A bearer revoked in the source before Q-1 is
refused after activation, and one issued after activation is accepted; an access
token issued before the floor is refused at Rahi; if D-8d(i), a manual ban set in
the source is enforced after activation; for a hot-archive restore with D-8c, a
revocation made after the capture is effective after the restore. The **restore
validator** for skewed hot archives (D-12): cross-store invariants with a defined
action each (for example every principal `sub` Rahi references exists in Rauthy,
else quarantine; every declared client exists, else re-provision; ledger
references to identity events resolve, else record a gap), run before
activation, with restores where Rauthy's image is older and newer than Rahi's
and a revocation made between the two captures. The two clusters' log ids share
no clock, term or index, and are recorded for provenance only; compatibility is
established by the validator, never inferred from them or from an RPO.

**14.5 A separate Rauthy observation, not about caches.** Read from source only,
Rauthy at `ccf2250`: `DPoPNonce::is_valid` (`src/data/src/entity/dpop_proof.rs:54-58`)
performs `DB::hql().get(Cache::DPoPNonce, value)`, which yields
`Result<Option<Self>, hiqlite::Error>`, and returns `slf.is_ok()`. A lookup that
finds no entry returns `Ok(None)`, so the function returns `true` for a nonce
that was never issued. Whether any caller relies on this function alone, and
whether it is exploitable, was not tested or assessed. For the Rauthy
maintainer; not part of D-8.

## 15. Reconciliation with Rahi (2026-09-23)

**First pass**, against Rahi's "decision packet 1: patched dependency adoption"
(D1 to D5, Rahi `b815b18`): changed sections 1, 4, 7, 8, 9, 11, 12.3 and added 13
and 14.

**Second pass**, against Rahi 043 as reviewed at `5707f60` (branch
`rahi-wt-043`, draft, not approved), Rahi 044 at `ecd28cc` (draft), and Rahi's
producer requests and notes, which were uncommitted in its adoption session.
What changed on Rahi's side since the first pass, as proposals awaiting
implementation and measurement, not facts about a running cell:

- V, the accepted validity of a bearer token, is `L + 120` s with L read back
  from Rauthy (038 D-7's 60 s leeway on `exp` and `nbf`); the manifest default
  `L = 600` gives the example `V = 720` s. The first pass's "600 s against 038's
  1800 s" comparison is superseded.
- The stop budget is composed from configured phases: `SERVE_GRACE` 40 s and the
  pod grace 50 s (section 4). The first pass's "ten-second store allowance" is
  superseded.
- The cache transition is a resumable verb with a verified pre-upgrade archive,
  durable SQL revocation and a transition floor (043 B-4 to B-6b).
- Rahi reproduced, and this pass reproduced again (`035` section 3), that
  hiqlite 0.15 moves a live 0.14 node's cache before excluding it (F-126).

This pass changed sections 4, 8, 9, 11, 13 and 14, and added 16. Decision state
after it: **D-14 decided**; D-1 to D-13, D-8a to D-8e and D-15 to D-18 pending
(section 11). The handoff to Rahi is `standards/spec/n3-rahi-reconciliation-handoff.md`;
the request to Rauthy's maintainer is `standards/spec/n3-rauthy-request.md`.

**Two adoption tracks, kept apart (proposed D-16).**

| | Track N1 | Track S7 |
|---|---|---|
| what | Rahi and the cell's Rauthy image adopt the **published** `hiqlite-patched 0.15.0-patched.1` and `rauthy-patched:0.36.2-patched.2` | Rahi and Rauthy adopt a later release that passed hiqlite stages 4 to 6 |
| topology claimed | N=1 only | N=3, only after stage 8 |
| owner | Rahi (043, draft) | Rahi and Rauthy |
| depends on this work | no; the `035` repair reaches it only by a separate re-pin | yes |
| produces | N=1 consumer evidence and early composition findings | the qualified cell |

Track N1's findings reached this proposal: the shutdown budget (section 4), the
cache transition's revocation loss (section 14), and the upgrade exclusion
hazard (`035`).

**Third pass (2026-09-23)**, against Rahi 043 revision 3 (`c2c7c72`, review
`abd66fd`, owner packet `fc3f339`), Rahi's producer requests version 2 (H-1 to
H-8), and Rauthy `d7d087aa` (unpublished). What changed, and what this pass did:

- **Rahi holds no hiqlite lock.** 043's T0 takes `cell.lock` and
  `transition.lock` only; the app store moves to `<data>/app-store` behind a
  permanent fence at `<data>/hiqlite`. The T0 to T3 conflict of the second pass
  is gone, and D-17's handle is no longer on Rahi's path (section 11). The fence
  route was reviewed adversarially (H-7; the Rahi handoff section 3): sound for
  its population under configuration exclusions 043 does not yet state.
- **The floor is permanent** and only rises; admission enforces the manifest's
  lifetime ceiling; pruning uses its own horizon. The second pass's "V = L + 120
  with L read back from Rauthy" is superseded (D-8b restated).
- **Rauthy's release assets exist**: eight, listed by authenticated and anonymous
  reads, binary digests equal to Rahi 043 D-P1. A verification task, not a gap.
- **Lane A1 is implemented as an unreleased candidate** (`035`, section 16), and
  F-130 is on Rahi's release gate (043 B-4b), so A1's release is Rahi's critical
  path, not D-17.
- Recurring barriers are specified (13.13), with 13.7 (b) corrected.
- Rauthy's handoff: the notice in `standards/spec/n3-rauthy-request.md`, third
  pass, names what its corrections still get wrong.

## 16. Dependency graph, critical path and lane authorizations

Proposed 2026-09-23. Seven nodes. An arrow is a prerequisite, not a schedule; a
node starts when its own prerequisites and authorization exist, whatever the
state of nodes it does not depend on. **No node below is authorized by this
document.**

```
A1 N1 exclusion repair ──► A-rel release ──► F-N1' re-pin (Rahi), rebuild (Rauthy)
A2 exclusion handle (D-17) ──┘
B  N=3 start/join/shutdown repairs ──┐
C1 identity, callable restore ───────┼──► candidate ──► E bounded qualification ──► F-S7 adoption ──► G
C2 export rule + clean-stop marker ──┤        ▲                                         ▲
C3 barrier (D-12) ───────────────────┘        │                                         │
D  real-node harness ─────────────────────────┴──► E                  7c candidate integration (early)
F-N1 Rahi 043 on 0.15.0-patched.1 (independent of every node above)
```

| node | prerequisites | files and spec ownership | acceptance | decisions that genuinely block it | authorization needed |
|---|---|---|---|---|---|
| **A1** N1 exclusion repair | none | `035` (establishes `hiqlite/src/upgrade_exclusion.rs`, `qualification/n1-upgrade/`); `extends` added by the implementing change on `start.rs` (010/024/027), `store/logs/mod.rs` (007/020/027), `storage_lock.rs` (024), the SQLite state machine, `hiqlite-wal/src/` (001/008/021); `amends` 027 | `035` U-1 to U-7 and X-1 to X-5, X-7 recorded | none | runtime repair lane (A) |
| **A2** exclusion handle | A1's sequence | `035` B-6; the public surface of `027` B-7 | `035` X-6 | D-17 | lane A, if D-17 adopts it |
| **A-rel** producer release | A1 (and A2 if adopted) merged | `031`'s release procedure and ledger | `031`'s qualification on the release commit | D-18 | publication, separately |
| **B** N=3 start, join, shutdown repairs | none | `033` B-3, B-4, B-5; `034` B-4; `extends` on `init.rs`, `client/mgmt.rs`, `membership_gate.rs` (010/027), `backup.rs`, `start.rs` by the implementing change | each repair's test observed failing without it | none for F-118, F-119, F-120 and the pre-delay option (its default stays 9.5 s); D-13 for F-121, after E | runtime repair lane (B) |
| **C1** identity, callable restore | none | `034` B-1, B-3, B-4, B-5 (establishes `hiqlite/src/restore.rs`); `amends` 026 | `034` R-1 to R-5, R-7, R-8 | D-7 (fields) | runtime repair lane (C) |
| **C2** export rule and clean-stop marker | C1's manifest | `034` B-2, B-7; section 13.3 to 13.5, 13.10 | `034` R-6, R-6a, R-6c | D-12 for whether the export is the migration's evidence; none for the marker itself, which every option uses | lane C |
| **C3** barrier | C1 | `034` B-6 | `034` R-6b | D-12 (B adopted) | lane C |
| **D** real-node harness | none | `033` B-2 (establishes `qualification/n3/`) | the harness runs one smoke scenario per layout within its bounds | none; D-3 and D-4 set the configurations it runs, which are parameters | harness construction lane (D) |
| **E** bounded qualification | B, C1 to C3 as adopted, D; a candidate | `033` B-6 A-1 to A-12, `034` section 4 | stage 4 to 6 pass criteria of section 9 | D-3, D-4, D-5 (stage 6), D-13 after its measurement | execution lane (E), per tranche |
| **F-N1** Rahi Track N1 | none here | Rahi 043 (Rahi's territory) | Rahi 043 AC-1 to AC-9 | Rahi's own; nothing of this packet except D-8a to D-8e for its release notes | Rahi's owner |
| **F-N1'** re-pin to A-rel | A-rel published | Rahi's dependency spec; Rauthy's release line | the consumer's own | none here | Rahi's owner; Rauthy's maintainer |
| **7c / F-S7** candidate integration, then adoption | 7c: a candidate; F-S7: E passed on the exact release, published | Rahi, Rauthy | section 9 rows 7c and 7 | D-8b to D-8e, D-15 | each consumer's owner |
| **G** Kubernetes acceptance, rehearsal, support | F-S7, E, D-5, Rahi 044 approved (which needs Rahi's own resolution of its spec 000 anchor) | the deploying repository; `026` B-7, the handoff and the ledger for the support record | section 9 stages 8 to 10 | D-1, D-9, D-10, D-11, D-12, D-15, D-8c | Kubernetes lane, rehearsal lane, and the owner's support decision, each separate |

**Immediate critical path** (third pass). **A1 → A-rel → F-N1'**. A1 is
implemented as a candidate (`035` section 5 records its tests); A-rel needs D-18
and a publication authorization; F-N1' is Rahi's re-pin and Rauthy's rebuild,
each its owner's act, and Rahi's B-4b waits on it. D-17 (A2) is off this path.
In parallel: F-N1 (Rahi 043 on the published build, now fenced, no longer
waiting on A2), D (the harness is built; smoke only), and B, which has no
blocking decision but no authorization. The N=3 path's first decision-gated step
is C2/C3 on D-12.

**Lane authorizations, ready to sign.** Each is complete as written; none is
granted by this document.

- **Lane A (runtime repair, N1).** "Implement `specs/035-n1-upgrade-exclusion`
  B-1 to B-5 (and B-6 only if D-17 is recorded as adopted) on a branch from
  `spec-spine` in `bartekus/hiqlite`, with U-1 to U-7 and the
  `qualification/n1-upgrade/` harness running X-1 to X-5 and recording X-7. Each
  X scenario: three consecutive runs, 60 s per run, stop at the first failure
  and keep its directory; total harness wall time at most 30 minutes on one
  host with 4 cores, 8 GB memory and 10 GB free disk. Governance gates and
  `just verify`'s checks on the result; a pull request against `spec-spine`.
  Not authorized: publication, version bump, tags, consumer edits, runs beyond
  these bounds, or retries of a failed scenario."
- **Lane A publication.** "Release the merged `035` repair as D-18 names, through
  `031`'s procedure, and update the consumer handoff. Not authorized: any
  consumer pin change."
- **Lane B (runtime repair, N=3 start and shutdown).** "Implement `033` B-3 and
  B-4 and `034` B-4 with a test per repair observed failing first; no harness
  runs beyond `cargo test`." (Same exclusions as lane A.)
- **Lane C (restore and export).** "Implement `034` B-1, B-3, B-5, B-7 and, if
  D-12 adopts B, B-6, and B-2 as D-12 records; acceptance R-1 to R-8 in-process
  or unit where the harness does not yet exist."
- **Lane D (harness construction).** "Build `qualification/n3/` per `033` B-2
  with one smoke scenario per layout, each bounded at 5 minutes, run at most
  three times in total to validate the harness. Not authorized: any A-scenario
  run counted as qualification."
- **Lane E, tranche 1 (bounded qualification smoke).** "On a named candidate
  commit: A-1 to A-8 and A-10 in the split layout (9 scenarios) and A-8 and A-10
  in the co-located layout (2), **3** consecutive runs each (33 runs), with A-9
  recorded from every `SIGTERM` in them rather than run on its own; each run
  bounded at 10 minutes, the tranche at 4 hours of wall time; one host with at least 8 cores, 16 GB memory and 20 GB free disk,
  no other hiqlite processes on the harness ports; **stop the tranche at the
  first failed run**, keep its logs and directories, report, and do not retry or
  resume without a new authorization." No restore scenario is in tranche 1:
  `034`'s entry points do not exist until lane C.
- **Lane E, tranche 2 (qualification).** "After tranche 1 passes on the same
  candidate: tranche 1's 11 scenarios plus A-11 (A-1 to A-8 in a debug build,
  8 scenarios), **20** consecutive runs each (380 runs), 10 minutes per run, at
  most 40 hours of wall time across sessions, the same host floor; stop at the
  first failure as in tranche 1, and a tranche that reaches its time cap stops
  and is reported incomplete, which is not a pass. `034` R-1 to R-8 in the harness: 20 runs
  each, the same rules." The existing 20-run figure of section 9 is this
  tranche's count; writing it here does not authorize it.
- **Run accounting, corrected (third pass).** A "run" starts and stops many node
  processes; the tranche budgets above count runs, and the table below counts the
  launches inside them, so an authorization names both. Upper bounds per run, two
  clusters of three nodes (the harness default), split layout:

  | scenario | node launches per run | runs, tranche 1 | runs, tranche 2 |
  |---|---|---|---|
  | A-1 bootstrap (six start orders plus simultaneous, and the negative case) | 7 × 6 + 3 = 45 | 3 | 20 |
  | A-2 immediate restart | 6 + 6 = 12 | 3 | 20 |
  | A-3 leader loss (both groups) | 6 + 2 = 8 | 3 | 20 |
  | A-4 partition and rejoin | 6 | 3 | 20 |
  | A-5 lagging rejoin | 6 + 1 = 7 | 3 | 20 |
  | A-6 replacement, same id (a follower, node 1 twice) | 6 + 3 = 9 | 3 | 20 |
  | A-7 membership and shutdown interleavings (four cases) | 4 × (6 + 1) = 28 | 3 | 20 |
  | A-8 rolling restart | 6 + 6 = 12 | 3 (+3 co-located) | 20 (+20) |
  | A-10 full stop and start | 12 | 3 (+3 co-located) | 20 (+20) |
  | A-11 debug pass of A-1 to A-8 | as A-1 to A-8 | 0 | 20 each (160) |
  | `034` R-1, R-2, R-3, R-4, R-5, R-8 (harness) | R-1 to R-3: 6; R-4: 6 per interrupted state × 5 states = 30; R-5: 7; R-8: 3 | 0 | 20 each (120) |
  | `034` R-6, R-6a, R-6b, R-6c, R-7 | unit tests, no node process | 0 | in `cargo test`, not counted |

  Totals: tranche 1, 33 runs and at most 3 × (45 + 12 + 8 + 6 + 7 + 9 + 28 +
  12 + 12) + 3 × (12 + 12) = 417 + 72 = **489 launches**; tranche 2, 380 A-runs
  plus 120 restore runs = **500 runs**, at most 2,780 + 480 + 2,540 + 1,160 =
  **6,960 launches** (split A-1 to A-10, co-located A-8 and A-10, A-11, restore). The restore runs need
  lane C first. Wall-time caps stay as written; a tranche that reaches its cap is
  incomplete, not passed.
- **Kubernetes, rehearsal and support** are each authorized only after the node
  before them has recorded evidence, and are not drafted here.

**Lane B, prepared (third pass), not implemented.** Each item: the change, the
ownership its implementing change adds in the same range, and the test that
must be observed failing on the unrepaired tree first.

| item | change | ownership added by the implementing change | failing-first test |
|---|---|---|---|
| F-118 (`033` B-3) | `should_node_1_skip_init` seeds no vote for node 1 and initializes only after at least `⌊N/2⌋` distinct peers **answered** "not initialized" (authenticated `200` with an empty membership); a connection error, timeout or `401` counts for nothing; bounded wait, not-ready meanwhile, then a startup error naming the silent peers; the cache group takes the same decision | `033` extends `010`'s `hiqlite/src/init.rs` | unit: a stubbed peer answering with a connection error makes today's code initialize (fails today), and the repair wait and then refuse; plus N=1 unchanged |
| F-119 (`034` B-4) | a restore instruction is applied at most once: node 1 records the applied instruction's identity (the backup name and digest) durably before it restores, and a later start with the same instruction logs and skips; followers do not quarantine again for an instruction already honoured | `034` extends `013`'s `hiqlite/src/backup.rs` and `010`'s `start.rs`; `amends` `026` for the quarantine | unit: two starts with the same instruction restore twice today, once after |
| F-120 (`034` B-4) | `restore_backup_finish` runs after the listeners are bound and after `become_cluster_member`, and its two waits are bounded; on expiry it returns a startup error; the `debug_assert!` on the leader becomes a returned error | as F-119 | unit with a stand-in raft that never initializes: today the start never returns (bounded by the test), after the repair it errors within the bound |
| pre-shutdown delay (`033` B-4) | `NodeConfig` option, code, TOML and environment, default 9.5 s; `009`'s configuration contract records it | `033` extends `010`'s `client/mgmt.rs` and `009`'s config units | unit: the default equals today's constant; a configured value is used |

**Lane B, implemented (2026-09-23), not merged.** Authorized by the owner on
2026-09-23 and implemented on the branch `fix/033-lane-b-n3-start`, as `033` D-8
and D-9 and `034` D-6 to D-8 record. Three departures from the table above, each
recorded there: F-118 also changes the peer's answer (`003`'s
`hiqlite/src/network/`), since no peer could give the explicit one before;
`client/mgmt.rs` is `003`'s, not `010`'s; and F-120's reordering needed a hold
on SQLite membership changes while node 1 finishes a restore. No harness run,
publication or consumer change is part of it.

Authorization to sign, when wanted: "Implement lane B as prepared in proposal
section 16 (F-118, F-119, F-120 and the pre-shutdown option), each with its
failing-first test observed and recorded, `033` and `034` edges in the same
range, `cargo test` and the governance gates only; no harness runs, no
publication, no consumer edits." F-121 (D-13) stays after lane E's stage 4.

**Lane C contracts, ready to decide (third pass).**

- **C1, identity and a callable restore** (`034` B-1, B-3, B-4, B-5; D-7 fields).
  *Contract:* every image carries the manifest D-7 names; restore is a function
  that checks the manifest against the target (cluster id, node id, version,
  key ids) before anything moves, applies at most once (F-119), and forms a
  single-voter cluster or fails (F-120); an upload's outcome is observable.
  *Decision wording:* D-7 as in section 11. *Does not claim:* anything about
  currency; that is C2 and C3.
- **C2, the export rule and the clean-stop marker** (`034` B-2, B-7; 13.3 to
  13.5, 13.10). *Contract:* an offline export refuses unless 13.3's cases pass
  and, at N=1, the 13.10 marker verifies against the directory; at N=3, 13.5's
  majority rule. *Decision wording (D-12, part 1):* "The migration export's
  evidence is rule A with the clean-stop marker." *Does not claim:* absence of
  a whole-directory rollback, or of loss before the stop (13.10's limit).
- **C3, the barrier** (`034` B-6). *Contract:* an entry point that commits a
  caller nonce through the replicated state machine and returns its log id once
  committed and applied; the export refuses an image without the recorded
  nonce; with recurring barriers, every recorded nonce (13.13). *Decision
  wording (D-12, part 2):* "Add a committed barrier per cluster after
  quiescence and drain, recorded off-cell." *Does not claim:* that writes
  acknowledged before the barrier survived (F-132); two barriers are not a
  cross-store transaction (13.11).
- **Recommendation**, unchanged: A + B, C optional, `LogSync::Immediate` for the
  committed-write objective subject to stage 4's measurement, and 13.7's
  historical assumptions stated wherever an RPO of 0 is.

## 17. A producer-side downgrade fence (proposal, D-19)

Rahi's H-5 asks for either an explicit unsupported-downgrade boundary or a
layout the unmodified 0.14 binary refuses before it writes. `035` B-4 states the
first. This section evaluates the second, as a proposal only; nothing is built.

**What does not work.** A check in 0.15 code never runs in a 0.14 binary, and a
format marker 0.14 does not read is not a fence. A permanent `state_machine/lock`
alone is not one either: 0.14 checks it only after its SQLite `LogStore` has
opened `logs/`, created or truncated `lock.hql`, written `meta.hql`, checked (and
under `auto-heal` repaired) the WAL, and created the state-machine directories
(Rahi handoff, H-7 Q4); and under `auto-heal`, hiqlite's default and so an
upstream Rauthy's, the marker is not a refusal at all: 0.14 deletes the database
and serves an empty store.

**A relocation that could work.** 0.15 keeps its raft logs, state machines and
caches under a new subdirectory 0.14 never opens, and leaves at the old paths
only a permanent `state_machine/lock` (tagged) and nothing else. Then, per 0.14
startup path at `8f3b9bd`:

| 0.14 path | what it does against the fenced layout | 0.15 data |
|---|---|---|
| default features | creates an empty `logs/` (lock, `meta.hql`, one WAL file) and state-machine directories at the old paths, then panics on the marker | untouched |
| `auto-heal` | as above, then deletes an empty old-path database, **serves an empty store**, and at a clean stop removes the marker | untouched, but a live empty node answers at the cell's addresses |
| `HQL_DANGER_RAFT_STATE_RESET` | deletes the old-path `logs/`, `logs_cache/` and snapshot directories, then as default | untouched |
| `HQL_BACKUP_RESTORE`, node 1 | writes a restored database at the old paths, then as default | untouched |
| `HQL_BACKUP_RESTORE`, other node | `remove_dir_all(data_dir)`: **deletes everything**, the new layout included | destroyed |

**Assessment.** It protects 0.15 data from an unmodified 0.14 in every path but
one, and it cannot close that one: a restore on a non-first node deletes the
whole data directory before anything is read. The `auto-heal` row turns a
destructive downgrade into a silent empty service, which for an identity
provider is its own hazard. It is a layout change for every consumer (paths,
backups, restore tooling, Rahi's relocation), needs a migration of its own for
0.15 directories already written, and must be demonstrated against the real
0.14 binary on both Linux architectures with `035`'s method. **Recommendation
(D-19): do not build it now**; state the unsupported downgrade (`035` B-4) in
every consumer handoff, and keep the verified pre-upgrade archive restored into a
fresh volume as the only supported way back.
