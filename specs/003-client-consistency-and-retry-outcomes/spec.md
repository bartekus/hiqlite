---
id: "003-client-consistency-and-retry-outcomes"
title: "Define client consistency and retry outcomes"
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
    - "hiqlite/src/query/"
    - "hiqlite/src/client/"
    - "hiqlite/src/network/"
    - "hiqlite/src/server/proxy/stream.rs"
    - "hiqlite/src/external_state_machine.rs"
establishes:
  - { kind: directory, path: "hiqlite/src/query/" }
  - { kind: directory, path: "hiqlite/src/client/" }
  - { kind: directory, path: "hiqlite/src/network/" }
  - "hiqlite/src/server/proxy/stream.rs"
co_authority:
  - { unit: { kind: file, path: "hiqlite/src/external_state_machine.rs" }, with_specs: ["002-snapshot-publication-and-recovery"] }
references:
  - { unit: { kind: file, path: "hiqlite/src/config.rs" }, role: "timeouts and durability configuration" }
summary: >
  Records local and explicitly consistent read behavior, write success and
  ambiguity, leadership and reconnect retries, temporary response buffering,
  and external mode's bounded durable receipt contract.
---

# 003: Define client consistency and retry outcomes

## 1. Purpose

The public client combines local SQLite reads, OpenRaft writes, leader-connected WebSocket requests, automatic leader
redirection, and temporary response correlation. These paths do not offer one uniform consistency or retry guarantee.
This spec names the outcome of each path and separates ordinary cluster mode from the durable receipts in external
state-machine mode.

## 2. Reads

For an embedded client with local state, ordinary `query_map`, `query_as`, and raw query methods read directly from that
node's read-only SQLite pool. They do not call OpenRaft and may observe a follower before it applies the latest committed
entry. A remote-only client's ordinary query travels over the client stream to the current leader endpoint and reads
that node's local SQLite pool without an explicit linearizability check.

`query_consistent` and `query_consistent_map` send the request to the leader path. Before reading SQLite, the leader
calls OpenRaft `ensure_linearizable`. Success means the read occurred after that check on the leader. Hiqlite does not
implement the heartbeat or term rules behind the check; OpenRaft owns them.

An explicitly consistent read can fail during leader loss, connection loss, timeout, or SQLite execution. Ordinary and
consistent reads MUST NOT be described as interchangeable. Local reads optimize availability and latency. Explicitly
consistent reads pay the leader and OpenRaft coordination cost.

## 3. Write outcomes

A successful local or remote write response means OpenRaft returned a client-write response and the target state
machine returned its operation result. The durability of the replicated log acknowledgement remains conditional on
the `wal_sync` mode in spec 001. A deterministic SQLite constraint or statement failure is returned as a known failure.

A connection error, closed response channel, 120-second request timeout, or leadership transition after submission can
be ambiguous. The operation may have committed and applied even though the caller did not receive its response. The
ordinary cluster request contains a process-local correlation identifier, not a durable client operation identity.
Hiqlite does not persist a response receipt or suppress a later duplicate write in this mode.

Callers MUST treat retry after an ambiguous ordinary-cluster outcome as potentially repeating the operation. They
SHOULD use application-level unique keys, conditional SQL, or another idempotency design when repetition is unsafe.

## 4. Leadership retries

Public write and remote-query methods inspect errors for OpenRaft `ForwardToLeader` evidence. When an error carries a
leader id and node, the client updates its leader address, requests a stream reconnect, and retries the cloned operation
once. The retry creates a new correlation id. Errors without complete forward evidence are returned.

An explicit stream `LeaderChange` closes the connection and fails currently active requests with `Error::LeaderChange`.
Those failures are not durable evidence that the server did not apply a write. The public helper's automatic retry is
specific to the structured OpenRaft forward error; it is not a general retry loop for timeouts or connection failures.

## 5. Reconnect response buffering

Each connected stream holds a map from process-local request id to a oneshot response sender. When a connection ends,
the manager drains unresolved entries into a temporary buffer and reconnects. It does not resend those requests.

After the next connection succeeds, a ten-second window accepts a late response whose request id matches the temporary
buffer. At the end of the window, remaining waiters receive `Error::Connect("request timed out")` and are removed. A
late response after expiry is logged as missing and discarded. If reconnect itself takes longer, the temporary entries
remain until a connection succeeds or the outer 120-second wait expires.

The buffer exists only in memory for the lifetime of the client process. It correlates a late response; it does not
deduplicate execution, survive restart, or create a retained retry window.

## 6. External mode receipts

External state-machine mode persists the typed SQLite operation, dense sequence checkpoint, exact command identity,
and encoded response receipt in one SQLite transaction. A retained retry must match sequence, caller coordinate,
command digest, state and receipt schemas, entry kind, and receipt codec. A match returns `ApplyOutcome::Recovered`
from stored bytes without executing the operation again. A mismatch returns `CommitConflict`.

Receipts form a contiguous bounded window. The configured retention defaults to 1024 and must be between 1 and
1,000,000. When a requested sequence is older than the retained floor, the engine returns `ReceiptUnavailable`; it
does not guess or rerun the operation. Snapshots preserve and validate the receipt floor and retained rows.

An operation error rolls back the SQLite mutation and does not consume the sequence. A deterministic business rejection
that must survive a lost response MUST be represented inside a successful operation output so it can be committed and
stored as a receipt. A receipt that exceeds the configured maximum rolls back the operation and frontier.

These rules provide bounded duplicate suppression for exact retained retries at the SQLite boundary. They do not make
the caller's consensus log and SQLite one atomic storage system. The caller must retry from its durable log after an
uncertain process failure and must preserve a stable command digest and receipt codec.

## 7. Known defects

Behavior this spec found and would not have chosen, left unfixed here.

- Ordinary cluster writes have no durable operation id or response receipt. Lost-response retries may repeat effects.
- The ten-second reconnect buffer is fixed in code and starts only after a successful connection. The public timeout is
  separately fixed at 120 seconds.
- Explicitly consistent query documentation currently overstates what a quorum has applied. The implementation calls
  `ensure_linearizable` and then reads the leader; this spec limits its claim to that observable sequence.

## 8. Evidence gaps, intentional limits, and follow-up

Neither entry below is behavior this spec would not have chosen, so neither belongs in section 7.

**Evidence gap.** Focused tests exercise late-response routing and expiry directly. A transport-level fault test that
drops the socket after apply but before response remains follow-up work.

**Intentional limit.** External receipt retention is count-based, not time-based. Applications must size it for their
maximum retry horizon.

Future work SHOULD add an ordinary-cluster idempotency key and durable receipt design before making any stronger retry
claim. It SHOULD also add a deterministic lost-response integration harness. Broader membership and distributed-lease
semantics remain outside this pilot.

## 9. Provenance

The client-id and serial-number pattern in the Raft literature and raft-corpus was evaluated as an idea, not assumed to
exist. Hiqlite ordinary cluster mode has no such persisted table. The external engine implements a bounded receipt
variant with its own dense sequence and exact identity rules, so its contract is written from hiqlite source and tests.
The raft-corpus client outline supplied no usable acceptance criteria.

## 10. Resolved decisions

**D-1 (2026-09-19, the known-defects heading was missing and its entries were
reclassified).** This spec recorded its adopted-as-found behavior under
`## 7. Known limitations and follow-up`. Constitution VI identifies that section
by its computed slug, `known-defects` or a slug ending in `-known-defects`, and
`known-limitations-and-follow-up` matches neither, so a consumer extracting
defects from this spec found none and three real defect records were invisible
to the corpus. `001` and `002` both used the recognized heading; this spec was
the outlier.

The five original entries were split by what they actually are, and no text was
dropped or softened. The ordinary-cluster receipt gap, the hardcoded reconnect
buffer and timeout, and the overstated consistent-query documentation are
behavior this spec would not have chosen, and are now section 7. The missing
transport-level fault test is an evidence gap, and the count-based receipt
retention is a deliberate design limit with a stated caller obligation; both are
now section 8, which is not a defect heading and does not pretend to be. The
follow-up paragraph is unchanged. Provenance moved from section 8 to section 9.

The classification is a documentation correction. No runtime behavior, no
acceptance command, and no claim in sections 2 through 6 changed.

## Verification

```verify:cli
cargo test -p hiqlite --lib --no-default-features --features sqlite client::stream::tests::reconnect_buffer_delivers_a_late_response_during_its_window -- --exact
cargo test -p hiqlite --lib --no-default-features --features sqlite client::stream::tests::reconnect_buffer_expiry_reports_an_ambiguous_timeout -- --exact
cargo test -p hiqlite --lib --no-default-features --features external-state-machine external_state_machine::tests::dense_frontier_exact_retries_conflicts_and_rollback -- --exact
cargo test -p hiqlite --lib --no-default-features --features external-state-machine external_state_machine::tests::oversized_receipt_rolls_back_operation_and_frontier -- --exact
cargo test -p hiqlite --lib --no-default-features --features external-state-machine external_state_machine::tests::snapshot_evidence_restore_receipts_and_staleness -- --exact
```
