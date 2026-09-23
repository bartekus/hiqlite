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
subdivisions D-8a to D-8e, and the proposed D-15 and D-16 (section 15).

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

**Reconciliation with Rahi packet D4.** The packet proposes a ten-second store
allowance inside one serve deadline, and describes hiqlite's stop as "5 s drain
plus stops, reported up to about 15". The 5 s is the drain's **bound**, reached
only with a membership change in flight, and 15 s is the caller's wait bound;
neither is an observed duration. At N=1 a ten-second allowance is plausible and
must be measured. In a split N=3 pod it cannot hold with the default pre-delay:
the delay alone is 9.5 s. It needs `033` B-4's option set well below the
allowance, or an allowance that covers hiqlite's own 15 s. Rahi must record
which of the three outcomes each stop reached; wrapping `Client::shutdown` in a
shorter timeout and then treating the node as stopped turns an unconfirmed
completion into an unrecorded forced exit.

**Measurements, recorded separately, not yet run.** **M-N1:** the single
container, both hiqlite nodes, Rahi's serve stages as packet D4 proposes:
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
| **Q-2 barrier** | if the owner adopts section 13's barrier (alternative B), write and record one barrier per cluster, after Q-1 | both barrier receipts recorded outside the cell | leave maintenance; retry |
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
  N=1 (Rahi packet D1 to D5), with D-8b's bearer floor on the cache transition
  (section 14). Claims N=1 only.
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

**Rauthy** (proposals to its maintainer). The cache inventory is in section
14.1; confirm it. Enforce the tombstone when Rauthy starts on its own (an image
entrypoint or init-container check, or a Rauthy-side refusal). A restore-time
invalidation of sessions and refresh tokens (D-8c). An admin-API export and
re-import of manual IP bans (D-8d). A way to hold scheduled jobs until
activation (7.3). Confirm `ENC_KEYS` and key-id handling across a restore into a
new cell. Separately, the DPoP nonce observation of 14.5.

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
| 1 | owner decisions D-1 to D-12, D-8a to D-8e, D-15 and D-16 recorded (D-13 follows stage 4) | 0 | each decision dated in `033` section 6 | a decision is not evidence |
| 2 | repairs for F-118, F-119, F-120; decision on F-121; pre-delay option; `034`'s export, restore and (if adopted) barrier entry points | 1 | each has a test observed failing without it | unit and in-process tests only |
| 3 | real-node harness (`033` B-2) | 2 | three **release-build** node processes per cluster, the consumers' exact feature sets and settings, a per-link TCP proxy for partitions, `SIGKILL`/`SIGTERM` by the harness | one host; no kernel crash; no real disk loss |
| 4 | hiqlite N=3 acceptance (`033` section 3, A-1 to A-12) | 3 | every scenario passes 20 of 20 consecutive runs with no retry, no masking sleep, fixed bounds | 20 runs bound the observed rate, they do not prove absence (F-107 appeared once in about seven) |
| 5 | restore acceptance (`034` section 3) | 3, and `034` implemented | every restore scenario, including wrong-backup rejection and interruption at each state | one host; object storage by a local S3 double unless stated |
| 6 | TLS or mesh boundary (D-5) | 3 | the chosen boundary carries raft and API traffic; a capture shows no plaintext; shutdown under the boundary is still within budget | proves the configured path, not every certificate lifecycle |
| N1 | Track N1 (parallel, Rahi-owned, not a stage of this plan) | none here | Rahi packet D1 to D5 acceptance at N=1 | N=1 consumer evidence; no N=3 claim |
| 7 | Track S7 consumer adoption | 4 | Rahi and the Rauthy image on the qualified release (R-a2); both consumers' suites green | consumer evidence, not hiqlite's |
| 8 | Kubernetes cell acceptance | 5, 6, 7 | on a real three-node cluster: section 10 prerequisites in place; stage 4's scenarios re-run as pod, node and network faults; measured shutdown budget within grace with margin; PDB-respecting drain of each node | one cluster, one provider |
| 9 | migration rehearsal | 8 | section 7 end to end on a copy of a real archive, twice, timings recorded, V-1 green | a rehearsal on a copy |
| 10 | owner: mark N=3 supported | 9 | a later change updates `026` B-7, the handoff and the ledger with the evidence | ratification and support are the owner's acts |

Consumer adoption (7) starts as soon as stage 4 has a candidate, in parallel
with 5 and 6, so composition problems surface before the hiqlite work is
declared done.

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

## 11. Owner decisions

Each with its implication and a recommendation. Recommendations are not
decisions.

| id | decision | implication | recommendation |
|---|---|---|---|
| D-1 | Adopt C for new cells and B for existing N=1 cells; defer A | live expansion stays unsupported; existing cells take a planned window | **adopt** |
| D-2 | Permanent replacement keeps the ordinal (same id, new PVC); a new id is unsupported | the static peer list never changes; replacement is automatic leave-and-rejoin | **adopt**, after F-118 is repaired |
| D-3 | `cache_storage_disk = true` required for N=3 production | no routine operation changes membership | **adopt** |
| D-4 | `LogSync` for N=3: `Immediate` or `ImmediateAsync` | `ImmediateAsync` keeps an acknowledged-loss window that N=3 does not close under correlated kernel failure (F-121); `Immediate` costs fsync latency per append | **`Immediate` for both SQLite groups**, unless stage 4's measured latency is unacceptable; the environment route cannot select it (F-035), so it is set in code or TOML |
| D-5 | Transport boundary: hiqlite TLS, a mesh, or CNI encryption | hiqlite TLS is untested on a running cluster (F-053) but under this fork's control; a mesh adds a sidecar whose shutdown order interacts with section 4; CNI encryption (WireGuard) has no process coupling and depends on the provider | **CNI encryption where the provider has it**, else hiqlite TLS after stage 6 passes; a mesh only with a qualified shutdown order |
| D-6 | Pre-shutdown delay becomes configurable; consumer graces from M-N1 and M-S3 | today's 9.5 s at N>1 does not fit the cell's graces; only confirmed graceful completions count toward the budget (section 4) | **adopt**, default unchanged; pending |
| D-7 | Image manifest: source cell and cluster ids, applied log id, content digest, barrier receipt, hiqlite version, key ids | closes `026` KD-5 for this procedure; the fields are provenance, and no cross-cluster compatibility is inferred from them | **adopt**, hiqlite image fields in `034`, archive fields in Rahi; pending |
| D-8 | Cache loss and stale-backup security, **subdivided** into D-8a to D-8e (section 14.3) | accepting empty caches (D-8a) does not accept weakened revocation, lifted bans or rolled-back revocations (D-8b to D-8e) | a: accept; b: bearer floor; c: invalidate on stale restore; d: export and re-apply manual bans (a separate owner decision); e: accept with exposure stated; all pending |
| D-9 | RPO/RTO targets (7.5) | migration RPO 0 holds only under section 13's conditions or its barrier; DR RPO on hot archives only with the restore validator | adopt as **targets**, revisit after stage 9; pending |
| D-10 | No hiqlite version change inside a migration window | two variables in one outage | **adopt** |
| D-11 | Supported upgrade is full stop unless a pair is qualified | every upgrade is a planned outage by default | **adopt** |
| D-12 | Offline export for migration, by a currency rule (section 13), versus hot backup | rule A proves currency only under stated conditions; alternative B (a barrier) proves it end to end; alternative C strengthens refusals; hot archives need the section 14.4 validator | **offline export with barrier B and rule A's refusals**, C optional; hot backup stays the DR archive with the validator; pending |
| D-13 | Keep or remove `openraft/loosen-follower-log-revert` from `cache` (F-121) | removing it restores openraft's panic on a reverted follower for both groups and needs the in-memory cache's rejoin shown correct without it; keeping it silently accepts a reverted durable follower | **measure without it in stage 4**; remove it if A-1 to A-8 pass, otherwise keep it and require `LogSync::Immediate` for durable groups at N=3 (D-4) |
| D-14 | **Decided 2026-09-23 by the owner.** N=3 is two StatefulSets, one per application, one hiqlite node per pod; the single-container composition is the N=1 local profile | section 12's replacements become obligations of new Rahi and Statecraft specs; the shutdown budget, blast radius and rollout coupling of sections 4 and 5 no longer apply at N=3; the harness qualifies both layouts | recorded as decided |
| D-15 | The five controls of 7.2 and activation by authoritative mutation (7.3), with the downstream tombstone | isolation controls stop Raft merges, not writes; only workload exclusion, storage locking and the tombstone keep the source from writing | **adopt**; proposed, pending |
| D-16 | Two adoption tracks: N1 now at N=1, S7 after stages 4 to 6 (section 15) | Track N1 is not evidence for N=3 and does not wait for it; stage 7 is S7 only | **adopt**; proposed, pending |

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

**13.2 Facts the rule rests on** (read at source, `72e09a6`; none executed):

- **F-124.** The committed log id is not persisted: `hiqlite-wal`'s log store
  does not implement `save_committed` / `read_committed`, so openraft's no-op
  defaults apply. Offline, a replica's committed position is unknown.
- **F-125.** The SQLite state machine's persisted applied log id (`_metadata`)
  is written when a snapshot is built and when the SQLite writer exits, not per
  applied entry. Offline it is accurate only after a clean writer exit.
- A clean WAL writer exit performs a blocking flush of the active WAL file and
  writes its metadata (`hiqlite-wal/src/writer.rs:717-723`), so a **confirmed**
  clean stop (section 4) makes every appended entry durable, whatever the mode.
- There is no persisted cluster identity (`034` KD-3). Membership node ids and
  addresses are the only identity a data directory carries today.

**13.3 Refusal cases common to every directory read.** The export refuses,
naming the case, unless all of these hold:

- *R-a identity:* the directory's latest membership (log and state machine)
  names exactly the expected node ids and addresses of the source cell, and,
  once `034` B-1 exists, its cluster id matches. Until then identity is weak and
  the manifest says so.
- *R-b confirmed clean stop:* the node's last shutdown is a confirmed graceful
  completion (section 4). Until hiqlite persists a clean-stop marker (proposed in
  `034`), the evidence is the consumer's recorded `Ok(())`. An unconfirmed or
  forced stop is refused; the remedy is one confirmed start and stop.
- *R-c complete log ids:* vote, last purged log id, last log id and the applied
  log id are all readable, and the WAL needs no repair (`021` B-8's torn-header
  case refuses rather than auto-heals).
- *R-d frontiers:* last purged ≤ applied ≤ last log id. An applied id beyond the
  last log id, or below the purge frontier, is refused.
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
nothing local detects it.

**13.5 The N=3 rule** (for exports **from** a split cell: a reverse migration or
an offline DR archive; the migration of section 7 exports from N=1). Per
cluster: the read set is at least a majority of the stable configuration's
voters; every member passes R-a to R-e and all agree on the configuration. Let
M be the highest last log id in the read set. Select a replica whose applied log
id equals M, index **and** term. Refuse if none exists, if the logs disagree on
the term at M, or if fewer than a majority are readable. The remedy is a
confirmed start and stop of the cluster, so a leader commits or truncates the
tail, then a new export.

*Safety argument.* R-e makes the configuration C committed (it is applied), so
every entry committed after C was reported complete by a majority of C, and every
entry before C precedes C in the committed prefix. A majority read set
intersects every majority of C, so each committed entry appears in some read log
at an index ≤ M. The selected replica applied M, so M is committed, and by log
matching its log up to M is the committed prefix; its state machine has applied
all of it. Applied implies committed, so the image holds nothing uncommitted.

*What it preserves:* every write acknowledged before the stop, provided (i)
storage was not rolled back on any replica, and (ii) under `ImmediateAsync` or
`IntervalMillis`, no replica suffered a host crash or power loss during the
cluster's last run. A replica that lost an acknowledged tail can rejoin silently
(F-121), so under (ii)'s failure the majority argument does not hold and nothing
local detects it. Under `LogSync::Immediate`, (ii) is not needed.

**13.6 Why rule A alone does not establish zero loss.** Storage rollback and
asynchronous-sync loss are invisible to any local evidence, and identity is weak
until `034` B-1. So the zero-loss objective holds under rule A only if the owner
accepts those conditions (for the source's last run: `LogSync::Immediate`, or no
host crash, and no storage rollback). This is **not** silently weakened: D-9's
migration RPO stays "0 for acknowledged writes" only together with either those
recorded conditions or alternative B.

**13.7 Alternative B: a consumer-coordinated committed barrier.** After Q-1 and
before Q-3, one barrier write per cluster through the ordinary client path,
carrying a fresh random nonce; the acknowledgment and nonce are recorded outside
the cell (the migration record) before any workload is excluded. openraft
appends in order and the barrier is appended after every write acknowledged
before it was issued, so every such write has a lower log index. An image whose
state contains the nonce therefore contains every write acknowledged before the
barrier. The export refuses an image without it. That check **detects** storage
rollback, asynchronous-sync loss and a wrong source, which rule A cannot.

Limits: it covers writes acknowledged before the barrier; quiescence must have
stopped user writes, and any background write after the barrier is in the image
or not, which 7.3 requires consumers to state. Two barriers after one quiescence
give both images every write acknowledged before quiescence, which is the
cross-application cut a synchronous cross-reference needs, and nothing more. Cost:
a hiqlite entry point that records the nonce in the replicated state machine
(`034` B-6, proposed), because Rahi does not own Rauthy's schema, and a
consumer step.

**13.8 Alternative C: durable commit metadata.** Implement `save_committed` in
`hiqlite-wal`, with a defined ordering: a committed id is persisted only after
every entry up to it is durable in the local WAL, and never ahead of it,
flushed with the same mode as appends. Offline it is a lower bound on the
cluster's commit (a follower learns commit late). It makes a corrected `034` B-2
check (`applied ≥ persisted committed`) implementable as a **necessary**
condition, strengthens R-d, and does not detect rollback or replace the majority
rule.

**13.9 Recommendation, pending the owner.** Alternative B as the zero-loss
evidence; rule A's refusal cases (13.3) and rules (13.4, 13.5) as mandatory
checks alongside it; alternative C as an optional strengthening. If B is not
adopted, record 13.6's conditions as accepted, or lower the migration RPO
explicitly.

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
| Rahi | bearer deny-lists by `jti` and by subject instant (Rahi 038 B-5, 025 B-5) | a revoked bearer token is accepted again until it expires; the bound is the maximum bearer lifetime `preflight` reports (038 B-6), not a constant |
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

**14.3 Proposed controls, each pending** (D-8 subdivisions; none decided):

- **D-8a functional loss** (in-flight flows, assertions, performance caches):
  recommend accept.
- **D-8b Rahi bearer floor:** refuse bearer tokens whose `iat` precedes the
  floor instant, for the reported maximum lifetime plus a clock-skew margin. The
  floor is set as a preparatory mutation (7.3) at the planned activation. It
  covers both threats for every token presented to Rahi, including outstanding
  access tokens, and applies alike to migration, restore and the Track N1
  upgrade. It covers no other relying party.
- **D-8c Rauthy stale-backup revocation:** on a restore from anything but the
  final offline export, invalidate every Rauthy session and refresh token before
  activation. For relying parties other than Rahi, outstanding access tokens
  are covered only by rotating the signing keys, which invalidates every token
  and is an owner call. Recommend invalidation; key rotation as a stated option.
- **D-8d manual IP bans, a separate owner decision:** (i) export the active
  manual bans through Rauthy's admin API after Q-1 and re-apply them as a
  preparatory mutation, or (ii) accept their loss. Recommend (i).
- **D-8e automatic abuse state** (bans, escalation counters, stuffing windows):
  accept with the exposure stated above, or ask Rauthy to persist it. Recommend
  accept for migration; revisit if restores become routine.

Accepting empty caches (D-8a) does **not** accept D-8b to D-8e by implication.

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

## 15. Reconciliation with Rahi decision packet 1 (2026-09-23)

Input: Rahi's "decision packet 1: patched dependency adoption" (D1 to D5, Rahi
baseline `b815b18`). This pass changed sections 1, 4, 7, 8, 9, 11, 12.3 and added
13 and 14. Decision state after it: **D-14 decided**; D-1 to D-13, the proposed
D-15 and D-16, and the D-8 subdivisions D-8a to D-8e are **pending**. The handoff
to Rahi is `standards/spec/n3-rahi-reconciliation-handoff.md`.

**Two adoption tracks, kept apart (proposed D-16).**

| | Track N1 | Track S7 |
|---|---|---|
| what | Rahi and the cell's Rauthy image adopt the **published** `hiqlite-patched 0.15.0-patched.1` and `rauthy-patched:0.36.2-patched.2` | Rahi and Rauthy adopt a later release that passed hiqlite stages 4 to 6 |
| topology claimed | N=1 only | N=3, only after stage 8 |
| owner | Rahi (packet D1 to D5, proposed Rahi spec 043) | Rahi and Rauthy |
| depends on this work | no | yes |
| produces | N=1 consumer evidence and early composition findings | the qualified cell |

Track N1's findings already reached this proposal: the shutdown budget
(packet D4, reconciled in section 4) and the cache transition's revocation loss
(packet D2, carried into section 14).
