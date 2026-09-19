---
id: "002-snapshot-publication-and-recovery"
title: "Define snapshot publication and recovery"
status: draft
created: "2026-09-18"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "001-wal-durability-and-completion"
origin:
  retroactive: true
  paths:
    - "hiqlite/src/store/state_machine/sqlite/"
    - "hiqlite/src/external_state_machine.rs"
    - "hiqlite/tests/cluster/self_heal.rs"
establishes:
  - { kind: directory, path: "hiqlite/src/store/state_machine/sqlite/" }
  - "hiqlite/src/external_state_machine.rs"
  - "hiqlite/tests/cluster/self_heal.rs"
references:
  - { unit: { kind: file, path: "hiqlite/src/config.rs" }, role: "recovery configuration" }
summary: >
  Records internal SQLite snapshot contents, publication, installation, and
  restart recovery, then distinguishes the separate external state-machine
  snapshot and ownership contract.
---

# 002: Define snapshot publication and recovery

## 1. Purpose

Snapshots connect a compacted state-machine image to the applied-log frontier that OpenRaft uses for further replay
and log retention. Hiqlite owns the image, metadata persistence, local publication and install steps, unclean-start
handling, and storage ownership assumptions. OpenRaft owns when the internal Raft group requests or installs a snapshot
and how later log entries are replicated.

External state-machine mode has no Hiqlite Raft group. Its snapshot image is local evidence that the caller must bind
into a caller-owned consensus snapshot. The two modes share SQLite machinery but not consensus ownership.

## 2. Internal SQLite snapshot contents

The internal snapshot is a complete SQLite database created with `VACUUM main INTO`. Before creation, the sole writer
sets `last_snapshot_id` and persists `StateMachineData` in the `_metadata` table. The copied image therefore contains:

- application tables and SQLite schema at one writer-queue point;
- `last_applied_log_id`;
- `last_membership`, including the log identifier attached to the most recently applied membership entry;
- `last_snapshot_id`, which must match the UUID filename selected during recovery.

The OpenRaft `SnapshotMeta` returned by the builder is derived from the image metadata. The image does not contain
retained Raft log entries. OpenRaft can replay entries after `last_log_id` from the WAL or transfer newer state from a
peer.

## 3. Internal publication

The SQLite writer creates `<uuid>.temp~`, completes `VACUUM INTO`, and renames it to `<uuid>.temp`. The builder waits for
that response, copies `<uuid>.temp` to the final `<uuid>` path, opens the final file, and returns it. Cleanup of every
other file is spawned after the final file has been opened.

Recovery considers only regular files whose complete filename parses as a UUID. Receive staging named `temp`, builder
staging ending in `.temp`, and the writer's `~` staging file are not published snapshots. A controlled test establishes
that those interrupted staging names are ignored.

The final builder copy is not atomic and neither the final file nor its parent directory is explicitly synchronized
before publication. An interruption during that copy can leave a partial UUID-named file that startup will select. This
is a known defect, not a publication guarantee.

## 4. Internal installation and retained logs

OpenRaft streams an incoming snapshot into `snapshots/temp`. Installation renames that file to the final snapshot UUID
and then asks the sole SQLite writer to restore it into the live database. After restore, the writer reloads
`StateMachineData` from `_metadata`. OpenRaft receives success only after restore and metadata reload succeed.

The rename occurs before restore and is not followed by an explicit file or directory sync. If restore fails, the
UUID-named image remains discoverable. There is no rollback to the prior live database in this code. The process treats
missing or malformed metadata as fatal rather than inventing an applied frontier.

Log retention coordination is through `SnapshotMeta.last_log_id` and OpenRaft's storage protocol. Hiqlite's WAL purge
path flushes dirty logs and persists its purge frontier before deleting covered entries. The existing cluster recovery
test is intended to exercise state-machine reconstruction through a live cluster, but it is coupled to earlier remote
client phases.

A focused storage test supplies the bounded recovery evidence independently. It writes two real Raft entries through
the Hiqlite WAL in `Immediate` mode, applies the first entry to SQLite, and builds a snapshot at that frontier. It then
performs an orderly shutdown, removes only the live SQLite projection, restarts the state machine from the snapshot,
reads the second entry through `RaftLogReader`, and applies it. The final query must contain both the snapshotted row
and the retained-log row, and the applied frontier must advance from log 1 to log 2. This proves orderly local
reconstruction from a SQLite snapshot plus an explicitly replayed retained WAL entry. The test invokes the replay
steps directly, so it does not prove OpenRaft's automatic replay orchestration, interrupted snapshot installation,
crash durability, or live peer transfer.

## 5. Internal startup and exclusive ownership

The state machine creates a marker at `state_machine/lock` and removes it on orderly writer shutdown. If a marker
already exists, builds without `auto-heal` panic for manual intervention. With `auto-heal`, startup removes the live
database directory, creates a new one, restores the newest local snapshot if present, and lets OpenRaft replay or fetch
the remaining state.

This marker is not an operating-system advisory lock and file creation is not exclusive. Safe `auto-heal` therefore
assumes the data directory cannot be mounted by another live process. The WAL has a separate OS file lock. Operators
MUST provide exclusive ownership of internal state-machine storage before removing a marker or allowing automatic
rebuild.

SQLite uses `synchronous=OFF` for the internal state machine. Its restart safety depends on discarding a database from
an unclean run and rebuilding from snapshot plus Raft logs whose own durability depends on the WAL mode in spec 001.

## 6. External state-machine mode

External mode accepts caller-committed operations in a dense sequence. Its snapshot uses SQLite Online Backup behind
the sole writer queue, which captures one checkpoint and its bounded receipt window without renumbering implicit
ROWIDs. The evidence contains format and application identity, sequence and schema configuration, checkpoint, receipt
floor and retention, receipt codec, page size, byte length, and SHA-256. It contains no consensus membership.

External snapshot creation writes a staging file, syncs it, publishes it without replacing an existing destination,
and syncs the published file and parent directory on Unix. Installation validates format, identity, digest, SQLite
integrity, metadata, receipt continuity, and staleness before restore. A failed live restore poisons the writer and
leaves a durable rebuild-required marker.

An OS advisory lock prevents two external engines from owning the database. Dirty markers distinguish active FULL,
NORMAL, replayable OFF, and restore-in-progress states. An unclean replayable-OFF or poisoned projection fails closed
and requires `rebuild_projection`; `open` does not auto-delete it. FULL mode can reopen after an unclean marker because
SQLite owns its recovery, while the caller still compares the persisted checkpoint with its durable log.

The caller owns outer snapshot retention, consensus membership, durable log reclamation, and replay after the restored
checkpoint. Installing the SQLite image does not authorize deleting the caller's consensus log.

## 7. Known defects

- Internal builder publication can expose a partial final UUID file because it uses `copy` rather than a same-directory
  staging rename and does not sync before returning.
- Internal install publishes the received file before restore. Failed restore leaves it eligible for startup selection.
- The internal lock marker does not prevent concurrent owners. Safe use depends on deployment-level exclusivity.
- Internal startup chooses the greatest UUID and then validates its embedded snapshot id. It does not fall back to an
  older valid snapshot if the newest UUID file is corrupt.
- The focused internal test proves staging names are ignored. A deterministic interruption test for the mid-copy final
  name and a fail-closed startup policy remain follow-up work.
- The repository cluster test did not reach its self-healing section during this pilot's local run because it stopped
  making progress in the earlier remote-client phase. The focused storage test now covers local snapshot plus retained
  WAL reconstruction, while live peer transfer and the remote-client stall remain separate follow-up work.

Future behavior SHOULD publish internal snapshots with a durable same-directory rename, validate before selection,
and define rollback or poison behavior for failed install. Those changes require a separate behavioral decision and
an amendment to this draft.

## 8. Provenance

The retained-frontier decomposition follows the Raft snapshot idea, but the requirements here are derived from
hiqlite's SQLite writer, snapshot builder, state-machine adapter, WAL purge path, external engine, and cluster recovery
test. The raft-corpus snapshot documents were only an outline and did not supply acceptance criteria.

## Verification

```verify:cli
cargo test -p hiqlite --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::restart_reconstructs_snapshot_then_replays_retained_wal -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::interrupted_staging_files_are_not_published_snapshots -- --exact
cargo test -p hiqlite --lib --no-default-features --features external-state-machine external_state_machine::tests::snapshot_evidence_restore_receipts_and_staleness -- --exact
cargo test -p hiqlite --lib --no-default-features --features external-state-machine external_state_machine::tests::online_backup_snapshot_preserves_implicit_rowids -- --exact
cargo test -p hiqlite --lib --no-default-features --features external-state-machine external_state_machine::tests::durability_is_explicit_and_unclean_replayable_off_fails_closed -- --exact
```
