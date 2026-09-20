---
id: "004-governance-harness"
title: "Operate the governance corpus: agent protocol, commands, hooks, and CI"
status: draft
kind: "governance"
created: "2026-09-19"
owner: "hiqlite maintainers"
risk: medium
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
origin:
  retroactive: true
  paths: ["justfile", ".github/workflows/"]
establishes:
  - "AGENTS.md"
extends:
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "justfile" }
    nature: additive
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: ".github/workflows/spec-spine.yaml" }
    nature: additive
references:
  - unit: { kind: file, path: "standards/spec/contract.md" }
    role: "context"
  - unit: { kind: file, path: "standards/spec/constitution.md" }
    role: "context"
summary: >
  The loop that operates the hiqlite governance corpus: AGENTS.md as the single
  cross-agent protocol authority, the justfile recipes that are the only
  sanctioned entry points to spec-spine, the read-only pull-request CI job, and
  the policy any future session hook must satisfy. Relates to the bootstrap by
  depends_on and by extends edges on the two units the bootstrap established;
  it does not re-establish them and does not replace the bootstrap's ownership.
---

# 004: Operate the governance corpus

## 1. Purpose

The corpus of specs `000` through `003` says what is governed. This spec says
how it is operated: where the protocol lives, which commands are the sanctioned
entry points, what pull-request CI runs, and what a hook would have to satisfy
before it is installed.

It exists as a separate ordinal so that a change to how sessions are steered is
itself a governed change, and so that the bootstrap's ownership record stays a
record of the bootstrap rather than absorbing the harness by implication.

The spec is `draft`. It ratifies nothing, including itself.

## 2. Territory

This spec **establishes** `AGENTS.md`. That file does not exist before this
change, and this spec is its origin.

This spec **extends**, additively, two units that
`000-hiqlite-ownership-bootstrap` established and continues to own:

- `justfile`, for the `spine-*` recipes;
- `.github/workflows/spec-spine.yaml`, for the read-only governance job.

`extends` is deliberate. Section 4 of `000` allows one origin per unit, so a
second `establishes` on either path would be a duplicate origin claim rather
than a relationship. With `extends`, the ledger carries the bootstrap as the
origin and this spec as an explicit, attributed second claim, and either owner
changing alongside the file satisfies the coupling gate.

`standards/spec/constitution.md` and `standards/spec/contract.md` are
**referenced**, not claimed: `000` owns them, and `references` is non-owning.

Not claimed, and not owned by this spec: `.claude/`, any editor or agent
configuration, and `CLAUDE.md`. See section 6.

## 3. Behavior

### B-1. AGENTS.md is the protocol, and the only place it lives

`AGENTS.md` at the repository root is the shared cross-agent protocol authority,
read through the AGENTS.md convention by whichever assistant a contributor uses.
It carries the governance model, the session-start reads, the gate command list,
the boundaries a session must not cross, and the house style for authored text.

Project-specific commands and policies live **there**. They MUST NOT be
duplicated into a skill, a rule file, an agent definition, or a second protocol
document. A skill that needs a project fact reads it from `AGENTS.md`. A
duplicated command list is how a step that CI enforces becomes a step every
session skips.

`AGENTS.md` MUST NOT contradict `standards/spec/constitution.md` or
`specs/000-hiqlite-ownership-bootstrap/spec.md`. Where it restates them it is a
pointer, not a competing source, and the tier order of `000` section 1 decides
any conflict.

### B-2. The justfile recipes are the sanctioned entry points

Five recipes, and the separation between them is the point:

- `just spine-install` installs the pinned revision.
- `just spine-regenerate` **writes**: `spec-spine compile` then
  `spec-spine index`.
- `just spine-check` is **read-only**: `spec-spine check --fail-on-unresolved
  --fail-on-warn`, `spec-spine lint --fail-on-warn`, `spec-spine index
  coverage`.
- `just spine-couple <base> <head>` compares a branch against its base. Its
  local default base is `origin/spec-spine`.
- `just spine-verify <spec-id>` executes one spec's acceptance block.

`spine-regenerate` MUST NOT be folded into `spine-check`. A read-only check that
repaired what it judges would hide that the committed copy was stale (`000`
section 5).

`spine-verify` MUST stay outside both, because it executes commands the corpus
authors rather than commands the tool defines.

### B-3. The local default base is this fork's integration branch

`spine-couple`'s default base is `origin/spec-spine`. It is not the repository's
default branch and not any upstream ref. It is a **convenience default for a
local run**, correct only for a branch that will merge into `spec-spine`.

CI MUST NOT use it. The workflow passes
`${{ github.event.pull_request.base.sha }}`, the pull request's actual base
commit, checked out with `fetch-depth: 0`. A default that happened to be right
for most branches would be silently wrong for the rest, and the gate's whole
value is that the comparison range is the one that will actually merge.

No governance command, recipe, workflow, or document in this repository may
target the upstream project or assume the repository's default branch (`000`
section 11). Pre-existing, unrelated references to the upstream organization do
survive and are deliberately left alone: the `build-image` recipe's default
image name, the three `ghcr.io/sebadob/nioca` invocations in the TLS recipes,
the builder image in `.github/workflows/code_style.yaml`, and
`.github/FUNDING.yml`. Each names a registry path or forge metadata, not a
governance target, which is why the acceptance below greps a named governance
surface rather than the repository at large.

### B-4. Pull-request CI runs the read-only gate and nothing that executes the corpus

`.github/workflows/spec-spine.yaml` runs on non-draft pull requests with
`permissions: contents: read`, installs the pinned revision, and runs the
`spine-check` triple followed by `couple` against the base SHA.

It MUST NOT run `spec-spine verify`, and MUST NOT run `spec-spine compile` or
`spec-spine index`. A pull request is untrusted input: executing acceptance
commands from a proposed tree would run attacker-authored shell in CI, and
regenerating in CI would repair drift the job exists to report.

`.github/` is on the coupling bypass floor, but `000` claims this file as an
explicit `file` unit and this spec `extends` that unit, and an explicit claim
overrides the floor (`000` section 13.1). Editing the workflow without an
authoring edit to `000` or to this spec therefore raises `C-001`. The gate still
only checks that the file and an owning spec moved together; whether the change
is sound is review's question.

### B-5. Hook policy, and the fact that no hook is installed

No session hook is installed in this repository. This spec claims no `.claude/`
path and overwrites no existing agent configuration.

The policy stands anyway, so that a hook added later is measured against
something rather than inventing its own rules. A session hook, if installed:

- MUST read and report, and MUST NOT write into the repository it judges. The
  one edit a live session may legitimately make, regenerating shards after its
  own spec edit, belongs to the session, which can commit the result; a hook
  firing at session end cannot.
- MUST read `spec-spine check`'s exit code for what it means, distinguishing
  `0` fresh, `2` stale, `1` validation failed or unresolved units refused, and
  `3` a read that could not be performed. Reporting every non-zero code as
  staleness sends a session to regenerate shards that were already correct.
- MUST exit `0` when `spec-spine` is absent, naming what it skipped, so a
  missing tool never blocks a session.
- MUST NOT write a `Spec-Drift-Waiver:` line. That is a human act (`000`
  section 6).

Every clause above is conditioned on a hook existing. Installing one is not an
obligation of this spec, and no clause is unmet while none is installed: the
tree satisfies B-5 today by carrying no hook. See D-4.

Until a hook exists, these controls are performed by the contributor running
`just spine-check` and by pull-request CI. This spec does not claim a hook-level
control that does not exist.

### B-6. House style for authored text

No em dash (U+2014) anywhere in authored content: specs, standards, `AGENTS.md`,
commit messages, pull-request bodies, issues, or review comments. Use a colon, a
semicolon, a comma, parentheses, or two sentences. U+2013 is for numeric and
section ranges only.

No agent-session URLs and no session-tracking trailers in repository content or
in anything published to the forge. No AI attribution lines.

Conventional commits naming the spec ordinal as the scope, for example
`docs(004): ...`.

## 4. Evidence and its limits

The acceptance block below runs the read-only gate and asserts that `AGENTS.md`,
the constitution, the contract, and both templates exist; that the five `spine-*`
recipes are defined and that `just --list` parses the file; that `spine-couple`'s
default base is `origin/spec-spine` and that CI passes the pull request's base
SHA; and that the workflow carries no uncommented `spec-spine verify`, no
`spec-spine compile`, and no bare `spec-spine index`.

Two lines are narrower than they may read. The `origin/main` grep covers four
files (`justfile`, `AGENTS.md`, `standards/spec/contract.md`, and the workflow),
and the upstream-organization grep covers three of them, excluding the `justfile`
because of the pre-existing container coordinates named in B-3. Neither sweeps
`standards/spec/constitution.md`, the templates, or `specs/`, so a stray upstream
target in those is caught by review rather than here. The em-dash line, by
contrast, is recursive over `AGENTS.md`, `standards/spec`, and `specs`.

What it does **not** establish: that any contributor or assistant follows
`AGENTS.md`, or that the hook policy of B-5 is enforced by anything, since no
hook exists. Those are held by human review, and B-5 says so.

It also does not exercise the coupling gate, which needs two commits and a diff
range and so cannot run from a single-command acceptance line. The gate's
behavior on the floor paths of KD-1 was established by probing the pinned
revision during authoring, and is re-established on every pull request by the
`couple` step of CI.

## 5. Known defects

**KD-1. The gate's coverage of floor paths depends on a claim, and is easy to
lose by accident.** `.github/workflows/spec-spine.yaml`, `.gitignore`, and
`standards/spec/constitution.md` are on spec-spine's built-in bypass floor and
are enforced here only because `000` claims each as an explicit `file` unit
(`000` section 13.1). Dropping one of those claims would silently move the path
from enforced to exempt, with no diagnostic, because an unclaimed floor path is
exactly what the floor is for. Nothing warns about that transition.

Recorded, not fixed: the fix would be a guard the tool does not offer, and the
honest mitigation is that `000` section 13.1 and the acceptance block below name
the arrangement so a reviewer can see it.

The reference documents this corpus was adapted from state the opposite, that a
constitution on the floor is undefended. That is true of the repository those
documents govern, which does not claim its constitution, and it was measured
here against the pinned revision rather than carried across.

**KD-2. The recipe split is a convention, not a mechanism.** Nothing stops a
contributor from running `spec-spine compile` directly before a check, which
silently repairs the tree the check was about to judge. The separation lives in
the recipes and in B-2, and a hook or a wrapper that enforced it would be the
fix.

**KD-3. The protocol has no mechanical conformance check.** B-1 forbids
duplicating project commands into skills, and nothing detects a duplicate. The
check is review.

## 6. Out of scope

- `.claude/`, `.cursor/`, `.github/copilot-instructions.md`, `CLAUDE.md`, and
  every other assistant configuration surface. None is created, modified, or
  claimed here. A contributor's existing local configuration is untouched.
- Skills, agent definitions, rule files, and merge drivers. The spec-spine kit
  ships versions of these; none is adopted by this change, and adopting one is
  a separate decision with its own review.
- A scaffolded `approved` status on anything. Templates and kit documents that
  arrive marked approved are re-marked `draft` on import; nothing in this
  repository inherits ratification from the repository it was adapted from.
- Every runtime concern of `001` through `003`, and the widening of the adoption
  bound, which `000` section 12 reserves.

## 7. Resolved decisions

**D-1 (2026-09-19, relationship to the bootstrap).** The harness is a separate
ordinal rather than an expansion of `000`. `000` is tier 1 and its `unamendable`
frontmatter is a freeze surface; folding an operational protocol into it would
put session mechanics behind the same freeze as the authored/derived boundary.
The two units the harness needs are reached by `extends`, which is the edge
`000` section 4 names for touching another spec's territory.

**D-2 (2026-09-19, hooks are specified but absent).** B-5 states a policy for a
thing that does not exist. The alternative was to say nothing about hooks, which
would leave the first hook to invent its rules, or to install the kit's hooks,
which would write into a contributor's agent configuration that this change has
no authority over. `implementation` is `in-progress` rather than `complete`
because of this clause: the spec describes a control surface the tree does not
yet carry.

**D-3 (2026-09-19, the bypass floor was measured, not inherited).** The
upstream spec-spine constitution and contract both state that a constitution on
the coupling bypass floor is undefended, and the first draft of this corpus
repeated it. Probing the pinned revision showed the opposite here: an explicit,
ownership-bearing unit claim overrides the floor, so `000`'s explicit `file`
claims make `standards/spec/constitution.md`, `.github/workflows/spec-spine.yaml`
and `.gitignore` all raise `C-001`, while unclaimed floor paths such as
`README.md` and `CHANGELOG.md` do not. `000` section 13.1, the constitution's
Amendment section, `standards/spec/contract.md`, `AGENTS.md`, `ARCHITECTURE.md`
and KD-1 were corrected to say what the tool does. The claims stay: they are
both a ledger fact and, here, real enforcement.

**D-4 (2026-09-19, the harness is implemented; B-5 is a policy, not a
deliverable).** `implementation` moves from `in-progress` to `complete`. Section
1, B-5, and section 4 agree when read together: section 1 says B-5 states what a
hook *would* have to satisfy *before* one is installed, every MUST in B-5 is
conditioned on "A session hook, if installed", and section 4 records that the
hook policy is enforced by nothing because no hook exists. B-5 therefore places
no obligation on this tree. Every unconditional obligation of the spec (B-1
through B-4 and B-6) is present in the tree, and the acceptance block passes in
full against it.

`implementation: complete` means what `000` section 10 and constitution X say it
means: the described implementation and its executable acceptance are present
and pass. It is not a claim that every sentence here is mechanically enforced,
which section 4 and KD-1 through KD-3 deny in detail, and it is not a claim
about approval; this spec remains `draft`.

D-2's closing clause read B-5's conditional policy as an unmet implementation
obligation and set `in-progress` on that reading. That clause is superseded
here. D-2's substantive decision, that hook policy is stated while no hook is
installed, stands unchanged, and no hook is installed by this change.

**D-5 (2026-09-20, the per-document transition reaches `AGENTS.md`, and stops
there).** The owner adopted the transition the constitution had recorded as a
proposal. For this spec it means one thing: the establishing-draft route for
`AGENTS.md`, the single document this spec `establishes`, closes when an owner
sets `status: approved` on this spec. That route is the licence to correct the
document in place under this spec's ownership while this spec is `draft`.
`AGENTS.md` is a standing document and goes on being edited in place afterwards;
what changes is that each edit then runs on an approved later spec's claim
rather than on this spec's ownership. The transition does **not** reach the
units this spec holds
`extends` claims on, the `justfile` and `.github/workflows/spec-spine.yaml`.
Those stay on the ordinary flow, defended as they are today by the coupling gate
and review, because they are build and CI inputs whose edits have nothing to do
with constitutional authority.

After that transition, `AGENTS.md` is changed by a later spec claiming the
affected text as a `section` unit of it: `establishes` where no spec owns that
section yet, `refines` with a named `aspect` or `co_authority` where one does. A
section unit is narrower than the `file` unit this spec holds, so the claim is
not a second origin and not an exception to one-origin-per-unit; `extends` stays
the edge for adding surface to a unit this spec already owns, and no duplicate
origin is ever declared to make the gate pass. That claim makes the later spec
an owning spec of the path, so its own authoring edit satisfies
`spec-spine couple` and this spec needs no routine edit for coupling's sake. The
claiming spec must
be `approved` to carry the authority: the gate accepts a `draft` claimant, which
makes that a review obligation rather than a mechanical one, and section 4's
account of what each mechanism enforces already says the gate proves
participation and not authority.

This decision changes no `unamendable` anchor, ratifies nothing, and leaves this
spec's `status` untouched. The rule itself is stated in the constitution's
Amendment section and summarized in `standards/spec/contract.md`; `AGENTS.md`
carries the operating pointer.

## Verification

```verify:cli
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
```
