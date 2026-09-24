---
id: "035-n1-upgrade-exclusion"
title: "Exclude every live node before the legacy cache move, and hand that exclusion to the node that starts"
status: draft
created: "2026-09-23"
owner: "hiqlite maintainers"
risk: critical
implementation: pending
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "001-wal-durability-and-completion"
  - "024-exclusive-storage-ownership"
  - "027-node-lifecycle-and-startup-errors"
  - "031-downstream-release-qualification"
origin:
  retroactive: false
establishes:
  # D-2: the exclusion sequence and the handle of B-6 live in a module of their own; it does
  # not exist yet.
  - { kind: file, path: "hiqlite/src/upgrade_exclusion.rs", planned: true }
  # D-3: the real-version acceptance harness. Workspace-excluded, because it builds hiqlite
  # 0.14 from its upstream Git revision beside the candidate.
  - { kind: directory, path: "qualification/n1-upgrade/", planned: true }
extends:
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
references:
  - unit: { kind: file, path: "hiqlite/src/start.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/store/logs/mod.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/storage_lock.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/src/store/state_machine/sqlite/state_machine.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite-wal/src/log_store.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite-wal/src/lockfile.rs" }
    role: "context"
  - unit: { kind: file, path: "hiqlite-wal/src/writer.rs" }
    role: "context"
  - unit: { kind: file, path: "standards/spec/consumer-handoff.md" }
    role: "context"
summary: >
  The immediate N=1 producer safety item, independent of any N=3 work. A
  hiqlite-patched 0.15.0-patched.1 start with HQL_CACHE_LEGACY_MOVE_ASIDE=true
  moves a live hiqlite 0.14 node's cache directories aside before it discovers
  the 0.14 node's WAL lock (F-126), and over a 0.14 unclean-stop marker it
  moves the cache and rewrites the SQLite raft log before it panics on the
  marker (F-127). This spec is the repair contract: exclusion of every live
  hiqlite node of either version before any rename or authoritative storage
  write; a defined lock lifecycle and handoff with no release-and-reacquire;
  refusals that are errors and say exactly what they left behind; an
  interruption-safe move; and, if the owner adopts it, a public exclusion
  handle so a consumer can run its own steps under the same locks. Records
  what probes on real 0.14 and 0.15 builds established and what they did not.
  Changes no code until an implementing change lands, and changes no
  consumer's pin.
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

- **Establishes**, planned: `hiqlite/src/upgrade_exclusion.rs` (the ordered
  exclusion sequence of B-1 and B-2, and B-6's handle if adopted) and
  `qualification/n1-upgrade/` (the real-version harness of section 5).
- **Extends** `005`'s findings register, additively, with F-126 to F-131 and
  F-133 (F-132 is `033`'s).
- **References**, without claiming, the files the repair will change. They are
  owned by `010`/`024`/`027` (`start.rs`), `007`/`020`/`027` (`store/logs/mod.rs`),
  `024` (`storage_lock.rs`), the SQLite state machine's owners, and
  `001`/`008`/`021` (`hiqlite-wal/src/`). As in `033` D-1, the implementing change
  adds `extends` on each unit it changes, in the same range, and declares
  `amends: ["027-node-lifecycle-and-startup-errors"]` for B-10's ordering,
  carrying forward any `027` acceptance line it changes.

**Boundaries.** hiqlite owns the order in which its own start excludes,
checks and mutates, the messages it returns, the lock files it creates, and the
consent move. Rahi owns its transition verb, `cell.lock`, its state file and
its guard (Rahi 043 B-4, B-5). Rauthy owns how and when it passes the consent
variable to its own node. The operator owns not running two cells on one
volume. Advisory locks exclude cooperating processes on one host and one
filesystem whose locks work (`024`); nothing here excludes a non-hiqlite
writer, another host, or a process on another mount.

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

**Not established by anything here:** Linux `flock` semantics by execution
(the Linux statement in P-5 is from `flock(2)`, not from a run); release builds
under `panic = "abort"`; the Rauthy image; a whole Rahi cell; the interrupted
move of F-130; whether a manual move-aside of a 0.15 cache makes a 0.14 start
safe (the SQLite raft log's compatibility from 0.15 back to 0.14 was not
probed); any behavior of a repaired build, which does not exist.

## 4. Behavior (the repair contract)

Planned. Every statement describes what the implementing change MUST do.

### B-1. Exclusion precedes every mutation, and covers both versions

A start with a data directory runs these steps in this order, and performs no
rename, no write into `logs/`, `logs_cache/`, `state_machine/` or
`state_machine_cache/`, and no SQLite open before step 4 completes:

1. **Owner lock.** `024`'s lock on `hiqlite-owner.lock`, acquired and held.
2. **Legacy WAL locks.** For each WAL directory this start will open
   (`logs/` with `sqlite`; `logs_cache/` with `cache` and
   `cache_storage_disk = true`) **that exists**: open `lock.hql` without
   truncating it, creating it if absent, and take a non-blocking exclusive lock
   on that descriptor. If any lock is held, refuse with `Error::StorageInUse`
   naming the path, which is the lock a live hiqlite 0.14 node (or any
   pre-owner-lock build) holds. A directory that does not exist has no live
   writer; its lock is taken by step 6 before anything is written into it.
3. **Unclean-stop marker.** If `state_machine/lock` exists, refuse with
   `Error::Startup`, not a panic, stating that the previous run of this
   directory, of either version, did not stop cleanly, that nothing was moved,
   and what the operator must establish before removing the marker. (`auto-heal`
   keeps its own policy, and it too runs only after steps 1 and 2.)
4. **Legacy cache check and consent move** (`027` B-10), now under steps 1 and 2,
   in B-5's interruption-safe order.

Only after step 4 may the restore, the reset, the log stores or the state
machines run. `027` B-10's "refused before it is decoded" is unchanged; what
changes is that it is also refused, and moved, only while every live hiqlite
node of either version is excluded.

### B-2. Lock lifecycle and handoff

- **Held, never probed.** Each lock of B-1 is held from acquisition until the
  component that uses the directory stops. A lock-then-release probe is not
  exclusion (P-5), and no step of this contract uses one.
- **Handed over, never re-acquired.** Because a second descriptor in the same
  process cannot take a lock the process already holds (P-5), the log store
  that opens `logs/` or `logs_cache/` MUST adopt the descriptor B-1 locked,
  together with whether `lock.hql` existed before this start created it, which
  keeps today's unclean-start signal. `hiqlite-wal` gains an entry point that
  takes an already-locked lock file; the probe-then-create path stays only for
  callers that pass none. There is no instant between B-1 and the log store's
  start at which a directory's lock is not held.
- **Across the move.** A held lock follows its directory's inode, so after
  `logs_cache` is renamed the lock protects the moved copy and nothing at the
  original path (P-5, P-2). The move therefore: keeps the old lock held; renames;
  creates the new `logs_cache/` and its `lock.hql` and locks it, refusing with
  `StorageInUse` if that fails (a contender created it in between; the refusal
  says the move completed and where the old directories are); syncs both parent
  directories; writes the format marker; and only then releases the old lock.
  The new lock is the one handed to the cache log store.
- **Files this start created.** On any refusal, a `lock.hql` that this start
  created in step 2 is unlinked **while still held**, then released, so the
  directory's entries are what they were. A `lock.hql` that existed before is
  left in place.
- **At a clean stop.** A WAL lock file is unlinked while still held and then
  released, reversing today's release-then-unlink (`writer.rs:725-727`, F-133),
  so no contender can hold a lock on an inode that is about to disappear from
  its path.
- **The owner lock** keeps `024`'s lifecycle: never unlinked, released when the
  process ends.

### B-3. Refusals are errors, and say exactly what they left

- Every refusal in B-1 and B-2 returns an error; none panics. A panic in a
  spawned task converted to `WAL: Generic` is not a refusal.
- The permitted side effects of a refusal are exactly: `data_dir` created if it
  did not exist; `hiqlite-owner.lock` created if it did not exist. Nothing else
  is created, renamed, truncated or written, including after the move has been
  refused.
- The owner note is written only after B-1 has passed, so a refused start
  leaves the previous note untouched.
- A message says what is true: "no data was changed; `hiqlite-owner.lock` was
  created by this start" or "... already existed", replacing "Nothing was
  changed." (`store/logs/mod.rs:163`) and "has changed nothing"
  (`storage_lock.rs:85`) wherever they are not literally true (F-128). A
  `StorageInUse` message says the recorded owner is diagnostic and may belong to
  an earlier, stopped process (P-4).
- A refusal after a completed move says the move completed and names the
  `pre-upgrade-<secs>/` directory.

### B-4. The old version, before and after the repair

- **Refusal leaves the old node whole.** With a live 0.14 node, a repaired
  start with or without consent refuses at B-1 step 2. The 0.14 node goes on
  serving reads and writes, stops with `Ok`, removes its lock files, and
  restarts; the only entry added to its directory is `hiqlite-owner.lock` if it
  was absent, which 0.14 ignores (P-1).
- **Race.** A 0.14 start racing a repaired start: exactly one proceeds. If the
  repaired start holds `logs/lock.hql` first, the 0.14 start panics in its
  `LogStore::start` before it creates `state_machine/lock` (source order above),
  leaving its directory unchanged apart from `logs/` if that did not exist. If
  the 0.14 start holds it first, the repaired start refuses at B-1 step 2.
- **Downgrade is not made safe by this contract.** 0.14 reads no format marker,
  so nothing a 0.15 build does to its own cache stops a 0.14 binary from
  decoding it (P-6: 3 of 3 panics, 2 of 3 destructive). The handoff MUST state:
  after any 0.15 start with consent, the supported way back is restoring the
  pre-upgrade archive into a fresh volume; starting 0.14 on the volume is
  unsupported; and a manual move-aside before a 0.14 start is **not** shown safe
  (not probed). F-129 records it.

### B-5. Interruption and concurrency, from exclusion to startup

- **The move is ordered so that the legacy evidence moves last.**
  `state_machine_cache` is renamed first and `logs_cache` second, so that a crash
  between them leaves the `.wal` files that make the next start take the legacy
  path (refusing without consent, completing the move with it). Today's order
  can leave a 0.14 snapshot for the next start to restore with no refusal and no
  consent (F-130).
- **Every interruption point has a defined next start.** Killed after step 1 or
  step 2: nothing moved; a `lock.hql` created by step 2 may remain, and the next
  start takes today's unclean-start path. Killed during the move: each rename is
  atomic; the next start with consent finishes it, and without consent refuses.
  Killed after the move and before the marker: the next start writes the
  marker. Killed after the marker: an ordinary start. No state leaves a 0.14
  cache log or snapshot where a 0.15 cache raft opens it.
- **Contenders.** Another 0.15 start: owner lock. A 0.14 start: B-1 step 2 and,
  for the window before a directory exists, its creation under lock. A consumer
  holding these locks itself: `StorageInUse` (P-4), unless it hands them over
  through B-6. A non-hiqlite process: not excluded (section 2).

### B-6. A consumer exclusion handle, if the owner adopts it (proposal D-17)

**Today no supported mechanism lets a consumer keep exclusion continuous from its
own locks into hiqlite's start.** A consumer that holds the owner lock or the WAL
lock files makes its own in-process start refuse (P-4), and releasing them first
opens a window in which a 0.14 process can start. Rahi 043's T0 to T3 is exactly
this case. Closing it needs a producer API and a new release. If adopted:

- A public, non-`Clone` handle acquired by a function that runs B-1 steps 1 to 3
  with the same refusals, for a named data directory.
- A method on the handle that performs B-1 step 4's move with B-2's re-lock, so a
  consumer never renames a locked directory itself.
- A start entry point that consumes the handle and adopts its descriptors instead
  of acquiring. At handoff it refuses if the data directory differs
  (canonicalized) or if any held descriptor is no longer the file linked at its
  path (device and inode of the descriptor against the path), which is the
  renamed-or-unlinked case.
- Dropping the handle releases everything. There is no method that releases a
  lock while the handle lives.

It does not add a start without listeners; a consumer that needs its in-process
start unreachable binds loopback.

### B-7. Release and adoption are separate acts

The repair reaches no one until it is released: a new `hiqlite-patched` and
`hiqlite-wal-patched` version (label: proposal D-18), qualified by `031`'s
procedure and published under its own authorization. Consumers pinned at
`=0.15.0-patched.1` do not receive it: Rahi's exact pin (Rahi 043 B-1) changes
only by Rahi's own governed decision, and Rauthy's image by a Rauthy rebuild and
its own release. Nothing here edits either, and the handoff says so.

## 5. Acceptance, to be implemented

Unit and in-process:

- **U-1. Order.** With a lock held by another process on each of the three lock
  paths in turn, a start with consent refuses before any rename: the directory's
  entry list, inode numbers and file bytes are identical before and after, apart
  from B-3's permitted additions.
- **U-2. Marker.** `state_machine/lock` present: `Error::Startup` before any
  rename and before any byte of `logs/` changes. A panic fails the test.
- **U-3. Handoff.** The log store adopts the descriptor; a test that releases
  and re-acquires fails; the unclean-start signal matches whether `lock.hql`
  pre-existed.
- **U-4. Move re-lock.** After the rename, a second process cannot lock
  `logs_cache/lock.hql` at the original path at any step.
- **U-5. Messages.** Each refusal's text names exactly the files it created,
  asserted against the directory listing.
- **U-6. Clean stop.** The WAL lock file is unlinked while held.
- **U-7. Interruption.** A fault point after each step of B-1 and B-5 (a test
  build feature, not a runtime option): the next start behaves as B-5 states, and
  never opens a legacy cache.

Real versions, in `qualification/n1-upgrade/` (0.14 from `8f3b9bd` and the
candidate, each its own process, the consumers' feature sets, release builds;
each scenario three consecutive runs, each run bounded at 60 s, stop at the
first failure, keep that run's directory; assertions on directory entries,
inodes, sizes, lock-file contents and bytes, never on hashes of surviving files
alone):

- **X-1.** P-2 against the candidate: refused at B-1 step 2; the 0.14 node writes,
  reads, stops `Ok` with no panic, and restarts with every row.
- **X-2.** P-1 against the candidate, with B-3's message.
- **X-3.** P-3 against the candidate: `Error::Startup`, `logs/` byte-identical, no
  `pre-upgrade-*`, no `meta.hql~`.
- **X-4.** The race of B-4, ten launches per run at offsets from 0 to 50 ms:
  exactly one proceeds, the other refuses unchanged, the survivor stops `Ok`.
- **X-5.** Interruption: the candidate killed at each of B-5's points; the next
  start matches B-5.
- **X-6.** If D-17 is adopted: a handle acquired, the move made through it, the
  start handed the handle; a 0.14 start attempted between each pair of steps
  refuses unchanged.
- **X-7.** P-6 repeated against the candidate and **recorded**, not passed or
  failed: this contract does not make it safe (B-4).

## 6. Known defects

Recorded, left unfixed here.

- **KD-1. Downgrade.** F-129. A 0.14 binary over a 0.15-written cache panics and
  can tear both raft groups' metadata. 0.14 is not ours to change; B-4 states the
  supported way back.
- **KD-2. Other writers.** Advisory locks do not stop a non-hiqlite process,
  another host, or another mount (`024`).
- **KD-3. Evidence platform.** P-1 to P-6 ran on macOS, APFS, debug builds. The
  Linux container filesystems the consumers deploy on are covered by X-1 to X-7
  when they run there, not before.

## 7. Resolved decisions

**D-1 (2026-09-23, territory is declared by the change that moves it).** As
`033` D-1: the source files the repair changes are referenced now and extended
by the implementing change, so this draft does not become an owning spec of
`start.rs` or `hiqlite-wal/src/` for unrelated ranges.

**D-2 (2026-09-23, a new module).** The ordered sequence spans `024`'s lock, the
WAL's lock files, the state machine's marker and `027`'s move. A module of its
own keeps each owner's file to the adoption of a descriptor, and gives B-6's
handle, if adopted, a unit to live in.

**D-3 (2026-09-23, real-version acceptance outside the workspace).** X-1 to X-7
need hiqlite 0.14 built from upstream Git beside the candidate. A
workspace-excluded directory keeps that graph out of the library's lockfile, as
`033` D-2 does for its harness.

**D-4 (2026-09-23, the contract is producer-side and N=1).** Rahi asked for
exclusion before the move and for the marker check before it. The contract also
covers what the probes found beyond the request: the lock that follows a renamed
directory, the same-process descriptor rule behind Rahi's T0 and T3, the SQLite
WAL rewrite before the marker refusal, the snapshot left by an interrupted move,
and release-then-unlink at stop. It prescribes nothing inside Rahi or Rauthy.

**Owner decisions pending.** D-17 (whether to add B-6's public handle) and D-18
(the release label and whether this repair ships alone) are in the proposal's
section 11. Neither blocks B-1 to B-5.

## Verification

```verify:cli
test -f standards/spec/findings-register.md
grep -q '^### F-126 ' standards/spec/findings-register.md
grep -q '^### F-131 ' standards/spec/findings-register.md
grep -q '^### F-133 ' standards/spec/findings-register.md
grep -q 'CACHE_LEGACY_MOVE_ASIDE_ENV' hiqlite/src/store/logs/mod.rs
grep -q '035-n1-upgrade-exclusion' standards/spec/n3-topology-proposal.md
```
