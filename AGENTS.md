# AGENTS.md: hiqlite (bartekus fork)

This file is the shared cross-agent protocol authority for this repository, read
through the AGENTS.md convention by Claude Code, Codex CLI, Cursor, GitHub
Copilot, and anything else that honors it.

**Project-specific commands and policies live here.** They are not duplicated
into a skill, a rule file, an agent definition, or a second protocol document. A
tool that needs a project fact reads it from this file. A duplicated command
list is how a step the gate enforces becomes a step every session skips.

This file is owned by `specs/004-governance-harness/spec.md` and is subordinate
to the governance documents below. Where it restates them it is a pointer, not a
competing source.

---

## What this repository is

hiqlite is an embeddable SQLite database with OpenRaft-backed replication. This
is the `bartekus/hiqlite` fork.

**Fork governance is local.** This repository's governance binds work on this
fork and nothing else. Nothing here has been proposed to, reviewed by, or
approved by the upstream project, and no command, workflow, or document here
targets it. The integration branch for governance work is `spec-spine`, and
governance pull requests are opened against that branch on `bartekus/hiqlite`,
never against upstream.

## Governance model

Governance is provided by `spec-spine`, pinned to source revision
`8f2a8f75000af8f8c7348d87928891306b8d65a6` (the `v0.26.0` tag). The binary reports `0.26.0`, and
`spec-spine.toml` sets `required_version = "=0.26.0"`: exact equality, not a
floor, so any other reported version is refused. That equality still admits
every revision that reports `0.26.0`, so the **revision** is the reproducibility
boundary and the version requirement is the coarser of the two.

Authority, highest wins:

1. `specs/000-hiqlite-ownership-bootstrap/spec.md` (tier 1; its `unamendable`
   anchors are non-overridable)
2. `standards/spec/constitution.md` (tier 2)
3. `standards/spec/contract.md` (tier 3, a normative summary)
4. Ordinary specs, `001`+

This is an authority ordering, not a statement about Git history or merge order.

The corpus. `000` through `004` are the **initial pilot**; the rest were added
afterwards and are not retroactive by default. `spec-spine registry list` is the
authority for what exists.

| id | what it governs |
|---|---|
| `000-hiqlite-ownership-bootstrap` | pilot: what a spec is here, the ownership boundaries, the adoption scope, the workflow |
| `001-wal-durability-and-completion` | pilot: WAL append acknowledgement, persistence, completion |
| `002-snapshot-publication-and-recovery` | pilot: snapshot publication, restore, recovery integration |
| `003-client-consistency-and-retry-outcomes` | pilot: client consistency and retry outcomes |
| `004-governance-harness` | pilot: this protocol, the commands, hook policy, CI |
| `005-adoption-assessment-and-plan` | the whole-project assessment, the adoption plan, the findings register (`origin.retroactive: false`) |
| `006-cache-state-machine` | the cache state machine, KV, dlock, TTL, notify |
| `007-cache-log-store` | the OpenRaft log-store adapter and its memory variant |
| `008-wal-append-completion-notification` | the WAL append completion repair; `amends` `001` and holds its acceptance |

**Read `status` per spec, from that spec's own frontmatter.** This file asserts
no corpus-wide lifecycle value, because ratification is per spec and partial
ratification is the normal case; `spec-spine registry list` prints the current
state of each. `implementation: complete` means a spec's claimed units exist and
its acceptance block passes; it says nothing about approval. Ratification is a
human flip of `status` to `approved`, performed by the repository owner. An
agent may propose, implement, validate, and merge under explicit authorization.
**An agent never ratifies.** Merge, publication, release, and upstream
acceptance are four further, separate things.

## Ownership boundaries

- **OpenRaft owns consensus**: elections, quorum, leader rules, log matching,
  membership protocol. No spec here claims it, and no upstream OpenRaft
  guarantee is evidence that hiqlite implements the machinery itself.
- **hiqlite owns its side of the storage traits**: WAL and vote persistence, the
  SQLite and cache state machines, snapshots and recovery integration, client
  and transport and configuration behavior, and exclusive-access assumptions for
  hiqlite-managed storage.
- **External state-machine callers own their own consensus**: the caller's
  durable log, membership, outer snapshot manifest, and replay decisions are the
  caller's. hiqlite owns the atomic mutation, the checkpoint, the receipt, the
  local snapshot image, and the durability configuration.

Never present a caller-owned or OpenRaft-owned guarantee as something hiqlite
provides.

## Session start

1. Read `standards/spec/contract.md`, then `standards/spec/constitution.md` if
   the task touches governance.
2. `spec-spine --version` **before believing any exit code below**. A binary
   predating a flag makes that flag's exit code meaningless, and reporting a
   version problem as spec drift sends someone chasing a phantom.
3. `just spine-check` for the read-only state of the corpus.
4. `git log --oneline -10` and `git status --porcelain`.

Read the smallest set of files the task needs. This is a large existing
codebase; full orientation is not warranted for a small change.

**Never parse `.derived/**` with `jq`, `awk`, `sed`, `python`, or a hand-rolled
reader.** Compiled shards are read through `spec-spine` subcommands only. A
typed read fails at the deserializer with a clean error; an ad-hoc read fails
silently somewhere downstream.

## Commands

| command | what it does | writes? |
|---|---|---|
| `just spine-install` | install the pinned revision | installs a binary |
| `just spine-regenerate` | `spec-spine compile` then `spec-spine index` | **yes** |
| `just spine-check` | `check --fail-on-unresolved --fail-on-warn`, `lint --fail-on-warn`, `index coverage` | no |
| `just spine-couple <base> <head>` | coupling gate for a branch against its base | no |
| `just spine-verify <spec-id>` | run one spec's acceptance block | executes the corpus |

**Regeneration and validation stay separate.** Do not run `spec-spine compile`
or `spec-spine index` as a step of checking. A read-only check that repaired
what it judges would hide that the *committed* copy was stale, and the drift
would then read as an uncommitted local edit rather than as a defect already on
the branch.

`just spine-verify` executes shell commands authored inside a spec. Run it only
after reading that spec's commands. Pull-request CI deliberately does not run
it: a proposed tree is untrusted input.

### The gate, in order

After a trusted edit to a spec, a governed source file, or a governance input:

```sh
just spine-regenerate                       # writes; commit the changed shards
just spine-check
just spine-couple origin/spec-spine HEAD    # the local default; see below
```

Then the stack's own checks for whatever code changed:

```sh
cargo +1.95.0 test -p <crate> --lib --features <the features the change needs>
cargo +1.95.0 clippy -p <crate> -- -D warnings
```

`just check`, `just clippy` and `just test` are the repository's pre-existing
recipes and remain the fuller local sweep. Cluster tests are slow and need
`cache_storage_disk=false`; run them when the change warrants it.

Commit the regenerated `.derived/` shards in the same commit as the change they
describe.

### Base refs

`just spine-couple`'s default base is `origin/spec-spine`, the integration
branch. It is a convenience for a local run and is correct only for a branch
that will merge there. Pass the real base explicitly when it differs.

CI never uses that default: `.github/workflows/spec-spine.yaml` passes
`${{ github.event.pull_request.base.sha }}`, the pull request's actual base
commit, with `fetch-depth: 0`. The comparison range must be the one that will
actually merge.

## What the gate actually enforces

Four different things, routinely confused. Do not describe one as doing
another's work.

1. **Ownership records.** Typed edges are ledger facts.
   `spec-spine registry relationships <id>` answers who claims what. A claim on
   its own enforces nothing.
2. **Freshness detection.** `spec-spine check` compares committed shards against
   what the corpus compiles to, without writing. Exit `0` fresh, `2` stale, `1`
   validation failed or unresolved units refused, `3` a read that could not be
   performed. Read the code for what it means; treating every non-zero as
   staleness sends a session to regenerate shards that were already correct.
3. **Coupling enforcement.** `spec-spine couple` refuses with `C-001` when a
   claimed unit changed and no owning spec changed in the same range.
   `require_ownership` is **off** here, so unclaimed source raises no `C-002`:
   coverage output is migration information, not a completeness gate.
4. **Human review.** Everything the first three do not cover.

**The bypass floor, and what overrides it.** spec-spine ships a built-in,
non-removable bypass list; configuration can only add to it. It covers
`.github/`, `docs/`, `README.md`, `CHANGELOG.md`, `LICENSE`, `CODEOWNERS`,
`.gitignore`, `.gitattributes`, `standards/spec/constitution.md`, `.derived/`,
and the lockfile tails.

An **explicit unit claim overrides the floor**; implicit path-level ownership
does not. Spec `000` claims `standards/spec/constitution.md`,
`.github/workflows/spec-spine.yaml`, and `.gitignore` as explicit `file` units,
so editing any of them without an authoring edit to an owning spec **does**
raise `C-001`. Unenforced here is every floor path nobody claims: `README.md`,
`CHANGELOG.md`, `LICENSE`, `CODEOWNERS`, `.gitattributes`, `docs/`, the
lockfiles, and `.derived/`.

The upstream spec-spine documents say their own constitution is undefended. That
is true there because that corpus does not claim it. Do not copy the sentence
here, and measure before writing either version.

## The refusal rule

If the coupling gate reports that a claimed unit and its owning spec disagree,
**do not** resolve it by editing the spec to match the code you just wrote.
Surface the contradiction and let a human decide.

A `Spec-Drift-Waiver:` line is a human act. A driven session never writes one
for itself.

If a spec's design is imprecise, record the choice you made as a dated `D-n`
entry in that spec's Resolved decisions section. If the design is *wrong*, stop
and report it. Never edit a spec afterwards to ratify what the code happened to
do.

## Writing a spec

Start from `standards/spec/templates/spec-template.md`. New specs are born
`draft`; approval is a human act, and nothing imported from another repository
carries its `approved` status across.

Every spec of the initial pilot, `000` through `004`, is retroactive: the code
existed first. A spec that adopts pre-existing code declares
`origin.retroactive: true` and describes the behavior that is actually there.
Read `origin` per spec rather than assuming it: `005` declares
`retroactive: false`, because its subject did not exist before its text. The
pilot's shape is not a rule for what comes next: a spec whose subject does not
yet exist says so,
declares the territory it will own with `planned: true` on the units it has not
written, and does not claim a retroactive origin it does not have (constitution
VI).

- **Name the configuration.** A durability or recovery claim that does not say
  whether it is about `LogSync::Immediate` or `LogSync::ImmediateAsync` states
  nothing.
- **State the limit of the evidence next to the claim.** Describe what the test
  does, not what a reader might hope it covers. A test that invokes replay steps
  directly has not proved that OpenRaft orchestrates them; a fault injected into
  an internal helper has not proved how it propagates through the public trait.
- **Record what you would not have chosen under a `Known defects` heading**, and
  leave it unfixed there. A proposed fix is a separate spec and a separate
  change. Keep the heading slug exactly `known-defects` or a slug ending in
  `-known-defects`; a heading that continues past the anchor names something
  wider.
- **One origin per unit.** To touch a unit another spec owns, declare `extends`
  on that unit. Do not re-declare `establishes`.
- **To change the constitution**, claim the affected text as a `section` unit of
  `standards/spec/constitution.md`. Do **not** use `amends`: that edge resolves
  to spec ids, and the constitution is not a spec.
- **The per-document transition is adopted** (owner decision, 2026-09-20). What
  closes when the establishing spec is `approved` is the **establishing-draft
  route**, the licence to correct a document in place under that still-`draft`
  spec's ownership: `000` for the constitution, the contract, and the templates
  subtree; `004` for this file. Its scope is those three documents plus every
  file under `standards/spec/templates/`, not the other units those specs own
  (`ARCHITECTURE.md`, the `justfile`, the workflow, `spec-spine.toml`,
  `.gitignore`), which keep the ordinary flow.
- **In-place editing does not stop; the authority changes.** These documents
  state what is true now, so they keep being edited in place. Afterwards the
  edit runs on a later spec's claim instead of the establishing draft's
  ownership, so **do not edit the approved establishing spec to satisfy
  coupling.** The later spec claims the affected text as a `section` unit
  (`establishes` where no spec owns that section, `refines` with a named
  `aspect` or `co_authority` where one does), which makes it an owning spec of
  the path, and its own authoring edit in the same range is what the gate
  requires. A `section` unit needs a sluggable heading, so template guidance
  written in a comment block is claimed at the file unit instead, through the
  same two non-establishing edges. A section unit is narrower than the file
  unit `000` holds, so this is not a second origin: never duplicate an existing
  origin to make the gate pass. The later spec must be `approved` to carry the
  authority: the gate accepts a `draft` claimant, so that requirement is yours
  and the reviewer's, not the tool's.
- **`amends` never edits the amended `spec.md`.** Declare the edge once in the
  amending spec's frontmatter; the amended document keeps its text as it stood.
  Read the inbound view with `spec-spine registry relationships <amended-id>`.

## Hooks

None is installed, and this repository does not create or modify any assistant
configuration directory. `specs/004-governance-harness/spec.md` B-5 states the
policy a hook would have to satisfy before one is added: read and never write,
read `spec-spine check`'s exit codes for what they mean, exit `0` when the
binary is absent, and never write a waiver.

Until then these controls are the contributor running `just spine-check` and
pull-request CI.

## House style

- **No em dash (U+2014)** in any authored content: specs, standards, this file,
  commit messages, pull-request bodies, issues, review comments. Use a colon, a
  semicolon, a comma, parentheses, or two sentences. U+2013 is for numeric and
  section ranges only.
- **No agent-session URLs and no session-tracking trailers** in repository
  content or in anything published to the forge. No substitute tracking link.
- **No AI attribution lines** in commits or pull-request descriptions.
- Conventional commits with the spec ordinal as the scope: `docs(004): ...`,
  `fix(001): ...`.

## Scope discipline

This corpus is deliberately bounded. Out of scope unless a task says otherwise:
repository-wide ownership coverage, runtime defect fixes for anything the specs
record under Known defects, membership integration, distributed leases, live
peer recovery, the remote-client stall, and broader transport coverage.

Widening the bound, by enabling `require_ownership` or
`index coverage --fail-on-untraced`, is a separate adoption decision the owner
makes.
