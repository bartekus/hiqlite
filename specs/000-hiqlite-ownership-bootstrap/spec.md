---
id: "000-hiqlite-ownership-bootstrap"
title: "Bound the first hiqlite governance corpus"
status: draft
created: "2026-09-18"
owner: "hiqlite maintainers"
risk: high
implementation: complete
origin:
  retroactive: true
  paths: ["ARCHITECTURE.md", "justfile", ".github/workflows/", "spec-spine.toml"]
establishes:
  - "ARCHITECTURE.md"
  - "justfile"
  - ".github/workflows/spec-spine.yaml"
  - "spec-spine.toml"
  - ".gitignore"
summary: >
  Defines the authority boundary for a bounded retroactive spec-spine pilot and
  owns the local regeneration, validation, and pull-request coupling workflow.
---

# 000: Bound the first hiqlite governance corpus

## 1. Purpose

This is a retroactive bootstrap over an existing repository. It records the ownership boundaries and governance
workflow needed by three focused contracts without claiming that the entire repository has specification coverage.
The spec is draft. Its implementation status records that the workflow exists and passes locally, not that a hiqlite
owner has ratified its policy.

## 2. Ownership boundary

OpenRaft owns its consensus algorithm, including elections, quorum calculation, leader rules, log matching, and
membership protocol. Hiqlite integrates that algorithm and owns:

- the WAL and vote persistence supplied through OpenRaft storage traits;
- internal SQLite and cache state machines, snapshots, and recovery integration;
- client, transport, configuration, and error behavior around OpenRaft;
- exclusive access assumptions for hiqlite-managed storage.

An upstream OpenRaft guarantee is context, not evidence that hiqlite implements the election machinery itself. The
contracts in this corpus cite the trait boundary and then specify only hiqlite behavior on its side of that boundary.

External state-machine mode is a separate integration. It does not create a Hiqlite Raft group or own consensus
membership. Its caller supplies committed operations in dense global order and owns its durable consensus log,
membership rules, outer snapshot manifest, and replay decisions. Hiqlite owns the atomic SQLite mutation, checkpoint,
receipt, local snapshot image, durability configuration, and exclusive local engine access described by the focused
contracts.

## 3. Adoption state

The pilot consists of this bootstrap and:

- `001-wal-durability-and-completion`;
- `002-snapshot-publication-and-recovery`;
- `003-client-consistency-and-retry-outcomes`.

All four specs are `draft` and retroactive. They describe the code as found, including configuration-dependent
guarantees, known defects, and unresolved questions. `implementation: complete` says the described implementation and
executable evidence are present. It does not mean approved, ratified, published, or released.

## 4. Governance workflow

The exact spec-spine source revision is `aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f`. The binary reports version
`0.20.0`, so the revision in `justfile` and CI is the reproducibility boundary. `spec-spine.toml` also refuses binaries
whose reported semantic version differs from `0.20.0`.

After a trusted edit to a spec, governed source, or governance input, `just spine-regenerate` MUST regenerate the
committed registry and codebase-index shards. `just spine-check` MUST validate the corpus, freshness, lint, and report
bounded coverage without writing. `just spine-couple <base-sha> HEAD` MUST compare against the actual pull-request base.
CI obtains that base from `github.event.pull_request.base.sha` with full Git history.

`coupling.require_ownership` is false. Existing specific claims still produce C-001 failures when governed code changes
without participation by an owning spec. Unclaimed code does not produce C-002 failures. Coverage output is therefore
migration information rather than a repository-wide completeness gate.

The critical source, test, architecture, command, and workflow paths for this pilot are explicit
`index.extra_hashed_inputs`. Directory claims are also recursively witnessed by the pinned tool. A byte change in those
inputs MUST make the committed index stale until regeneration.

## 5. Pull-request trust boundary

`spec-spine verify` executes shell commands authored in a spec. Pull requests can modify those commands, so pull-request
CI MUST NOT execute verification blocks from the proposed tree. Maintainers MAY run `just spine-verify <spec-id>` only
after reviewing the commands. CI runs the read-only freshness and lint checks plus coupling, all of which interpret the
corpus but do not execute its acceptance commands.

## 6. Known limitations and follow-up

The pilot does not claim full ownership coverage. Future work SHOULD separately evaluate membership integration,
distributed leases, broader transport behavior, and the remaining source tree. Enabling `require_ownership` or
`index coverage --fail-on-untraced` is a separate adoption decision after that debt is classified.

No spec in this corpus ratifies another spec. Publication, release, waivers, and owner approval remain separate actions.

## 7. Provenance

The configuration and workflow follow the spec-spine source and adoption guide at revision
`aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f`. The raft-corpus repository was consulted for invariant decomposition and
provenance style. Its specs were not copied, and its supersession graph has no authority here.

## Verification

```verify:cli
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
```
