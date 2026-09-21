---
id: "005-adoption-assessment-and-plan"
title: "Record the whole-project assessment and the proposed adoption plan"
status: draft
kind: "governance"
created: "2026-09-19"
owner: "hiqlite maintainers"
risk: low
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "004-governance-harness"
origin:
  retroactive: false
establishes:
  - "standards/spec/adoption-plan.md"
  - "standards/spec/findings-register.md"
  - "standards/spec/wal-repair-proposal.md"
  - "standards/spec/cache-log-repair-proposal.md"
references:
  - unit: { kind: file, path: "standards/spec/constitution.md" }
    role: "context"
  - unit: { kind: file, path: "standards/spec/contract.md" }
    role: "context"
summary: >
  Owns the four assessment documents under standards/spec: a reconciled
  whole-project inventory with a dependency-ordered adoption proposal and a
  current assignment table, a source-backed findings register with stable
  identifiers, and the two traced repair proposals, for the WAL append contract
  and for the cache log store. Records what was measured and what is proposed.
  It adopts nothing, schedules nothing, enables no gate, and claims no territory
  outside those four documents.
---

# 005: Record the whole-project assessment and the proposed adoption plan

## 1. Purpose

The pilot corpus (`000` through `004`) governs part of this repository and says
so. The owner's objective is now eventual whole-project adoption, with
specifications that expose defects, missing evidence, and unclear contracts so
they can be repaired in governed follow-up work.

That objective needs three kinds of thing written down before any of it can be
scheduled: an honest inventory of what the repository actually contains against
what the ledger currently sees, a register of what the assessment found, and the
traced analysis a repair is authorized against. This spec owns all four
documents, the last kind having two instances.

**It authorizes nothing; it records.** Nothing in
`standards/spec/adoption-plan.md` is scheduled or approved **by that document**:
it enables no gate, changes no configuration, and claims no territory beyond
itself. Several waves are explicitly blocked on owner decisions the plan lists.
This spec is `draft` and ratifies nothing, including itself.

**Recording delivered work is not authorizing it, and the plan must do the
first.** Since this spec was written, work it described as proposed has been
separately authorized and delivered: wave 1 as `006` and `007`, and the WAL
repair as `008`. The plan says so, dated, and names the spec that carries each
contract. A plan that kept calling delivered work proposed would not be more
conservative; it would be inaccurate, and it would send the next session to
redo finished work. What B-1 forbids is a proposal presented as a decision.
What B-1.1 requires is a decision taken elsewhere recorded as history.

`implementation: complete` here means exactly what `000` section 10 says: the
units this spec claims, the four documents, exist in the tree and its
acceptance block passes. It is not a claim that the plan has been carried out.

## 2. Territory

This spec **establishes** four units, none of which existed before it claimed
them:

- `standards/spec/adoption-plan.md`, the reconciled inventory, the wave
  proposal, the completion criteria, and the enforcement ladder;
- `standards/spec/findings-register.md`, the findings with stable `F-NNN`
  identifiers;
- `standards/spec/wal-repair-proposal.md`, the traced repair proposal for F-001
  and F-002, which implements nothing and changes no runtime behavior;
- `standards/spec/cache-log-repair-proposal.md`, the traced repair proposal for
  W-04 (F-021 to F-024, F-029 and F-047), added 2026-09-21, which likewise
  implements nothing, authorizes nothing, and changes no runtime behavior.

A repair proposal is territory of this spec rather than of the spec that owns
the code it analyses, for the reason the WAL one set: it is assessment output,
it is superseded by the repair spec when one is written (`008` took
`wal-repair-proposal.md` by a `superseding` `extends` edge), and until then it
must be editable as the analysis is corrected without touching the adopted
contract it is about.

`standards/spec/constitution.md` and `standards/spec/contract.md` are
**referenced**, not claimed. `000` owns both, and `references` is non-owning.

Not claimed here, and deliberately: every source file the plan proposes to
govern later. Naming an area in a wave is not a claim on it. A wave becomes
territory when its own spec declares its units and is reviewed, which is the
point of proposing the waves separately rather than claiming them here.

`standards/spec/**/*.md` is in `coverage.governed_scope`, so all four documents
enter the coverage denominator; this spec's claims are what keep that scope at
full coverage rather than adding unclaimed files to it.

## 3. Behavior

### B-1. The plan authorizes nothing, and says so in its own text

`standards/spec/adoption-plan.md` MUST state its own status as proposed, and
MUST NOT present a wave it proposes as adopted, scheduled, or authorized **by
that document**. A reader who opens it without context must not be able to
mistake a proposal for a decision that has been taken.

This clause is about proposals. It does not require the plan to be silent about
work that has been authorized and delivered elsewhere, and B-1.1 requires the
opposite.

### B-1.1. The plan is reconciled against what has been delivered

Where work the plan described has been separately authorized and delivered, the
plan MUST record it as delivered, dated, naming the spec that carries the
contract, and MUST NOT continue to recommend it as outstanding. It MUST NOT
propose a spec ordinal that an existing spec already holds. Delivered work is
rewritten as history, with what it did **not** settle stated explicitly;
superseded ordinal proposals are vacated rather than silently renumbered,
because an ordinal is identity and is allocated when work starts (`000` section
3). The plan MUST carry one current assignment table in which implementation
state, evidence state, and any outstanding owner decision are separate fields.

Recording a delivery is not an authorization and never reads as one: the
authority is the delivered spec, and the plan points at it.

### B-2. The three milestones stay apart

The plan MUST keep ownership recorded, behavior specified with evidence, and
enforcement enabled as three separate milestones, and MUST NOT report reaching
one as evidence of another. This is constitution XII applied over time: coverage
is a measurement, intended scope is a plan, and enforcement is configuration.

### B-3. Findings carry stable identifiers and separate observation from inference

Every entry in `standards/spec/findings-register.md` MUST carry a stable `F-NNN`
identifier that is never reused, a class, a confidence level, precise source
references, the affected configuration, the practical consequence, and the limit
of its evidence.

A finding's **class** and its **state** are separate fields and neither is read
off the other. Class is what the finding is (`defect`, `contradiction`, `gap`,
`evidence`, `limit`, `decision`); state is what has happened to it (open,
repaired, closed at a milestone, withdrawn). A repaired entry keeps its
identifier, its class, and its original text, and gains a dated annotation; it
is never deleted, renumbered, or reclassified to record that it was fixed. An
entry whose surrounding facts have moved gains a dated **disposition** appended
below its original observation, which narrows the entry without closing it.

An absent test MUST be classified as an evidence gap and never as a defect. A
consequence inferred from configuration or control flow that was not executed
MUST be marked as inferred, and its finding MUST NOT claim `high` confidence on
the strength of the inference alone.

A finding is a record. It authorizes no repair, and a repair is a separate
governed change with its own spec and its own evidence (constitution VI).

### B-4. External ownership is described, never claimed

Neither document may state or imply that this corpus owns the OpenRaft consensus
algorithm or an external state-machine caller's consensus log, membership, outer
snapshot manifest, or replay decisions (constitution VII and VIII). Findings
about those boundaries address hiqlite's integration responsibilities only.

### B-5. Generated output is governed through its source and its build

Where the plan proposes governing generated output, it MUST do so by claiming
the generator, its configuration, and a verification command, and MUST NOT
propose claiming the generated bytes as authored units. It must also state the
provenance chain for any generated artifact it discusses.

## 4. Evidence and its limits

The acceptance block asserts that all four documents exist; that the plan
states its proposed status, names all three milestones, carries its current
assignment table, and records the WAL repair as delivered rather than as the
next task; that the register carries its first and last finding identifiers and
its classification vocabulary; and that no document has acquired an em dash. It
then runs the read-only gate, which establishes that the corpus still compiles,
lints, and is fresh with these three units claimed and resolved.

What it does **not** establish, and what nothing mechanical can:

- that the inventory is complete or the counts correct. They were measured on
  2026-09-19 with `git ls-files`, `spec-spine index coverage`, and
  `spec-spine config show`, and they will drift as the repository changes. The
  plan dates its measurements for that reason.
- that any finding is correctly classified. Classification is a judgment, and
  F-014 is recorded as an open decision precisely because the assessment could
  not tell intent from source.
- that the proposed waves are the right decomposition, or that their proposed
  spec ordinals will be the ones used. Ordinals are identity, not schedule
  (`000` section 3).
- that the repository behaves as any finding describes at runtime. The
  assessment read source and ran no cluster suite; F-017 is reported unresolved
  for exactly that reason rather than guessed at. F-029, added on 2026-09-20, is
  the same shape: the trait mismatch is confirmed at two sources and the runtime
  consequence is explicitly not established.
- that the assignment table's states are current beyond the date it carries.
  They are a dated reading of the tree and the merged history, and they go stale
  the moment either moves.

The forward-looking probe recorded in the plan's section 5 was run against the
pinned revision on 2026-09-19 and reverted; its result is reproducible by
repeating it, and the plan states the commands and the exit codes observed.

## 5. Known defects

**KD-1 (recorded 2026-09-20, repaired the same day): this spec's own acceptance
block was false on the integration branch.** Command 4 asserted a phrase in
`standards/spec/wal-repair-proposal.md` that PR #7 had removed under an
authorized superseding edit, and it failed at the unmodified head `58ee7fa`.
The assertion is replaced under D-7, and the finding that records it, F-030, is
marked repaired. What this spec would not have chosen, and does not resolve, is
narrower than a defect: acceptance blocks are executed by hand here and by
nothing automatically, so a stale assertion survives between two hand runs.
Whether that should change is an optional owner decision, carried as W-24, and
CI's abstention is a deliberate trust boundary (`000` section 15) rather than
an oversight to correct.

Otherwise none in this spec's own territory: the documents were authored
by this change and describe themselves.

The defects this spec's documents *record* belong to the specs that own the
affected code (`001`, `002`, `003`) and to the register, which is a record and
not an owner. Nothing here repairs any of them, and the register says so.

## 6. Out of scope

- Every source file named in any wave. Naming an area is not claiming it.
- Every configuration change in the plan's enforcement ladder, including
  `coupling.require_ownership`, `index coverage --fail-on-untraced`,
  `coupling.bypass_prefixes`, and the layout declarations for `examples/` and
  `dashboard/`. None is made here, and the plan marks each as requiring a probe
  against the pinned revision first.
- Every repair. The first one the plan recommends, the WAL append and completion
  error contract, is described so it can be authorized; it is not begun.
- Ratification of anything, including this spec.

## 7. Resolved decisions

**D-1 (2026-09-19, the assessment is a spec-owned document rather than loose
prose).** The alternative was to leave the inventory and the register in a pull
request body or an issue. Both documents state what the corpus accepts as
evidence and what it intends to govern, which constitution I places in markdown
under `specs/` or `standards/spec/`. Owning them also keeps
`coverage.governed_scope`, which already globs `standards/spec/**/*.md`, at full
coverage instead of gaining two unclaimed files.

**D-2 (2026-09-19, this spec is the corpus's first non-retroactive origin).**
`origin.retroactive: false`. Every other spec here adopts code that predates
the graph; the documents this spec claims did not exist before this change, so
a retroactive marker would be false. Constitution VI, as corrected on this
branch, requires a spec whose subject does not yet exist to declare its origin
truthfully rather than copy the shape of the existing corpus. The pinned
revision accepts the field: a probe on 2026-09-19 compiled a throwaway spec
with `origin.retroactive: false` and with a `planned: true` unit, cleanly, and
the result is recorded in the plan's section 5.

**D-3 (2026-09-19, `implementation: complete` for a spec that proposes future
work).** The field is measured against the obligations this spec places on the
tree (`000` section 10), and its obligations are the documents it claims, which
exist and satisfy B-1 through B-5. The waves are proposals inside a claimed
document, not obligations of this spec. Marking it `in-progress` would repeat
the error `004` D-4 corrected: reading a spec's description of future
possibility as unmet implementation debt.

**D-4 (2026-09-19, the inventory configuration was corrected, and the plan's
numbers with it).** The owner placed the dashboard and the examples in the
adoption target, so `spec-spine.toml` now declares them and the plan's section
2.1 is rewritten against the result: denominator 134 to 226, numerator unchanged
at 60, reported share 44.8% to 26.5%. The share fell because the denominator
grew. That is recorded in the plan as the clearest available demonstration that
the figure measures a configured set rather than progress.

The configuration itself is owned by `000`, which records the probed semantics
in its section 12.1. This spec records only that the plan's measurements were
updated to match, which is why both files move in the same change.

**D-5 (2026-09-19, wave 1 was executed, so the documents were updated).**
`006` and `007` claim the cache state machine and the cache log store. The plan
records wave 1 as executed rather than proposed, and the register gains F-021
through F-027 for what wave 1 found. Those specs own their defects as `KD-n`
entries and remain authoritative; the register carries stable ids so later work
can cite one. Recording an executed wave does not adopt the remaining five: they
stay proposals, and the plan still says so.

**D-6 (2026-09-20, the record is reconciled against delivered work, and a
fourteenth defect is recorded).** Waves and repairs this spec's documents
described as proposed were delivered (`006`, `007`, `008`), and the plan had
begun to mislead: it recommended the WAL repair as "not started", proposed
ordinals `008` through `020` when `008` is taken, called rung 0 done while its
second half is blocked, and listed the cache territory as unclaimed. The
register's own arithmetic disagreed with its class table, reporting ten open
defects where thirteen recorded minus two repaired is eleven.

The reconciliation keeps every historical observation in place and dates every
restatement, per B-3. Findings gain dated dispositions rather than edits:
F-015 and F-016 record that the visibility half of each is fixed and the
adoption and denominator halves are not; F-018 records that its `AGENTS.md`
half no longer holds while the three-way naming collision does. Vacated ordinal
proposals are described as vacated rather than renumbered, and the plan gains
the single assignment table B-1.1 now requires.

**F-029 is new, and was confirmed at source in this pass rather than
inherited.** `hiqlite/src/store/logs/memory.rs:210,217` drains an exclusive
range, while
OpenRaft 0.9.24 documents `RaftLogStorage::purge` as inclusive at
`openraft-0.9.24/src/storage/v2.rs:138`, read from the locked crate in the local
registry. Wave 1's characterization test pins the exclusive behavior as
expected. No runtime consequence was executed, and the entry says so. `007`
records four known defects and not this one, so the register is deliberately
ahead of its owning spec on this point; reconciling a KD entry into `007` is
queued as W-04 and is not done here, because changing an adopted spec's
known-defects section is a separate governed change.

No spec is ratified by this decision, no enforcement setting changes, no runtime
behavior changes, and no Rust or cluster test was run for it.

**D-8 (2026-09-20, the record is updated for the configuration adoption).**
W-01 was delivered as `009-configuration-contract`, so B-1.1 requires this
document's plan to say so rather than keep recommending it. The assignment table
row is rewritten as delivered with its remaining evidence gap named, A5 and A22
are re-cut around the units `009` now claims, and the register gains F-031
through F-036 for what the adoption found. F-010 gains a dated disposition: the
configuration half of it is closed and the other 33 environment reads are not.

`009` records its own defects as `KD-n` and stays authoritative for them; the
register carries the stable ids. Nothing here repairs any of them, and no
enforcement setting changed: the `spec-spine.toml` edit `009` required is a
freshness and denominator declaration, which constitution XII keeps separate
from enforcement.

**D-9 (2026-09-21, the inventory table is reconciled against eight delivered
waves).** B-1.1 requires this document's plan to describe delivered work as
delivered. Section 8's assignment table was kept current as each wave landed;
section 2.2's **current owner** column was not, and had gone stale for ten rows:
A8 and A9 when `010` and `011` were merged, and A10 to A14, A16, A17, A19 and
A20 across the eight waves that followed. Each still read **none** for territory
a merged spec establishes.

That is the same defect section 8's closing note names, in a different column,
and it is worse there: section 8 is explicitly the queue, while 2.2 is the
inventory a reader consults to find out who is responsible for a path. Ten rows
answering "nobody" about paths with an owning spec is a false statement about
the ledger, not merely a stale recommendation.

Each row is rewritten as history rather than deleted, per B-3: the owning spec
and its date, the evidence it has, the findings it produced, and what the
delivery did **not** close. The last of those is the part that keeps the table a
queue: A9 has nothing tested on a wire, A12 has no reconnect or shutdown
evidence because F-067 means the proxy does not run, A13 served no route end to
end, A14's presentational components are deliberately unclaimed, and A16 records
its owner as "none, deliberately" with `019` D-2 as the reason.

**No new finding, and nothing else changes.** No spec is ratified, no
enforcement setting moves, no source or configuration is touched, and coverage
is unchanged at 149/236: this decision claims nothing. The edit you are reading
is also the mechanism working as designed. `C-001` refuses a change to
`standards/spec/adoption-plan.md` that carries no authoring edit to an owning
spec, so reconciling the plan is not something that can be done quietly beside
it; the decision has to be written down here first. A first attempt at this
change omitted that and the gate rejected it.

**D-7 (2026-09-20, a stale acceptance assertion is replaced, and the class of
defect is recorded).** Running this spec's acceptance block found it failing at
command 4, and it fails identically at the unmodified integration head
`58ee7fa`: `grep -q 'changes no runtime behavior'
standards/spec/wal-repair-proposal.md` exits 1. The phrase was removed by
`8bce5ca` (PR #7), which rewrote that document's header to record that the
repair had been implemented. `008` holds an `extends` edge on the file with
nature `superseding`, so the edit was authorized and the coupling gate passed;
what nobody updated was this spec's assertion about the file.

The assertion had encoded an assumption rather than an obligation: that the
proposal would remain unimplemented forever. That assumption was always going to
expire, and the document's B-1 obligation is really that a reader can tell which
text governs. The replacement asserts exactly that, against wording the document
carries now, and the original line is kept as a comment beside it so the change
is legible in the diff rather than silent. This is recorded as a decision, not
performed as a repair of something the tool complained about: no coupling gate
reported it, and nothing here edits a spec to match code this session wrote.

The general defect is recorded as **F-030**: a spec's acceptance block can rot
on the integration branch without detection, because `spec-spine verify` is
deliberately not run in pull-request CI (`000` section 15, `004` B-2) and no
other control reads it. That is a governance gap with a real instance, and its
remedy is an owner decision about where acceptance blocks get executed, queued
as W-24. This change does not choose one.

## Verification

```verify:cli
test -f standards/spec/adoption-plan.md
test -f standards/spec/findings-register.md
test -f standards/spec/wal-repair-proposal.md
test -f standards/spec/cache-log-repair-proposal.md
# was: grep -q 'changes no runtime behavior' ... - stale since PR #7 rewrote the
# header this asserted. Replaced per D-7 with the statement the document must
# carry now: which spec governs it. See F-030.
grep -q 'which is the contract' standards/spec/wal-repair-proposal.md
grep -q '008-wal-append-completion-notification' standards/spec/wal-repair-proposal.md
grep -q 'log_io_completed' standards/spec/wal-repair-proposal.md
grep -q 'Status: proposed, not adopted' standards/spec/adoption-plan.md
grep -q 'M1, ownership recorded' standards/spec/adoption-plan.md
grep -q 'M2, behavior specified with evidence' standards/spec/adoption-plan.md
grep -q 'M3, enforcement enabled' standards/spec/adoption-plan.md
grep -q 'F-001' standards/spec/findings-register.md
grep -q 'F-020' standards/spec/findings-register.md
grep -q 'F-029' standards/spec/findings-register.md
grep -q 'F-047' standards/spec/findings-register.md
grep -q 'record, not a mandate' standards/spec/findings-register.md
grep -q '## 8. Current assignment table' standards/spec/adoption-plan.md
sh -c '! grep -q "^Not started, and not authorized by this document" standards/spec/adoption-plan.md'
grep -q 'Delivered 2026-09-19 as' standards/spec/adoption-plan.md
grep -q 'The proposed spec ordinals below are vacated' standards/spec/adoption-plan.md
grep -q 'are separate columns on purpose' standards/spec/adoption-plan.md
grep -q 'Status, 2026-09-21: proposed, not authorized' standards/spec/cache-log-repair-proposal.md
grep -q 'openraft-0.9.24' standards/spec/cache-log-repair-proposal.md
grep -q 'drain(..=purge_until)' standards/spec/cache-log-repair-proposal.md
sh -c '! grep -n "origin/main" standards/spec/adoption-plan.md standards/spec/findings-register.md standards/spec/wal-repair-proposal.md standards/spec/cache-log-repair-proposal.md'
sh -c '! grep -rl "$(printf "\342\200\224")" standards/spec/adoption-plan.md standards/spec/findings-register.md standards/spec/wal-repair-proposal.md standards/spec/cache-log-repair-proposal.md'
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c 'spec-spine index owner standards/spec/adoption-plan.md | grep -q 005-adoption-assessment-and-plan'
sh -c 'spec-spine index owner standards/spec/findings-register.md | grep -q 005-adoption-assessment-and-plan'
sh -c 'spec-spine index owner standards/spec/wal-repair-proposal.md | grep -q 005-adoption-assessment-and-plan'
sh -c 'spec-spine index owner standards/spec/cache-log-repair-proposal.md | grep -q 005-adoption-assessment-and-plan'
```
