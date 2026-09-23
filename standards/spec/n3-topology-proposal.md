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
consequence. D-1 to D-13 remain pending.

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
repository's (section 8).

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
| caller's wait | **15 s**, then `Error::Timeout`; the sequence continues while the runtime lives | `SHUTDOWN_WAIT` |

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

**Recommendations:** a hiqlite option for the pre-shutdown delay, defaulting to
today's 9.5 s so nothing changes for current callers (`033` B-4, planned), and
graces derived from a **measured** end-to-end budget with margin (D-6; consumer
obligation). The budget is a measurement in section 9, not a number chosen here.

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
state. The **source** is the N=1 cell; the **target** is the new cell.

**Prerequisites (P).**

- P-1. Both consumers run a `hiqlite-patched` release that passed section 9's
  hiqlite stages, at N=1 in the source **and** at N=3 in the target. Migrating
  across hiqlite versions at the same time is a second change in one window and
  is refused (D-10).
- P-2. The target is isolated: its own namespace, its own Service and pod DNS
  names, a NetworkPolicy that admits no traffic between the two namespaces, and
  **distinct `secret_raft` and `secret_api` for each of its two clusters**. The
  secrets are the fence hiqlite itself enforces: a node of one cell cannot
  complete the challenge-response of the other, and a membership probe answered
  `401` is not a membership.
- P-3. The target's pods carry **no** restore instruction in their template.
  Restore is a one-shot act on node 1 (step R-3), never an environment variable
  a restart can repeat (F-119).
- P-4. `033` and `034` acceptance passed on the release in use; the procedure
  was rehearsed end to end against a copy of the source's archive in a scratch
  cell, with its timings recorded.

**Steps.**

| state | action | done when | on failure |
|---|---|---|---|
| **Q-1 quiesce** | the source enters maintenance: Rahi's edge refuses every mutating request, its own and those it proxies to Rauthy | a probe write is refused | leave maintenance; nothing changed |
| **Q-2 stop** | scale the source StatefulSet to 0; wait for both hiqlite nodes to finish shutdown and release their storage locks | both locks free, pod gone | the old PVC is intact; restart the source |
| **Q-3 snapshot the volume** | a CSI VolumeSnapshot of the source PVC | snapshot `readyToUse` | retry; this is the byte-for-byte rollback artifact |
| **B-1 export** | produce the cell archive **offline** from the stopped volume: both clusters' backup images, `/data/keys`, and a manifest (D-7) | archive written, manifest complete | retry; the source is stopped and unchanged |
| **B-2 verify provenance** | check every part's hash against the manifest; check the manifest names the source cell, both clusters, each image's applied log id, the hiqlite version and the key ids | all match | stop; do not upload a mismatched archive |
| **B-3 upload and prove remote** | upload; then **independently** list and fetch it back and re-hash, because `Client::backup` reports only the local copy (`026` KD-8) | remote hash equals local | retry the upload |
| **R-1 provision** | create the target at N=3 as D-14's two StatefulSets (Rahi, Rauthy) in one namespace, with empty PVCs, the internal Rauthy Service, the NetworkPolicies of section 12, and no restore instruction; do not start pods yet | objects exist | delete and recreate |
| **R-2 keys** | install the source's keys in the target (Rahi keys Secret, Rauthy `ENC_KEYS`); refuse if key ids differ from the manifest | ids match | fix the Secret |
| **R-3 restore node 1** | on ordinal 0 of **each** StatefulSet, apply that cluster's image (split out of the source archive) through a restore that checks the manifest's identity and digest and records a restore id, so a repeat is a no-op (`034`) | node 1 of each cluster is a single-voter leader on the restored state | destroy the target's PVCs; back to R-1 |
| **R-4 join** | start ordinals 1 and 2 of each StatefulSet; each joins its own cluster by `add_learner`, snapshot installation, and promotion | four groups report voters `{1,2,3}` | stop joins; inspect; if not resolvable, back to R-1 |
| **V-1 validate** | section 7.1 | every check passes | back to R-1; the source is still stopped and intact |
| **C-1 fence the source** | delete the source StatefulSet with its PVC retained (`Retain`), remove its Service endpoints and ingress, and record the source as decommissioned | nothing can schedule a source pod without re-creating the object | stop; the target has taken no traffic |
| **C-2 cut over** | point ingress and DNS at the target; lift maintenance on the target | first authoritative write accepted | **this is the point of no return** |

**7.1 Validation before cutover (V-1).** All of these, on the target, with no
external traffic:

- every group of both clusters: three voters, one leader, no learner, the same
  membership log id on every node;
- each SQLite group's applied index on every node at or past the manifest's
  applied log id, and a deterministic content digest of each database equal on
  all three nodes **and** equal to the digest the manifest recorded at export
  (D-7);
- a leader kill in each cluster, with writes resuming inside the stage-4 bound
  of section 9, and the digest unchanged afterwards;
- consumer checks: Rahi `preflight` and `ledger verify` at the manifest's ledger
  head; Rauthy's discovery document with the unchanged issuer and a token signed
  by the expected key id; Statecraft's own acceptance (section 8);
- Rahi reaches Rauthy only through the internal Service over the encrypted
  path, and a pod outside Rahi's labels is refused by Rauthy's NetworkPolicy
  (section 12);
- with Rauthy scaled to 0, every Rahi pod goes not-ready and **none restarts**,
  and readiness returns when Rauthy does (section 12);
- no pod of the target has restarted since R-4, or each restart is explained.

**7.2 Old and new never both accept authoritative writes.** Four independent
layers, any one of which suffices for the window:

1. The source is **stopped** from Q-2 onwards and its StatefulSet is deleted at
   C-1, before the target takes any traffic at C-2.
2. Distinct hiqlite secrets and DNS names (P-2): no node of either cell can join
   or replicate into the other, whatever a peer list says.
3. NetworkPolicy between namespaces (P-2).
4. The target lifts maintenance only at C-2, after C-1.

Rahi 041's deployment epochs are the natural place for a consumer-level check
that an instance serves under the cell generation it expects; proposed to Rahi,
not assumed.

**7.3 Interrupted operation.** Every state before C-2 is recoverable without
reconciliation, because the source volume is unchanged and the target is
disposable. The rule is deliberately blunt: **a failure anywhere in R-1 to V-1
destroys the target's PVCs and resumes at R-1**; nothing is repaired in place.
Within R-3, `026` B-8's roll-forward finishes a committed restore on restart, and
`034`'s restore id makes a repeated instruction a no-op rather than a second
restore. A failure in Q-1 to B-3 leaves the source stopped, snapshotted and
restartable.

The source of this protocol is always an N=1 cell, one volume, so Q-2 to B-1
stop and export one StatefulSet. A later export **from** a split N=3 cell (a
reverse migration, or a DR archive taken offline) follows section 12's rule:
both StatefulSets to 0, both fully terminated, then both exported.

**7.4 Rollback and the point of no return.** Before C-2: stop the target,
re-create the source StatefulSet on its retained PVC, restore its ingress, lift
maintenance. No data is reconciled because none diverged. After the first
authoritative write on the target, rolling back is a **reverse migration** by
the same protocol (target to a fresh N=1 or N=3 cell); re-starting the old
source instead would discard those writes, and would require application-level
reconciliation that no consumer has specified.

**7.5 Proposed recovery objectives, for owner decision (D-9).** None of these is
an accepted requirement.

| scenario | proposed RPO | proposed RTO | depends on |
|---|---|---|---|
| migration (B) | 0 for writes acknowledged before Q-1; cache-only state lost by design | write outage at most 30 min per cell, **set from the rehearsal's measurement** | P-4, D-8 |
| one node lost at N=3 | 0 for committed writes **under `LogSync::Immediate`** only | writes resume within 10 s | D-4, F-121 |
| cell lost (DR, procedure B from the last archive) | the archive interval: today daily (Rahi CronJob 03:15), proposed hourly | 60 min | D-9 |

## 8. Consumer follow-up, planned alongside

Proposed obligations, each for its owning repository. None is written there by
this change.

**Rahi.**
- R-a. Adopt `hiqlite-patched` for the app node, and change the Rauthy base
  image to the patched build (`rauthy-patched:0.36.2-patched.2` or its
  successor), by the route the handoff's section 10 lists. Blocks everything
  else for the cell.
- R-b. Replace the file-level restore of Rahi 030 D-2 with hiqlite's repaired
  restore once `034` makes it callable. D-2 re-implements the 0.14 sequence that
  F-057 and F-100 record, without staging, sync or roll-forward.
- R-c. Shutdown: graces from the measured budget; set the hiqlite pre-delay
  option once it exists; a `preStop` only if the measurement shows it is
  needed. Under D-14 this is one shutdown per pod.
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
  volumes after both StatefulSets are at 0 (section 12).

**Rauthy.** Enumerate what lives only in its cache group and is lost by a
restore (D-8); confirm its background writers are stopped by Q-2 rather than
Q-1; confirm `ENC_KEYS` and key-id handling across a restore into a new cell.

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
| 1 | owner decisions D-1 to D-12 recorded (D-13 follows stage 4) | 0 | each decision dated in `033` section 6 | a decision is not evidence |
| 2 | repairs for F-118, F-119, F-120; decision on F-121; pre-delay option | 1 | each has a test observed failing without it | unit and in-process tests only |
| 3 | real-node harness (`033` B-2) | 2 | three **release-build** node processes per cluster, the consumers' exact feature sets and settings, a per-link TCP proxy for partitions, `SIGKILL`/`SIGTERM` by the harness | one host; no kernel crash; no real disk loss |
| 4 | hiqlite N=3 acceptance (`033` section 3, A-1 to A-12) | 3 | every scenario passes 20 of 20 consecutive runs with no retry, no masking sleep, fixed bounds | 20 runs bound the observed rate, they do not prove absence (F-107 appeared once in about seven) |
| 5 | restore acceptance (`034` section 3) | 3, and `034` implemented | every restore scenario, including wrong-backup rejection and interruption at each state | one host; object storage by a local S3 double unless stated |
| 6 | TLS or mesh boundary (D-5) | 3 | the chosen boundary carries raft and API traffic; a capture shows no plaintext; shutdown under the boundary is still within budget | proves the configured path, not every certificate lifecycle |
| 7 | consumer adoption | 4 | Rahi and the Rauthy image on the qualified release (R-a); both consumers' suites green | consumer evidence, not hiqlite's |
| 8 | Kubernetes cell acceptance | 5, 6, 7 | on a real three-node cluster: section 10 prerequisites in place; stage 4's scenarios re-run as pod, node and network faults; measured shutdown budget within grace with margin; PDB-respecting drain of each node | one cluster, one provider |
| 9 | migration rehearsal | 8 | section 7 end to end on a copy of a real archive, twice, timings recorded, V-1 green | a rehearsal on a copy |
| 10 | owner: mark N=3 supported | 9 | a later change updates `026` B-7, the handoff and the ledger with the evidence | ratification and support are the owner's acts |

Consumer adoption (7) starts as soon as stage 4 has a candidate, in parallel
with 5 and 6, so composition problems surface before the hiqlite work is
declared done.

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
| D-6 | Pre-shutdown delay becomes configurable; consumers set it from measurement | today's 9.5 s at N>1 does not fit the cell's graces | **adopt**, default unchanged |
| D-7 | Backup identity manifest: source cell and cluster ids, applied log id, content digest, hiqlite version, key ids | closes `026` KD-5 for this procedure and lets V-1 compare digests | **adopt**, hiqlite image fields in `034`, archive fields in Rahi |
| D-8 | Accept that cache groups start empty after B | anything only in a cache is lost at migration and DR | **accept**, conditional on Rahi and Rauthy each enumerating that state |
| D-9 | RPO/RTO targets (section 7.5) | sets archive frequency and rehearsal bounds | adopt as **targets**, revisit after stage 9 |
| D-10 | No hiqlite version change inside a migration window | two variables in one outage | **adopt** |
| D-11 | Supported upgrade is full stop unless a pair is qualified | every upgrade is a planned outage by default | **adopt** |
| D-12 | Offline export (B-1) versus hot backup plus a write watermark | offline export is coherent across both clusters by construction, needs a stopped volume and a new hiqlite entry point; hot backup needs quiescence of both applications, including Rauthy's background writers. Under D-14 a hot archive of a split cell is two images taken at two instants, with no single-volume cut behind it | **offline export** for migration; hot backup stays the scheduled DR archive, with its cross-cluster skew stated in the DR RPO (D-9) |
| D-13 | Keep or remove `openraft/loosen-follower-log-revert` from `cache` (F-121) | removing it restores openraft's panic on a reverted follower for both groups and needs the in-memory cache's rejoin shown correct without it; keeping it silently accepts a reverted durable follower | **measure without it in stage 4**; remove it if A-1 to A-8 pass, otherwise keep it and require `LogSync::Immediate` for durable groups at N=3 (D-4) |
| D-14 | **Decided 2026-09-23 by the owner.** N=3 is two StatefulSets, one per application, one hiqlite node per pod; the single-container composition is the N=1 local profile | section 12's replacements become obligations of new Rahi and Statecraft specs; the shutdown budget, blast radius and rollout coupling of sections 4 and 5 no longer apply at N=3; the harness qualifies both layouts | recorded as decided |

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

- An export that must be coherent across both clusters (a migration **from** a
  split cell, a reverse migration, an offline DR archive) MUST scale **both**
  StatefulSets to 0, wait until every pod of both has terminated and released
  its storage lock, and only then export both volumes offline (`034` B-2).
- `034` B-2 takes `024`'s exclusive lock on each data directory and refuses a
  state machine that has not applied what its log committed. Together with both
  workloads stopped, that shows no write reached either raft log during the
  export window, which is the guarantee the single volume gave. The Statecraft
  and Rahi specs cite it as such.
- The cell archive (Rahi 030 B-5) becomes an archive of two exported volumes
  plus the keys, with one manifest naming both images, both applied log ids and
  both digests (D-7).
- A **hot** archive of a split cell, taken while serving, is two images at two
  instants. It stays the scheduled DR archive (D-12), and its skew between the
  two clusters is stated in the DR objective (D-9) rather than hidden.

**12.4 What stays.** The N=1 profile: one container, `rahi supervise`, loopback,
die-together, one volume, one archive. The migration protocol of section 7,
whose source is always an N=1 cell: only its target side changes (R-1, R-3, R-4,
V-1). The hiqlite plan: stages 2 to 6 are layout-independent, and the harness
runs both layouts (`033` B-2).

**12.5 Fallback, not chosen.** Rauthy as a native sidecar container in the Rahi
pod keeps loopback and one volume, gains per-container limits and an ordered
shutdown, and keeps coupled rollouts. Recorded so a later reader sees it was
weighed.
