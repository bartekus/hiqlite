# Handoff to Rahi: producer status, N=1 upgrade exclusion, and N=3 reconciliation

Owned by `specs/033-n3-topology-qualification/spec.md`. A record, written for
Rahi's owner and sessions, of where this fork stands against Rahi's current
proposals: Rahi 043 as reviewed at `5707f60` (draft, not approved), Rahi 044 at
`ecd28cc` (draft), and Rahi's producer requests and notes, uncommitted in its
adoption session on 2026-09-23. It changes nothing in Rahi's repository, claims
nothing about Rahi's specs beyond what is cited, and authorizes nothing. The
authority for every hiqlite statement is `specs/035-n1-upgrade-exclusion/spec.md`
for the upgrade hazard, `standards/spec/n3-topology-proposal.md` ("proposal §n")
for everything else, and the findings register; where this summary is terser,
they govern.

**N=3 is not supported by this fork.** N=1 is, and stays the local profile. No
release of this fork carries any repair named below.

**Evidence classes used throughout.** *Source*: read at this repository's
source, which equals the published `3392c12`. *Rahi-reported*: Rahi's probes, as
Rahi recorded them. *Reproduced*: executed by this fork on 2026-09-23 on
disposable directories (macOS arm64, APFS, debug builds, one host; `035`
section 3, P-1 to P-6, and F-132). *Qualified*: behavior of a released build under
`031`'s qualification. **Nothing below is in the qualified class.**

## 1. Decision status

D-14 is decided (two StatefulSets at N=3, the supervised single container at
N=1). Everything else is pending, and all of it is in one table, proposal
section 11: D-1 to D-13, D-8a to D-8e, D-15, D-16, and two added by this pass,
D-17 (a public exclusion handle) and D-18 (the label of the repair release).

## 2. Rahi's request to hiqlite, answered

| Rahi's item | answer | evidence | where |
|---|---|---|---|
| 1. Exclude before moving | **Accepted and widened.** Every live hiqlite node of either version is excluded, by holding (not probing) the owner lock and both legacy WAL locks, before any rename or storage write; the locks are handed to the log stores, never released and re-taken; the move re-locks the new `logs_cache/` before releasing the old lock, which follows the renamed inode | reproduced (P-2), plus what Rahi did not report: the live 0.14 node wrote into the **moved** WAL after the move, and at stop wrote its metadata into the new format-2 directory | `035` B-1, B-2; F-126 |
| 2. `state_machine/lock` before the move, as an error | **Accepted.** Also: today the SQLite raft's WAL is rewritten and `logs/meta.hql~` created before the panic | reproduced (P-3) | `035` B-1 step 3; F-127 |
| 3. Message accuracy | **Accepted**, by creating nothing but the owner lock (and `data_dir`) before a refusal, writing the owner note only after the checks pass, and saying which of those two files this start created | reproduced (P-1, P-4) | `035` B-3; F-128 |
| 4. Record the unsafe downgrade | **Recorded.** Three further runs: 3 of 3 panics, 2 of 3 torn metadata in both raft groups; with Rahi's two, 5 panics, 3 destructive. Whether a manual move-aside before a 0.14 start is safe was **not** probed | reproduced (P-6) | `035` B-4; F-129 |

Your proposed acceptance for item 1 ("every file hash unchanged") is necessary
and not sufficient: the moved directory kept its files' hashes while moving.
`035`'s real-version scenarios assert directory entries, inode numbers, sizes,
lock-file contents and bytes, and that the old node writes, reads, stops `Ok`
without a panic, and restarts.

Found beyond the request: an interrupted consent move can leave a 0.14 snapshot
that the next start restores without refusal (F-130, source only); a clean WAL
stop releases its lock before unlinking the file (F-133).

**Delivery.** The repair is a contract, not code. It needs lane A's
authorization (proposal section 16), then a release (D-18) under its own
publication authorization. **Rahi's exact pin `=0.15.0-patched.1` does not change
by any act of this fork**; moving it is Rahi's governed decision, and Rauthy's
image receives it only through a Rauthy rebuild.

## 3. Rahi 043's T0 to T3: a confirmed conflict

043 B-4's T0 holds `hiqlite-owner.lock`, `logs/lock.hql` and
`logs_cache/lock.hql` until the verb exits, and T3 opens the app store in-process
with hiqlite 0.15. **Reproduced (P-4):** that start refuses with `StorageInUse`
in 0.27 ms. An advisory `flock` belongs to an open file description, so the same
process cannot take a lock it holds on another descriptor (P-5), and
`StorageOwnership` is not public.

**There is no supported mechanism in `0.15.0-patched.1` that keeps exclusion
continuous from Rahi's locks into hiqlite's start.** Every rearrangement using
only the public API that we examined leaves a window in which neither Rahi's
locks nor hiqlite's are held against a 0.14 start; we do not recommend any of
them as exclusion. A continuous handoff needs a producer API and a new release:
`035` B-6, pending D-17, which also lets the handle perform the move so no
locked directory is renamed under a consumer. What Rahi does meanwhile is Rahi's
decision: wait for that release, or state the window as covered only by the
operator precondition 043 B-5 already relies on for Rauthy, which is an
assumption, not exclusion.

Two smaller facts for 043: there is no hiqlite start mode without listeners (a
loopback bind is the available route for T3); and T0's empty `lock.hql` files
remain after the verb exits, so the next 0.15 start takes the "not a clean
start" path (it started normally in P-4; a 0.14 start over such a file was not
probed).

## 4. Corrections to the first version of this handoff

| first version said | now |
|---|---|
| Rahi baseline `b815b18`, decision packet 1 | Rahi 043 at `5707f60` |
| the packet's `iat` floor used 600 s, against a 1800 s default in 038 | 043 uses V = L + 120 s with L read back from Rauthy; the manifest default L = 600 gives V = 720 s |
| a ten-second store allowance is plausible but unmeasured | 043 composes H = 15 s (hiqlite's caller wait), `SERVE_GRACE` 40 s and the pod grace 50 s; all proposals, none measured under 043's workload |
| the barrier alternative "detects" rollback and asynchronous loss | only after the barrier commits; a loss before it passes the check (F-132, reproduced with a SQL-row stand-in) |
| "Nothing below exists at `da4c910`" | nothing in section 7 exists at this commit either |

## 5. Security and consumer ownership

Cache loss and stale-database restore stay separate, and each item is its own
owner decision (proposal 14.3): **D-8a** functional cache loss; **D-8b** Rahi's
bearer floor, which covers tokens presented to Rahi only; **D-8c** revived Rauthy
sessions and refresh tokens after a stale restore, and other relying parties'
outstanding access tokens, for which signing-key rotation is **not** immediate
invalidation while relying parties cache JWKS; **D-8d** manual IP bans; **D-8e**
automatic abuse controls. Accepting D-8a accepts none of the others.

Revocation state Rauthy holds in SQLite is protected from cache replacement,
not from database rollback. The DPoP nonce observation stays a source finding
(section 8, F9).

## 6. Consumer work proposed for N=3

None of it is urgent before the owner decides. Your 044 draft records that D-14
conflicts with your spec 000's `one-deployment-unit` anchor; how that anchor is
treated is Rahi's owner's act, and nothing here depends on its outcome for N=1.

1. The D-14 composition: an internal ClusterIP route to Rauthy over an
   encrypted path, a NetworkPolicy admitting only Rahi's pods, liveness and
   startup independent of Rauthy, readiness failing without it,
   `publishNotReadyAddresses: true` on the peer headless Service (proposal §12).
2. The tombstone, enforced on every startup path including Rauthy on its own
   (§7.2; Rauthy request item 1).
3. Quiescence that also refuses requests proxied to Rauthy, then an in-flight
   drain observed by counters, then background writers held (§13.11, §7.3).
4. If D-12 adopts the barrier: one per cluster after the drain, recorded outside
   the cell, **with its limit stated** (§13.7): it proves the export faithful to
   the cluster at barrier time, not that nothing acknowledged was lost before.
   Stronger evidence is off-cell per-write receipts or periodic recorded
   barriers (§13.7 (a), (b)).
5. The export procedure for split cells, the restore validator for cross-store
   invariants, the bearer floor at activation, the D-7 manifest fields, and the
   correction of `deploy/README.md:265` (the cache group is replicated).
6. Replace 030 D-2's file-level restore with hiqlite's repaired restore once
   `034` makes it callable.

## 7. Unresolved questions

For Rahi's owner and, where marked, Rauthy's maintainer.

- **F1.** Can every source startup path enforce a tombstone, including Rauthy
  started without Rahi? (Rauthy request item 1.)
- **F2.** Which Rahi and Rauthy background writers run without ingress, and can
  each be held until activation? (Rauthy request item 2.)
- **F3.** Does the owner accept the barrier's historical assumptions (§13.7) for
  the migration RPO, or require receipts? With `ImmediateAsync`, every zero-loss
  statement carries §13.4 (ii); D-4 recommends `Immediate` for the SQLite groups.
- **F4.** Which cross-store invariants must hold between Rahi and Rauthy, and
  what is the action for each violation?
- **F5.** Do other relying parties consume Rauthy tokens, and what are their JWKS
  cache policies? (D-8c.)
- **F6.** Manual IP bans: export and re-apply, or accept their loss, at the Track
  N1 upgrade as well as at migration? (D-8d; Rauthy request item 5.)
- **F7.** Is restore-time invalidation acceptable? (D-8c; Rauthy request item 4.)
- **F8.** Which rule will a split-cell export use: §13.5's majority rule or an
  operator procedure?
- **F9** (Rauthy maintainer). `DPoPNonce::is_valid` returns `slf.is_ok()` on a
  cache lookup, read at `ccf2250`; callers and exploitability were not analysed.
- **F10.** For 043: wait for `035` B-6, or state T0 to T3's window as an
  operator precondition (section 3)?

## 8. What hiqlite plans to provide

Nothing below exists at this commit; each is a planned unit, subject to the
owner's decisions and to its own authorized implementation (proposal section 16).

- **A (N=1, now):** `035`'s exclusion repair, and the public handle if D-17
  adopts it; then a release under D-18.
- **B:** the F-118, F-119 and F-120 repairs and the configurable pre-shutdown
  delay (`033` B-3, B-4; `034` B-4).
- **C:** a public offline export and a checked, at-most-once restore (`034` B-1 to
  B-3), the clean-stop marker (B-7), and a barrier entry point if D-12 adopts it
  (B-6).
- **D, E:** a real-node harness and bounded qualification tranches (`033` B-2,
  B-6).

## 9. Evidence status

- **Executed for this pass:** `035`'s probes P-1 to P-6 and the barrier probe of
  F-132, on disposable directories, with the limits stated there; the pinned
  `spec-spine` gates and the documentation verification blocks of the specs this
  pass changed (reported with the change that carries this file).
- **Not executed:** any repaired build (none exists), Linux, release builds under
  `panic = "abort"`, the Rauthy image, a whole Rahi cell, the interrupted move of
  F-130, a manual-move downgrade, M-N1, M-S3, the harness, Kubernetes acceptance,
  the rehearsal.
