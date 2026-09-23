---
id: "035-n1-upgrade-exclusion"
title: "Exclude every live node before the legacy cache move, and hand that exclusion to the node that starts"
status: draft
created: "2026-09-23"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "001-wal-durability-and-completion"
  - "024-exclusive-storage-ownership"
  - "027-node-lifecycle-and-startup-errors"
  - "031-downstream-release-qualification"
origin:
  retroactive: false
# D-1: `027` B-10's ordering (the legacy check right after the owner lock) and its refusal
# text are replaced by B-1 and B-3. `027`'s own file is not edited.
amends: ["027-node-lifecycle-and-startup-errors"]
# D-9: `027`'s block pinned the removed call site by name. This spec's block carries it forward
# whole, with those commands replaced, rather than editing a predecessor's acceptance.
amends_verification: ["027-node-lifecycle-and-startup-errors"]
establishes:
  # D-2: the exclusion sequence, the lock lifecycle and the resumable consent move.
  - "hiqlite/src/upgrade_exclusion.rs"
  # The regression tests that run unchanged against the published source (section 5).
  - "hiqlite/tests/upgrade_exclusion.rs"
  # D-3: the real-version acceptance harness. Its own workspaces, so hiqlite 0.14 never
  # enters the library's graph.
  - { kind: directory, path: "qualification/n1-upgrade/" }
extends:
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/start.rs" }
    nature: superseding
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/app_state.rs" }
    nature: additive
  - spec: "024-exclusive-storage-ownership"
    unit: { kind: file, path: "hiqlite/src/storage_lock.rs" }
    nature: superseding
  - spec: "007-cache-log-store"
    unit: { kind: directory, path: "hiqlite/src/store/logs/" }
    nature: superseding
  - spec: "001-wal-durability-and-completion"
    unit: { kind: directory, path: "hiqlite-wal/src/" }
    nature: superseding
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  # The two new files' freshness hash (lint L-008), as `024` and `027` did for theirs.
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
references:
  - unit: { kind: file, path: "hiqlite/src/store/mod.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/lib.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/Cargo.toml" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/store/state_machine/sqlite/state_machine.rs" }
    role: "context"
  - unit: { kind: file, path: "standards/spec/consumer-handoff.md" }
    role: "context"
summary: >
  The immediate N=1 producer safety item, independent of any N=3 work. A
  hiqlite-patched 0.15.0-patched.1 start with HQL_CACHE_LEGACY_MOVE_ASIDE=true
  moves a live hiqlite 0.14 node's cache directories aside before it discovers
  the 0.14 node's WAL lock (F-126), and over a 0.14 unclean-stop marker it
  moves the cache and rewrites the SQLite raft log before it panics on the
  marker (F-127). This spec is the repair contract and, since lane A, its
  implementation: exclusion of every live hiqlite node of either version
  before any rename or authoritative storage write, by WAL locks held from
  before the first mutation until this node's last write and handed to the
  log stores rather than released; refusals that are errors and say exactly
  what they left; a consent move that is one resumable operation and moves the
  legacy evidence last; and, only if the owner adopts it, a public exclusion
  handle. Records what probes on real 0.14 and 0.15 builds established, what
  the repaired build was tested against, and what it was not. Releases nothing
  and changes no consumer's pin.
---

# 035: Exclude every live node before the legacy cache move, and hand that exclusion to the node that starts

## 1. Purpose

`027` B-10 made a hiqlite 0.14 cache log a refusal, with an opt-in,
`HQL_CACHE_LEGACY_MOVE_ASIDE=true`, that moves `logs_cache` and
`state_machine_cache` into `pre-upgrade-<unix seconds>/`. The refusal runs
right after `024`'s owner lock. That ordering assumed every contender takes the
owner lock. hiqlite 0.14 does not: a live 0.14 node holds advisory locks on
`logs/lock.hql` and `logs_cache/lock.hql` and nothing else (Rahi's D-P3,
reproduced here). So the owner lock excludes another 0.15 node and nothing
older, and the consent move runs against a live 0.14 node's directories.

Rahi reported this on 2026-09-23 as a producer request. It is an N=1 hazard of
the release already published: the first start of 0.15.0-patched.1 on an
existing 0.14 volume is exactly when an operator is most likely to have the old
process still running. It does not wait for, and is not part of, the N=3
qualification of `033`.

## 2. Territory

- **Establishes** `hiqlite/src/upgrade_exclusion.rs` (B-1 to B-5: the ordered
  exclusion, the lock lifecycle, the marker precheck and the resumable consent
  move), `hiqlite/tests/upgrade_exclusion.rs` (the regression tests of section
  5 that also run against the published source) and `qualification/n1-upgrade/`
  (the real-version harness of section 5).
- **Extends**, in the range that changes them: `010`'s `start.rs` (the start
  order) and `app_state.rs` (the clean release); `024`'s `storage_lock.rs` (the
  owner note written after the checks, the created-or-found fact, the WAL locks
  kept with the owner lock); `007`'s `hiqlite/src/store/logs/` (the legacy check
  and move leave it; the consent variable and `027`'s named tests stay);
  `001`'s `hiqlite-wal/src/` (a lock acquired without truncation and held, the
  log store's `start_with_lock`, unlink-while-held at a clean stop); `005`'s
  findings register, additively, with F-126 to F-131 and F-133 (F-132 is
  `033`'s).
- **Amends** `027`: B-10's position in the start and its refusal text; and takes
  over `027`'s acceptance block (D-9).
- **References**, without claiming, the unowned files the change touches
  (`hiqlite/src/store/mod.rs`, `hiqlite/src/lib.rs`, `hiqlite/Cargo.toml`) and
  the SQLite state machine, whose marker the precheck reads and does not change.

**Boundaries.** hiqlite owns the order in which its own start excludes,
checks and mutates, the messages it returns, the lock files it creates, and the
consent move. Rahi owns its transition verb, `cell.lock`, its state file, its
fence and the relocation of its app store (Rahi 043 revision 2, B-4, B-4a).
Rauthy owns how and when it passes the consent variable to its own node. The
operator owns not running two cells on one volume. Advisory locks exclude
cooperating processes on one host and one filesystem whose locks work (`024`);
nothing here excludes a non-hiqlite writer, another host, or a process on
another mount.

## 3. Evidence, by class

Four classes, kept apart. The fourth, qualified release behavior, is empty: no
release carries this repair, and no probe below ran a release build.

**Source-established** (this repository at `ec8fc6d`, which carries the
published source `3392c12` unchanged under `hiqlite/` and `hiqlite-wal/`):

- `start_node_inner` takes the owner lock (`start.rs:251`), then runs
  `ensure_cache_log_format` (`:266`), which renames both cache directories when
  consent is given (`store/logs/mod.rs:36-74`), and only afterwards starts the
  SQLite raft, whose `LogStore::start` probes `logs/lock.hql` and panics if it
  is held (`hiqlite-wal/src/log_store.rs:45-50`), and then the SQLite state
  machine, which panics if `state_machine/lock` exists
  (`state_machine.rs:287-316`).
- `LockFile::is_locked` takes the lock on a fresh descriptor and releases it
  when the descriptor drops (`lockfile.rs:33-38`): a momentary probe.
- A clean WAL stop releases the lock and then unlinks the file
  (`writer.rs:725-727`), in that order.
- `move_legacy_cache_aside` renames `logs_cache` before `state_machine_cache`,
  and the legacy check keys on `.wal` files in `logs_cache` only
  (`store/logs/mod.rs:52,151`). A disk-backed cache restores its latest
  snapshot at start (`memory/state_machine.rs:405-411`).
- hiqlite 0.14 (`8f3b9bd`) starts the SQLite `LogStore` (which takes
  `logs/lock.hql`) before it creates `state_machine/lock`
  (`hiqlite/src/store/mod.rs:50-56` there).
- A start always binds its API and raft listeners (`start.rs:103`); there is no
  start mode without them.

**Reported by Rahi** (adoption session, 2026-09-23, disposable directories,
0.14 at `8f3b9bd` and the published 0.15.0-patched.1, Rahi's feature set,
`cache_storage_disk = true`; recorded in Rahi 043 D-P2, D-P3, D-P5): the consent
move before the WAL-lock failure; the live 0.14 node's panic at stop; the
`state_machine/lock` panic after the move; the owner lock created by a refusal
that says nothing changed; two unsafe-downgrade runs, one destructive.

**Reproduced here** (this pass, 2026-09-23, macOS arm64, APFS, debug builds of
the same two probe crates, one host, fixed loopback ports; each scenario run
once unless stated):

| id | scenario | observed |
|---|---|---|
| P-1 | live 0.14, then 0.15 **without** consent | refused in 11 ms (exit 3, `Startup`, "Nothing was changed."). Directory entries and inodes unchanged except one addition: `hiqlite-owner.lock`, 188 bytes, carrying the refused process's pid. The 0.14 node then wrote and read SQL, cache and counter, stopped `Ok`, removed its lock files, and restarted cleanly with all rows. |
| P-2 | live 0.14, then 0.15 **with** consent | refused after 47 ms, but only after renaming the live `logs_cache` (same inode, now under `pre-upgrade-<secs>/`) and `state_machine_cache`, creating a new `logs_cache/` with the format marker, and creating the owner lock. The refusal was a panic in the SQLite `LogStore` on `logs/lock.hql`, surfaced as `WAL: Generic: task panicked`. The 0.14 node stayed usable, and its later cache writes went into the **moved** WAL, whose hash changed after the move. At stop its cache writer panicked on `LockFile removal failed` while `shutdown` returned `Ok` and the process exited `0`; it wrote its cache `meta.hql` into the **new** format-2 `logs_cache/`. Its restart failed (`InitializeError ... vote: T1-N1:committed`), recreated `state_machine_cache/`, and left `state_machine/lock`; afterwards 0.15 with and without consent panicked on that marker. |
| P-3 | 0.14 killed with `SIGKILL`, then 0.15 with consent | cache moved aside; the SQLite raft's WAL file rewritten (same size, different hash) and `logs/meta.hql~` created; then a panic on `state_machine/lock`, exit 101. |
| P-4 | Rahi 043 T0 (owner lock and both WAL lock files, each held on its own descriptor), then an in-process 0.15 start (T3) | refused with `StorageInUse` in 0.27 ms; the note it printed was a previous, stopped run's. T0 left two empty `lock.hql` files; the next plain start succeeded after the "not a clean start" warning. |
| P-5 | advisory-lock semantics (Python `fcntl.flock`, the same `flock` the `fs4` crate uses on Unix) | a second descriptor in the **same** process cannot take a lock the process holds; a held lock stays with a renamed directory's inode, and a new file at the original path is free to another process; a lock on an unlinked file leaves a new file at the same path free; a lock-then-unlock probe leaves nothing held. |
| P-6 | 0.14 writes; 0.15 moves with consent and writes its cache; 0.14 started over that cache with no manual move; three runs | 0.14 panicked in all three (`DecodeError: integer 14, expected variant index 0 <= i < 14`). In two, it left `logs/meta.hql` and `logs_cache/meta.hql` torn (7 and 8 bytes; 0 and 7 bytes, from 32) and `state_machine/lock` behind; 0.15 then refused the directory with `WAL: FileCorrupted: invalid metadata file length`. In one, 0.15 started afterwards with its data. |

**Not established by the probes** (P-1 to P-6): Linux `flock` semantics by
execution; release builds under `panic = "abort"`; the Rauthy image; a whole
Rahi cell; the interrupted move of F-130; whether a manual move-aside of a 0.15
cache makes a 0.14 start safe (the SQLite raft log's compatibility from 0.15
back to 0.14 was not probed). The repaired candidate's tests (section 5) cover
the first, second and fourth on Linux arm64; the others stay open.

## 4. Behavior (the repair contract)

Implemented by lane A (section 5 says what was tested, where and how often).
Every statement describes what the repaired build does and MUST keep doing.

### B-1. Exclusion precedes every mutation, and covers both versions

A start with a data directory runs these steps in this order, and performs no
rename, no write into `logs/`, `logs_cache/`, `state_machine/` or
`state_machine_cache/` other than the lock files of step 2, and no SQLite open
before step 4 completes:

1. **Owner lock.** `024`'s lock on `hiqlite-owner.lock`, acquired and held. The
   file is opened without truncation; the owner note is not written yet.
2. **Legacy WAL locks.** For each WAL directory this start will open (`logs/`
   with `sqlite`; `logs_cache/` with `cache` and `cache_storage_disk = true`):
   create the directory if it is absent (owner-only on Unix), open `lock.hql`
   without truncating it, creating it if absent, and take a non-blocking
   exclusive lock on that descriptor. If a lock is held, refuse with
   `Error::StorageInUse` naming the path: that is the lock a live hiqlite 0.14
   node (or any pre-owner-lock build) holds. Creating an absent directory here,
   rather than when its log store starts, leaves no window in which a 0.14
   process could create and lock it between this check and the log store (D-6).
3. **Unclean-stop marker.** Without `auto-heal`: if `state_machine/lock` exists,
   refuse with `Error::Startup`, not a panic, stating that the previous run of
   this directory, of either version, did not stop cleanly, that nothing was
   moved, and what the operator must establish before removing the marker. With
   `auto-heal` (Rauthy's build: hiqlite's default features) the marker is not a
   refusal: the state machine applies its rebuild policy later, and the consent
   move still runs only under steps 1 and 2.
4. **Legacy cache check and consent move** (`027` B-10), under steps 1 and 2, as
   the resumable operation of B-5.

Then the owner note is written, and only then may the restore, the reset, the
log stores or the state machines run. `027` B-10's "refused before it is
decoded" is unchanged; what changes is that it is also refused, and moved, only
while every live hiqlite node of either version is excluded.

### B-2. Lock lifecycle and handoff

- **Held, never probed.** Each lock of B-1 is held from acquisition until this
  node's last write to the data directory. No step uses a lock-then-release
  probe (P-5).
- **Handed over, never re-acquired.** A second descriptor in the same process
  cannot take a lock the process holds (P-5), so the log store adopts the lock
  B-1 took: `hiqlite-wal`'s `LogStore::start_with_lock` takes the held
  `LockFile`, refuses it when it is no longer the file linked at
  `<dir>/lock.hql` (device and inode of the descriptor against the path), and
  neither releases nor unlinks it. `LogStore::start`, for callers that pass no
  lock, acquires and holds its own, without truncating, and returns an error
  instead of panicking when it is held elsewhere.
- **The unclean-start signal survives** (D-8). Whether `lock.hql` existed before
  step 2 opened it is recorded with the lock and handed to the log store, which
  runs today's deep integrity check exactly when it did before: when a previous
  run left the file.
- **Across the consent move.** A held lock follows its inode, so a renamed
  `logs_cache` takes its lock along and protects nothing at the original path
  (P-5, P-2). The move therefore stages the replacement first: it creates
  `logs_cache.hiqlite-next/`, creates and locks its `lock.hql`, writes the format
  marker and syncs; then renames the legacy `logs_cache` into the operation
  directory (its lock still held, on the moved inode); then renames the staged
  directory to `logs_cache`. The staged lock is the one handed to the cache log
  store. Between the two renames there is no file at `logs_cache/lock.hql`; a
  contender that creates the directory in that instant either makes the second
  rename fail (a non-empty directory: the move stops as incomplete, B-3) or has
  its empty directory replaced and then finds the staged lock held (D-7). A
  `lock.hql` that this start created inside the legacy directory is unlinked
  from its new place while still held, so the preserved directory holds exactly
  its original entries (one that an earlier, interrupted start created stays
  there, empty).
- **After a restore's quarantine or `HQL_DANGER_RAFT_STATE_RESET`**, which move or
  delete the WAL directories wholesale, the lock is taken again on the file now
  at each path before the log stores adopt it, and the lock on the old inode is
  released only after the new one is held. Between the removal and that re-lock
  no lock is held at the path (KD-4).
- **The last writer** (F-133, D-8). The WAL locks live with `024`'s owner lock and
  are released by the same call, at the end of a clean shutdown, after every raft
  group, both WAL writers and the SQLite writer have acknowledged their stop and
  the SQLite writer has removed `state_machine/lock`. Each `lock.hql` is unlinked
  while still held, and only then released; the owner lock is released last and
  never unlinked. The proof, and its limit:
  - *Every hiqlite raft or WAL write precedes the release.* The WAL writers
    have flushed and written their metadata before they acknowledge; the SQLite
    writer has persisted its metadata and removed its marker before it
    acknowledges. No WAL writer releases or unlinks an adopted lock.
  - *Which exclusion remains until then.* All of it: the owner lock (a 0.15
    contender) and both WAL locks (a 0.14 contender, whose first storage step is
    `logs/lock.hql`). Unlinking first means no contender can take a lock on a file
    about to disappear from its path; a contender after the unlink creates a new
    file, which is correct because nothing of this node writes afterwards.
  - *What can still write after the release.* The read pool's SQLite
    connections live until the `Client`'s state is dropped, and the last SQLite
    connection to close may checkpoint. Such a write is serialized by SQLite's
    own inode-level locks, which a contender's SQLite also takes; it is not a
    hiqlite raft or WAL write. This is source reading, not a probe.
  - *Old-version contenders.* A 0.14 start after the release meets a directory in
    0.15's format; that is the downgrade of B-4 and KD-1, not a race this lock
    can close.
- **A start that fails after its log stores opened** drops its storage
  ownership, and with it the adopted locks, possibly before a log store's writer
  has made its last write. Each writer therefore holds a second descriptor on the
  same open file description (`LockFile::share`); an advisory `flock` is released
  only when every descriptor is closed, so the lock stays held until the writer
  too is done, and a share never unlinks (D-11). The lock files stay, so the next
  start takes the deep-integrity path (KD-5). A refusal inside B-1 does not: it
  undoes its creations (B-3).
- **The owner lock** keeps `024`'s lifecycle: never unlinked, released last, on a
  clean release and on any other drop.

### B-3. Refusals are errors, and say exactly what they left

- Every refusal of B-1, B-2 and B-5 returns an error; none panics. A panic in a
  spawned task converted to `WAL: Generic` is not a refusal.
- **Observable effects of a refusal before any move** (D-5). Permanent:
  `data_dir` if it did not exist, and `hiqlite-owner.lock` if it did not exist.
  Transient, undone before the error returns: each `lock.hql` this start created
  (unlinked while still held), and each WAL directory it created (removed if
  still empty). An existing `lock.hql`, an existing owner lock and every other
  file keep their bytes; the owner note is not written. Parent directories'
  modification times change. If an undo fails, the message names what is left.
  Nothing is claimed as "zero writes".
- A message says what is true: "No data was changed" or "has changed nothing"
  only where literally true, and whether `hiqlite-owner.lock` was created by
  this start or already existed. A `StorageInUse` for the owner lock says the
  recorded owner is diagnostic and may be an earlier, stopped process (P-4).
- **A failure after the consent move started** says that the move is
  incomplete, that nothing was deleted, which directory holds its state
  (`pre-upgrade-<secs>.partial/`), that the next start with consent resumes it,
  and not to start hiqlite 0.14 on the directory. A lock taken again after a
  restore or reset that finds a contender says whether this start's move had
  completed and where the legacy cache is.

### B-4. The old version, before and after the repair

- **Refusal leaves the old node whole.** With a live 0.14 node, a repaired
  start with or without consent refuses at B-1 step 2. The 0.14 node goes on
  serving reads and writes, stops with `Ok`, removes its lock files, and
  restarts; the only entry added to its directory is `hiqlite-owner.lock` if it
  was absent, which 0.14 ignores (P-1).
- **Race.** A 0.14 start racing a repaired start: exactly one proceeds. If the
  repaired start holds `logs/lock.hql` first, the 0.14 start panics in its
  `LogStore::start` before it creates `state_machine/lock`, leaving its
  directory unchanged. If the 0.14 start holds it first, the repaired start
  refuses at B-1 step 2 and changes nothing but the owner lock.
- **Downgrade is not made safe by this contract.** 0.14 reads no format marker,
  so nothing a 0.15 build does to its own cache stops a 0.14 binary from
  decoding it (P-6: 3 of 3 panics, 2 of 3 destructive). A format marker 0.14
  does not read is not a downgrade fence. After any 0.15 start with consent, the
  supported way back is restoring the verified pre-upgrade archive into a fresh
  volume; starting 0.14 on the volume is unsupported; a manual move-aside before
  a 0.14 start is **not** shown safe (not probed). F-129 records it; a
  producer-side fence that old code demonstrably respects is a separate
  proposal (proposal section 11, H-5).

### B-5. The consent move is one resumable operation

- **Identity.** A new move first creates `pre-upgrade-<secs>.partial/` and syncs
  `data_dir`. That name is the operation's identity: every later start resumes
  into it and never picks a new timestamp. It takes its final name,
  `pre-upgrade-<secs>/`, only after the new `logs_cache` is in place.
- **Order.** Inside the operation: `state_machine_cache` first; then the log, by
  B-2's staged swap; then the rename to the final name. The legacy log is the
  evidence that makes a start take this path, so it moves last: no interruption
  leaves a 0.14 snapshot at the original path without the log that refuses it
  (F-130).
- **Every interruption has a defined next start** (the fault points of U-7 and
  X-5):

  | killed after | on disk | next start without consent | next start with consent |
  |---|---|---|---|
  | the WAL locks (step 2) | possibly `logs/` and `logs_cache/lock.hql` created, nothing moved | refused as legacy; the leftover lock files are the unclean-start signal | the move from the beginning |
  | `.partial` created | an empty operation directory | refused, naming it | resumes into it |
  | snapshots moved | `state_machine_cache` in the operation directory, the legacy log in place | refused | resumes: moves the log |
  | replacement staged | `logs_cache.hiqlite-next/` with its lock file and marker | refused | removes the stale staging (only if nobody holds its lock and it holds nothing else), stages again, moves |
  | legacy log moved | no `logs_cache`, the legacy log in the operation directory, the staging present | refused | step 2 creates and locks `logs_cache`, which is then marked; the staging is removed; the operation completes |
  | the swap | the new `logs_cache` marked, the operation directory still `.partial` | refused | renames it to its final name |
  | completion | ordinary | ordinary | ordinary; consent is a no-op |

- **Refused, never guessed.** More than one `.partial` directory; a
  `state_machine_cache` both at the source and in the operation directory; a
  legacy log both in place and in the operation directory; a final name that
  already exists (never merged); `logs_cache.hiqlite-next/` without an operation
  beside it, or holding anything but its lock file and marker; a staged lock
  held by someone else. Each is `Error::Startup` naming the paths.
- **Missing sources.** An absent `state_machine_cache` is skipped. A log already
  in the operation directory is not moved again.
- **The published build's interrupted move** (F-130). 0.15.0-patched.1 renamed
  `logs_cache` first. When `state_machine_cache` is at its original path, there
  is no legacy log and no marker in `logs_cache`, and exactly one completed
  `pre-upgrade-<secs>/` holds a `logs_cache` without a `state_machine_cache`,
  the start refuses without consent and, with consent, finishes that move into
  that directory. Cost: a memory-only node of this build that earlier completed
  such a move and is now switched to `cache_storage_disk = true` meets the same
  refusal, and with consent has its (non-durable) snapshots moved aside (KD-6).
- **Contenders.** Another 0.15 start: the owner lock. A 0.14 start: B-1 step 2,
  including the absent-directory case. A consumer holding these locks itself:
  `StorageInUse` (P-4), unless B-6 exists. A non-hiqlite process: not excluded.

### B-6. A consumer exclusion handle, if the owner adopts it (proposal D-17)

**Not implemented; D-17 is not adopted.** Today no supported mechanism lets a
consumer keep lock-based exclusion continuous from its own locks into hiqlite's
start: a consumer holding the owner lock or a WAL lock file makes its own
in-process start refuse (P-4). Rahi 043 revision 2 no longer needs one for its
app store: it holds no hiqlite lock of its own and excludes pre-043 binaries
with a permanent fence at the store's old path (proposal section 15, the H-7
answer). The handle is therefore a separate capability decision, not the
critical path. If adopted:

- A public, non-`Clone` handle acquired by a function that runs B-1 steps 1 to 3
  with the same refusals, for a named data directory.
- A method on the handle that performs B-1 step 4's move with B-2's re-lock, so a
  consumer never renames a locked directory itself.
- A start entry point that consumes the handle and adopts its descriptors instead
  of acquiring, refusing if the data directory differs (canonicalized) or any
  held descriptor is no longer the file linked at its path.
- Dropping the handle releases everything; nothing releases a lock while it lives.

It does not add a start without listeners.

### B-7. Release and adoption are separate acts

The repair reaches no one until it is released: a new `hiqlite-patched` and
`hiqlite-wal-patched` version (label: proposal D-18), qualified by `031`'s
procedure and published under its own authorization. Consumers pinned at
`=0.15.0-patched.1` do not receive it: Rahi's exact pin (Rahi 043 B-1) changes
only by Rahi's own governed decision, and Rauthy's image by a Rauthy rebuild and
its own release. Nothing here edits either. End-to-end N=1 resumability is not
established until a rebuilt Rauthy and Rahi's consumer acceptance have
exercised it.

## 5. Acceptance

### Unit and in-process (`hiqlite/src/upgrade_exclusion.rs`, `hiqlite-wal`)

A lock "held by another process" is held through a second open file
description in the test process, which `flock` treats exactly as another
process's (P-5); the integration file below uses a real second process.

- **U-1. Order.** With a lock held on each of `hiqlite-owner.lock`, `logs/lock.hql`
  and `logs_cache/lock.hql` in turn, a start with consent refuses with
  `StorageInUse` before any rename: entries, inode numbers and bytes are
  identical before and after, apart from the owner lock it may create.
- **U-2. Marker.** Without `auto-heal`, `state_machine/lock` present:
  `Error::Startup` before any rename and before any byte of `logs/` changes.
- **U-3. Handoff.** The log store adopts the held lock; `LogStore::start` on the
  same directory, which is what release-and-reacquire would need, is refused as
  an error; the unclean-start signal matches whether `lock.hql` pre-existed; a
  lock whose file is no longer at the path is refused.
- **U-4. Move re-lock** (restated, D-7). At every step of a consent move the file
  at `logs_cache/lock.hql`, when one exists, is held by this start; in the one
  instant between the two renames there is none, and the staged lock is held;
  a contender that creates `logs_cache` in that instant stops the move as
  incomplete, keeps the legacy log in the operation directory, and its own lock
  file is left alone.
- **U-5. Messages.** Each refusal names what it created and whether the owner
  lock existed, asserted against the directory listing.
- **U-6. Clean stop.** `hiqlite-wal`: the WAL lock file is unlinked while held
  (observed held by another descriptor at the instant of the unlink); a lock the
  caller holds outlives the writer. hiqlite: the WAL lock is still held after
  the log store's writer stopped, and is unlinked and released only by the clean
  release.
- **U-7. Interruption.** A crash injected after each of B-5's seven points (a
  test build's fault point, never a runtime option): the next start without
  consent refuses naming the consent; with consent it completes into one
  `pre-upgrade-<secs>/`, with the legacy log and snapshots byte-identical, the
  new `logs_cache` marked, no staging left and no legacy file where the cache
  raft opens; the start after that is ordinary.

`027`'s three B-10 tests keep their names and now drive this sequence (D-9).

### Regression tests that also run against the published source

`hiqlite/tests/upgrade_exclusion.rs` uses only the public start API, so the
same file runs against `8e4ec4b` (whose `hiqlite/` and `hiqlite-wal/` are the
published `3392c12`) with one test-only dev-dependency added:

| test | published source (Rahi features) | repaired |
|---|---|---|
| `f126_a_live_node_is_refused_before_the_move` | **fails**: with another process holding `logs_cache/lock.hql`, the start with consent *succeeded*, next to the moved directory | passes, for both lock files |
| `f127_the_unclean_marker_is_refused_before_the_move` | **fails**: the start panicked (`Lock file already exists`) after moving the cache | passes |
| `f128_a_refusal_names_what_it_created` | **fails**: "Nothing was changed." beside a created owner lock | passes |
| `f130_an_interrupted_published_move_is_refused_then_finished` | **fails**: started without consent over the 0.14 snapshot | passes |
| `the_consent_move_completes_once` | passes | passes |

Each repair was also checked by reverting it alone: releasing before the unlink
(U-6), dropping the operation identity (U-7), moving the log before the
snapshots (U-7), dropping the lock identity check (U-3) and dropping the undo of
created files (U-5) each made the named test fail. Run on macOS arm64, debug
builds, under Rahi's feature set (`sqlite, cache, counters, dlock,
listen_notify_local, backup, s3`) and Rauthy's (hiqlite defaults, which include
`auto-heal`, plus `cache, cast_ints, counters, dashboard, listen_notify_local,
macros`), `cache_storage_disk = true` throughout. F-127's test is compiled out
under `auto-heal`, where the marker is not a refusal.

### Real versions, in `qualification/n1-upgrade/`

0.14 from `8f3b9bd` and the candidate, each its own process, both consumers'
feature sets, `cache_storage_disk = true`, release builds with `panic =
"abort"`, on Linux, on a Linux filesystem (a container volume). Each scenario
three consecutive runs, each run bounded at 60 s, the invocation at 30 minutes,
4 CPUs, 8 GB memory; stop at the first failure and keep that run's directory;
assertions on directory entries, inodes, sizes, content hashes and lock-file
bytes, and on continued old-node writes and a clean stop, never on hashes of
surviving files alone.

- **X-1.** P-2 against the candidate: refused at B-1 step 2 as `StorageInUse`
  naming the WAL lock and the owner lock; the tree unchanged apart from the owner
  lock; the 0.14 node then writes, stops `Ok` with exit 0 and no panic, and
  restarts with every row.
- **X-2.** P-1 against the candidate: the same, without consent.
- **X-3.** P-3 against the candidate. Rahi's set (no `auto-heal`):
  `Error::Startup` naming `state_machine/lock`, the tree unchanged apart from the
  owner lock, no `pre-upgrade-*`, no `meta.hql~`. Rauthy's set (`auto-heal`): the
  start proceeds under the locks, the move completes into one final directory,
  and the moved legacy WAL files are byte-identical to the killed node's.
- **X-4.** The race of B-4, ten launch pairs per run (twenty processes), offsets
  0 to 50 ms, alternating which side starts first, the candidate with consent on
  a legacy directory: exactly one proceeds; a refused candidate is
  `StorageInUse` and moved nothing; a refused 0.14 left no marker and the moved
  legacy log is byte-identical; the survivor stops `Ok`.
- **X-5.** Interruption: the candidate's fault build aborted at each of B-5's
  seven points; the next start without consent refuses; with consent it
  completes with the rows, one final directory, byte-identical legacy log and
  snapshots, no staging. Twenty-one candidate launches and one 0.14 launch per run.
- **X-6.** Not applicable: D-17 is not adopted, and B-6 does not exist.
- **X-7.** P-6 against the candidate, **recorded**, never passed or failed: B-4
  does not make it safe.

Results: section 5.1.

### 5.1 Results

**Linux arm64, executed 2026-09-23 21:19:56Z to 21:28:20Z.** Docker Desktop's
Linux VM (kernel `6.12.76-linuxkit`, aarch64), `rust:1.95-bookworm`, rustc
1.95.0; the container limited to 4 CPUs and 8 GB, runs on a named volume (ext4
inside the VM, not a host bind mount), network off. Release builds, `panic =
"abort"`, no link-time optimization (the harness's profile; the library's own
release profile adds it). Build time, separate from the run budget: 823 s for
the six binaries, then 101 s after one change to the node binary (below).

| scenario | Rahi's set | Rauthy's set | node launches per run |
|---|---|---|---|
| X-1 live 0.14, candidate with consent | 3/3 pass | 3/3 pass | 3 |
| X-2 live 0.14, candidate without consent | 3/3 pass | 3/3 pass | 3 |
| X-3 0.14 `SIGKILL`ed, candidate with consent | 3/3 pass (refused, tree unchanged) | 3/3 pass (`auto-heal`: moved, legacy WAL byte-identical) | 2 |
| X-4 race, ten pairs per run | 3/3 pass, each launch exactly one proceeding, the side alternating with the start order | 3/3 pass | 21 |
| X-5 interruption at seven points | 3/3 pass | 3/3 pass | 22 |
| X-6 | not applicable (D-17 not adopted) | not applicable | 0 |
| X-7 downgrade, **recorded** | 3 runs | 3 runs | 4 |

Totals: 36 passing runs and 6 recorded, 330 node launches, 503 s of harness wall
time (the cap is 1,800 s), no run near its 60 s bound (longest 31.9 s, X-4), 1.1
MB of volume. No failure, so no retry and no kept failing directory. Evidence:
`~/DevDep/hiqlite-release-artifacts/n1-upgrade-x-2026-09-23/linux-arm64/`
(outside the repository, per its convention).

**X-7, as recorded.** In all six runs hiqlite 0.14 over a directory the candidate
had upgraded aborted (`panic = "abort"`): four in `hiqlite-wal`'s reader
(`reader.rs:122`), two in its store setup (`store/mod.rs:178`). Unlike P-6 (macOS,
debug builds), neither raft group's `meta.hql` was torn (32 bytes each), but
0.14 left `state_machine/lock`. The candidate afterwards refused the directory
under Rahi's set (the marker, B-1 step 3) and started under Rauthy's
(`auto-heal`). Six runs bound nothing; the downgrade stays unsupported (B-4).

**Harness development before the run, not counted.** On macOS arm64 with debug
builds, the driver itself was corrected four times: an exit status leaking from
a loop, `set -e` ending a run before a refused start's exit code was recorded,
`SIGKILL` sent to a wrapper shell instead of the 0.14 process, and BSD `wc`
padding. The fifth defect was in the design of X-4: a winner that stopped at
once could release its locks before the other side launched, so a pair could run
one after the other and "both proceed" (observed on macOS). The node's `race`
mode now holds for one second, and the Linux binaries were rebuilt with it
before the run above. No candidate behavior changed.

**Candidate identity.** The run above tested `3b11e4a`. D-11's follow-up commit
changes the lock lifecycle on failed starts and three messages; it was not
re-run in the harness (section 5's bounds allow no unchanged retry, and a new
sequence needs a new allowance).

**Not executed.** A native Linux amd64 leg: none was available; an emulated
amd64 run on this arm64 kernel, if it completes, is recorded below as what it
is. Real consumer images (Rauthy, a Rahi cell). A kernel crash or power loss at
the fault points (the aborts end the process, not the machine).

## 6. Known defects

Recorded, left unfixed here.

- **KD-1. Downgrade.** F-129. A 0.14 binary over a 0.15-written cache panics and
  can tear both raft groups' metadata. 0.14 is not ours to change; B-4 states the
  supported way back, and X-7 records what the repaired build does.
- **KD-2. Other writers.** Advisory locks do not stop a non-hiqlite process,
  another host, or another mount (`024`).
- **KD-3. Evidence platform.** P-1 to P-6 ran on macOS, APFS, debug builds. The
  real-version acceptance ran where section 5.1 says; any architecture it names
  as not run is not covered.
- **KD-4. Restore and reset re-lock.** A restore's quarantine and
  `HQL_DANGER_RAFT_STATE_RESET` move or delete the WAL directories while this
  start holds locks on them; the locks follow the old inodes. Between the move
  or deletion and the re-lock, a 0.14 process could create and lock the path, in
  which case this start then refuses. Both operations are operator-initiated and
  destructive; closing the window would change `026`'s quarantine and `010`'s
  reset, which is not this spec's.
- **KD-5. A failed start leaves its WAL lock files.** A start that fails after
  its log stores opened drops its WAL locks without the clean release, so the
  files stay and the next start runs the deep integrity check. Previously the
  failed start's WAL writer removed them. Conservative, and recorded because it
  is a change. On non-Unix targets the share relies on the platform keeping a
  lock held across duplicated handles, which was not examined.
- **KD-6. The published interrupted-move detection can refuse a legitimate
  configuration change.** B-5's detection of 0.15.0-patched.1's interrupted move
  cannot tell a 0.14 snapshot from a memory-only snapshot of this build; the
  second case is refused until consent, which moves the non-durable snapshots
  aside.
- **KD-7. SQLite writes after the release.** B-2's last-writer argument stops at
  hiqlite's writers: the read pool's connections close later, and the last close
  may checkpoint. Serialized by SQLite's own locks; not probed.

## 7. Resolved decisions

**D-1 (2026-09-23, territory is declared by the change that moves it).** As
`033` D-1: the draft referenced the files the repair would change; the
implementing change (lane A, 2026-09-23) extends exactly the owned units it
changed and references the unowned ones.

**D-2 (2026-09-23, a new module).** The ordered sequence spans `024`'s lock, the
WAL's lock files, the state machine's marker and `027`'s move. A module of its
own keeps each owner's file to the adoption of a descriptor, and gives B-6's
handle, if adopted, a unit to live in.

**D-3 (2026-09-23, real-version acceptance outside the workspace).** X-1 to X-7
need hiqlite 0.14 built from upstream Git beside the candidate. Two workspaces of
their own under `qualification/n1-upgrade/` keep that graph out of the library's
lockfile, as `033` D-2 does for its harness.

**D-4 (2026-09-23, the contract is producer-side and N=1).** Rahi asked for
exclusion before the move and for the marker check before it. The contract also
covers what the probes found beyond the request. It prescribes nothing inside
Rahi or Rauthy.

**D-5 (2026-09-23, lane A: what a refusal may create).** The draft's B-1 created
missing WAL lock files before later refusals, and its B-3 permitted only
`data_dir` and the owner lock. No ordering satisfies both: excluding a 0.14
contender on a directory without `lock.hql` needs a file there to lock. The
stricter rule is kept for what survives a refusal, and the transient creation is
stated: lock files and directories this start created are undone before the
error returns, lock files unlinked while held, directories only if still empty,
and anything that could not be undone is named. Existing files keep their bytes.
No material guarantee changes: the entries a refused start leaves are the ones
the draft listed.

**D-6 (2026-09-23, lane A: absent directories are locked at step 2).** The draft
left an absent WAL directory to its log store. Between the check and the log
store a 0.14 process could create and lock it, and the start would then fail
late, after the restore and reset had run. Creating and locking it at step 2
closes that window. A strengthening.

**D-7 (2026-09-23, lane A: U-4 restated).** The draft's U-4 ("a second process
cannot lock `logs_cache/lock.hql` at the original path at any step") cannot hold
for any rename-based move: between the legacy directory leaving the path and
its replacement arriving, there is no file to hold. The draft's own B-2 sequence
(rename, then create and lock) had that instant too, and a longer one. The
implementation stages and locks the replacement first, so the instant is two
`rename` calls apart, and a contender in it stops the move with the legacy log
kept. An atomic exchange (`renameat2(RENAME_EXCHANGE)`, `renamex_np`) would
remove it at the cost of per-platform code; not taken. B-2's guarantee (refuse
when a contender took the path) is unchanged; U-4 now states what is tested.

**D-8 (2026-09-23, lane A: the last writer, F-133).** The draft's B-2 reversed
the WAL writer's release-then-unlink. That alone is not enough: the WAL writer
released `logs/lock.hql` before the SQLite writer had finished and removed its
marker, so a 0.14 contender could pass the WAL lock while that writer was still
writing, and meet its marker (a panic, or under `auto-heal` the deletion of the
database being closed). The locks now live
with the owner lock until every writer has stopped (B-2's proof), and are
unlinked while held.

**D-9 (2026-09-23, lane A: `027`'s acceptance carried forward).** `027`'s block
runs three `store::logs::tests` by name, and three commands pin the old call
site: `store::logs::ensure_cache_log_format` before the restore and the SQLite
raft, and called twice in `start.rs`. The tests keep their names and now drive
the whole sequence, with one assertion following B-3's message ("No data was
changed" and the owner lock, where it said "Nothing was changed"). The call-site
commands cannot pass: the check runs inside `upgrade_exclusion::acquire_storage`
and the second call became the re-lock and re-mark after the reset. So this
spec takes over `027`'s block (`amends_verification`), carried whole, with those
commands replaced by the same ordering properties on the new names and marked
as replaced; `027`'s file is not edited.

**D-10 (2026-09-23, lane A: the consent move is resumable).** The draft said a
killed move is finished by the next start with consent. Reversing two renames is
not a recovery protocol: it needs an identity that survives restarts, a rule for
destinations that exist, sources that are missing, a staging directory left
behind, and repeated consent. B-5's `.partial` directory, its table and its
refusals are that protocol; the published build's interrupted state, which has
no `.partial`, is detected separately (KD-6 is its cost).

**D-11 (2026-09-23, lane A: the independent review of `3b11e4a`).** Acted on
all four findings, in a follow-up commit: (1) a defect: a start failing after
its log store opened released the adopted WAL lock when its ownership dropped,
while the writer could still write `meta.hql` and the WAL header; the writer now
holds a share of the lock (B-2), tested by
`a_failed_start_keeps_the_wal_lock_until_the_writer_stops` and
`a_shared_lock_is_held_until_the_writer_stops`, the first observed failing with
the share removed; (2) two refusals said "No data was changed" after a finished
interrupted move or a created `.partial`: now incomplete-move errors; (3) an
incomplete swap claimed every created lock file was removed: it now claims
nothing it did not do; (4) the owner lock could be released before the WAL locks
on an unclean drop: `StorageOwnership` now drops them first. The real-version
run of section 5.1 was on `3b11e4a`, before these; they were verified by the unit
and regression tests only, and restarting the bounded acceptance on the new
commit needs a new execution allowance.

**Owner decisions pending.** D-17 (whether to add B-6's public handle) and D-18
(the release label and whether this repair ships alone) are in the proposal's
section 11. Neither blocked B-1 to B-5.

## Verification

Run with `RUSTUP_TOOLCHAIN=1.95.0 just spine-verify 035-n1-upgrade-exclusion`
(the crate's minimum Rust version). **This block is `027`'s acceptance as well as
this spec's** (D-9), and `027`'s is `010`'s, so `verify 010` and `verify 027`
resolve here: `027`'s block is carried forward whole, with the three commands
that pinned the removed call site replaced by their equivalents and marked. The
real-version harness is not part of this block: it needs a container runtime,
builds hiqlite 0.14 from Git, and runs for minutes under its own bounds (section 5).

```verify:cli
# --- 027's acceptance (and through it 010's), carried forward ---
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
# `035` D-9: was the order of `store::logs::ensure_cache_log_format`; the legacy check now runs
# inside `upgrade_exclusion::acquire_storage`, under the WAL locks, and keeps its place: before
# the restore and before either raft group.
sh -c 'a=$(grep -n "upgrade_exclusion::acquire_storage(" hiqlite/src/start.rs | head -1 | cut -d: -f1); b=$(grep -n "backup::restore_backup_start(&node_config)" hiqlite/src/start.rs | head -1 | cut -d: -f1); c=$(grep -n "store::start_raft_db(" hiqlite/src/start.rs | head -1 | cut -d: -f1); test -n "$a" && test -n "$b" && test -n "$c" && test "$a" -lt "$b" && test "$a" -lt "$c"'
grep -q "fn cache_log_format" hiqlite/src/upgrade_exclusion.rs
sh -c '! grep -q "ensure_cache_log_format" hiqlite/src/store/mod.rs'
# `035` D-9: was two calls of `ensure_cache_log_format` in `start.rs`; the second, after the reset,
# is now the re-lock and re-mark, still after `check_execute_reset` and before the SQLite raft.
sh -c 'r=$(grep -n "init::check_execute_reset(" hiqlite/src/start.rs | head -1 | cut -d: -f1); m=$(grep -n "mark_cache_log_if_unmarked()" hiqlite/src/start.rs | head -1 | cut -d: -f1); c=$(grep -n "store::start_raft_db(" hiqlite/src/start.rs | head -1 | cut -d: -f1); test -n "$r" && test -n "$m" && test -n "$c" && test "$r" -lt "$m" && test "$m" -lt "$c"'
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
# --- what 035 adds ---
test -f standards/spec/findings-register.md
grep -q '^### F-126 ' standards/spec/findings-register.md
grep -q '^### F-131 ' standards/spec/findings-register.md
grep -q '^### F-133 ' standards/spec/findings-register.md
grep -q 'CACHE_LEGACY_MOVE_ASIDE_ENV' hiqlite/src/store/logs/mod.rs
grep -q '035-n1-upgrade-exclusion' standards/spec/n3-topology-proposal.md
test -f hiqlite/src/upgrade_exclusion.rs
test -f hiqlite/tests/upgrade_exclusion.rs
test -x qualification/n1-upgrade/run.sh
test -f qualification/n1-upgrade/old/Cargo.lock
test -f qualification/n1-upgrade/new/Cargo.lock
cargo test -p hiqlite-wal-patched --lib lockfile::tests
cargo test -p hiqlite-wal-patched --lib writer::tests::a_clean_stop_unlinks_the_lock_file_while_holding_it -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::a_caller_held_lock_outlives_the_writer -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache,counters,dlock,listen_notify_local,backup,s3 upgrade_exclusion::tests
cargo test -p hiqlite-patched --no-default-features --features sqlite,cache,counters,dlock,listen_notify_local,backup,s3 --test upgrade_exclusion
cargo test -p hiqlite-patched --lib --features cache,cast_ints,counters,dashboard,listen_notify_local,macros upgrade_exclusion::tests
cargo test -p hiqlite-patched --features cache,cast_ints,counters,dashboard,listen_notify_local,macros --test upgrade_exclusion
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::tests
sh -c '! grep -rl --exclude-dir=target "$(printf "\342\200\224")" specs/035-n1-upgrade-exclusion qualification/n1-upgrade hiqlite/src/upgrade_exclusion.rs hiqlite/tests/upgrade_exclusion.rs'
```
