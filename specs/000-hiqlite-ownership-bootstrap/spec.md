---
id: "000-hiqlite-ownership-bootstrap"
title: "Bound the first hiqlite governance corpus"
status: draft
kind: "constitutional-bootstrap"
created: "2026-09-18"
owner: "hiqlite maintainers"
risk: high
implementation: complete
origin:
  retroactive: true
  paths: ["ARCHITECTURE.md", "justfile", ".github/workflows/", "spec-spine.toml"]
unamendable:
  - "markdown-truth-boundary"
  - "json-truth-boundary"
  - "directory-name-equals-id"
  - "typed-authority-graph"
  - "determinism-requirement"
  - "refusal-rule"
  - "openraft-consensus-boundary"
  - "external-caller-consensus-boundary"
  - "evidence-names-configuration"
  - "lifecycle-words-are-distinct"
  - "fork-governance-boundary"
establishes:
  - "ARCHITECTURE.md"
  - "justfile"
  - ".github/workflows/spec-spine.yaml"
  - "spec-spine.toml"
  - ".gitignore"
  - "standards/spec/constitution.md"
  - "standards/spec/contract.md"
  - "standards/spec/templates/"
summary: >
  Tier-1 bootstrap for the hiqlite fork's specification corpus. Defines what a
  spec is here: the authored versus derived boundary, spec identity, the typed
  authority graph, determinism, and the refusal rule. Fixes the ownership
  boundary against OpenRaft and against external state-machine callers, states
  what evidence and lifecycle words mean, and records the explicit, bounded
  adoption scope of the pilot. Owns the local regeneration, validation, and
  pull-request coupling workflow, and the tier-2 and tier-3 governance
  documents under standards/spec.
---

# 000: Bound the first hiqlite governance corpus

This is the spec that defines what a spec is for the `bartekus/hiqlite` fork. It
sits at the top of the constitutional hierarchy; `standards/spec/constitution.md`
is subordinate to it.

It is a retroactive bootstrap over an existing, working repository. hiqlite was
built before any of this text existed, and the corpus is built to describe it,
not the other way round.

The spec is `draft`. Nothing in it has been ratified.

## 1. The constitutional hierarchy

Four tiers, highest wins:

1. `specs/000-hiqlite-ownership-bootstrap/spec.md`: this document. Its
   `unamendable` anchors are non-overridable.
2. `standards/spec/constitution.md`: the durable principles.
3. `standards/spec/contract.md`: a normative summary of this document and the
   constitution, for quick reference. Where it is terser, the first two govern.
4. Ordinary specs (`001`+): feature-level claims inside that envelope.

When two specs conflict, resolve in this order, then by the typed authority
graph of section 4.

This hierarchy is an **authority ordering**. It states which text wins when two
texts disagree. It is not a statement about Git: it does not require that the
tier-1 document reach a branch before the specs it governs, and merge order
carries no constitutional weight of its own.

The freeze surface in this document's `unamendable` frontmatter is stated now so
that later work cannot quietly erode it. While this spec is `draft` the freeze
binds the corpus as drafted; it takes final force when an owner ratifies the
spec.

## 2. Authored truth and derived truth

There are exactly two kinds of governance truth in this repository.

- **Authored truth** lives only in markdown with YAML frontmatter:
  `specs/NNN-slug/spec.md` and the documents under `standards/spec/`. Humans,
  and agents holding explicit authority, write it. *(anchor: `markdown-truth-boundary`)*
- **Machine-consumable truth about the corpus** is emitted only by
  `spec-spine compile` and `spec-spine index`, as JSON, into `.derived/`. No
  hand-authored JSON is authoritative, and no consumer may treat a hand-edited
  shard as truth. *(anchor: `json-truth-boundary`)*

Corollary: **typed reads or nothing.** A committed shard is read through the
`spec-spine` binary or the `spec-spine-core` library. Reading one with `jq`,
`awk`, `sed`, or a hand-rolled parser is a workflow violation: it encodes schema
assumptions that then fail far from the read instead of at the deserializer.

This boundary is about **governance authority**, and it does not reclassify the
rest of the repository. `Cargo.toml`, `hiqlite.toml`, `hiqlite.env`, SQL
migrations, WAL segments, snapshot images, and the live database remain
manifests, configuration, and data. A spec may describe such a file and may
claim authority over it; the file does not thereby become prose, and a runtime
value is not demoted to commentary because a spec cites it.

The committed shard trees are `.derived/spec-registry/` and
`.derived/codebase-index/`. `build-meta.json` is the only wall-clock artifact
and is gitignored.

## 3. Identity: directory name equals id

A spec's directory under `specs/` is named exactly `NNN-slug`, where `NNN` is a
three-digit zero-padded ordinal and `slug` is kebab-case. The spec's `id`
frontmatter field MUST equal that directory name, and `NNN` is unique across the
corpus. *(anchor: `directory-name-equals-id`)*

The ordinal is an identity, not a schedule. This is a retroactive corpus over
finished code; a higher ordinal does not mean later work, and nothing here
defines a build order.

Required frontmatter is `id`, `title`, `status`, `created`, and `summary`.
`status` is one of `draft`, `approved`, `superseded`, `retired`; `superseded`
requires a resolving `superseded_by`, and `retired` requires a
`retirement_rationale`. `implementation` is one of `pending`, `in-progress`,
`complete`, `n-a`, `deferred`, or absent.

## 4. The typed authority graph

A spec does not merely claim that it exists. It declares typed edges in
frontmatter and the authority units it owns, and authority over any unit is
**derived by walking the graph**, never declared in prose alone.
*(anchor: `typed-authority-graph`)*

Eight edges: `establishes` (first brings a unit into being), `extends` (adds
surface to a predecessor), `refines` (tightens a named aspect), `supersedes`
(replaces a predecessor and inherits its current authority), `amends` (patches a
predecessor in place, granting co-authority over its `spec.md`), `co_authority`
(a genuinely shared unit), `constrains` (an invariant others must respect), and
`references`, which is the only non-owning edge and which the coupling gate
ignores.

`origin` is a bootstrap marker, not an edge.

Six unit kinds: `file` (a bare string is shorthand; a trailing slash denotes the
subtree), `section` (`{file, anchor}`), `symbol` (`{id}`), `directory`, `crate`,
and `module`.

**One origin per unit.** A spec that must touch a unit another spec owns
declares `extends` on that unit. It does not re-declare `establishes`. The
ledger then carries one historical origin and an explicit, attributed second
claim, and "who established this" stays a different, answerable question from
"who currently owns this".

`amends` resolves to **spec ids**. It is not the instrument for changing
`standards/spec/constitution.md`, which is not a spec. That document is changed
by an approved spec claiming the affected text as a `section` unit of the file;
the constitution's own Amendment section states the rule.

## 5. Determinism

Every artifact-producing step is a pure function of `(config, file contents)`:
the same committed inputs MUST produce byte-identical output. No ambient clock
or environment read enters an artifact; the sole exception is `builtAt` in the
gitignored `build-meta.json`, which every determinism and golden check excludes.
*(anchor: `determinism-requirement`)*

Determinism is what makes the committed shards a ledger: they are diffable,
mechanically mergeable, and staleness is detectable by content-hash comparison
alone. The hash must therefore fold every input that can change a resolved
artifact, which is why the pilot's contract-bearing source, command, and
workflow paths are explicit `index.extra_hashed_inputs` in addition to the
directory claims the pinned tool walks. A byte change in those inputs MUST make
the committed index stale until regeneration.

**Regeneration and validation are separate acts.** `spec-spine compile` and
`spec-spine index` write. `spec-spine check`, `lint`, `couple`, and `index
coverage` only read and report. A read-only check that repaired what it was
judging would hide that the *committed* copy was stale, and the drift would then
read as an uncommitted local edit rather than as a defect already on the branch.

## 6. The refusal rule

When the coupling gate reports that a claimed unit and its owning spec disagree,
no agent resolves it by editing the spec to match the code it just wrote. The
agent MUST surface the contradiction and let a human, or an agent holding
authority this corpus records explicitly, decide.
*(anchor: `refusal-rule`)*

The gate is the merge-time defense; this rule is the prompt-time defense.
Together they close the failure mode of an agent erasing the contract to keep
going. A `Spec-Drift-Waiver:` line is a human act; a driven session never writes
one for itself.

## 7. Ownership boundary: OpenRaft

OpenRaft owns its consensus algorithm: elections, quorum calculation, leader
rules, log matching, and the membership protocol. No spec in this corpus claims
that territory. *(anchor: `openraft-consensus-boundary`)*

hiqlite integrates that algorithm and owns:

- the WAL and vote persistence supplied through the OpenRaft storage traits;
- the internal SQLite and cache state machines, their snapshots, and recovery
  integration;
- client, transport, configuration, and error behavior around OpenRaft;
- exclusive-access assumptions for hiqlite-managed storage.

An upstream OpenRaft guarantee is context, not evidence that hiqlite implements
the election machinery itself. A spec cites the trait boundary and then
specifies only the hiqlite side of it.

## 8. Ownership boundary: external state-machine callers

External state-machine mode is a separate integration. It does not create a
hiqlite Raft group and does not own consensus membership.
*(anchor: `external-caller-consensus-boundary`)*

The caller supplies committed operations in dense global order and owns its
durable consensus log, its membership rules, its outer snapshot manifest, and
every replay decision. hiqlite owns the atomic SQLite mutation, the checkpoint,
the receipt, the local snapshot image, the durability configuration, and
exclusive local engine access.

No spec may present a caller-owned guarantee as something hiqlite provides.

## 9. Evidence: configuration and limits are part of the claim

A durability or recovery statement is incomplete unless it names the
configuration under which it holds and the evidence that was actually produced.
`LogSync::Immediate` and `LogSync::ImmediateAsync` are different contracts, and
a sentence that does not say which one it is about states nothing.
*(anchor: `evidence-names-configuration`)*

Evidence is described by what the test does, not by what a reader might hope it
covers, and the limit is stated in the same place as the claim. A test that
invokes replay steps directly has not proved that OpenRaft orchestrates them. A
fault injected into an internal helper has not proved how that fault propagates
through the public storage trait.

Code adopted as found is specced as found. Behavior a spec would not have chosen
is recorded under a `known-defects` heading, named, and left unfixed by that
spec. Recording a defect does not bless it: it is what lets a later spec be
written against it. A proposed fix is a separate spec and a separate change.

## 10. Lifecycle words are distinct

Seven states, never collapsed: *(anchor: `lifecycle-words-are-distinct`)*

- **`status: draft`**: no human has ratified this text.
- **`implementation: complete`**: the described implementation and its
  executable acceptance are present in the tree and pass. It says nothing about
  approval.
- **Ratification**: a human flip of `status` to `approved`. An agent may
  propose, implement, validate, and merge under explicit authorization. An agent
  never ratifies.
- **Merge**: a Git operation that places text in a branch. Merging a
  specification does not ratify it.
- **Publication**: making an artifact available outside the repository.
- **Release**: a versioned, tagged distribution.
- **Upstream acceptance**: a decision by the upstream project.

Every spec in this corpus is `draft`. No spec ratifies another spec.

## 11. Fork governance is local

This corpus governs `bartekus/hiqlite` and binds work on this fork alone.
*(anchor: `fork-governance-boundary`)*

Nothing here has been proposed to, reviewed by, or approved by the upstream
`sebadob/hiqlite` project, and no spec, command, or workflow in this repository
may state or imply otherwise. The integration branch for this work is
`spec-spine`. Local default branch targets, pull-request bases, and comparison
refs name this fork's integration branch; they never name upstream, and they no
longer name `main`.

## 12. Adoption scope

The pilot consists of this bootstrap and:

- `001-wal-durability-and-completion`;
- `002-snapshot-publication-and-recovery`;
- `003-client-consistency-and-retry-outcomes`;
- `004-governance-harness`, which owns the shared agent protocol and describes
  the commands, hooks, and CI that operate this corpus.

`004` relates to this spec through `depends_on` and through `extends` edges on
the `justfile` and workflow units established here. It does not re-establish
them, and it does not replace this spec's ownership of them.

All five are `draft` and retroactive. `implementation: complete` on this spec
says its claimed units exist and its acceptance block passes against the tree.
It does not mean approved, ratified, published, or released.

**The bound is deliberate.** `coupling.require_ownership` is false. Existing
specific claims still produce `C-001` failures when a governed path changes
without participation by an owning spec. Unclaimed source produces no `C-002`
failure. `spec-spine index coverage` output is therefore migration information,
not a repository-wide completeness gate. Widening the bound, by enabling
`require_ownership` or `index coverage --fail-on-untraced`, is a separate
adoption decision.

Deliberately out of scope for this corpus: repository-wide ownership coverage,
membership integration, distributed leases, broader transport coverage, live
peer recovery, the remote-client stall, and fixes for the defects the `001`
through `003` specs record.

## 13. What each mechanism actually enforces

Four mechanisms are routinely confused. This spec keeps them apart, and no
document in this repository may describe one as doing another's work.

1. **Ownership records.** The typed edges of section 4 are ledger facts.
   `spec-spine registry relationships <id>` answers who claims what. A claim on
   its own enforces nothing.
2. **Freshness detection.** `spec-spine check` compares the committed shards
   against what the corpus compiles to, in memory, without writing. It catches a
   ledger that is stale, not one that is wrong.
3. **Coupling enforcement.** `spec-spine couple --base <sha> --head HEAD`
   refuses with `C-001` when a claimed unit changed and no owning spec changed
   in the same range.
4. **Human review.** Everything the first three do not cover.

### 13.1 The coupling bypass floor, and what overrides it

The gate ships a built-in, non-removable bypass list, and
`coupling.bypass_prefixes` can only add to it. The floor includes `.github/`,
`docs/`, `README.md`, `CHANGELOG.md`, `LICENSE`, `CODEOWNERS`, `.gitignore`,
`.gitattributes`, `standards/spec/constitution.md`, `.derived/`, and the
lockfile tails.

**An explicit, ownership-bearing unit claim overrides the floor.** A path a spec
names as a `file`, `section`, `symbol`, `directory`, `crate`, or `module` unit is
judged by the gate even when the floor would otherwise exempt it. Implicit
path-level ownership does not override: a manifest `[package.metadata]` pointer
or a crate-level floor keeps deferring to bypass, because an explicit unit is an
author saying *this exact surface is governed* and a blanket floor is not.

Three of this spec's claims sit on paths that appear in the floor:
`.github/workflows/spec-spine.yaml`, `.gitignore`, and
`standards/spec/constitution.md`. Because this spec claims each of them as an
explicit `file` unit, all three **are** enforced: editing one without an
authoring edit to an owning spec raises `C-001`. The behavior was measured
against the pinned revision rather than assumed, because the upstream
spec-spine documents describe their own constitution as undefended, which is
true there only because that corpus does not claim it.

What remains genuinely unenforced here is every floor path no spec claims:
`README.md`, `CHANGELOG.md`, `LICENSE`, `CODEOWNERS`, `.gitattributes`, `docs/`,
the lockfile tails, and `.derived/`. Those are held by human review. A declared
`layout.state_dir` would be bypassed unconditionally, ahead of any claim; this
repository declares none.

`standards/spec/contract.md` and `standards/spec/templates/` are not on the
floor at all and are enforced normally.

## 14. Governance workflow

The exact `spec-spine` source revision is
`aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f`. The binary reports version `0.20.0`,
so the revision named in `justfile` and in CI is the reproducibility boundary;
`spec-spine.toml` `[meta] required_version` additionally refuses a binary whose
reported semantic version differs from `0.20.0`. Install it with
`just spine-install`.

After a trusted edit to a spec, a governed source file, or a governance input,
`just spine-regenerate` MUST regenerate the committed registry and
codebase-index shards, and they are committed with the change they describe.
`just spine-check` MUST validate the corpus, freshness, and lint, and report
bounded coverage, without writing.

`just spine-couple <base> <head>` compares a branch against its actual base. Its
local default base is `origin/spec-spine`, the integration branch of section 11.
CI does not use that default: it passes
`${{ github.event.pull_request.base.sha }}`, the pull request's real base
commit, with full Git history, so a branch is always compared against what it
will actually merge into.

## 15. Pull-request trust boundary

`spec-spine verify` executes shell commands authored inside a spec. A pull
request can modify those commands, so pull-request CI MUST NOT execute
verification blocks from the proposed tree. Maintainers MAY run
`just spine-verify <spec-id>` locally, after reading that spec's commands. CI
runs the read-only freshness, lint, and coverage checks plus coupling, all of
which interpret the corpus without executing it.

## 16. Known limitations and follow-up

The pilot does not claim full ownership coverage. Future work SHOULD separately
evaluate membership integration, distributed leases, broader transport behavior,
and the remaining source tree, and SHOULD classify the remaining debt before
enabling `require_ownership` or `index coverage --fail-on-untraced`.

No hooks are installed in this repository. Spec `004` states the policy a hook
would have to satisfy and records its absence; nothing here claims a hook-level
control that does not exist.

Ratification of this corpus, waivers, publication, and release remain separate
owner actions.

## 17. Provenance

The configuration and workflow follow the spec-spine source and adoption guide
at revision `aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f`. The constitution, the
contract, and the templates under `standards/spec/` are adapted from that
repository's own documents, corrected where the upstream text is wrong for an
adopter: the constitution-amendment instruction there is restated in terms of
section-unit authority rather than the `amends` edge, and every default branch
reference is retargeted from `origin/main` to this fork's `spec-spine`.

The hqgit corpus was consulted for the shape of a bootstrap and a harness spec.
Its greenfield build order, its layer taxonomy, its full-coverage requirement,
and its ledger-specific invariants are deliberately not imported: they describe
a repository that is specified before it is written, and this one is not. Its
supersession graph has no authority here.

## Verification

```verify:cli
test -f standards/spec/constitution.md
test -f standards/spec/contract.md
test -f standards/spec/templates/spec-template.md
test -f standards/spec/templates/constitution-template.md
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
spec-spine registry list --ids-only
sh -c 'spec-spine registry relationships 000-hiqlite-ownership-bootstrap | grep -q 004-governance-harness'
```
