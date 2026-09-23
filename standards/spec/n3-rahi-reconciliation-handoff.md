# Handoff to Rahi: N=3 reconciliation with decision packet 1

Owned by `specs/033-n3-topology-qualification/spec.md`. A record, written for
Rahi's owner and sessions, of where this fork's N=3 proposal stands after being
reconciled with Rahi's "decision packet 1: patched dependency adoption"
(2026-09-23, Rahi baseline `b815b18`). It changes nothing in Rahi's repository,
claims nothing about Rahi's specs beyond what is cited, and authorizes nothing.
The authority for every hiqlite statement is
`standards/spec/n3-topology-proposal.md` (cited as "proposal §n") and the
findings register; where this summary is terser, they govern.

**N=3 is not supported by this fork.** N=1 is, and stays the local profile.

## 1. Decision status

Decided means recorded by the owner; everything else is a proposal.

| id | subject | status |
|---|---|---|
| D-14 | at N=3 the cell is two StatefulSets (`rahi`, `rauthy`), one hiqlite node per pod; the single-container supervised composition stays the N=1 profile | **decided 2026-09-23** |
| D-16 | two adoption tracks: **N1** (published patched builds, N=1, now) and **S7** (an N=3-qualified release, later) | pending |
| D-15 | five separated controls and activation by authoritative mutation, with a downstream tombstone (proposal §7.2, §7.3) | pending |
| D-8a to D-8e | cache-replacement and stale-backup security controls (proposal §14.3) | pending |
| D-12 | export currency: rule A, barrier B, commit metadata C (proposal §13) | pending |
| D-1 to D-11, D-13 | architecture, replacement, cache on disk, `LogSync`, transport, pre-delay, manifest, RPO/RTO, version pinning, upgrades, log-revert flag | pending |

## 2. The two tracks

**Track N1 (Rahi packet D1 to D5).** It does not depend on the N=3 work, and
the N=3 work does not depend on it. It claims N=1 only. Two of this pass's
corrections apply to it now:

- **Packet D2, cache transition.** The move-aside empties both cache groups.
  Rahi's bearer deny-lists (038 B-5) go with them; the packet's `iat` floor is
  the same control as the proposal's D-8b and should use the **reported**
  maximum bearer lifetime (038 B-6), not a constant: the packet says 600 s, while
  038 records a Rauthy default of 1800 s. On the Rauthy side the same event lifts
  every IP ban, including manual ones with no default expiry, and resets the
  failed-login escalation counters (proposal §14.1). Revocation state in Rauthy
  lives in SQLite and is not affected by the move-aside.
- **Packet D4, shutdown budget.** hiqlite's "5 s drain" is a bound reached only
  with a membership change in flight, and "about 15" is the caller's wait bound;
  neither is a measured duration. At N=1 the 9.5 s pre-delay is skipped, so a
  ten-second store allowance is plausible but unmeasured. `Client::shutdown`
  returning `Err(Timeout)` is **unconfirmed completion**: the stop may still be
  running, and it fails graceful-within-budget acceptance. Record each stop as
  confirmed completion, unconfirmed completion, or forced exit (proposal §4).

**Track S7.** Only a release that passed the proposal's stages 4 to 6 makes a
hiqlite N=3 result apply to the cell. Nothing in this handoff asks Rahi to
start it.

## 3. Consumer work proposed to Rahi

For N=3 (Track S7 and D-14), none of it urgent before the owner decides:

1. **D-14 composition spec** amending 031 and 032 for N=3 only: routing to
   Rauthy through an internal ClusterIP Service over an encrypted path; a
   NetworkPolicy admitting only Rahi's pods; liveness and startup independent of
   Rauthy; readiness failing without it; `publishNotReadyAddresses: true` on the
   peer headless Service (proposal §12).
2. **Tombstone** (proposal §7.2, control 5): written by the export job, and
   enforced on **every** startup path: `serve`, `supervise` before it spawns
   Rauthy, `preflight`, `migrate`, `backup`, `restore`, the backup CronJob and
   migrate Job, and Rauthy started on its own (which needs a check in the Rauthy
   image or in Rauthy). Removal only by a logged rollback.
3. **Quiescence** that also refuses requests proxied to Rauthy (Q-1).
4. **Background writers** held off until activation, or each one's writes stated
   and shown disposable (proposal §7.3).
5. **Barrier** per cluster after quiescence, recorded outside the cell, if the
   owner adopts D-12's alternative B.
6. **Export procedure** for split cells: both StatefulSets to 0, workloads
   excluded, every stop's outcome confirmed, every lock held throughout
   (proposal §12.3, §13).
7. **Restore validator** for skewed hot archives: cross-store invariants with a
   defined action each (proposal §14.4). Log ids of the two clusters are not
   comparable and are provenance only.
8. **Bearer floor** at activation for migration, restore and upgrade (D-8b).
9. **Manifest fields** of D-7 in the cell archive.
10. **Correct `deploy/README.md:265`**: the cache group is replicated, and holds
    the deny-lists and the leases behind Rahi's fencing tokens.
11. Replace 030 D-2's file-level restore with hiqlite's repaired restore once
    `034` makes it callable.

## 4. Unresolved security and fencing questions

For Rahi's owner and, where marked, Rauthy's maintainer.

- **F1.** Can every source startup path enforce a tombstone, including Rauthy
  started without Rahi? If one cannot, which control covers it?
- **F2.** Which Rahi and Rauthy background writers run without ingress, and can
  each be held until activation? Until answered, the target is not provably
  disposable before activation (proposal §7.3).
- **F3.** Is the barrier (D-12 B) acceptable, or does the owner accept rule A's
  conditions (no storage rollback; `LogSync::Immediate` or no host crash during
  the source's last run)? The zero-loss objective depends on the answer.
- **F4.** What cross-store invariants must hold between Rahi and Rauthy, and what
  is the action for each violation? Needed before any hot archive is a
  qualified restore input.
- **F5.** Do other relying parties consume Rauthy tokens? The bearer floor covers
  Rahi only; outstanding access tokens elsewhere are covered only by signing-key
  rotation (D-8c).
- **F6.** Manual IP bans (D-8d, a separate owner decision): export and re-apply,
  or accept their loss? (Rauthy maintainer: an admin-API export and import.)
- **F7.** A restore from a stale image rolls back Rauthy session and
  refresh-token deletions and `issued_tokens.revoked`. Is restore-time
  invalidation acceptable (D-8c)? (Rauthy maintainer: the capability.)
- **F8.** The migration's source is N=1 and exported by the N=1 rule; exports
  from a split cell need the majority rule or the barrier (proposal §13.5).
  Which one will Rahi's procedure use?
- **F9** (Rauthy maintainer; separate from D-8). Read from source only at Rauthy
  `ccf2250`: `DPoPNonce::is_valid` returns `slf.is_ok()` on a cache lookup, so a
  nonce that was never issued evaluates as valid. Callers and exploitability
  were not tested or assessed.

## 5. What hiqlite plans to provide, and when

Nothing below exists at `da4c910`; each is a planned unit of `033` or `034`,
subject to the owner's decisions and to a separate implementation change.

- F-118, F-119, F-120 repairs; the configurable pre-shutdown delay (`033` B-3,
  B-4; `034` B-3, B-4).
- A public offline export and a checked, at-most-once restore (`034` B-1 to
  B-3), a barrier entry point if D-12 adopts it (B-6), and a clean-stop marker
  (B-7).
- A real-node harness and bounded acceptance (`033` B-2, B-6). No run is
  authorized by this handoff.

## 6. Evidence status

- **Executed for this pass:** the pinned `spec-spine` gates and the
  documentation verification blocks of `005`, `033` and `034` (reported with the
  change that added this file).
- **Not executed:** every runtime acceptance scenario, M-N1, M-S3, the harness,
  Kubernetes acceptance, the rehearsal. The facts about Rahi and Rauthy were read
  from source, not run.
