# Handoff to Rahi: producer status, N=1 upgrade exclusion, and N=3 reconciliation

Owned by `specs/033-n3-topology-qualification/spec.md`. A record, written for
Rahi's owner and sessions, of where this fork stands against Rahi's current
proposals. **Third pass, 2026-09-23**, against Rahi 043 revision 3 (`c2c7c72`,
its review `abd66fd`, its owner packet `fc3f339`; draft, not approved) and
`docs/design/04-patched-adoption-producer-requests.md` version 2 (requests H-1
to H-8), all committed on Rahi's `corpus/043-patched-dependency-adoption`. It
changes nothing in Rahi's repository, claims nothing about Rahi's specs beyond
what is cited, and authorizes nothing. The authority for every hiqlite statement
is `specs/035-n1-upgrade-exclusion/spec.md` for the upgrade hazard,
`standards/spec/n3-topology-proposal.md` ("proposal §n") for everything else,
and the findings register; where this summary is terser, they govern.

**N=3 is not supported by this fork.** N=1 is, and stays the local profile.

**Evidence classes.** *Source*: read at a named revision. *Rahi-reported*: Rahi's
probes, as Rahi recorded them. *Reproduced*: executed by this fork on disposable
directories (`035` section 3, P-1 to P-6; F-132). *Candidate-tested*: the
unreleased repair, tested as `035` section 5 says, on the platforms it names.
*Qualified*: a released build under `031`'s qualification. **Nothing below is
in the qualified class, and no release carries the repair.**

## 1. Decision status

D-14 is decided. Everything else is pending, in one table, proposal section 11.
Changed by this pass: **D-17** (the public exclusion handle) is now recommended
"when a consumer asks", because 043 revision 2 onward holds no hiqlite lock of
its own and does not need it; **D-8b** is restated for 043's permanent floor and
lifetime ceiling; **D-18** is the release label, to be confirmed free at
publication; **D-19** is new: whether hiqlite builds a producer-side layout
fence (H-5's first option, proposal §17).

## 2. Rahi's requests H-1 to H-8, answered

| request | state | evidence | where |
|---|---|---|---|
| H-1 continuous exclusion | **Implemented in the candidate**, as asked and wider: the WAL locks are created if absent and held from before the first mutation until the node's last write, handed to the log stores, re-established on the new `logs_cache` by a staged swap before the legacy lock is released, and unlinked while held at the clean release | candidate-tested: U-1, U-3, U-4, U-6; a test that fails on the published source (there a start with consent *succeeded* beside another process holding `logs_cache/lock.hql`); X-1, X-2, X-4 (section 5.1 of `035`) | `035` B-1, B-2; F-126 |
| H-2 marker before the move, as an error | **Implemented** without `auto-heal`. With `auto-heal` (hiqlite's defaults, so Rauthy's build) the marker stays the rebuild policy, reached only under the locks | U-2, X-3 | `035` B-1 step 3; F-127 |
| H-3 truthful refusals | **Implemented**: the owner note is written only after the checks; each refusal says whether it created `hiqlite-owner.lock`; WAL lock files and directories a refused start created are removed again | U-1, U-5 | `035` B-3; F-128 |
| H-4 downgrade and racy marker refusal on record | **Recorded**: F-129; and 0.14's pre-lock startup phase and marker-refused writes in the H-7 answer below | | F-129, section 3 |
| H-5 a downgrade boundary the old binary meets | Option 2 **stated** (`035` B-4; the consumer handoff when the repair is released). Option 1 is **a separate proposal, not built**: proposal §17 and D-19 | source; P-6; X-7 recorded | `035` B-4, proposal §17 |
| H-6 publish the reconciliation | **Done** by the pull requests that carry this file | | |
| H-7 review of revision 3's fence route | **Answered**, section 3 | source, with the limits stated there | |
| H-8 F-130 on the normal crash path | **Implemented in the candidate**: one resumable operation, snapshots moved before the log, every interruption point defined, and the published build's interrupted state detected and refused or finished | U-7, X-5, and a test that fails on the published source (it started over the 0.14 snapshot) | `035` B-5; F-130 |

**What H-1 and H-8 still need before Rahi's B-4b can close**, which is Rahi's
gate and not ours: a published `hiqlite-patched` release carrying the repair
(D-18, then a separate publication authorization), and a Rauthy image rebuilt on
that release with its own leg J. **Rahi's exact pin `=0.15.0-patched.1` changes
by no act of this fork.** The candidate's identity and interface are in
section 8; they are not for pinning.

## 3. H-7: an adversarial reading of revision 3's fence route

Read against hiqlite 0.14 at `8f3b9bd` (the build Rahi v0.1.0 and v0.2.0 use:
`sqlite, cache, counters, dlock, listen_notify_local, backup, s3`, no
`auto-heal`) and `0.15.0-patched.1`, with the repaired candidate where it
differs. **Source only** unless marked; no interleaving below was executed by
this fork. File references are to `8f3b9bd` unless marked.

**Verdict.** Revision 3's route is sound for the population it names (pre-043
Rahi binaries: hiqlite 0.14 without `auto-heal`, node id 1) **under three
configuration exclusions that 043 does not yet state** (Q2): no
`HQL_DANGER_RAFT_STATE_RESET` and no `HQL_BACKUP_RESTORE` in any pre-043
environment on the volume, because either makes a starting 0.14 process
destructive before it takes any lock or meets the marker, where no quiescence
check can see it. Two smaller corrections: debris and evidence names must be
unique per occurrence (Q4), and the relocation target should leave hiqlite's
`pre-upgrade-*` namespace (Q5). Nothing here blesses the consumer algorithm
beyond these five questions.

**Q1. Can a 0.14 path without `auto-heal` remove, truncate or bypass
`state_machine/lock`, other than D-P11's two?**

- The marker check is `File::open` then `File::create`
  (`store/state_machine/sqlite/state_machine.rs:258-291`); `File::create`
  truncates, it does not use `O_EXCL`. D-P11's first case is exactly this, and
  the property that makes it safe is that 0.14 starts the SQLite `LogStore`,
  which takes and holds `logs/lock.hql`, **before** the marker check
  (`store/mod.rs:50-56`, `hiqlite-wal/src/log_store.rs` at `8f3b9bd`). T1(c)'s
  probe after the guard is published therefore sees any 0.14 that could have
  passed the check. Keep T1's order (publish, fsync, then probe, then re-verify
  identity); reversing any two reopens the case.
- The only removal by a stopping 0.14 is its SQL writer's
  `remove_lock_file(path)` (`writer.rs:688`), by path, once, after its
  metadata persist and `PRAGMA optimize` and before its connection closes.
  While the marker is still there, T1(b)'s `link` gets `EEXIST`. Once it is gone
  it is not removed again. D-P11's second case, confirmed.
- **Bypass before the marker, by configuration.**
  `HQL_BACKUP_RESTORE` is read before any lock or marker
  (`start.rs:52`, `backup.rs:273-287`): on a node id other than 1 it runs
  `remove_dir_all(data_dir)`, which removes the fence with everything else; on
  node 1 it removes `state_machine/db`, the snapshots and `logs/`, calls
  `remove_dir_all` on the marker path (a regular file: on rustc 1.95's standard
  library this fails with `ENOTDIR` and leaves the file, checked here on macOS;
  v0.2.0's toolchain, 1.96.0, was not checked), writes the restored database
  under the fence, and then panics on the marker. `HQL_DANGER_RAFT_STATE_RESET`
  (`init.rs:28-69`) sleeps 10 s holding nothing, then deletes `logs/`,
  `logs_cache/` and both snapshot directories, and keeps the marker. v0.2.0's
  own preflight reports `HQL_BACKUP_RESTORE` as a failure
  (`crates/rahi-cli/tests/cli.rs:504-530` at `v0.2.0`); whether `serve` refuses
  it was not checked.
- With `auto-heal` (not in Rahi's builds; it is in hiqlite's defaults, so in an
  upstream Rauthy): the marker is not a refusal: 0.14 deletes `state_machine/db`
  and serves an empty store at the legacy path, and its clean stop then removes
  the marker. That population is outside the fence's claim.

**Q2. Does a stopping 0.14 node write under its data directory after its SQLite
`-wal` is removed, and is "`-wal` and `-shm` absent, both WAL locks free, marker
absent" sufficient?**

- 0.14's stop order (`client/mgmt.rs:315-370`): cache raft, cache WAL writer
  (release, then unlink its lock file: F-133's order), SQLite raft, SQLite WAL
  writer (the same), SQL writer (metadata, `PRAGMA optimize`, marker removal),
  then the writer's connection closes; the read pool's connections close when
  the `Client`'s state is dropped. 0.14 runs `journal_mode=WAL`
  (`state_machine.rs:366`), so `-wal` and `-shm` go only with the last
  connection. After that nothing of hiqlite holds the directory open: every
  writer has stopped and every SQLite connection is closed. The one thing that
  could still run is the embedding process's own code, which is Rahi's to know.
  **For a stopping node, the evidence is sufficient** with the default and
  with every 0.14 configuration we found.
- **For a starting node it is not.** Between process start and
  `LogStore::start`, a 0.14 start holds no lock, has no marker and no SQLite
  file open, and so passes all three checks. Without the two variables of Q1
  that phase writes nothing, and the process then meets the guard, which T1
  published before it probed: safe. With either variable it is destructive
  inside that invisible phase (Q1). **Counterexample**, smallest: a 0.14 start
  with `HQL_DANGER_RAFT_STATE_RESET=true` begins its 10 s sleep; T1 publishes the
  guard and probes (free, no `-wal`, no `-shm`); T2 has not yet moved `logs/`;
  the sleep ends and 0.14 deletes `<legacy>/logs`, the SQLite raft log T2 was
  about to relocate. State the exclusion (neither variable in any pre-043
  environment on the volume) or detect the process by other means; no check of
  the directory can.
- F-133 at the probe: a 0.14 WAL writer releases before it unlinks, so T1(c)'s
  probe can open the file in between and read it free. Correct in effect: that
  writer has finished.

**Q3. Is a 0.14 data directory relocated entry by entry to a new `data_dir`
sound for 0.15?** Yes, from source, under three conditions. Nothing hiqlite 0.15
persists is keyed by the data directory's path: the raft logs and metadata, the
SQLite `_metadata`, snapshots and backups carry node ids, addresses, log ids and
file names; the owner lock's note records the canonical path for diagnosis
only. The conditions: `filename_db` unchanged; `state_machine/db` moved as one
directory, so the database, `-wal` and `-shm` stay together (T2 does); and the
marker never relocated (T2 keeps it at the fence; at the app store a present
`state_machine/lock` is 0.15's unclean-stop marker). A `lock.hql` a crashed 0.14
left and T2 moved makes 0.15 run its deep integrity check once, which is
correct.

**Q4. What can 0.14's racing WAL task write under a marker-refused start beyond
D-P7 and D-P14?** In order, before the marker check: `create_dir_all(logs/)`
(mode 0700 on Linux), `lock.hql` created or truncated and locked,
`Metadata::read_or_create` writes `meta.hql`, and the writer spawn reads the WAL
set, checks integrity (deep, if `lock.hql` pre-existed) and creates the first
WAL file at full `wal_size` if the set is empty. Then
`StateMachineSqlite::new` creates `state_machine/db/`, `backups/` and
`snapshots/` and sets 0700 on them and on `state_machine/`, before it panics on
the marker. The panic ends the start; the WAL writer, on channel close, writes
its header and metadata and unlinks `lock.hql`, unless the process aborts first
(then `lock.hql` stays). The cache raft is never reached. All of it is under the
legacy path; none of it touches the app store. Plus Q1's two variables, which
act before any of this. **For T2's debris rule:** these entries appear at source
paths T2 may already have moved; a second refused start creates them again, so
evidence names must be unique per occurrence. `rename` onto an existing empty
directory replaces it silently and onto a non-empty one fails, so a fixed
`evidence/<id>/logs` is not enough across retries.

**Q5. Does 0.15 treat an unknown entry under its `data_dir` as its own?** The
published build looks only at `logs/`, `logs_cache/`, `state_machine/`,
`state_machine_cache/` and `hiqlite-owner.lock`; a restore's quarantine
(`026`) moves every other entry too, and `HQL_DANGER_RAFT_STATE_RESET` deletes
only `logs/`, `logs_cache/` and the two snapshot directories. The **repaired**
build additionally reserves `pre-upgrade-<secs>.partial/` and
`logs_cache.hiqlite-next/`, and inspects completed `pre-upgrade-*` directories
to detect the published build's interrupted move (`035` B-5, KD-6): it acts only
when `state_machine_cache` is at its original path, `logs_cache` holds no
marker, and exactly one `pre-upgrade-*` holds a `logs_cache` without a
`state_machine_cache`. Revision 3's target holds both caches, so it never
matches. Still, `<app store>/pre-upgrade-<instant>/` sits in hiqlite's own
namespace; a Rahi-specific prefix removes the question for every later hiqlite
version. `<data>/upgrade-cache/evidence/` is outside the app store and invisible
to hiqlite.

**What Rahi relies on from the producer**, collected: 0.14 takes
`logs/lock.hql` before its marker check and holds it for its life; 0.14 without
`auto-heal` never removes the marker except by its own clean stop; 0.14 with
neither Q1 variable writes nothing before `LogStore::start`; 0.14 closes SQLite
last; 0.15 persists no path; 0.15 opens only its configured `data_dir`.
**Filesystem assumptions:** one filesystem for `<data>`, the legacy path and the
app store (T2 checks `st_dev`); atomic `rename` and `link`; durable directory
`fsync`; working `flock` for the probe. **Configuration exclusions:** no
`auto-heal` in a pre-043 build; node id 1; neither Q1 variable in a pre-043
environment on the volume.

## 4. Corrections to earlier versions of this handoff

| earlier version said | now |
|---|---|
| Rahi 043 at `5707f60`; T0 holds hiqlite's locks, and T0 to T3 conflict | 043 revision 2 onward holds no hiqlite lock (its T0 takes `cell.lock` and `transition.lock`); the conflict is gone, and F10 is closed |
| wait for `035` B-6's handle, or state an operator precondition | revision 3 fences the app store; the handle is a separate capability (D-17), not on Rahi's path |
| V = L + 120 s with L read back from Rauthy, and a floor that lifts at `before + V` | 043's floor is permanent and only rises; admission enforces the manifest's lifetime ceiling; pruning uses a separate horizon. No historical or read-back lifetime enters |
| Rauthy's `v0.36.2-patched.2` release has no assets | eight assets, listed by authenticated and anonymous reads; binary digests match 043 D-P1 (re-checked by this fork on 2026-09-23). A consumer still verifies digests before pinning |
| the repair is a contract, not code | a candidate exists, unreleased (section 8) |
| the barrier alternative "detects" rollback | only a rollback that crosses a recorded barrier (F-132, proposal §13.13) |

## 5. Security and consumer ownership

Each item is its own owner decision (proposal 14.3): **D-8a** functional cache
loss; **D-8b** Rahi's bearer floor and ceiling, which cover tokens presented to
Rahi only; **D-8c** revived Rauthy sessions and refresh tokens after a stale
restore, and other relying parties' outstanding access tokens, for which
signing-key rotation is **not** immediate invalidation while relying parties
cache JWKS; **D-8d** manual IP bans; **D-8e** automatic abuse controls. Accepting
D-8a accepts none of the others. Revocation state Rauthy holds in SQLite is
protected from cache replacement, not from database rollback.

## 6. Consumer work proposed for N=3

Unchanged in substance, none urgent before the owner decides: the D-14
composition (§12); the tombstone on every startup path (§7.2); quiescence, an
in-flight drain observed by counters, and background writers held (§13.11,
§7.3); if D-12 adopts the barrier, one per cluster after the drain with its
limit stated (§13.7), and recurring recorded barriers only with every nonce
checked (§13.13); the split-cell export procedure, the restore validator, the
bearer floor at activation, the D-7 manifest fields, the `deploy/README.md`
correction; and replacing 030 D-2's file-level restore once `034` is callable.

## 7. Unresolved questions

- **F1 to F9** as in the second version (tombstone, background writers, the
  barrier's assumptions, cross-store invariants, other relying parties, manual
  bans, restore invalidation, split-cell export rule, DPoP). Rauthy's
  maintainer is answering several from Rauthy's side on an unpublished branch;
  this fork cites none of it until it is published.
- **F10.** Closed (section 4).
- **F11.** Does 043 state Q2's configuration exclusions, or detect a pre-043
  process in its pre-lock phase by other means?

## 8. What hiqlite provides now, and what it does not

- **A1, the `035` repair: implemented, candidate only.** Commit `048fcec` (`3b11e4a` plus the independent review's fixes, `035` D-11) on
  branch `fix/035-n1-upgrade-exclusion` of `bartekus/hiqlite`. Public API unchanged;
  behavior: refusals are errors (`StorageInUse` for a held lock, `Startup` for
  the marker, the legacy cache and an incomplete move), messages name what the
  refusal created, the consent move is resumable and names its operation
  directory `pre-upgrade-<secs>.partial/` until complete. Not published; not for
  pinning. Test hooks exist only behind the internal feature
  `__upgrade-fault-points`.
- **Not provided:** the public handle (D-17), a downgrade fence (D-19), the
  release (D-18 and a publication authorization), any N=3 repair (lane B, not
  authorized), export and restore (lane C, gated on D-12).

## 9. Evidence status

- **Executed for this pass:** `035` U-1 to U-7 and its regression tests, on macOS
  arm64 debug builds under both consumers' feature sets, including the same
  regression file against the published source; the real-version harness on
  Linux as `035` section 5.1 records, with its platform legs; the pinned
  `spec-spine` gates.
- **Not executed:** Q1 to Q5's interleavings (source); any interleaving on a
  real Rahi cell; the Rauthy image on the candidate; any architecture section
  5.1 names as not run; Kubernetes; the rehearsal.
