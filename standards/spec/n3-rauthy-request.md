# Request to Rauthy's maintainer: capabilities the N=1 upgrade and the N=3 migration need

Owned by `specs/033-n3-topology-qualification/spec.md`. A record of what this
fork's proposal asks of Rauthy, written 2026-09-23 for Rauthy's maintainer
(`bartekus/rauthy`, line `patched/0.36.2`). Issues are disabled on that
repository, so forwarding this, as a pull-request description, a discussion or
directly, is the owner's act; this file sends nothing. It changes nothing in
Rauthy, asserts nothing about Rauthy beyond what is cited, and authorizes
nothing. The authority for every hiqlite statement is
`standards/spec/n3-topology-proposal.md` ("proposal §n") and its section 11
decision packet.

Rahi's own request to the same maintainer (a terminal-storage signal, an
enumeration of cache-only state, and the missing release assets of
`v0.36.2-patched.2`) is separate and is not repeated here; item 6 overlaps with
its item 2.

**Source basis.** Rauthy was read at `ccf2250` (`rauthy-hq015-worktree`, the only
tree that builds against `hiqlite-patched`) in the first pass. Nothing of Rauthy
was executed by this fork.

## What each item blocks

| # | request | blocks N1 adoption | blocks the N=3 migration | otherwise |
|---|---|---|---|---|
| 1 | tombstone enforcement when Rauthy starts on its own | no | **yes** (proposal 7.2, P-5) | |
| 2 | background-writer control | no | **yes**, unless each job's writes are stated disposable (7.3) | |
| 3 | barrier access for Rauthy's cluster | no | **yes, if** the owner adopts D-12's barrier | |
| 4 | restore-time invalidation of sessions and refresh tokens | no | no, for a final offline export | **blocks** a qualified DR restore from a stale archive (D-8c) |
| 5 | manual IP ban export and re-import | **only if** D-8d is chosen for the upgrade | **yes, if** D-8d is chosen | otherwise the loss is stated (D-8d (ii)) |
| 6 | confirmed cache inventory | no; it fixes the release notes' loss statement (D-8a, D-8e) | no; same | |
| 7 | rebuild on a release carrying `035`'s repair | no; a later improvement | no | closes F-126 for a Rauthy started with consent on its own |

## 1. Tombstone enforcement when Rauthy starts on its own

**Why.** After an export, the migration writes a tombstone into each source data
directory so a re-created source refuses to serve (proposal 7.2, control 5). Rahi
can enforce it on every path that passes through Rahi. A Rauthy started without
Rahi (a split cell's own container, a manual invocation, a debugging pod on the
volume) passes through nothing of Rahi's.

**Requested.** Before Rauthy opens its hiqlite data directory, if a tombstone
file exists at the directory's root at its final name, Rauthy exits non-zero and
names the migration id it contains, and changes nothing. An image entrypoint or
init-container check is acceptable if a check inside Rauthy is not wanted; either
must cover every way the published image starts. The file format is Rahi's to
define; this item needs only "exists at final name, therefore refuse".

## 2. Background-writer control

**Why.** Before activation, the target cell must hold only disposable,
preparatory writes (7.3). Rauthy's scheduled jobs write to its database without
any request.

**Requested.** A list of every scheduled or background task that writes, what
each writes, and whether it may run while the instance is in maintenance; and a
way to start Rauthy with those tasks held until an explicit signal (a
configuration value read at start is enough). Where a task cannot be held, a
statement of why its writes are disposable before activation.

## 3. Barrier access for Rauthy's cluster

**Why.** If the owner adopts the barrier (D-12), each cluster gets one barrier
write after quiescence and drain, recorded outside the cell (proposal 13.7).
Rahi does not own Rauthy's schema and should not write into Rauthy's database.

**Requested, only if D-12 adopts the barrier and hiqlite ships `034` B-6.** An
authenticated, internal-only admin endpoint that commits a hiqlite barrier with a
caller-supplied nonce through Rauthy's own client and returns once it is
committed and applied, with the barrier's log id. **The limit travels with it:**
a barrier proves an export faithful to the cluster as it stood when the barrier
committed; it does not prove that nothing acknowledged earlier was lost before it
(F-132).

## 4. Restore-time invalidation

**Why.** A restore from anything but the final offline export rolls back
Rauthy's SQLite state: session and refresh-token deletions,
`issued_tokens.revoked`, disabled users and clients, rotated client secrets
(proposal 14.2). Rahi's bearer floor covers tokens presented to Rahi only.

**Requested.** An operation, run offline or before the restored instance serves,
that invalidates every session and every refresh token, and a statement of which
other rolled-back state an operator must re-apply by hand. Separately, if Rauthy
rotates signing keys for this purpose, a statement of whether the retired public
key stays in JWKS and for how long: **rotation is not immediate invalidation** for
relying parties that cache JWKS (14.3, D-8c).

## 5. Manual IP ban export and re-import

**Why.** Manual bans live only in Rauthy's cache raft and have no default expiry;
every cache replacement lifts them: the 0.14 to 0.15 move-aside, every restore,
every migration (14.1). D-8d asks the owner whether to preserve them.

**Requested.** Admin-API export of the active manual bans (address or range,
reason, expiry) and an import that re-applies them, idempotently.

## 6. Confirmed cache inventory

**Requested.** Confirm or correct the proposal's 14.1 table from source: every
piece of state that lives only in Rauthy's cache raft, what losing it does, and
which items an ordinary restart already loses (release builds clear only the
`Html` and `App` caches on start, `src/bin/src/server.rs:239-241` at `ccf2250`).

## 7. A rebuild on a release carrying `035`'s repair

**Why.** A Rauthy started with `HQL_CACHE_LEGACY_MOVE_ASIDE=true` while a 0.14
node is live on the same directory moves that node's cache before failing on its
WAL lock (F-126), and over a 0.14 unclean-stop marker it rewrites the SQLite raft
log before panicking (F-127). Rahi covers its own supervisor path by an operator
precondition (Rahi 043 B-5). `035` is the producer repair; it is not released.

**Requested, once a release carries it.** A Rauthy release built on it, through
Rauthy's own qualification. Until then, Rauthy's handoff states the precondition:
stop and remove the old container before the first start with consent.

## A separate source observation, not a request

Read from source only, at `ccf2250`: `DPoPNonce::is_valid`
(`src/data/src/entity/dpop_proof.rs:54-58`) performs a cache lookup that yields
`Result<Option<Self>, hiqlite::Error>` and returns `slf.is_ok()`, so a lookup that
finds nothing evaluates as valid. Whether any caller relies on this function
alone, and whether it is exploitable, was **not** analysed or tested; it is
recorded for the maintainer's own caller analysis and tests, and makes no
severity claim.
