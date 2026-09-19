# hiqlite constitution

Durable principles that govern the hiqlite specification corpus on the
`bartekus/hiqlite` fork. This document is **tier 2**: it is subordinate to the
bootstrap spec (`specs/000-hiqlite-ownership-bootstrap/spec.md`), whose
`unamendable` anchors it may not contradict, and it governs all ordinary specs
(`001`+).

**Normative hierarchy (highest wins):**

1. `specs/000-hiqlite-ownership-bootstrap/spec.md`: the bootstrap spec. Non-overridable.
2. `standards/spec/constitution.md`: this document.
3. `standards/spec/contract.md`: a normative summary of the bootstrap spec.
4. Ordinary specs (`001`+): feature-level claims within this envelope.

When two specs conflict, resolve in this order, then by the typed authority
graph. This hierarchy is an authority ordering. It says which text wins when two
texts disagree; it says nothing about the order in which branches merge.

---

## I. Markdown-only authored governance truth

Authored governance truth lives only in markdown with YAML frontmatter: a
`spec.md` under `specs/`, or a document under `standards/spec/`. If a fact says
who owns what, what a contract requires, or what the corpus accepts as evidence,
it is written there and nowhere else. *(Bootstrap anchor: `markdown-truth-boundary`.)*

This boundary is about governance authority, not about the repository at large.
`Cargo.toml`, `hiqlite.toml`, `hiqlite.env`, SQL migrations, WAL files, snapshot
images, and the running database remain what they have always been: manifests,
configuration, and data that the code reads and writes. A spec describes and may
claim authority over such a file; it does not thereby turn the file into prose,
and a runtime value is not demoted to commentary because a spec cites it.

## II. Compiler-owned JSON machine truth

Everything under `.derived/` is emitted by `spec-spine compile` and `spec-spine
index` and is read only through a typed consumer: the `spec-spine` binary or the
`spec-spine-core` library. Hand-editing a derived shard is a workflow violation,
and parsing one with `jq`, `awk`, `sed`, or a hand-rolled reader is equally
forbidden: a typed read fails at the deserializer with a clean error instead of
silently somewhere downstream. *(Bootstrap anchor: `json-truth-boundary`.)*

## III. Determinism and mechanical validation

Every artifact-producing step is a pure function of `(config, file contents)`:
the same committed inputs produce byte-identical output. Staleness is detected
by content-hash comparison alone, so the hash must fold every input that can
change a resolved artifact. Validation is mechanical: `compile` and `index`
write, `check`, `lint`, `couple`, and `index coverage` only read and report. No
artifact carries an ambient clock or environment read except the excluded
`builtAt` field in the gitignored `build-meta.json`.
*(Bootstrap anchor: `determinism-requirement`.)*

Regeneration and validation are separate acts. A read-only check that repaired
what it was judging would hide that the *committed* copy was stale.

## IV. Typed authority, derived by walking the graph

A spec declares typed edges and the units it owns. Authority over a unit is
derived by walking that graph, never asserted in prose alone.
*(Bootstrap anchor: `typed-authority-graph`.)*

The eight edges are `establishes`, `extends`, `refines`, `supersedes`, `amends`,
`co_authority`, `constrains`, and `references`; only `references` is
non-owning. A spec that must touch a unit another spec owns declares `extends`
on that unit rather than re-declaring `establishes`, so the ledger carries one
origin and an explicit, attributed second claim.

## V. The refusal rule

When the coupling gate reports that a claimed unit and its owning spec disagree,
no agent resolves it by editing the spec to match the code it just wrote. The
contradiction is surfaced to a human, or to an agent holding authority the spec
records explicitly. The gate is the merge-time defense; this rule is the
prompt-time defense. *(Bootstrap anchor: `refusal-rule`.)*

## VI. Adopted code is specced as found

Every spec in this corpus is retroactive. hiqlite existed, and worked, before
any of it was written. A spec that claims authority over pre-existing code
declares `origin.retroactive: true` and then describes the behavior that is
actually there, including the behavior it would not have chosen.

Behavior the spec would not have chosen is recorded under a **known-defects**
heading, named, and left unfixed by that spec. Recording a defect does not bless
it; it is what lets a later spec be written against it. Without the heading an
adopting spec has only bad options, since an accurate description would ratify
the defect as the specified behavior. `origin.retroactive: true` says *when* the
authority began; the defects heading says what the adopting spec makes of what
it found. A proposed fix belongs in a separate spec and a separate change.

The heading is identified by its computed slug: `known-defects`, or any slug
ending in `-known-defects`, at any level. `## Known defects` and
`## 6. Known defects` both name the section; `## Known defects and open
questions` does not, because a section that continues past the anchor is about
something wider.

## VII. OpenRaft owns consensus; hiqlite owns its side of the trait boundary

OpenRaft owns the consensus algorithm: elections, quorum calculation, leader
rules, log matching, and the membership protocol. No spec in this corpus may
claim that territory, and no upstream OpenRaft guarantee is evidence that
hiqlite implements the machinery itself.

hiqlite owns, and this corpus may specify:

- WAL and vote persistence supplied through the OpenRaft storage traits;
- the SQLite and cache state machines, their snapshots, and recovery integration;
- client, transport, configuration, and error behavior around OpenRaft;
- exclusive-access assumptions for hiqlite-managed storage.

A spec cites the trait boundary and then specifies only the hiqlite side of it.

## VIII. External state-machine callers own their own consensus

External state-machine mode does not start a hiqlite Raft group. The caller
supplies committed operations in dense global order and owns its durable
consensus log, its membership rules, its outer snapshot manifest, and every
replay decision. hiqlite owns the atomic SQLite mutation, the checkpoint, the
receipt, the local snapshot image, the durability configuration, and exclusive
local engine access.

No spec may describe a caller-owned guarantee as something hiqlite provides.

## IX. Durability and recovery claims name their configuration and their evidence

A durability or recovery statement in this corpus is incomplete unless it names
the configuration under which it holds and the evidence that was actually
produced. `LogSync::Immediate` and `LogSync::ImmediateAsync` are different
contracts, and a claim that does not say which one it is about states nothing.

Evidence is described by what the test did, not by what the reader might hope it
covered. A test that invokes replay steps directly has not proved that OpenRaft
orchestrates them. A fault injected into an internal helper has not proved how
that fault propagates through the public storage trait. A spec states the limit
of its evidence in the same place as the claim.

## X. Lifecycle words are not synonyms

Seven states are distinct and this corpus never collapses them:

- **`status: draft`** is a text a human has not ratified. A draft's claims bind
  nobody. Every spec in this corpus is currently draft.
- **`implementation: complete`** says the described implementation and its
  executable acceptance are present in the tree. It says nothing about approval.
- **Ratification** is the human flip of `status` to `approved`. It is an act of
  the repository owner and of nobody else, agent included.
- **Merge** is a Git operation. Merging a specification into a branch places the
  text in the branch. It does not ratify the text.
- **Publication** is making an artifact available outside the repository.
- **Release** is a versioned, tagged distribution.
- **Upstream acceptance** is a decision by the upstream project.

An agent may propose, implement, validate, and merge under explicit
authorization. An agent never ratifies.

## XI. Fork governance is local

This corpus governs `bartekus/hiqlite`. It binds work on this fork and nothing
else. Nothing here has been proposed to, reviewed by, or approved by the
upstream `sebadob/hiqlite` project, and no spec, workflow, or command in this
repository may state or imply otherwise. Default branch targets, pull-request
bases, and comparison refs in this repository name this fork's integration
branch; they never name upstream.

## XII. Bounded adoption is a declared state, not a defect

This corpus deliberately governs part of the repository. `coupling.require_ownership`
is off, so unclaimed source is migration debt that `spec-spine index coverage`
reports rather than a repository-wide refusal. The bound is recorded in the
bootstrap spec and is widened by an explicit adoption decision, not by drift.

---

## Amendment

This constitution is changed by an ordinary spec that is `approved`, **claims the
affected text as an authority unit of this file**, and contradicts no anchor in
the `unamendable` list of `specs/000-hiqlite-ownership-bootstrap`. The bootstrap
spec's freeze surface is the hard boundary; everything else here is revisable
through the normal governed flow.

The claim uses the ordinary ownership vocabulary over a **section unit of this
file**, not the `amends` edge:

- `establishes` a `{ kind: section, file: "standards/spec/constitution.md",
  anchor: <heading-slug> }` unit for a principle the spec adds;
- `refines` that unit, with a named `aspect`, for a principle it tightens or
  restates;
- `co_authority` on that unit where a principle is genuinely shared.

`amends` is **not** the instrument here. Its targets are spec ids, and this file
is not a spec. An `amends: ["standards/spec/constitution.md"]` entry does not
resolve and is a validation error, not a governed constitutional change.

The anchor is the heading slug the indexer computes, so `## VII. OpenRaft owns
consensus; hiqlite owns its side of the trait boundary` is
`vii-openraft-owns-consensus-hiqlite-owns-its-side-of-the-trait-boundary`.

Unlike an amended `spec.md`, which is a record of what the corpus held when it
was ratified and is therefore never edited to mention its successors, this
document is a standing statement of what is true now. It is edited in place, and
its history lives in the specs that claimed each section and in git.

**The gate does defend this file here.** `standards/spec/constitution.md` sits
on the coupling gate's built-in bypass floor, and an explicit unit claim
overrides that floor. `specs/000-hiqlite-ownership-bootstrap` claims this file
as an explicit `file` unit, so editing it without an authoring edit to an owning
spec raises `C-001`. The same holds for `.github/workflows/spec-spine.yaml` and
`.gitignore`, which `000` also claims explicitly.

This differs from the upstream spec-spine corpus, whose own constitution is
undefended because that corpus does not claim it. The behavior here was measured
against the pinned revision, not inherited from that text.

The gate still checks only that a claimed path and an owning spec moved
together. It does not read what either says. Whether an amendment is
constitutionally sound remains a question for human review, and `000` section 13
keeps ownership records, freshness detection, coupling enforcement, and review
apart for that reason.
