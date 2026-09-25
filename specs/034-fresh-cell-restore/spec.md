---
id: "034-fresh-cell-restore"
title: "Restore a backup into a fresh three-voter cell, once, from an image that proves whose it is"
status: draft
created: "2026-09-23"
owner: "hiqlite maintainers"
risk: critical
implementation: in-progress
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "013-backup-retention-and-object-storage"
  - "024-exclusive-storage-ownership"
  - "026-backup-and-restore-integrity"
  - "033-n3-topology-qualification"
origin:
  retroactive: false
# D-6: B-4 changes what a follower does with a restore instruction it has already honoured
# (`026` B-4's quarantine) and what `restore_backup_finish` returns. `026`'s file is not edited;
# its acceptance block is unchanged and still passes.
amends: ["026-backup-and-restore-integrity"]
establishes:
  # D-3: the public export and restore entry points, which do not exist yet. `backup` is a
  # private module, and a new module keeps the public surface out of it.
  - { kind: file, path: "hiqlite/src/restore.rs", planned: true }
# Added by the lane B change that implements B-4 (F-119, F-120), in the range that changes them.
extends:
  - spec: "013-backup-retention-and-object-storage"
    unit: { kind: file, path: "hiqlite/src/backup.rs" }
    nature: superseding
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/start.rs" }
    nature: superseding
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/app_state.rs" }
    nature: additive
  # D-7: the membership hold while node 1 finishes a restore.
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/network/" }
    nature: additive
  # D-9: the dashboard's writes are held while a restore is finished.
  - spec: "018-dashboard-service-and-ui"
    unit: { kind: file, path: "hiqlite/src/dashboard/query.rs" }
    nature: additive
  # D-7: the in-process suite's second file restore names its own copy of the image.
  - spec: "012-cluster-integration-evidence"
    unit: { kind: file, path: "hiqlite/tests/cluster/backup_restore.rs" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
references:
  - unit: { kind: file, path: "hiqlite/src/init.rs" }
    role: "context"
  - unit: { kind: file, path: "standards/spec/n3-topology-proposal.md" }
    role: "context"
summary: >
  Proposes the hiqlite half of the migration and disaster-recovery procedure in
  033's proposal: a backup image that carries its own identity and digest, an
  offline export from a stopped data directory, a callable restore that is
  applied at most once and refuses the wrong image before it destroys
  anything, and a node-1 restore into an empty cell that either initializes or
  fails with an error, never waits forever. Addresses F-119, F-120, 026 KD-5
  and KD-8 for that procedure. The export's currency rule is a proposal whose
  safety argument and alternatives are in the topology proposal's section 13;
  the committed index it first relied on is not persisted (F-124). In-place
  multi-node restore (F-058) stays unsupported. Lane B (2026-09-23)
  implements B-4 on a branch, each repair with a test observed failing without
  it; B-1 to B-3 and B-5 to B-7 are not implemented.
---

# 034: Restore a backup into a fresh three-voter cell, once, from an image that proves whose it is

## 1. Purpose

`026` B-7 states N=1 as the supported restore topology because nothing
coordinates followers with node 1 during an **in-place** restore. The owner's
migration path avoids that problem by not restoring in place: an existing cell
is quiesced and stopped, its data exported, and the export restored into a new
cell whose followers are **empty**, so there is nothing of theirs to coordinate.

That path still needs things hiqlite does not provide:

- an image that says which cell and cluster it came from, at which applied log
  id, with which content, so a restore can refuse the wrong one (`026` KD-5);
- a way to produce that image from a **stopped** data directory, so both of a
  cell's clusters are captured at one coherent point without quiescing two
  applications (proposal D-12);
- a restore a consumer can **call** rather than reach through an environment
  variable (`backup` is a private module; Rahi 030 D-2 re-implements the unsafe
  0.14 file sequence for that reason), which is applied **at most once**
  (F-119);
- a node-1 restore in a configuration with peers that ends in a single-voter
  leader or in a returned error, not in F-120's unbounded wait.

## 2. Territory

**Establishes** `hiqlite/src/restore.rs`, planned: the public export (B-2) and
restore (B-3) entry points, in a module of their own because `backup` is private
and `027` B-7 froze the public surface this release exposes (D-3).

**Extends, since lane B** (D-6, D-7): `013`'s `hiqlite/src/backup.rs` and
`010`'s `start.rs` (superseding: the restore start and finish are replaced),
`010`'s `app_state.rs`, `003`'s `hiqlite/src/network/`, `018`'s
`hiqlite/src/dashboard/query.rs` (D-9), `012`'s
`hiqlite/tests/cluster/backup_restore.rs` and `005`'s findings register; and
**amends** `026` for the follower quarantine.

Everything else is referenced, for the reason `033` D-1 gives: the
implementing change adds `extends` on `hiqlite/src/backup.rs` (owned by `013`,
superseded in part by `026`), on `hiqlite/src/start.rs` and
`hiqlite/src/init.rs` where it changes them, declares
`amends: ["026-backup-and-restore-integrity"]` for B-7 and KD-5, and carries `026`'s acceptance forward if it changes a line that block asserts
(the rule `026` KD-1 records).

**Boundaries.** hiqlite owns the per-cluster image, its manifest fields, the
export, the restore, and what node 1 does after it. The **cell archive** that
combines both clusters' images with `/data/keys` and a cell-level manifest is
Rahi's (Rahi 030 B-5); this spec gives Rahi the fields it needs and does not
define the archive. Quiescence, the source cell's stop, the fence between cells,
the cutover and the key custody are the consumers' and the operator's. The
membership changes by which nodes 2 and 3 join the restored node 1 are
openraft's protocol through hiqlite's gate, qualified by `033` A-1 and A-7, and
are not re-specified here.

## 3. Behavior

Planned; none of this is in the tree at `72e09a6`.

### B-1. An image carries its manifest

Every backup image, whether taken by `Client::backup` or by B-2, MUST carry a
manifest recording: a cluster identity (an opaque id the operator supplies at
first start and hiqlite persists; its absence is recorded, not invented), the
raft group, the source node id, the **applied log id** of the state machine at
the moment of the copy, a deterministic **content digest** of the database, the
hiqlite version, and the time. The manifest travels with the image and is
covered by its own digest. A consumer's archive MAY embed it; hiqlite MUST NOT
depend on the archive to read it.

### B-2. An offline export from a stopped data directory

*Revised 2026-09-23 (D-4).* The first draft required the export to refuse "if
the state machine's applied log id is behind the last committed entry in the
raft log". The committed log id is not persisted (F-124), so that requirement
cannot be implemented as written. The replacement below is **proposed, not
settled**: it is the topology proposal's section 13 rule A, whose safety
argument, preserved-write statement and alternatives live there, and the
owner's D-12 decides between them.

A public entry point MUST produce a B-1 image from a data directory whose node
is **not running**. It MUST take `024`'s exclusive storage lock first, refuse if
it cannot, and hold it until the image is written and verified; a caller
exporting several directories holds all their locks for the whole operation. It
MUST NOT modify the directory.

It MUST refuse, naming the case, on each of the proposal's refusal cases R-a to
R-e (13.3): identity not matching the expected membership (and, once B-1 exists,
cluster id); no confirmed clean stop (B-7); an unreadable vote, purge frontier,
last log id or applied id, or a WAL that needs repair; applied outside
`[last purged, last log id]`; a joint membership or a membership change in the
uncommitted tail.

For a single-voter directory it MUST also refuse unless `applied == last log
id` (13.4). For a directory of a multi-voter cluster, a single directory cannot
prove currency: the entry point MUST either accept the log state of a majority
of the configuration's voters and apply 13.5's selection rule, or refuse and
leave selection to a documented operator procedure; which of the two is the
implementing change's decision. *Corrected 2026-09-23 (D-5):* log ids compare in
openraft's `LogId` order, term before index; the read set counts voters of the
uniform configuration only, each with a B-7 marker; and a purged prefix counts
as present. No step compares against a committed id, because none is persisted
(F-124).

If the owner adopts the barrier (B-6), the export MUST also refuse an image
whose state does not contain the barrier nonce it is given.

What a passing export establishes is stated in the proposal, 13.4 and 13.5,
with their conditions on storage rollback and `LogSync` mode. The storage lock
establishes only that no cooperating hiqlite process on the same mount holds the
directory while the export runs.

### B-3. Restore is callable, checked, and applied at most once

A public restore entry point, callable only while the node is not running (the
storage lock proves it), MUST:

- verify the image's manifest digest, its content digest, `026` B-5's
  structural checks, and, when the caller supplies an expected cluster identity
  or digest, that they match. **Every refusal happens before anything is
  moved**, and names what differed;
- apply the image through `026` B-3 and B-8's staged, synced, roll-forward
  sequence, unchanged;
- record the applied manifest digest in the data directory as part of the
  committed restore, so a later instruction naming the **same** image is a
  logged no-op, and one naming a **different** image is refused unless the
  caller explicitly asks to replace a restored database.

The `HQL_BACKUP_RESTORE` route MUST go through the same checks and the same
record, so a template that keeps the variable no longer restores on every start
(F-119). At N=1 this changes behavior only by removing the repeat, and the
handoff MUST say so.

### B-4. A fresh-cell restore either forms a single-voter cluster or fails

When node 1 applies a restore in a configuration with peers, the fresh-cell
precondition is that **no peer holds an initialized group**. Node 1 MUST decide
this by `033` B-3's positive-evidence rule; if a peer reports an initialized
group, node 1 MUST return a startup error saying the cell is not fresh,
**before** it moves anything in its own data directory and before any raft
group is initialized, so an operator can correct the cell and retry with
nothing to undo. `restore_backup_finish` MUST NOT wait without a bound:
every wait in it is bounded and its expiry is a returned error (F-120). The
listeners MUST be bound before any wait that another node's action could end.

Followers in the fresh cell receive **no** restore instruction; they are empty
and join by the ordinary path (`033` A-1). A follower that does receive one keeps
`026` B-4's quarantine behavior; this spec does not change it.

### B-5. An upload's outcome can be observed

A caller MUST be able to learn whether the S3 upload of a named image completed
and with which remote digest, without reading the log (`026` KD-8). The shape is
the implementing change's choice; the migration's B-4 step (proposal section 7)
still verifies the remote object independently.

### B-6. A committed barrier, if the owner adopts it

*Proposed 2026-09-23 (D-4); alternative B of the proposal's 13.7; pending D-12.
Narrowed 2026-09-23 (D-5, F-132).* A client entry point MUST commit a replicated
barrier carrying a caller-supplied nonce through the ordinary write path, return
only once it is committed and applied, and record the nonce in the state machine
so that an image proves it (B-1's manifest carries it). It works in a cluster
whose schema the consumer does not own.

What an image containing the nonce establishes: it is faithful to the cluster as
the cluster stood when the barrier committed, with nothing lost, rolled back or
truncated after that, and it is neither another cluster's image nor an older copy
of this one. What it does **not** establish: that the cluster still held, at that
moment, every write acknowledged before it. A loss before the barrier committed
(storage rollback, asynchronous-sync loss, a reverted follower elected at N=3)
leaves a shorter log beneath the barrier, and the check passes (F-132). Every
statement built on this entry point states those historical assumptions or
cites independent evidence (the proposal's 13.7).

### B-7. A clean stop is recorded where an export can read it

*Proposed 2026-09-23 (D-4); defined 2026-09-23 (D-5), per the proposal's 13.10.*
A shutdown that reaches confirmed graceful completion (the topology proposal's
section 4) MUST leave a durable marker in the data directory:

- **identity:** a random run id generated at start after `035` B-1's exclusion,
  the node id, raft group and hiqlite version, and the end state the stop left
  (vote, last purged, last log id, applied log id, last membership log id, a
  digest of the WAL metadata, the active WAL file's id and length, and a digest
  of the SQLite `_metadata` row), covered by the marker's own digest;
- **durability:** written as the stop's last act, after the WAL writer's flush and
  metadata, the SQLite writer's metadata persist and the database's close, by
  temporary name, fsync, rename and directory fsync; the stop reports `Ok(())`
  only after it is durable;
- **invalidation:** removed, durably, by the next start after exclusion and
  before that start's first write to the WAL or the database, so a marker never
  survives into a run that has written;
- **verification:** an export refuses R-b unless the marker is present, its
  digest holds, and every recorded field equals what the export reads.

It proves the directory is as the named run's confirmed stop left it. It does
not prove absence of rollback: a restored copy of the whole directory carries a
marker that describes the copy. Until it exists, the evidence is the consumer's
recorded `Ok(())`, which is bound to nothing in the directory.

## 4. Acceptance, to be implemented

Each scenario runs in `033`'s harness (release build, separate processes,
consumer feature sets) unless it is a unit test, which says so.

- **R-1. Fresh-cell restore.** An image exported from a stopped N=1 node is
  restored on node 1 of an empty three-node cell; nodes 2 and 3 join; every node's
  content digest equals the manifest's.
- **R-2. Repeat is a no-op.** The same instruction on node 1's next start
  changes nothing and logs that; a different image is refused (F-119).
- **R-3. Wrong image rejected before anything moves.** A structurally valid
  image of another cluster, one whose digest does not match, one with a
  truncated body, and one whose manifest was edited: each refused, and the data
  directory byte-identical before and after (`026` KD-5).
- **R-4. Interrupted at every state.** Node 1 killed during staging, after the
  staged commit, during the removals, before first initialization, and during
  node 2's join: each restart either completes the restore once or refuses with
  an error, and no node ever opens a database without its write-ahead log.
- **R-5. Not a fresh cell.** A peer holding an initialized group: node 1 returns
  the B-4 error within its bound and does not initialize (F-120, F-118).
- **R-6. Export refusal cases.** Unit-level, one directory per case: identity
  mismatch; no clean-stop marker; missing log ids; applied outside the frontiers;
  joint membership or a membership entry in the tail; single-voter
  `applied < last log id`. Each is refused, naming the case, with the directory
  unchanged.
- **R-6a. Multi-voter selection.** Three replicas' directories with a tail on one:
  the export either selects per 13.5 or refuses, never exports an unapplied or
  uncommitted state.
- **R-6b. Barrier.** An image without the given nonce is refused; with it,
  accepted (if B-6 is adopted). Also recorded, as the limit and not as a pass:
  a directory rolled back to before an acknowledged write and then given the
  barrier is **accepted** (F-132); the acceptance report states it.
- **R-6c. Marker lifecycle.** A clean stop leaves a verifiable marker; a start
  removes it durably before its first write; a kill after that start leaves none
  and the export refuses; a marker whose recorded end state differs from the
  directory (one field at a time) is refused; a whole-directory copy, marker
  included, is **accepted**, and the report states that the marker cannot detect
  it.
- **R-7. Export does not modify.** The directory's bytes before and after an
  export are identical, and a running node's directory is refused by the lock.
- **R-8. Upload outcome.** Against a local S3 double: success, a failed upload
  and a digest mismatch are each reported to the caller.

## 5. Evidence and its limits

**Lane B (2026-09-23), B-4 only, unit level.** Each repair's test was run first
on the unrepaired behavior and observed failing, with the unrepaired bodies
moved verbatim behind two seams (the instruction as a parameter of
`restore_backup_start_from`, the raft behind the `RestoreRaft` trait), which is
what let a stand-in raft and a temporary directory drive them:
`f119_node_1_applies_an_instruction_at_most_once` (restored again),
`f119_a_follower_quarantines_once_per_instruction` (moved the rejoined state
aside), `f120_a_cell_that_is_not_fresh_is_refused_before_anything_moves`
(restored), `f120_a_raft_that_never_initializes_is_a_bounded_error`,
`f120_no_leader_is_a_bounded_error` and
`f120_a_snapshot_that_is_never_built_is_a_bounded_error` (each still waiting at
the test's 5 s bound), and `f120_a_leader_other_than_this_node_is_an_error`
(the `debug_assert!` panicked). The crash tests construct the on-disk state an
interruption leaves at each point (after the record, after the staged commit,
before completion, during the record's own replacement, during a follower's
quarantine); they do not kill a process. The reordering in `start.rs` and the
membership hold are exercised only by the in-process cluster suite's restore
phases, not by a test that fails without them. R-1 to R-8 have not run.

**After review (2026-09-23, D-9).** Observed failing on the first lane B
commit, then passing: `f119_a_committed_but_unfinished_restore_is_finished_without_the_instruction`
(the start without the instruction returned "nothing to do") and
`f120_at_n1_a_missing_snapshot_or_purge_does_not_fail_the_restore` (a startup
error), the latter behind a seam that added `RestoreRaft::has_peers` unused.
`app_state::restore_hold_tests` failed only by not compiling, since the check did
not exist; it tests the check, not the three places that call it. The teardown's
drain, bounds and aborts have no test that fails without them.

**Before lane B: nothing here had been executed.** What the
acceptance would establish when it passes: the hiqlite half of the procedure,
on one host, against a local S3 double. It would not establish the consumer
archive's coherence, the cell-level fence between old and new, key custody,
cutover, or anything about a real object store, which are Rahi's, the
operator's, and stage 8's.

## 6. Known defects

**KD-1. In-place multi-node restore stays uncoordinated.** F-058 and `026`
KD-2 are unchanged. This spec supports restoring into an empty cell, not into
one whose followers hold state.

**KD-2. The cache groups are not restored, and an older image rolls back
SQLite.** A backup image is the SQLite state machine. A restored cell's cache
groups start empty (cache replacement), and any image older than the source's
last state undoes what was written after it, revocations included (stale
backup). The two threats and the proposed, pending controls D-8a to D-8e are
in the proposal's section 14; this spec restores images and implements none of
those controls.

**KD-3. A cluster identity cannot be proven for a cell that never recorded
one.** Existing N=1 cells were started without B-1's identity. Their first
export records its absence, and a restore of such an image can match only on
digest, which the operator must carry from the export to the restore.

**KD-4. Currency is proven only under stated conditions.** F-124 and F-125 limit
what a stopped directory can prove. Rule A (B-2) preserves acknowledged writes
only without storage rollback and, under asynchronous sync modes, without a host
crash during the source's last run. The barrier (B-6) detects such a violation
only when it happens after the barrier commits (F-132); the marker (B-7) detects
none that copies the whole directory. The migration's zero-loss objective needs
the owner to accept the proposal's 13.7 assumptions, or independent evidence
(13.7 (a) or (b)), under D-9 and D-12.

## 7. Resolved decisions

The D-numbers below are this spec's own. A decision of the proposal's section 11
is always cited as "the proposal's D-n" (its D-7 names the manifest fields, its
D-8 the stale-backup controls); this spec's D-7 and D-8 are unrelated to them.

**D-1 (2026-09-23, fresh cell, not coordinated followers).** The alternative
was to repair F-058 directly: a coordination point through which followers wait
for node 1's validated restore. It is not needed by the owner's migration path,
it is exactly the multi-node restore `026` declined to attempt, and a fresh cell
also gives the migration a disposable target and a byte-identical rollback.
Recorded so a later spec that does need in-place restore starts from here.

**D-2 (2026-09-23, offline export is the migration's source).** Proposal D-12,
recommended and pending the owner's decision. *Corrected 2026-09-23 (D-4):* the
earlier sentence here said a hot backup would make B-1's applied log id "the
watermark a consumer compares across both clusters". Two independent clusters'
log ids share no clock, term or index and cannot be compared; a hot archive
needs the proposal's restore validator (14.4) instead.

**D-3 (2026-09-23, a new public module).** The entry points could be made
public inside `backup`. A separate `restore` module keeps `backup`'s internals
private, makes the addition to `027` B-7's frozen surface one named module, and
gives the implementing change a unit it establishes rather than a private file
it has to open up.

**D-4 (2026-09-23, reconciliation with Rahi decision packet 1).** B-2 rewritten
as a proposal because its committed-index operand does not exist (F-124);
B-6 (barrier) and B-7 (clean-stop marker) added as proposals; KD-2 split into
cache replacement and stale backup; KD-4 added; D-2 corrected. No decision was
taken: D-12 and the D-8 subdivisions stay pending.

**D-5 (2026-09-23, second reconciliation pass).** B-6 narrowed: the first
version said the barrier lets a consumer establish that an image contains every
write acknowledged before it, which holds only if nothing was lost before the
barrier committed; a bounded probe with a SQL row standing in for the barrier
showed the check passing over a rolled-back directory (F-132). B-7 defined:
identity, durability, invalidation, verification and limit. B-2's multi-voter
rule restated against the fields a stopped directory holds. R-6b extended and
R-6c added. KD-4 corrected. No decision taken: D-12, D-7 and the D-8
subdivisions stay pending in the proposal's section 11.

**D-6 (2026-09-23, lane B: the at-most-once record of B-4).** Owner
authorization of lane B, 2026-09-23. B-3's "record the applied manifest digest"
needs B-1's manifest, which lane C adds; lane B records what exists now. The
record is `hiqlite-restore.record` at the data directory root, JSON with a
format number, a state (`pending` or `applied`), the instruction as
`HQL_BACKUP_RESTORE` spells it (`s3:<object>` or `file:<path>`), and on node 1
the SHA-256 of the image it applied. Node 1 writes it `pending` (temporary name,
fsync, rename, directory fsync) after the image is fetched and validated and
before anything of its own is staged or removed; the start that applied it marks
it `applied` only after `restore_backup_finish` returned `Ok`. A follower writes
it `pending` before its quarantine and `applied` after. A later start skips an
instruction that is `applied` and **equal** to the one given, and applies one
that is `pending` or different. Decided, because the spec is silent: the
identity compared is the instruction, and the digest is recorded, not compared,
since comparing it would pull the image from S3 on every start; an object
replaced under the same name is therefore treated as already applied, which
errs toward not destroying data. A different instruction is applied, as before
this change, so N=1 behavior changes only by removing the repeat; B-3's refusal
of a different image without an explicit override is lane C's. An unreadable
record is a startup error, since it alone says whether a destructive restore
happened. The quarantine never moves the record. Re-applying the same image on
purpose needs a different instruction (another name or path) or the record's
removal, an operator act.

**D-7 (2026-09-23, lane B: F-120's ordering, and what the reordering needed).**
`restore_backup_finish` runs after both listeners serve and after both
`become_cluster_member` tasks returned, as B-4 requires, and is bounded: 60 s
each for initialization, a leader and the applied index, and 30 minutes for the
snapshot build, which grows with the database. Expiry, a leader other than node
1 (the former `debug_assert!`), a snapshot that cannot be triggered and a purge
that gives up are each an `Error::Startup`; the last two were only logged
before, and are errors now because the snapshot and the purge are what carry the
restored database to a follower. Serving the listeners first opened a hazard the
old order hid: a follower could be added as a learner before the snapshot and
the purge, and catch up from a log that does not contain the restored data. Node
1 therefore refuses membership changes of its SQLite group while it finishes a
restore (`AppState::restore_hold`, checked in `admit_membership_change`); a
joining node retries that refusal as it retries any other. A start that fails
after its listeners serve now stops the listeners, both raft groups and both
writers, and releases storage ownership (`024`, with `035`'s WAL locks) only if
every component acknowledged its stop; before, it returned with all of them
running. The in-process suite restores the same `file:` image twice under
`TEST_SKIP_S3_RESTORE`, which is now a no-op the second time, so its restore
helper names a fresh copy per restore.

**D-8 (2026-09-23, lane B: the fresh-cell precondition applies to every node-1
restore with peers).** B-4 states it for a fresh cell; the code cannot tell a
fresh-cell restore from `026`'s in-place one, so it applies to both. In an
in-place restore at N>1 node 1 now waits, bounded by `init_peer_wait_secs`, for
enough restarted followers to answer that they are empty, and refuses if any
answers with an initialized group; a follower left running used to be restored
around, silently. That in-place topology stays unsupported (`026` B-7).

**D-9 (2026-09-23, lane B: an independent review's corrections to D-6 and
D-7).**

- *The record has three states, not two.* `pending` is written before anything
  moves; `committed` once the image is in place; `applied` once the start that
  finished it completed. D-6's two states forgot a committed image as soon as the
  operator removed `HQL_BACKUP_RESTORE`: the next start took the ordinary path,
  with no hold, no snapshot and no purge, and a follower then caught up from a log
  without the restored data. A `committed` record now makes node 1 finish the
  restore **whatever the environment says**, without pulling the image again;
  only a different instruction replaces it. A `pending` record without the
  instruction is abandoned with a warning, since nothing had moved.
- *Not ready, and no client writes, while a restore is finished.*
  `become_cluster_member` marks the start finished before the finish runs, so
  `/ready` answered ready and client streams were accepted for as long as the
  finish took (up to 60 s + 60 s + 30 min). `restore_hold` now also refuses
  readiness, the client stream and the dashboard's writes: a write acknowledged
  there would be lost if the finish failed.
- *N=1.* A snapshot that is not built, or a purge that gives up, is logged and
  the restore completes: no follower can catch up from the log at N=1, and D-7's
  fatal treatment made every restart apply the image again while the instruction
  stayed set.
- *The failed-start teardown.* It aborts both join tasks (one left running kept
  retrying and kept the node's state, and its storage ownership, alive), drains
  the membership gate with `SHUTDOWN_DRAIN` before stopping anything (F-107: if
  the drain times out nothing is stopped), and bounds each component's stop at
  10 s. Storage ownership is released only after a drain and every stop within
  its bound.

## Verification

```verify:cli
test -f standards/spec/n3-topology-proposal.md
grep -q 'fresh-cell restore' standards/spec/n3-topology-proposal.md
grep -q '^### F-120 ' standards/spec/findings-register.md
grep -q '^### F-124 ' standards/spec/findings-register.md
grep -q '^## 13. Export currency' standards/spec/n3-topology-proposal.md
grep -q '^### F-132 ' standards/spec/findings-register.md
grep -q '13.10 The clean-stop marker' standards/spec/n3-topology-proposal.md
sh -c 'spec-spine index owner hiqlite/src/backup.rs | grep -q 034-fresh-cell-restore'
grep -q 'const RESTORE_RECORD_FILE: &str = "hiqlite-restore.record";' hiqlite/src/backup.rs
sh -c '! grep -q "debug_assert!(" hiqlite/src/backup.rs'
sh -c 'a=$(grep -n "member_db.await??;" hiqlite/src/start.rs | head -1 | cut -d: -f1); b=$(grep -n "backup::restore_backup_finish(" hiqlite/src/start.rs | head -1 | cut -d: -f1); c=$(grep -n "\"the external API endpoint\"," hiqlite/src/start.rs | tail -1 | cut -d: -f1); test -n "$a" && test -n "$b" && test -n "$c" && test "$c" -lt "$a" && test "$a" -lt "$b"'
grep -q 'fn teardown_started_node' hiqlite/src/start.rs
cargo test -p hiqlite-patched --lib backup::lane_b_tests
cargo test -p hiqlite-patched --lib app_state::restore_hold_tests
grep -q 'Committed,' hiqlite/src/backup.rs
sh -c 'test "$(grep -c "ensure_not_restoring(&state.restore_hold)?;" hiqlite/src/network/api.rs)" -eq 2'
grep -q 'ensure_not_restoring(&state.restore_hold)?;' hiqlite/src/dashboard/query.rs
sh -c 'grep -A40 "async fn teardown_started_node" hiqlite/src/start.rs | grep -q "SHUTDOWN_DRAIN"'
grep -q 'abort_member_cache.abort();' hiqlite/src/start.rs
```
