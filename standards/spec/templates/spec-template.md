---
id: "NNN-slug"                 # MUST equal the directory name; NNN = unique 3-digit ordinal
title: "Short imperative title"
status: draft                  # draft | approved | superseded | retired
created: "YYYY-MM-DD"
summary: >
  One short paragraph: what territory this spec claims and why it exists.
# --- optional descriptive keys ---
# owner: "hiqlite maintainers"
# authors: ["name"]
# risk: medium                 # low | medium | high | critical
implementation: pending        # pending | in-progress | complete | n-a | deferred
# depends_on: ["NNN-other"]
#
# Lifecycle words are not synonyms (constitution X). `status: draft` means no
# human has ratified this text. `implementation: complete` means the described
# implementation and its executable acceptance are present in the tree, and
# says nothing about approval. Ratification is a human flip of `status` and is
# never performed by an agent. Merge, publication, release, and upstream
# acceptance are four further, separate things.
#
# --- typed edges (declare territory + relationships) ---
# Eight edges. `references` is the only non-owning one: the coupling gate
# ignores it, so it names context without claiming it.
# establishes:
#   - { kind: file, path: "hiqlite/src/thing.rs" }
#   - "hiqlite/src/whole_file.rs"   # bare string == { kind: file, path: ... }
#   - "hiqlite/src/subtree/"        # trailing slash == the subtree rooted there
#   - { kind: symbol, id: "hiqlite::module::function" }
#   - { kind: section, file: "ARCHITECTURE.md", anchor: "a-heading-slug" }
#   - { kind: directory, path: "hiqlite-wal/src/" }
#   - { kind: crate, id: "hiqlite-wal" }
#   - { kind: module, id: "hiqlite::store::state_machine" }
#   # `planned: true` declares territory this spec owns and has not written yet.
#   - { kind: file, path: "hiqlite/src/not_written_yet.rs", planned: true }
# extends:
#   # Adds surface to a predecessor. This is how a spec touches a unit another
#   # spec owns WITHOUT re-declaring `establishes` on it. Use it instead of a
#   # second origin claim: the ledger then carries one origin plus an explicit,
#   # attributed second claim (constitution IV).
#   - { spec: "NNN-predecessor", unit: { kind: file, path: "justfile" }, nature: additive }
# refines:
#   - { aspect: "durability-configuration", unit: { kind: symbol, id: "hiqlite_wal::writer::run" } }
# supersedes: ["NNN-predecessor"]
# amends: ["NNN-predecessor"]      # targets a SPEC ID. Never a file path.
# co_authority:
#   - { unit: { kind: section, file: "ARCHITECTURE.md", anchor: "a-heading-slug" }, with_specs: ["NNN-other"] }
# constrains:
#   - { unit: { kind: file, path: "hiqlite/src/api.rs" }, note: "public API is frozen" }
# references:
#   - { unit: { kind: file, path: "README.md" }, role: "context" }
# --- lifecycle / amendment (as applicable) ---
# superseded_by: "NNN-successor"     # required when status: superseded
# retirement_rationale: "why"        # required when status: retired
# amends_sections: ["anchor"]        # which anchors of the amended spec change
# unamendable: ["anchor"]            # anchors of THIS spec no later spec may amend
#
# To change `standards/spec/constitution.md`, do NOT use `amends`: that edge
# resolves to spec ids and the constitution is not a spec. Claim the affected
# text as a section unit of that file instead (constitution, Amendment):
#   establishes:
#     - { kind: section, file: "standards/spec/constitution.md", anchor: "xiii-new-principle" }
# The file is on the coupling gate's built-in bypass floor, so that claim is a
# ledger fact rather than an enforced refusal. Human review is the control.
#
# --- bootstrap marker (NOT an edge) ---
# Every spec in this corpus is retroactive: hiqlite existed before the graph.
# `origin.retroactive: true` records authority held since before the graph
# existed, so the claim does not pose as a fresh `establishes`.
origin:
  retroactive: true
  paths: ["hiqlite/src/"]
---

# NNN: Title

## 1. Purpose

What problem this spec solves and what it owns.

## 2. Territory

The units this spec claims authority over (mirrors the frontmatter edges, in
prose). Name the ownership boundary explicitly: OpenRaft owns consensus, and an
external state-machine caller owns its own consensus log, membership, and replay
decisions (constitution VII and VIII). Say which side of each boundary this spec
is on.

## 3. Behavior

What the governed code must do. Use MUST / SHOULD / MAY.

Every durability or recovery statement names the configuration under which it
holds. `LogSync::Immediate` and `LogSync::ImmediateAsync` are different
contracts, and a sentence that does not say which one it is about states nothing
(constitution IX).

## 4. Evidence and its limits

What the acceptance commands below actually establish, described by what the
test does rather than by what a reader might hope it covers. State the limit in
the same place as the claim: a test that invokes replay steps directly has not
proved that OpenRaft orchestrates them, and a fault injected into an internal
helper has not proved how that fault propagates through the public trait.

## 5. Known defects

Behavior this spec found and would not have chosen, named, and left unfixed
here. Recording a defect does not bless it: it is what lets a later spec be
written against it (constitution VI). A proposed fix is a separate spec and a
separate change.

Keep the heading slug exactly `known-defects`, or a slug ending in
`-known-defects`. `## 5. Known defects` is fine. `## Known defects and open
questions` is not: it names a wider section and a consumer extracting defects
from it would extract the open questions too.

## 6. Out of scope

What this spec deliberately does not cover, including what remains governance
debt rather than a claim.

## 7. Resolved decisions

Dated entries (`D-1 (YYYY-MM-DD, what was decided)`) for choices this spec was
silent on and a build had to make. Recording one is always legitimate; changing
what the spec *requires* mid-build is not. If the gate and the spec disagree,
surface the contradiction rather than editing the spec to match the code
(constitution V).

## Verification

The acceptance, as commands. `just spine-verify <id>` runs this block; each line
is one command and no shell variable survives to the next. Write lines that fail
against the tree this spec is built on and pass after: a block that is green
before the work asserts nothing.

This block executes. It is therefore run by a maintainer who has read the
commands, and never by pull-request CI, which treats a proposed tree as
untrusted input.

```verify:cli
# one command per line
```
