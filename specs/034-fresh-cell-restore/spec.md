---
id: "034-fresh-cell-restore"
title: "Restore a backup into a fresh three-voter cell, once, from an image that proves whose it is"
status: draft
created: "2026-09-23"
owner: "hiqlite maintainers"
risk: critical
implementation: pending
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "013-backup-retention-and-object-storage"
  - "024-exclusive-storage-ownership"
  - "026-backup-and-restore-integrity"
  - "033-n3-topology-qualification"
origin:
  retroactive: false
establishes:
  # D-3: the public export and restore entry points, which do not exist yet. `backup` is a
  # private module, and a new module keeps the public surface out of it.
  - { kind: file, path: "hiqlite/src/restore.rs", planned: true }
references:
  - unit: { kind: file, path: "hiqlite/src/backup.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/start.rs" }
    role: "context"
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
  and KD-8 for that procedure. In-place multi-node restore (F-058) stays
  unsupported. Changes no code until an implementing change lands.
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

Everything else is referenced, for the reason `033` D-1 gives: the
implementing change adds `extends` on `hiqlite/src/backup.rs` (owned by `013`,
superseded in part by `026`), on `hiqlite/src/start.rs` and
`hiqlite/src/init.rs` where it changes them, declares `amends: ["026-backup-and-restore-integrity"]` for B-7 and KD-5,
and carries `026`'s acceptance forward if it changes a line that block asserts
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

A public entry point MUST produce a B-1 image from a data directory whose node
is **not running**. It MUST take `024`'s exclusive storage lock first and refuse
if it cannot. It MUST refuse, naming both values, if the state machine's applied
log id is behind the last committed entry in the raft log, because an image of a
state machine that has not applied what was committed is not the cluster's
state; the remedy it names is to start and stop the node once. It MUST NOT
modify the directory.

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
every wait in it is bounded and its expiry is a returned error (F-120). The listeners MUST be bound before any wait that another
node's action could end.

Followers in the fresh cell receive **no** restore instruction; they are empty
and join by the ordinary path (`033` A-1). A follower that does receive one keeps
`026` B-4's quarantine behavior; this spec does not change it.

### B-5. An upload's outcome can be observed

A caller MUST be able to learn whether the S3 upload of a named image completed
and with which remote digest, without reading the log (`026` KD-8). The shape is
the implementing change's choice; the migration's B-3 step (proposal section 7)
still verifies the remote object independently.

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
- **R-6. Export refuses a lagging state machine.** Unit-level: a directory whose
  applied id is behind its committed log is refused, naming both.
- **R-7. Export does not modify.** The directory's bytes before and after an
  export are identical, and a running node's directory is refused by the lock.
- **R-8. Upload outcome.** Against a local S3 double: success, a failed upload
  and a digest mismatch are each reported to the caller.

## 5. Evidence and its limits

**Nothing here has been executed; nothing here is implemented.** What the
acceptance would establish when it passes: the hiqlite half of the procedure,
on one host, against a local S3 double. It would not establish the consumer
archive's coherence, the cell-level fence between old and new, key custody,
cutover, or anything about a real object store, which are Rahi's, the
operator's, and stage 8's.

## 6. Known defects

**KD-1. In-place multi-node restore stays uncoordinated.** F-058 and `026`
KD-2 are unchanged. This spec supports restoring into an empty cell, not into
one whose followers hold state.

**KD-2. The cache groups are not restored.** A backup image is the SQLite
state machine. A restored cell's cache groups start empty; what that loses is
each consumer's to enumerate (proposal D-8).

**KD-3. A cluster identity cannot be proven for a cell that never recorded
one.** Existing N=1 cells were started without B-1's identity. Their first
export records its absence, and a restore of such an image can match only on
digest, which the operator must carry from the export to the restore.

## 7. Resolved decisions

**D-1 (2026-09-23, fresh cell, not coordinated followers).** The alternative
was to repair F-058 directly: a coordination point through which followers wait
for node 1's validated restore. It is not needed by the owner's migration path,
it is exactly the multi-node restore `026` declined to attempt, and a fresh cell
also gives the migration a disposable target and a byte-identical rollback.
Recorded so a later spec that does need in-place restore starts from here.

**D-2 (2026-09-23, offline export is the migration's source).** Proposal D-12,
recommended and pending the owner's decision. If the owner chooses hot backup
instead, B-2 is dropped and B-1's applied log id becomes the watermark a
consumer compares across both clusters.

**D-3 (2026-09-23, a new public module).** The entry points could be made
public inside `backup`. A separate `restore` module keeps `backup`'s internals
private, makes the addition to `027` B-7's frozen surface one named module, and
gives the implementing change a unit it establishes rather than a private file
it has to open up.

## Verification

```verify:cli
test -f standards/spec/n3-topology-proposal.md
grep -q 'fresh-cell restore' standards/spec/n3-topology-proposal.md
grep -q '^### F-120 ' standards/spec/findings-register.md
```
