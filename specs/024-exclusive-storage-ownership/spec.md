---
id: "024-exclusive-storage-ownership"
title: "Take an OS advisory lock on the data directory before anything touches it"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "002-snapshot-publication-and-recovery"
  - "010-node-lifecycle-and-split-brain"
  - "013-backup-retention-and-object-storage"
amends: ["002-snapshot-publication-and-recovery"]
# D-5: this spec's `## Verification` block IS 002's acceptance from now on, and 002's own file
# is not edited. Whole-block replacement is the mechanism's unit.
amends_verification: ["002-snapshot-publication-and-recovery"]
amends_sections:
  - "5-internal-startup-and-exclusive-ownership"
  - "7-known-defects"
establishes:
  - "hiqlite/src/storage_lock.rs"
extends:
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/start.rs" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/app_state.rs" }
    nature: additive
  - spec: "013-backup-retention-and-object-storage"
    unit: { kind: file, path: "hiqlite/src/backup.rs" }
    nature: additive
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/client/" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Repairs F-005. A node now takes an exclusive OS advisory lock on its data
  directory before the restore, the state machines, or anything else touches
  it, and holds it until every task that can write storage has stopped. A
  contender is refused without mutating anything, a crash releases ownership
  with no cleanup step, and the lock file is the one thing a restore sweep may
  not delete.
---

# 024: Take an OS advisory lock on the data directory before anything touches it

## 1. Purpose

`002` section 5 describes what exclusive ownership was: a marker file at
`state_machine/lock`, tested with `File::open` and then created with
`File::create`, empty, removed on orderly shutdown, and removed again by a
restore. `002` states plainly that it "is not an operating-system advisory lock
and file creation is not exclusive", and puts the obligation on the operator.
F-005 is that gap, and `000` section 7 places it on hiqlite's side of the
ownership boundary.

The register also carries an **observation** against it, recorded on 2026-09-21
and explicitly not an experiment: two instances of the `sqlite-only` example
were started against one data directory by accident, the second was not refused,
and on shutdown the two raced on a lock file and one panicked. One occurrence
establishes neither reproducibility nor the general behavior. It does establish
that the failure mode is a panic during shutdown rather than a refusal at
startup, which is the wrong end of the process to find out.

One responsibility: **one live process per data directory, established by the
operating system rather than by convention.**

## 2. Territory

**Establishes** `hiqlite/src/storage_lock.rs`, which no spec claimed.
`spec-spine.toml` gains its freshness declaration: the file is inside the
`hiqlite` package walk, so it enters the coverage denominator on its own, but
package membership does not feed the freshness hash and without the entry a byte
change here would leave the committed index reporting `fresh`.

**Extends**, additively, four units it has to touch: `start.rs` and
`app_state.rs` (`010`), where ownership is taken and held; `backup.rs` (`013`),
whose restore sweep must now leave one file alone; and `hiqlite/src/client/`
(`003`), whose shutdown sequence releases ownership at the right point.

**Amends** `002` sections 5 and 7 and carries its acceptance (D-5).

**Ownership boundary, and it is narrow.** This establishes that two processes on
one machine cannot own one directory on a filesystem whose advisory locks work.
It is **not** a distributed lease, **not** object-store writer fencing, and
**not** a guarantee about any network filesystem. Section 5 says which
arrangements it cannot speak for.

## 3. Behavior

### B-1. Ownership is taken before anything touches storage

`start_node_inner` acquires it as its first storage-affecting act, **before**
`restore_backup_start`, before the reset check, and before either state machine
is constructed. That order is the point: a restore deletes the database, the
snapshots and the logs, and `auto-heal` startup deletes the live database
directory. A contender that discovered it had lost *after* doing either of those
would already have destroyed what it lost to.

A node that keeps nothing on disk takes no lock. That is exactly one
configuration: a cache-only node with `cache_storage_disk = false` built with
`in-memory-snapshots`, which never touches `data_dir` at all. Every other
combination writes a database, a WAL, or a snapshot, and takes the lock.

### B-2. The lock is an OS advisory lock on a file at the data directory root

`{data_dir}/hiqlite-owner.lock`, opened read-write with `create(true)` and
**without** truncation, locked exclusively through `fs4`, which is already a
dependency of `hiqlite-wal` and of the external engine.

Two placement decisions:

- **At the root, not inside `state_machine/`.** A restore removes
  `state_machine/` wholesale. A lock file a restore can unlink is not a lock: the
  holder would go on locking an inode nobody can reach while the next process
  created a different file, and both would believe they owned the storage.
- **Never unlinked, and never truncated on open.** There is no removal step on
  any path. The file persisting is not a stale-lock problem, because the lock is
  the open file description and not the file's existence.

The file's contents are a note for an operator: process id, hostname, an RFC 3339
timestamp, and the canonical data directory path. It is **read back only to put
into an error message**. A process id can be reused, a hostname can be a
container's, and a canonical path is a useful diagnostic and not a proof of
storage identity. Nothing decides anything on it.

### B-3. A refusal changes nothing, and says so

A contender that loses gets `Error::StorageInUse`, mapped to `409 Conflict`,
naming the directory, stating that the node has changed nothing, and quoting the
owner note. Nothing else in the directory has been opened, created or removed at
that point.

An advisory lock the filesystem **rejects** is a different case and is refused
just as explicitly: hiqlite says it cannot establish exclusive ownership of that
storage and does not start on it. Treating an unsupported arrangement as
"probably fine" is what section 5 exists to prevent.

### B-4. Ownership is released when nothing can write any more

The guard lives in `AppState`, and `shutdown_execute` releases it **last**: after
both raft groups have shut down, after the WAL writer's shutdown handle has
returned, and after the SQLite writer has acknowledged its own shutdown. Those
are the three things in the process that write to the data directory, and
releasing before any of them had stopped would let a second node in while the
first was still writing.

If that sequence never runs, dropping `AppState` releases it anyway, and so does
the process ending for any reason at all. **There is no cleanup step that has to
succeed**, which is precisely what the marker file could never say: a marker is
removed by a shutdown path that a crash skips, which is why an unclean shutdown
used to leave one behind and `auto-heal` used to treat that as permission to
wipe the database.

### B-5. The restore sweep keeps the lock file

`restore_backup_start`'s non-leader branch used to `remove_dir_all` the whole
data directory. It now removes the entries individually, skips the owner lock,
and **returns** its errors instead of discarding them: a node that could not
clean its own directory must not proceed into a cluster join with a half-removed
one.

That second half is a piece of F-058 and is done here because B-2's placement
decision requires it. The rest of F-058, that a follower destroys its data on an
environment variable alone and before node 1 has validated anything, is not
repaired here.

### B-6. The old marker keeps its job

`state_machine/lock` is unchanged. It answers "did the last run end cleanly",
which is what `auto-heal` reads, and that is a different question from "is
someone running now". The two were conflated because only one file existed.

## 4. Evidence and its limits

Ten tests in `storage_lock.rs`. Four of them use **two real processes**: the
test binary re-runs itself with an environment variable that turns one otherwise
inert test into the child.

- a second process is refused, and the sentinel file it would have deleted is
  byte-identical afterwards;
- this process is refused while a **child** holds the lock, and the refusal
  carries the owner note;
- an orderly shutdown releases ownership, and the next owner gets it;
- a **crash** releases ownership: the child calls `abort()`, so no destructor,
  no unwinding and no cleanup step runs, and ownership is still available.

Four more cover the identity questions:

- a restore is refused while ownership is held, and the live database file is
  untouched;
- a **second node in the same process** is refused, which a lock keyed on a
  process id, a canonical path or a global registry would have allowed;
- an **aliased path** to the same directory is refused, through a symlink, which
  is right because the lock is on the inode and nothing reasons about paths;
- dropping the guard releases ownership in-process.

What the acceptance does **not** establish:

- **No node is started.** Every test drives `StorageOwnership` directly. That
  `start_node_inner` calls it before the restore is a source change, and the
  `#[cfg]` selecting which configurations take a lock at all has no test.
- **Only this machine's filesystem.** Everything ran on one local filesystem on
  one operating system. Section 5 states what that does and does not cover.
- **Descriptor inheritance is reasoned about, not tested.** Rust opens files
  `CLOEXEC` on Unix, so a `fork` + `exec` child does not inherit the lock; a
  plain `fork` child would. No test forks.
- **Nothing is killed mid-write.** The crash test aborts a process that is
  holding the lock and doing nothing else.
- **The release ordering in B-4 is not executed.** That shutdown sequence needs
  a running node.

## 5. Known defects

**KD-1. Network filesystems are not established and are not detected.** `fs4`
uses `flock` on Unix and `LockFileEx` on Windows. On NFS without a working lock
daemon, on some overlay and container filesystems, and on several distributed
filesystems, `flock` can succeed on two nodes at once. hiqlite cannot tell the
difference between a lock that works and one that silently does not, and it does
not try. **A shared network volume is an unsupported storage arrangement for a
hiqlite data directory**, and this repair does not change that.

**KD-2. A plain `fork` child inherits the lock.** The lock belongs to the open
file description, which a forked child shares. A parent that forks after
acquiring ownership has two processes holding one lock, and neither is refused.
`CLOEXEC` covers `fork` + `exec`; it does not cover `fork` alone.

**KD-3. Ownership is per directory, not per database.** Two nodes configured with
different `data_dir` values that resolve to overlapping storage through nested
paths are not detected. The lock answers "this directory", not "any storage
reachable from this directory".

**KD-4. The refusal is at startup only.** Nothing revalidates ownership while a
node runs. If a filesystem drops the lock underneath a live process, which KD-1
says can happen, nothing notices.

**KD-5. `002`'s other four known defects are untouched.** Non-atomic publication,
install-before-restore, no fallback to an older snapshot, and the missing
deterministic interruption test are W-20's, not this spec's.

## 6. Resolved decisions

**D-1 (2026-09-21, a new file rather than locking the existing marker).**
Locking `state_machine/lock` would have reused a path a restore deletes and would
have conflated "the last run ended cleanly" with "someone is running now". Two
questions, two files (B-6).

**D-2 (2026-09-21, `fs4`, not a hand-rolled `create_new` lock).** Owner direction
was to prefer an existing suitable dependency. `fs4` is already in the tree
twice, for the WAL's lock file and for the external engine's `owner.lock`, and a
`create_new` lock is exactly the marker this spec is replacing: it needs a
removal step, and a removal step is skipped by a crash.

**D-3 (2026-09-21, released explicitly at the end of shutdown rather than only
on drop).** Drop alone would have kept a restarted node in the same process out
until the last `Arc<AppState>` happened to go away, which is not an observable
moment. The explicit release has a stated position in the shutdown order (B-4)
and the drop remains as the backstop.

**D-4 (2026-09-21, refuse an unsupported filesystem instead of warning).** A
lock error is not a lost race; it means the exclusion cannot be established at
all. Starting anyway would restore exactly the situation F-005 describes while
appearing to have fixed it.

**D-5 (2026-09-21, this block is `002`'s acceptance).** All five of `002`'s
commands are carried forward unchanged.

## 7. Out of scope

- **Snapshot publication, installation and restart selection.** W-20 and `002`'s
  other known defects.
- **Distributed leases and object-store writer fencing.** Named as out of scope
  by owner direction and by `000`'s scope discipline.
- **The rest of F-058**, the follower that destroys its data on an environment
  variable alone.
- **`auto-heal`'s policy.** B-6 leaves the marker and its meaning alone.
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 024`. **This block is `002`'s acceptance as well as
this spec's** (D-5). `002`'s own file is not edited, and `spec-spine verify 002`
prints the attribution line naming this spec before it runs a command.

```verify:cli
# --- 002's acceptance, carried forward unchanged ---
cargo test -p hiqlite --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::restart_reconstructs_snapshot_then_replays_retained_wal -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::interrupted_staging_files_are_not_published_snapshots -- --exact
cargo test -p hiqlite --lib --no-default-features --features external-state-machine external_state_machine::tests::snapshot_evidence_restore_receipts_and_staleness -- --exact
cargo test -p hiqlite --lib --no-default-features --features external-state-machine external_state_machine::tests::online_backup_snapshot_preserves_implicit_rowids -- --exact
cargo test -p hiqlite --lib --no-default-features --features external-state-machine external_state_machine::tests::durability_is_explicit_and_unclean_replayable_off_fails_closed -- --exact
# --- what this repair adds ---
cargo test -p hiqlite --lib --no-default-features --features sqlite storage_lock::tests::a_second_process_is_refused_without_touching_the_data -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite storage_lock::tests::this_process_is_refused_while_another_process_holds_it -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite storage_lock::tests::an_orderly_shutdown_releases_ownership -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite storage_lock::tests::a_crash_releases_ownership -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite storage_lock::tests::a_contender_is_refused_while_ownership_is_held_even_to_restore -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite storage_lock::tests::a_second_node_in_the_same_process_is_refused -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite storage_lock::tests::an_aliased_path_to_the_same_directory_is_refused -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite storage_lock::tests::dropping_the_guard_releases_ownership -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite storage_lock::tests::the_owner_lock_file_is_recognisable -- --exact
# the lock is an OS advisory lock, taken before the restore, and released last
sh -c 'grep -q "FileExt::try_lock" hiqlite/src/storage_lock.rs'
sh -c 'grep -q "StorageInUse" hiqlite/src/error.rs'
sh -c 'grep -q "release_storage_ownership" hiqlite/src/client/mgmt.rs'
# the follower restore path must never wipe the data directory wholesale again
sh -c '! grep -q "fs::remove_dir_all(node_config.data_dir.as_ref())" hiqlite/src/backup.rs'
```
