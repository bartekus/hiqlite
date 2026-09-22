---
id: "028-enforcement-readiness-and-acceptance-control"
title: "Correct the coverage denominator, measure the enforcement rungs, and run acceptance on trusted commits"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: medium
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "004-governance-harness"
  - "005-adoption-assessment-and-plan"
  - "019-dashboard-build-contract"
amends: ["004-governance-harness"]
# D-5: this spec's `## Verification` block IS 004's acceptance from now on, and 004's own file
# is not edited. Whole-block replacement is the mechanism's unit.
amends_verification: ["004-governance-harness"]
amends_sections:
  - "3-behavior"
establishes:
  - ".github/workflows/acceptance.yaml"
extends:
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: superseding
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
summary: >
  Resolves W-14 against the pinned tool rather than a pin upgrade: the earlier
  probe read `resolver_exclusions` as path prefixes where the tool matches path
  components, so the key that was recorded as having no effect does exactly what
  rung 0 asked. Measures rungs 1 and 2 and refuses to enable either, because
  both are global refusals and the corpus is not ready for one. Adds W-24's
  restricted post-merge acceptance job, which runs on trusted commits only and
  references no secret. Reassesses OD-4 with an accurate account of what the
  per-document transition does and does not reach.
---

# 028: Correct the coverage denominator, measure the enforcement rungs, and run acceptance on trusted commits

## 1. Purpose

Three governance rows were outstanding, and one of them was blocking the other
two on a conclusion that turns out to have been wrong.

- **W-14** recorded rung 0 as "blocked on the pinned tool", with a recommended
  resolution of a pin upgrade. It is not blocked, and no pin upgrade is needed.
- **W-23** asks which enforcement rungs to enable. Answering it needs the
  denominator W-14 owns, and needs a probe of each rung rather than a reading of
  what it is supposed to do.
- **W-24** asks whether the acceptance blocks should be executed by an automated
  control, given that pull-request CI deliberately abstains.

One responsibility: **what the governance harness measures, what it refuses, and
on which commits it executes the corpus.**

## 2. Territory

**Establishes** `.github/workflows/acceptance.yaml`.

**Extends** `000`'s `spec-spine.toml` additively, which is the ordinary flow for
that unit and is what every adoption wave has done; and `005`'s adoption plan as
`superseding`, because this spec replaces a recorded conclusion in it rather
than adding to it.

**Amends** `004` section 3 and carries its acceptance (D-5).

**Ownership boundary.** The tool's semantics are the tool's. What this spec owns
is the configuration this repository sets and the claims it makes about what
that configuration does, each of which is measured here rather than read off a
key name.

## 3. Behavior

### B-1. The denominator excludes generated and vendored files, by component

`index.resolver_exclusions` gains `"static"` and `"spow"`.

**They are path components, not path prefixes.** That single fact is the whole
of W-14. The pinned tool's coverage universe is a conjunction, and one conjunct
is `!has_excluded_component(path, resolver_exclusions)`: it splits a path into
components and asks whether any of them is in the list. The 2026-09-19 probe
tried `"hiqlite/static"` and `"dashboard/src/spow"`, which are not components of
anything, and correctly observed no effect. `"static"` **is** a component of
`hiqlite/static/_app/immutable/chunks/…`, and `"spow"` is a component of
`dashboard/src/spow/…`.

Measured, on the pinned revision: the denominator goes from 239 to 223, all
sixteen files leave, and the numerator does not move.

What each set is, and why it may leave:

- **`static`**, twelve generated `.js` files. Generated output, governed through
  its build contract by `019` and never claimed as authored bytes (OD-2, and
  section 4 milestone 4 of the plan).
- **`spow`**, four vendored WebAssembly binding files. Third-party source,
  referenced and never claimed (A15, milestone 3).

### B-2. No ownership check on authored source is weakened, and that was measured

W-14's blocker was that the only key which removed these files also exempted
them from the coupling gate. `resolver_exclusions` is a different conjunct from
the bypass predicate, and `coupling.bypass_prefixes` stays empty. Three probes,
each run against the pinned revision:

| probe | result |
|---|---|
| change `hiqlite/src/config.rs` without an authoring edit to an owning spec | **`C-001` still refuses**, naming `001` and `009` |
| change `dashboard/src/spow/spow-wasm.js` | **the index still goes stale**, because `index.extra_hashed_inputs` hashes it through the `dashboard/src/**/*.js` glob and that is a separate mechanism from the coverage universe |
| change a generated `.js` file alone | no drift, which is unchanged: nobody claims those bytes |

The second is the one worth stating plainly: a vendored file leaving the
*denominator* does not stop a change to it from staling the committed index.
Coverage and freshness are different questions and the tool keeps them apart.

### B-3. Neither enforcement rung is enabled, and both were measured before that was decided

**Rung 1, `coupling.require_ownership = true`: probed, refuses, not enabled.**
With it on, changing `dashboard/src/lib/components/Button.svelte` raises
`C-002 … is not claimed by any spec`. That file is unclaimed **on purpose**:
`018` D-3 left the presentational components out of its claim. So the rung
refuses a change the corpus deliberately does not govern.

**Rung 2, `spec-spine index coverage --fail-on-untraced`: probed, exits `1`,
not enabled.** Seventy-one files are unclaimed, and the gate does not
distinguish "unclaimed because nobody got to it" from "unclaimed on purpose".

**`require_ownership` is a global boolean.** It is not per-area, per-package or
per-path, and describing it as anything else would be a claim the tool does not
support. There is no configuration at this pin that enables it for the adopted
areas alone. The only thing that makes it safe is the corpus reaching a state
where nothing legitimate is unclaimed, and the plan's rung 1 already says so.

So this spec enables **no** refusal. What it changes is that the reason is now a
measurement rather than an inherited blocker.

### B-4. Acceptance runs on trusted commits, and only those

`.github/workflows/acceptance.yaml` runs `spec-spine verify` for every spec in
the registry, on `push` to the integration branch.

`004`'s trust boundary is preserved exactly. A pull request is untrusted input,
because it could author a command and have CI execute it, and pull-request CI
still does not run `verify`. A push to the integration branch is a commit that
has already been reviewed and merged, so the commands in it are the ones a
maintainer would be running by hand: the same trust level, not a new one.

Four restrictions, each deliberate and each visible in the workflow:

- **`push` to `spec-spine` only.** No `pull_request`, no
  `pull_request_target`, no tag, no `workflow_dispatch` with a ref input.
- **`permissions: contents: read`.** It writes nothing back.
- **No `secrets` are referenced**, so a command authored in a spec cannot read
  one. The publication credentials live in a different workflow with a
  different trigger.
- **The read-only chain runs first**, because an acceptance block asserts things
  about a tree whose derived shards are supposed to be fresh, and a failure
  caused by staleness should say staleness.

It is a **detector, not a gate on merging**: it runs after the merge, so it
reports rather than prevents. F-096 is exactly the failure it is for, and
section 5 says what it would and would not have caught.

### B-5. OD-4 is reassessed, accurately

Three statements, and the middle one is the correction.

**The trigger has occurred and the reassessment is this.** OD-4 defers
ratification of `000` and reassesses after the first substantive adoption wave.
Wave 1 landed on 2026-09-19 and eight more have landed since.

**A routine `spec-spine.toml` change does not require a constitutional
amendment, and never did.** `000` section 1.1 and the contract both say the
per-document transition's scope is the constitution, the contract, the templates
subtree and `AGENTS.md`, and that the **other** units `000` owns, naming
`ARCHITECTURE.md`, the `justfile`, the workflow, `spec-spine.toml` and
`.gitignore`, keep the ordinary flow. The ordinary flow is an `extends` edge
from an owning spec that moves in the same range, which is what this spec does
in B-1 and what every adoption wave has done. Anything that describes a
configuration change as needing an amendment is reading the transition wider
than it is.

**Ratification is still a human act and this release performs none.** Every spec
in the corpus is `draft`, `spec-spine registry list` prints that, and nothing in
this release changes it. Implementation, acceptance, merge, publication and
release have all happened here; ratification has not, and the five are separate
(constitution X). An agent never ratifies.

**The recommendation this reassessment makes is unchanged: alternative (a),
defer.** The reason is stronger now than it was, not weaker. This release
corrected tier-1-adjacent configuration twice, replaced eleven acceptance
commands in one spec and twelve in another, and found a rule violation in its
own delivery (F-096). Under alternative (b) each of those would have needed its
own amending spec and an owner ratification, during a release. The freeze
surface not being final is the cost, and it is the smaller one.

## 4. Evidence and its limits

Every claim in section 3 is a probe run against the pinned revision, and the
numbers are in the text rather than summarized: 239 to 223 with the numerator
unchanged; `C-001` still refusing `hiqlite/src/config.rs`; the index still
staling on a vendored edit; `C-002` refusing `Button.svelte` under rung 1;
`--fail-on-untraced` exiting `1` with 71 unclaimed.

What the acceptance does **not** establish:

- **The workflow has not run.** It runs on a push to the integration branch,
  which happens when this spec merges. Its YAML is validated by nothing here
  beyond being read.
- **No rung is enabled, so no refusal is demonstrated in CI.** The rung probes
  were local.
- **"Component, not prefix" is read from the tool's source and confirmed by
  behavior**, not proved for every possible pattern. A pattern containing a `/`
  was tried and had no effect; a bare component was tried and worked.
- **The exclusion is as wide as the component name.** `"static"` excludes any
  directory called `static` anywhere in this tree, and `"spow"` likewise. Today
  each matches exactly one directory, which was checked, and nothing enforces
  that it stays that way.

## 5. Known defects

**KD-1. The acceptance job cannot catch what it is most needed for, before the
fact.** F-096 is an acceptance block that started failing when another spec's
repair landed. This workflow runs **after** the merge, so it would have reported
that within one run of the integration branch, and it would not have stopped the
pull request. Running `verify` on a pull request is what would, and `004`'s trust
boundary refuses it.

**KD-2. It runs every block, which is slow and will get slower.** One of them
builds a `--release` binary. There is no selection by what changed.

**KD-3. A red acceptance run blocks nothing.** It reports. Branch protection
that required it would be a policy decision this spec does not take.

**KD-4. The component-name exclusion is not scoped to a path.** KD in section 4's
last bullet: a future directory named `static` or `spow` anywhere in the tree
would silently leave the denominator.

**KD-5. Rung 1's readiness is bounded by a decision nobody has taken.** Seventy
one files are unclaimed and some of them, the presentational dashboard
components in particular, are unclaimed by a recorded decision (`018` D-3). Rung
1 cannot be enabled until either they are claimed or the tool grows a way to say
"deliberately unclaimed", and neither is scheduled.

## 6. Resolved decisions

**D-1 (2026-09-21, fix the configuration rather than upgrade the pin).** W-14's
recommended resolution was a pin upgrade adding a denominator-exclusion key.
That upgrade is unnecessary, because the key that does the job is already there
and was mis-probed. Checked at the tool's current head as well: there is still
no separate denominator key, so the pin upgrade would not have delivered what
W-14 asked for either.

**D-2 (2026-09-21, `resolver_exclusions` and not `bypass_prefixes`).** The plan's
rung 3 says source paths are never added to the bypass floor, and
`dashboard/src/spow/` is source-shaped even though it is third-party. B-2's
probes are what establish that this route does not touch the floor.

**D-3 (2026-09-21, enable nothing).** Owner direction was to enable a control
only where the pinned tool supports the intended scope and the probes pass.
Neither rung passes, and `require_ownership` has no per-area scope to support.
Recording the measurement is the deliverable; the refusal is not.

**D-4 (2026-09-21, post-merge acceptance rather than pull-request acceptance).**
The alternative closes KD-1 and reopens the trust boundary `004` exists to hold.
Declined. A detector that runs a merge later is worth having and is not worth
executing a proposed tree's commands to get.

**D-5 (2026-09-21, this block is `004`'s acceptance).** `004`'s commands are
carried forward unchanged and this spec's are added; nothing in `004`'s block
asserted anything this spec removes.

## 7. Out of scope

- **Enabling any enforcement rung.** D-3.
- **Claiming the remaining unclaimed files.** KD-5.
- **Ratification.** B-5, and it is not an agent's act.
- **A pin upgrade.** D-1.
- **Branch protection.** KD-3.

## Verification

Run with `just spine-verify 028`. **This block is `004`'s acceptance as well as
this spec's** (D-5).

```verify:cli
# --- 004's acceptance, carried forward unchanged ---
test -f AGENTS.md
test -f standards/spec/constitution.md
test -f standards/spec/contract.md
test -f standards/spec/templates/spec-template.md
test -f standards/spec/templates/constitution-template.md
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c 'just --list > /dev/null'
grep -q '^spine-install:' justfile
grep -q '^spine-regenerate:' justfile
grep -q '^spine-check:' justfile
grep -q '^spine-verify spec:' justfile
grep -q 'spine-couple base="origin/spec-spine" head="HEAD":' justfile
grep -q 'github.event.pull_request.base.sha' .github/workflows/spec-spine.yaml
sh -c '! grep -n "origin/main" justfile AGENTS.md standards/spec/contract.md .github/workflows/spec-spine.yaml'
sh -c '! grep -n "sebadob" AGENTS.md standards/spec/contract.md .github/workflows/spec-spine.yaml'
sh -c '! grep "spec-spine verify" .github/workflows/spec-spine.yaml | grep -qv "^[[:space:]]*#"'
sh -c '! grep -q "spec-spine compile" .github/workflows/spec-spine.yaml'
sh -c '! grep -qE "spec-spine index[[:space:]]*$" .github/workflows/spec-spine.yaml'
sh -c '! grep -rl "$(printf "\342\200\224")" AGENTS.md standards/spec specs'
# --- what this change adds ---
# the denominator exclusion, by component and not by prefix
sh -c 'grep -q "\"static\"," spec-spine.toml'
sh -c 'grep -q "\"spow\"," spec-spine.toml'
# and the floor is still empty: the exclusion did not go through the coupling gate
sh -c 'grep -q "bypass_prefixes = \[\]" spec-spine.toml'
sh -c 'grep -q "require_ownership = false" spec-spine.toml'
# no generated or vendored file is in the denominator any more
sh -c '! spec-spine index coverage | grep -q "static/_app"'
sh -c '! spec-spine index coverage | grep -q "src/spow"'
# the acceptance workflow exists, runs on a push to the integration branch only, and
# references no secret
test -f .github/workflows/acceptance.yaml
sh -c 'grep -q "branches:" .github/workflows/acceptance.yaml'
sh -c 'grep -q "      - spec-spine" .github/workflows/acceptance.yaml'
sh -c '! grep -q "pull_request" .github/workflows/acceptance.yaml || grep -q "# " .github/workflows/acceptance.yaml'
sh -c '! grep -q "secrets\." .github/workflows/acceptance.yaml'
sh -c 'grep -q "contents: read" .github/workflows/acceptance.yaml'
sh -c 'grep -q "spec-spine verify" .github/workflows/acceptance.yaml'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/028-enforcement-readiness-and-acceptance-control'
```
