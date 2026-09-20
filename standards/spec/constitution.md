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

A spec that claims authority over pre-existing code declares
`origin.retroactive: true` and then describes the behavior that is actually
there, including the behavior it would not have chosen. hiqlite existed, and
worked, before any of this corpus was written, so every spec of the initial
pilot, `000` through `004`, declares a retroactive origin.

That is a historical fact about the pilot, not a property of the corpus and not
a requirement on what may be written next. `origin` is read per spec, from that
spec's own frontmatter, and specs written since the pilot have declared theirs
truthfully: `005-adoption-assessment-and-plan` declares `retroactive: false`,
because the assessment and plan it owns did not exist before the text that
states them. `origin.retroactive` records when authority began. A future spec
whose subject does not yet exist states that truthfully, declares the territory
it will own (`planned: true` on a unit that is not yet written), and does not
claim a retroactive origin it does not have. Misdeclaring origin to match the
shape of the existing corpus would corrupt the one field that answers whether
the text or the code came first.

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
something wider. A heading that names something else entirely, such as "Known
limitations and follow-up", does not name this section at all, and entries that
belong under it are reclassified into a recognized heading rather than left
where a consumer cannot find them.

**Ratifying a defect record is not endorsing the defect.** An approved spec that
carries a known-defects section says the description was accurate for the code
as adopted and that the defect was known when authority attached. It does not
declare the behavior desirable, it does not make the behavior a requirement, and
it does not bar a repair. A repair is an ordinary governed change: a later spec
that `refines`, `amends`, or `supersedes` the adopting spec, carrying the
behavior change and its own evidence. Nothing here makes a recorded defect
harder to fix than an unrecorded one; the record is what lets the repair be
reviewed against a stated baseline instead of against memory.

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
  nobody. `status` is per-spec frontmatter and is read there; this document
  asserts no corpus-wide value for it, because ratification is per spec and
  partial ratification is the normal case.
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

This corpus deliberately governs part of the repository. Three different things
are routinely collapsed into the word "adoption", and no document may cite one
of them as evidence for another:

- **Current coverage** is a measurement. `spec-spine index coverage` reports
  which files in its indexed denominator a spec specifically claims, at the
  moment it runs. It is a fact about the ledger and never a statement about test
  coverage or behavioral completeness: a claimed file may be only partly
  specified, and an unclaimed file may be thoroughly tested. The denominator is
  itself a configured artifact, so what it omits is part of what the number
  means.
- **Intended adoption scope** is a plan: which parts of the repository this
  corpus means to govern eventually, and which are deliberately excluded and
  why. It is authored text, it may exceed current coverage by a wide margin, and
  a gap between the two is an expected state rather than a defect.
- **Enforcement settings** are configuration. `coupling.require_ownership` is
  off, so unclaimed source is migration debt that coverage reports rather than a
  repository-wide refusal, and `index coverage` is not run with
  `--fail-on-untraced`. Explicit, ownership-bearing unit claims still raise
  `C-001` on the paths that carry them, including paths on the built-in bypass
  floor.

Each moves independently. Coverage rises when a spec claims new territory; the
intended scope changes when an owner decides it should; an enforcement setting
changes only by an explicit owner decision recorded in this corpus, never by
drift and never as a side effect of coverage rising.

---

## Amendment

This constitution is changed by an ordinary spec that is `approved`, **claims the
affected text as an authority unit of this file**, and contradicts no anchor in
the `unamendable` list of `specs/000-hiqlite-ownership-bootstrap`. The bootstrap
spec's freeze surface is the hard boundary; everything else here is revisable
through the normal governed flow.

**Which route applies depends on one spec's `status`, and on nothing else.**
Ratification is an owner act and an agent never performs one (section X). This
section is written to read correctly whatever each spec's current `status` is,
rather than to describe a moment.

**While the establishing spec is `draft`, the establishing-draft route is
open.** Requiring an approved amending spec then would freeze this document
against its own corrections until the day it is ratified, which is not the
intent. While
`specs/000-hiqlite-ownership-bootstrap` is `draft`, this document is authored
text that spec establishes and owns, and it is corrected in place under that
ownership: the coupling gate still requires an owning spec to move in the same
range, and the tier order and the `unamendable` anchors still bind.

**Ratification is per spec.** `status` is per-spec frontmatter and approving a
spec settles that spec. There is no aggregate state in which "the corpus" becomes
ratified, and no document in this repository may invoke one. Partial ratification
is the normal case: approving a spec that does not establish this file, such as
`001`, `002` or `003`, changes nothing about how this file is amended.

**Adopted policy, owner decision of 2026-09-20: the transition is per
document.** What closes, per document, is the **establishing-draft route**: the
licence the paragraph above grants to correct a document in place under the
ownership of the still-`draft` spec that establishes it. It closes at the moment
the owner sets `status: approved` on that establishing spec:
`specs/000-hiqlite-ownership-bootstrap` for this file, for
`standards/spec/contract.md`, and for `standards/spec/templates/`;
`specs/004-governance-harness` for `AGENTS.md`. Earlier revisions recorded this
as a proposal awaiting a decision. The decision is made, and it is recorded
here, in `000` section 1, and in `004` D-5.

**Editing in place does not stop; the authority for it changes.** A standing
document states what is true now, so it is edited in place before and after the
transition alike, and this file is no more frozen after `000` is approved than
before. What the transition changes is whose authority each edit runs on:
beforehand the establishing spec's own ownership while that spec is `draft`,
afterwards an approved later spec's claim on the affected text. No edit to this
file is routed through `000` once `000` is approved.

**The policy covers three governance documents and one templates subtree, and
nothing else.** Those are `standards/spec/constitution.md`,
`standards/spec/contract.md`, and `AGENTS.md`, plus every file under
`standards/spec/templates/`: today `spec-template.md` and
`constitution-template.md`, and any template added later, because the unit `000`
establishes there is the subtree and not a file list. The policy is not a
general rule about every unit an approved spec owns. `000` also
establishes `ARCHITECTURE.md`, the `justfile`,
`.github/workflows/spec-spine.yaml`, `spec-spine.toml`, and `.gitignore`, and
`004` holds `extends` claims on the `justfile` and that workflow. Approving
either spec closes nothing for those units. They are code, configuration, and
build inputs; they change for reasons that have nothing to do with
constitutional authority, and routing each such edit through an approved
amending spec would turn ordinary maintenance into a ratification queue. They
keep the ordinary flow and the defense they have today, which is the coupling
gate plus review.

### After the transition: a claim, not an exception

Once `000` is `approved`, this document is still changed, and changing it
requires no edit to `000` itself. The route is the ordinary ownership
vocabulary, applied by a **later spec**:

1. The later spec claims the affected text as a `section` unit of this file,
   through `establishes`, `refines` with a named `aspect`, or `co_authority`,
   exactly as described below.
2. That claim makes the later spec **an owning spec of this path**. Its own
   authoring edit, in the same range as the edit to this file, is what the
   coupling gate requires. `000` does not move, and needs no routine edit merely
   to satisfy coupling.
3. The later spec must be `approved` by the owner to carry the authority the
   first paragraph of this section requires. Approving it is the constitutional
   act; the coupling gate is not.

**The gate and the authority are two different controls, and only one of them is
mechanical.** This was measured against the pinned revision rather than assumed.
A spec claiming a `section` unit of this file and moving in the same range
satisfies `spec-spine couple` **whatever its `status` is**, including `draft`;
editing this file with no owning spec moving is refused with
`C-001 'standards/spec/constitution.md' changed without an authoring edit to any
owning spec`. So the gate proves participation, never authority. The requirement
that the claiming spec be `approved` is a human review obligation under section
X and the bootstrap's refusal rule, and no tool enforces it. A `draft` claiming
spec that edits this file after `000` is approved is a governed-flow violation
that passes CI, and reviewers are the control that catches it.

Nothing in this section ratifies anything, extends the policy past the three
documents and the templates subtree named above, or widens what a session may
change on its own authority.

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

**Which of the three edges, in the three cases that actually arise.**

- **A principle this file does not yet carry.** The later spec `establishes` a
  `{ kind: section, file: "standards/spec/constitution.md", anchor:
  "xiii-new-principle" }` unit. That section unit has no owner, so the claim is
  a first origin and not a second one: `000` holds the `file` unit, which is a
  different, wider unit.
- **A principle this file already states.** The section already has an origin,
  which is `000` for everything written during the pilot. The later spec does
  **not** re-declare `establishes` on it. It `refines` the section unit with a
  named `aspect` where it tightens the principle, or takes `co_authority` on
  that unit where the principle becomes genuinely shared from then on.
- **Template text.** `000` establishes `standards/spec/templates/` as a subtree
  unit. A later spec that changes a template claims the file it touches, as
  `{ kind: file, path: "standards/spec/templates/spec-template.md" }`, through
  `refines` with a named `aspect` or `co_authority`. A `section` unit is
  available only where a template carries a heading the indexer can slug;
  guidance that lives in a comment block does not, so the file unit is the
  addressable one there.

**How this meets the one-origin rule.** "One origin per unit" (`000` section 4)
is unchanged, and none of the three cases is an exception to it. They meet on
unit identity: a `section` unit of this file is narrower than, and separate
from, the `file` unit `000` holds, so claiming an unowned section is a first
claim rather than a duplicate. `extends` remains the ordinary edge for adding
surface to a unit a predecessor already owns, and it is what the units outside
this policy keep using. What is never acceptable is declaring a duplicate origin
for a unit that already has one in order to make the coupling gate pass: that is
the move section V refuses, and it would destroy the ledger's ability to answer
"who established this" separately from "who owns this now".

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
