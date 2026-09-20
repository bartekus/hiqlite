---
id: "001-wal-durability-and-completion"
title: "Define WAL durability and completion ordering"
status: draft
created: "2026-09-18"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on: ["000-hiqlite-ownership-bootstrap"]
origin:
  retroactive: true
  paths: ["hiqlite-wal/src/", "hiqlite/src/config.rs"]
establishes:
  - { kind: directory, path: "hiqlite-wal/src/" }
  - "hiqlite/src/config.rs"
references:
  - { unit: { kind: file, path: "hiqlite-wal/Cargo.toml" }, role: "crate and OpenRaft dependency boundary" }
summary: >
  Records the existing WAL append, synchronization, vote, truncation, purge,
  and recovery contract, including the weaker guarantees of async modes and
  the distinct append-return and OpenRaft completion events.
---

# 001: Define WAL durability and completion ordering

## 1. Purpose

Hiqlite supplies `hiqlite-wal` through OpenRaft's log storage traits. OpenRaft decides when a replicated entry may
advance under its consensus rules. Hiqlite decides when local WAL bytes, vote metadata, and purge metadata are written,
synchronized, reported complete, removed, and recovered. This spec owns that local boundary.

The spec is retroactive and draft. Requirements in this document describe the current implementation. The known
defects section records behavior that should not be read as a desired guarantee.

## 2. Append events

One logical append has two observable events:

1. The writer sends the internal append result through a oneshot channel. `RaftLogStorage::append` waits for this result
   and then returns. Success means each serialized entry was copied into the active memory mapping and the in-memory
   WAL view was updated. It does not mean a synchronization step completed.
2. The writer performs the mode-specific persistence step and invokes `LogFlushed::log_io_completed(Ok(()))` through
   the completion callback. OpenRaft uses this callback as the log I/O completion signal.

The internal append result MUST be sent before the mode-specific persistence step. The completion callback MUST be
invoked only after that step returns successfully. A persistence error after an append success therefore occurs after
the storage method can return success but before its completion callback.

The WAL writer serializes append, sync, vote, truncate, purge, and shutdown actions through one bounded channel and one
writer thread. Reader state is updated after bytes are appended to the active mapping and before the append result is
sent.

## 3. Synchronization modes

| Mode | Work before completion callback | Supported acknowledgement statement |
|---|---|---|
| `Immediate` | Updates the active WAL header and performs a blocking memory-map flush. | The callback follows a successful OS and VFS flush request for the active WAL. Hardware cache and platform guarantees still bound the result. |
| `ImmediateAsync` | Starts writeback with `sync_file_range(SYNC_FILE_RANGE_WRITE)` on Linux or asynchronous `msync` elsewhere. | The callback means writeback was requested. It does not establish stable storage. |
| `IntervalMillis(ms)` | Performs no per-append flush. A ticker later queues a blocking header update and flush. | The callback means the bytes entered the mapping. Loss is possible until a later ticker, vote, purge, rollover, or shutdown flush succeeds. |

`ImmediateAsync` is the default. Both async modes trade an acknowledged-loss window for throughput. A kernel crash,
power loss, storage failure, or election before local persistence may leave a node without an entry for which it
already emitted a completion callback. If enough nodes lose such entries, a client-visible successful write can later
be absent. The implementation therefore MUST NOT describe those modes as power-loss durable.

An orderly shutdown performs a blocking active-WAL flush and writes metadata. That proves orderly restart behavior. It
does not prove survival of abrupt power loss.

## 4. Vote and log ordering

`save_vote` is serialized through the WAL writer. Before changing vote metadata, the writer MUST perform a blocking
flush of any dirty active WAL. It then updates the in-memory vote and writes `meta.hql` through a temporary file,
`sync_all`, atomic rename, and a best-effort parent-directory sync on Unix. The vote acknowledgement follows the
metadata write result.

This order prevents a persisted newer vote from overtaking dirty log bytes in the writer queue. A directory-sync
failure is ignored, so rename durability across power loss remains platform-dependent. A failed metadata write can
also leave the process's in-memory vote newer than the last readable file; restart reads the file.

## 5. Errors, truncation, and purge

Serialization, WAL append, reader, metadata, truncation, purge, and channel errors are mapped into OpenRaft storage
errors at the trait boundary. Read corruption MUST be reported rather than returned as a silent short read.

Truncation removes the requested suffix through the writer and returns the deletion result. Purge first performs a
blocking flush of dirty WAL bytes, then persists the new purge frontier, then removes the covered files or records. If
removal fails, the implementation attempts to restore the prior frontier and reports the original failure. A failure
to persist that rollback is logged and leaves recovery state uncertain.

Rolling to a new WAL file performs a blocking flush of the sealed file before it can be treated as immutable. A purge
or vote action also closes the async writeback window by using the blocking flush path.

## 6. Startup and recovery

Startup holds an operating-system file lock for the WAL directory, reads metadata, discovers WAL files, and checks
header and optional deep record integrity. The default `auto-heal` feature may repair certain damaged tails. Recovery
uses the persisted purge frontier, vote, file headers, CRCs, and contiguous log identifiers.

Unreadable WAL files are currently warned about and skipped during discovery. This can allow startup with a shorter
log than the directory once contained. The surrounding Raft recovery may repair the node from peers, but the skip is
not proof that an acknowledged entry remains available anywhere else.

## 7. Acceptance boundary

The focused persistence-failure test injects an error into the internal `complete_append` helper and verifies that the
helper returns the error without invoking the completion callback. It does not inject a failure through
`RaftLogStorage::append` or establish how that failure propagates through the complete OpenRaft storage interface.

## 8. Known defects

- When entry append returns an error, the writer currently continues through the persistence step and invokes the
  completion callback as success. The storage method reports the error, but the callback conveys a conflicting event.
- A blocking persistence failure occurs after the internal append acknowledgement. It exits the writer loop and drops
  the completion callback without reporting an explicit `log_io_completed(Err(...))` to OpenRaft.
- The durability boundary of `mmap.flush`, `sync_all`, the filesystem, and the device cache has not been validated with
  a power-cut harness. The tests use controlled call ordering and restart checks only.
- Recovery after an unreadable file is skipped needs a focused integration test that proves the node either rejoins
  safely or fails closed for each missing-range shape.

Future work SHOULD decide the callback error contract, add injectable end-to-end flush failures at the storage trait
boundary, and test actual abrupt-loss behavior on supported filesystems. Those decisions are outside this retroactive
pilot and MUST amend this contract rather than silently rewriting its baseline.

## 9. Provenance

The invariant split follows the Raft rule that current-term vote and log state are persistent before they are relied on,
but this contract is derived from hiqlite's writer, metadata, WAL, reader, and OpenRaft adapter code. The raft-corpus
log decomposition was used only as a checklist. Its membership claims and supersession graph are not adopted.

## Verification

```verify:cli
cargo test -p hiqlite-wal --lib writer::tests::append_result_precedes_persistence_and_completion -- --exact
cargo test -p hiqlite-wal --lib writer::tests::persistence_failure_suppresses_completion_callback -- --exact
cargo test -p hiqlite-wal --lib writer::tests::append_failure_is_returned_but_completion_still_fires -- --exact
cargo test -p hiqlite-wal --lib reader::tests::logs_action_reports_read_errors -- --exact
cargo test -p hiqlite-wal --lib metadata::tests::metadata_overwrite_replaces_existing -- --exact
cargo test -p hiqlite-wal --lib wal::tests::roll_over_purge_front -- --exact
cargo test -p hiqlite-wal --lib wal::tests::roll_over_truncate_end -- --exact
```
