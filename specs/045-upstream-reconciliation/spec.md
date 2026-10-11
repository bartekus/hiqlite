---
id: "045-upstream-reconciliation"
title: "Reconcile compatible upstream transport and dependency improvements"
status: draft
created: "2026-10-10"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "044-port-upstream-015-fixes"
origin:
  retroactive: false
establishes:
  - { kind: file, path: "standards/spec/upstream-reconciliation.md" }
extends:
  - spec: "025-snapshot-crash-recovery-contract"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/sqlite/" }
    nature: additive
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/network/" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/start.rs" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/split_brain_check.rs" }
    nature: additive
  - spec: "015-server-binary-and-proxy"
    unit: { kind: directory, path: "hiqlite/src/server/" }
    nature: additive
references:
  - { unit: { kind: file, path: "Cargo.toml" }, role: "dependency feature boundary" }
  - unit: { kind: file, path: "hiqlite/Cargo.toml" }
    role: "dependency feature boundary"
summary: >
  Builds on the pending 044 port with upstream WebSocket task cancellation,
  TCP keepalives, reusable split-brain HTTP clients, and a smaller external
  state-machine dependency surface. Preserves fork persistence formats,
  recovery gates, storage ownership, truthful shutdown, and durability behavior.
  Does not release or qualify either downstream consumer.
---

# 045: Reconcile compatible upstream improvements

## 1. Purpose

Reconcile the released fork with upstream v0.15.2 and the subsequent main at
`83a9680a35860b83405c42284739a1761cd8111c`, without replacing stronger fork
contracts with superficially similar upstream behavior. The implementation builds
on spec 044 at `0d11c08779af29ceb6bcfea0e1991c5adc5655c4`.

## 2. Territory

Hiqlite owns transport task lifetimes, socket configuration, and its local storage
integration. OpenRaft retains consensus authority. External state-machine callers
retain their own consensus log, membership, and replay decisions.

The manifests have no ownership-bearing indexed claim at the starting tree. Their
feature edits preserve the regular SQLite feature set and package identities.

## 3. Behavior

### B-1. Aborting a Raft connection cancels its socket tasks

Follow upstream `f8391c89` (#373): reader and writer task handles MUST abort their
tasks on drop. Both explicit reconnect cancellation and cancellation of the parent
handler MUST release the child tasks. Dropping a bare Tokio handle detaches it and
is insufficient.

### B-2. Configure TCP keepalives on transport sockets

Follow upstream `80a3c2c6` (#374): listening sockets MUST be nonblocking before
handoff to Tokio, use TCP_NODELAY, and enable keepalives (60 seconds idle, 10 seconds
between probes, three retries). Outgoing WebSocket streams, accepted API/Raft
streams, and proxy streams use the same configuration. Linux-specific tuning stays
platform gated. Bind errors MUST remain returned startup errors and listener exits
MUST remain observable through the fork lifecycle machinery.

### B-3. Reuse the split-brain HTTP client

Follow upstream `83a9680a` (#384): create one HTTP client per checker task and pass
it to both group checks. The fork already routes trust through its API CA and API
verification configuration; preserve that configuration and interval validation.

### B-4. Isolate external state-machine dependencies

Follow upstream `b8642d67` (#382): move the regular SQLite extension features from
the workspace dependency to the `sqlite` feature, and enable `constant_time_eq`
only through `__cluster`. Retain `backup`, `functions`, and `hooks` for the fork's
external state-machine engine. The fork has no `rustls-platform-verifier`
dependency to move. This changes feature activation, not database formats.

### B-5. Retain the consumer contract

Do not change cache command discriminants, cache snapshot layout, local notification
readiness, lock lease semantics, startup replay gating, exclusive storage ownership,
or shutdown error propagation. Keep file and parent-directory synchronization from
040. `LogSync::ImmediateAsync` still does not promise power-loss preservation of
acknowledged writes; `LogSync::Immediate` is the synchronous configuration to
qualify for that requirement. Socket keepalives do not establish N=3 support.

## 4. Evidence and its limits

Linux CI exposed a fixture race in the invalid received-snapshot regression. Its
snapshot builder acknowledges shutdown before its SQLite connection destructor
finishes the final WAL checkpoint. The test now copies the published snapshot to
an independent database fixture with no background writer. The byte-identical
assertion and restart selection check remain intact; runtime behavior is unchanged.

The cancellation regression runs child tasks holding real TCP halves, cancels their
owner, and requires the peer to observe EOF within a fixed bound. Demonstrate failure
with bare task handles while leaving the test present, then success with guarded
handles. Socket tests inspect actual listener and stream options and connect over
loopback. These tests do not establish partition recovery or distributed consensus.

Run the existing recovery, lifecycle, WAL, dashboard, and derive regressions alongside
the exact Rahi and Rauthy feature sets. A local candidate is neither a published
version nor consumer adoption. Record the precise commands and results separately.

## 5. Known defects

The fork still has the limitations recorded by specs 033 and 034. Upstream's cache
commands and snapshots differ, so a direct dependency swap or mixed-version cluster
is not qualified. Async WAL acknowledgements remain conditional on synchronization.

## 6. Out of scope

Cache migration, heartbeat lock semantics, replacing the derive implementation,
renaming public configuration fields, N=3 qualification, consumer pin changes,
publication, and ratification. The reconciliation matrix records these next steps.

## 7. Resolved decisions

D-1 (2026-10-10): adapt upstream socket setup to the fork's common prebound listener
and lifecycle reporting helpers instead of replacing those helpers with upstream
startup code. Keep the current dependency versions except adding socket2 0.6.

## Verification

```verify:cli
cargo +1.95.0 test --locked -p hiqlite-patched --lib -F full network::raft_client::tests
cargo +1.95.0 test --locked -p hiqlite-patched --lib -F full network::tcp_socket::tests
cargo +1.95.0 test --locked -p hiqlite-patched --lib -F full dashboard::query::tests
cargo +1.95.0 check --locked -p hiqlite-patched --no-default-features -F external-state-machine
```
