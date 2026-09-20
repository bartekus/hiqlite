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
references:
  - unit: { kind: file, path: "standards/spec/constitution.md" }
    role: "context"
  - unit: { kind: file, path: "standards/spec/contract.md" }
    role: "context"
summary: >
  Owns the two assessment documents under standards/spec: a reconciled
  whole-project inventory with a dependency-ordered adoption proposal, and a
  source-backed findings register with stable identifiers. Records what was
  measured and what is proposed. It adopts nothing, schedules nothing,
  enables no gate, and claims no territory outside the two documents.
---

# 005: Record the whole-project assessment and the proposed adoption plan

## 1. Purpose

The pilot corpus (`000` through `004`) governs part of this repository and says
so. The owner's objective is now eventual whole-project adoption, with
specifications that expose defects, missing evidence, and unclear contracts so
they can be repaired in governed follow-up work.

That objective needs two things written down before any of it can be scheduled:
an honest inventory of what the repository actually contains against what the
ledger currently sees, and a register of what the assessment found. This spec
owns both documents.

**It is a record, not an adoption.** Nothing in
`standards/spec/adoption-plan.md` is scheduled or approved. No wave it proposes
exists, no spec it names has been written, no configuration is changed, and no
enforcement control is enabled. Several waves are explicitly blocked on owner
decisions the plan lists. This spec is `draft` and ratifies nothing, including
itself.

`implementation: complete` here means exactly what `000` section 10 says: the
units this spec claims, the two documents, exist in the tree and its acceptance
block passes. It is not a claim that the plan has been carried out.

## 2. Territory

This spec **establishes** two units, neither of which existed before this
change:

- `standards/spec/adoption-plan.md`, the reconciled inventory, the wave
  proposal, the completion criteria, and the enforcement ladder;
- `standards/spec/findings-register.md`, the findings with stable `F-NNN`
  identifiers;
- `standards/spec/wal-repair-proposal.md`, the traced repair proposal for F-001
  and F-002, which implements nothing and changes no runtime behavior.

`standards/spec/constitution.md` and `standards/spec/contract.md` are
**referenced**, not claimed. `000` owns both, and `references` is non-owning.

Not claimed here, and deliberately: every source file the plan proposes to
govern later. Naming an area in a wave is not a claim on it. A wave becomes
territory when its own spec declares its units and is reviewed, which is the
point of proposing the waves separately rather than claiming them here.

`standards/spec/**/*.md` is in `coverage.governed_scope`, so both new documents
enter the coverage denominator; this spec's claims are what keep that scope at
full coverage rather than adding two unclaimed files to it.

## 3. Behavior

### B-1. The plan records a proposal, and says so in its own text

`standards/spec/adoption-plan.md` MUST state its own status as proposed, and
MUST NOT describe any wave as adopted, scheduled, or in progress. A reader who
opens it without context must not be able to mistake it for a record of work
already agreed.

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

The acceptance block asserts that both documents exist, that the plan states its
proposed status and names all three milestones, that the register carries its
first and last finding identifiers and its classification vocabulary, and that
neither document has acquired an em dash. It then runs the read-only gate, which
establishes that the corpus still compiles, lints, and is fresh with these two
units claimed and resolved.

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
  for exactly that reason rather than guessed at.

The forward-looking probe recorded in the plan's section 5 was run against the
pinned revision on 2026-09-19 and reverted; its result is reproducible by
repeating it, and the plan states the commands and the exit codes observed.

## 5. Known defects

None in this spec's own territory: the two documents were authored by this
change and describe themselves.

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
`origin.retroactive: false`. Every other spec here adopts code that predates the
graph; these two documents did not exist before this change, so a retroactive
marker would be false. Constitution VI, as corrected on this branch, requires a
spec whose subject does not yet exist to declare its origin truthfully rather
than copy the shape of the existing corpus. The pinned revision accepts the
field: a probe on 2026-09-19 compiled a throwaway spec with
`origin.retroactive: false` and with a `planned: true` unit, cleanly, and the
result is recorded in the plan's section 5.

**D-3 (2026-09-19, `implementation: complete` for a spec that proposes future
work).** The field is measured against the obligations this spec places on the
tree (`000` section 10), and its obligations are the two documents, which exist
and satisfy B-1 through B-5. The waves are proposals inside a claimed document,
not obligations of this spec. Marking it `in-progress` would repeat the error
`004` D-4 corrected: reading a spec's description of future possibility as
unmet implementation debt.

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

## Verification

```verify:cli
test -f standards/spec/adoption-plan.md
test -f standards/spec/findings-register.md
test -f standards/spec/wal-repair-proposal.md
grep -q 'changes no runtime behavior' standards/spec/wal-repair-proposal.md
grep -q 'log_io_completed' standards/spec/wal-repair-proposal.md
grep -q 'Status: proposed, not adopted' standards/spec/adoption-plan.md
grep -q 'M1, ownership recorded' standards/spec/adoption-plan.md
grep -q 'M2, behavior specified with evidence' standards/spec/adoption-plan.md
grep -q 'M3, enforcement enabled' standards/spec/adoption-plan.md
grep -q 'F-001' standards/spec/findings-register.md
grep -q 'F-020' standards/spec/findings-register.md
grep -q 'record, not a mandate' standards/spec/findings-register.md
sh -c '! grep -n "origin/main" standards/spec/adoption-plan.md standards/spec/findings-register.md standards/spec/wal-repair-proposal.md'
sh -c '! grep -rl "$(printf "\342\200\224")" standards/spec/adoption-plan.md standards/spec/findings-register.md standards/spec/wal-repair-proposal.md'
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c 'spec-spine index owner standards/spec/adoption-plan.md | grep -q 005-adoption-assessment-and-plan'
sh -c 'spec-spine index owner standards/spec/findings-register.md | grep -q 005-adoption-assessment-and-plan'
sh -c 'spec-spine index owner standards/spec/wal-repair-proposal.md | grep -q 005-adoption-assessment-and-plan'
```
