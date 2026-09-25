# hiqlite spec-spine contract (normative summary)

A one-page operational summary of the bootstrap spec
(`specs/000-hiqlite-ownership-bootstrap/spec.md`) and the constitution
(`standards/spec/constitution.md`), for quick reference. Those two documents are
authoritative; where this summary is terser, they govern.

## Tool pin

`spec-spine`, source revision `8f2a8f75000af8f8c7348d87928891306b8d65a6` (the
`v0.26.0` tag). The binary reports `0.26.0`, and `spec-spine.toml`
`[meta] required_version` is `"=0.26.0"`: exact equality, not a floor, so a
binary reporting any other version is refused. Equality on a reported version still admits every revision
that reports it, so the **revision** pinned in the `justfile` and in CI is the
reproducibility boundary and the version requirement is the coarser of the two.
Install it with `just spine-install`.

## Inputs (authored governance truth: markdown only)

- `specs/NNN-slug/spec.md`: one spec per directory; directory name equals `id`;
  `NNN` is a unique three-digit ordinal.
- `standards/spec/`: the constitution, this contract, and the templates.
- `spec-spine.toml`: this repository's configuration.
- `AGENTS.md`: the shared agent protocol, owned by the governance-harness spec.

## Outputs (machine truth: compiler-owned JSON, typed reads only)

- `.derived/spec-registry/by-spec/<id>.json`: spec-as-source shards (`compile`).
- `.derived/codebase-index/by-spec/<id>.json` and `.../by-package/<slug>.json`:
  code-as-source shards (`index`).
- `.derived/**/build-meta.json`: wall-clock metadata; the only non-deterministic
  artifact, gitignored, excluded from every determinism check.

Read these only through `spec-spine` subcommands. Never with `jq`, `awk`, `sed`,
or a hand-rolled parser.

## Required frontmatter

`id`, `title`, `status` (`draft` / `approved` / `superseded` / `retired`),
`created` (`YYYY-MM-DD`), and `summary`. Everything else is optional.

## Typed edges (8; `references` is the only non-owning one)

`establishes`, `extends`, `refines`, `supersedes`, `amends`, `co_authority`,
`constrains`, `references`. `origin` is a bootstrap marker, not an edge.

## Authority units

`file` (bare string shorthand; trailing slash means the subtree), `section`
(`{file, anchor}`), `symbol` (`{id}`), `directory`, `crate`, `module`.

## Amending the constitution

`standards/spec/constitution.md` is tier 2 and is **not** changed by an `amends`
edge: `amends` resolves to spec ids and the constitution is not a spec. An
approved ordinary spec changes it by claiming the affected text as a **section
unit of that file** (`establishes` a new principle, `refines` one it tightens
with a named `aspect`, `co_authority` on one genuinely shared) and by
contradicting no `specs/000` `unamendable` anchor. The constitution is a
standing statement, so unlike an amended `spec.md` it is edited in place.

While `specs/000` is `draft`, that rule has no subject and the constitution is
authored text owned by `specs/000`, edited in place under that ownership, with
the coupling gate still requiring an owning spec to move in the same range
(constitution, Amendment).

Ratification is **per spec**, and there is no aggregate corpus-level ratified
state.

**Adopted, owner decision of 2026-09-20: the transition is per document.** What
closes per document is the **establishing-draft route**, the licence to correct
a document in place under the ownership of the still-`draft` spec that
establishes it. It closes when that spec is approved: `specs/000` for the
constitution, the contract, and the templates subtree; `004` for `AGENTS.md`.
This is a settled rule, not a proposal. Its scope is those three documents plus
every file under `standards/spec/templates/`, and not the other units those
specs own (`ARCHITECTURE.md`, the `justfile`,
`.github/workflows/spec-spine.yaml`, `spec-spine.toml`, `.gitignore`), which
keep the ordinary flow.

**In-place editing itself does not stop.** A standing document states what is
true now and is edited in place before and after the transition alike; what
changes is whose authority the edit runs on. Afterwards the affected document
is changed by a **later** spec that claims the affected text as a `section`
unit of it (`establishes` where that section has no owner, `refines` with a
named `aspect` or `co_authority` where it has one) and moves in the same range.
A `section` unit needs a heading the indexer can slug, so template guidance
that lives in a comment block is claimed at the file unit instead, through the
same two non-establishing edges. That claim makes the later spec an owning spec
of the path, which is what the coupling gate requires, so the approved
establishing spec needs no routine edit to satisfy coupling and is not edited
for one. A section unit is narrower than the file unit `specs/000` holds, so
this is not a second origin and not an exception to one-origin-per-unit;
`extends` stays the edge for adding surface to a unit a predecessor owns. The
later spec must itself be `approved` to carry the authority; the gate proves
participation and accepts a `draft` claimant, so that requirement is enforced
by review, not by the tool.

## Amending a spec

An `amends` edge is declared once, in the amending spec's frontmatter. The
amended `spec.md` is not edited to record that it has been amended: its text is
the contract as it stood. The inbound view is `spec-spine registry
relationships <amended-id>`.

## Lifecycle

`status` is `draft` / `approved` / `superseded` / `retired`; `implementation` is
`pending` / `in-progress` / `complete` / `n-a` / `deferred`, or absent.

| `status` | `implementation` | schedulable | unresolved unit is |
|---|---|---|---|
| `draft` | absent, `pending`, `in-progress` | yes | `W-001` warning |
| `approved` | `pending`, `in-progress` | yes | `W-001` warning |
| `approved` | absent | no (settled) | error |
| any | `complete` | no | error |
| any | `n-a`, `deferred` | no | takes its answer from `status` |
| `superseded`, `retired` | any | no | takes its answer from `status` |

`status` is read per spec, from that spec's own frontmatter; no statement here
asserts a corpus-wide value for it. Draft is never a claim about code, and
`implementation: complete` is never a claim about approval. Ratification, merge,
publication, release, and upstream acceptance are five further, separate things
(constitution X). Ratification is per spec: approving one spec settles that
spec, and the corpus has no ratified state of its own.

Ratifying a spec that carries a known-defects section ratifies the record, not
the behavior. It does not endorse the defect and does not bar a repair, which is
an ordinary later spec that `refines`, `amends`, or `supersedes` the adopting
one and carries its own evidence (constitution VI).

## What the gate actually enforces

Four mechanisms are routinely confused. They are not the same thing.

1. **Ownership records.** `establishes` / `extends` / the rest are ledger facts.
   `spec-spine registry relationships <id>` answers who claims what. A claim on
   its own enforces nothing.
2. **Freshness detection.** `spec-spine check` compares the committed
   `.derived/` shards against what the corpus compiles to, in memory, without
   writing. Exit `0` fresh, `2` stale, `1` validation failed or unresolved units
   refused, `3` a read that could not be performed. It catches a stale ledger,
   not a wrong one.
3. **Coupling enforcement.** `spec-spine couple --base <sha> --head HEAD`
   refuses (`C-001`) when a claimed unit changed and no owning spec changed in
   the same range. `require_ownership` is **off** here, so unclaimed source
   raises no `C-002`.
4. **Human review.** Everything the first three do not cover.

**The bypass floor, and what overrides it.** The gate ships a built-in,
non-removable bypass list: `.github/`, `docs/`, `README.md`, `CHANGELOG.md`,
`LICENSE`, `CODEOWNERS`, `.gitignore`, `.gitattributes`,
`standards/spec/constitution.md`, `.derived/`, and the lockfile tails.
Configuration can only add to it.

An **explicit, ownership-bearing unit claim overrides the floor**; implicit
path-level ownership (manifest metadata, a crate floor) does not. Spec `000`
claims `standards/spec/constitution.md`, `.github/workflows/spec-spine.yaml`,
and `.gitignore` as explicit `file` units, so all three are enforced here and
editing one without an authoring edit to an owning spec raises `C-001`. The
upstream spec-spine documents describe their own constitution as undefended,
which is true there only because that corpus does not claim it; do not carry
that sentence across.

Unenforced here is every floor path no spec claims: `README.md`, `CHANGELOG.md`,
`LICENSE`, `CODEOWNERS`, `.gitattributes`, `docs/`, the lockfile tails, and
`.derived/`. Those are held by human review.

## The gate chain

Regeneration and validation are separate, in this order:

```sh
just spine-regenerate                       # writes: compile + index
just spine-check                            # read-only: check + lint + coverage
just spine-couple origin/spec-spine HEAD    # local default; CI passes the PR base SHA
```

`just spine-verify <id>` executes commands authored inside a spec and therefore
sits **outside** the read-only chain and outside pull-request CI. A maintainer
runs it after reading the commands.

## Determinism

Pure function of `(config, file contents)` to byte-identical output; the ledger
is diffable and mechanically mergeable; staleness is detected by content-hash
comparison alone.
