---
id: "017-examples-as-documentation"
title: "Adopt the examples as executable documentation"
status: draft
kind: "adoption"
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: medium
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "009-configuration-contract"
  - "016-derive-macros"
origin:
  retroactive: true
  paths:
    - "examples/"
establishes:
  - "examples/bench/src/main.rs"
  - "examples/bench/src/bench.rs"
  - "examples/cache-only/src/main.rs"
  - "examples/derive-complex-types/src/main.rs"
  - "examples/external-state-machine/src/main.rs"
  - "examples/sqlite-only/src/main.rs"
  - "examples/walkthrough/src/main.rs"
extends:
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Adopts the seven example source files, closing the adoption half of F-015.
  States what each of the six crates demonstrates and under which feature set,
  and records that they are self-checking programs CI never runs: 54 assertions
  compiled and not executed. Four were run by hand for this spec and all four
  passed. Three findings, all demonstrated, including four committed lockfiles
  that the documented build rewrites on every run. Repairs no runtime behavior
  and changes no example.
---

# 017: Adopt the examples as executable documentation

## 1. Purpose

F-015 recorded the examples as invisible to the ledger, and its 2026-09-20
disposition closed the visibility half: the six crates are declared and counted.
The adoption half stayed open, and the note was specific about it, "all seven
example `.rs` files are still unclaimed".

This spec claims them. It also answers the question the adoption plan attached to
this row, "every example builds under documented configurations and
representative behavior is exercised where promised", with a measured answer in
both halves: the builds are verified, and the behavior mostly is not, because
nothing runs them.

**It is an adoption.** No example is modified. This spec changes no source file
at all.

## 2. Territory

**Establishes** the seven `.rs` files of the six example crates.

**Does not claim** the six `README.md` files, the five migration `.sql` files or
the two `config` files. B-2 and B-3 describe them; claiming documentation and
fixtures that no example-specific contract depends on would widen the denominator
without adding a statement anyone needs. The `.sql` and `Cargo.toml` files are
already in `coverage.governed_scope`, declared by the pilot.

**Extends**, without re-establishing: `000` on `spec-spine.toml`; `005` on the
findings register and the adoption plan.

**Depends on** `009`, whose TOML route two examples use, and `016`, whose macros
five of them derive.

## 3. Behavior

### B-1. Six crates, six feature sets, six different things demonstrated

The examples are the only place several feature combinations are exercised at
all. Each crate's `Cargo.toml` is the documented configuration.

| crate | hiqlite features | what it demonstrates |
|---|---|---|
| `walkthrough` | `cache`, `dlock`, `listen_notify`, `macros`, `shutdown-handle` | the full path: a 1-node or 3-node cluster from a programmatic `NodeConfig`, migrations, typed queries through `FromRow`, cache, locks, listen/notify, shutdown |
| `sqlite-only` | defaults plus `macros`, `shutdown-handle` | SQLite alone, with the `NodeConfig` read from a `config` file rather than built in code |
| `cache-only` | `cache`, `macros`, `toml`, no defaults | the cache alone, single node, configuration from file |
| `derive-complex-types` | `cast_ints`, `macros` | the `#[column]` grammar `016` B-2 states, over nested types, `Url`, `Uuid`, enums and a newtype wrapper. The only exercise of `cast_ints` anywhere |
| `external-state-machine` | `external-state-machine`, no defaults | the opt-in engine with no Raft node: replay, receipts, exact and conflicting retries, snapshot build and install on a second node, reopen |
| `bench` | `cache`, `dlock`, `listen_notify`, `macros`, `shutdown-handle` | insert throughput under varying `LogSync` and cluster shapes; a measurement tool, not a correctness demonstration |

`walkthrough` and `bench` take command-line arguments; the other four run with a
bare `cargo run`, which is what their READMEs say.

### B-2. They are self-checking programs

The six crates contain **54** `assert*` calls: 16 in `walkthrough`, 15 each in
`sqlite-only` and `derive-complex-types`, 5 in `cache-only`, 2 in `bench` and 1
in `external-state-machine`. They are not illustrations with printed output; they
are programs that fail if the library stops behaving as the README says.

Nothing runs them. `.github/workflows/code_style.yaml` has four steps and the
example one is `just clippy-examples`, which compiles. Section 4 is the
consequence, and KD-1 records it.

### B-3. Two configuration routes, and two of the crates read from disk

`walkthrough` and `bench` build `NodeConfig` in code (`walkthrough/src/main.rs:52-65`);
`cache-only` and `sqlite-only` read a `config` file from the crate directory,
which is the environment-variable route `009` describes and `009` D-3 could not
test in-process. `derive-complex-types` and `external-state-machine` configure no
node at all.

Five of the six embed migrations with `rust_embed` from their own `migrations/`
directory, which is the same mechanism `014` B-1 specifies.

### B-4. Four of six lockfiles are tracked, and all four are stale

The root `.gitignore:7` ignores `Cargo.lock`, so any example lockfile in the tree
is there by force-add. Four are: `bench`, `cache-only`, `sqlite-only`,
`walkthrough`. Neither `derive-complex-types` nor `external-state-machine` has a
tracked one: the first has an untracked file on disk, and the second has no
lockfile at all until the example is built.

Running the documented build rewrites all four. KD-3.

## 4. Evidence and its limits

**Builds: verified, 2026-09-21.** `just clippy-examples` was run on this tree and
exited `0` for all six crates with **zero warnings**. That is the whole of what
CI establishes, and it is the whole of the "every example builds under documented
configurations" half of this row's closing condition.

**Behavior: four of six executed by hand, 2026-09-21.** The four that take no
arguments were run with `cargo run` from their own directories, on this tree:

| crate | result |
|---|---|
| `external-state-machine` | exit `0`. Replayed five sequences, advanced over a non-SQLite entry, answered a retry from a retained receipt, rejected a conflicting retry, built and installed a 20,480-byte snapshot on a second node, reopened at the same frontier |
| `derive-complex-types` | exit `0`. Mapped the nested entity, both enums, the `Url`, the `Uuid` and the newtype wrapper |
| `cache-only` | exit `0`. Started a single node, exercised the cache, shut down cleanly |
| `sqlite-only` | exit `0` |

**What is not established.**

- **`walkthrough` and `bench`,** which take arguments and, for `walkthrough`'s
  interesting case, three processes. Not run.
- **Anything by a control.** The four runs above are a dated manual observation,
  the same class of evidence as `012` section 5. Nothing re-runs them, so this
  spec's claim about them decays the moment the tree changes. KD-1 is precisely
  that gap, and this spec records it rather than closing it, for the reason D-1
  gives.
- **Any behavior the examples assert that a library change could break silently.**
  All 54 assertions are compiled and, in CI, never evaluated.
- **The `config` files' environment route** beyond the two runs above, which is
  `009` D-3's open gap.
- **Warnings.** The local run was clean, but KD-2 means a future warning would not
  fail CI, so "zero warnings" is a property of today's tree and not a gate.

## 5. Known defects

Recorded as found, none repaired. Each is also filed in
`standards/spec/findings-register.md`.

**KD-1. Fifty-four assertions are compiled and never evaluated** (F-081). B-2.
The examples are the fork's executable user documentation, they check themselves,
and the only thing CI does with them is `just clippy-examples`. Consequence: a
library change that breaks what a README promises compiles, lints and merges; the
examples' value as documentation is exactly the part CI does not verify. Four of
the six were run for this spec and passed (section 4), which is a dated
observation and not a control. Related to F-019, which is the same shape for the
`dashboard` and `server` features, and to F-030's question about who runs a
manual step. **Observed**: the assertion count is source-established, and the
four runs are executed.

**KD-2. The examples are the one clippy step that does not deny warnings**
(F-082). `justfile:112-123`: `clippy-examples` is `cargo clippy` with no
`-- -D warnings`, while `cargo clippy -- -D warnings` and every line of `just
clippy` deny them, and the workflow step immediately above this one is named
"Clippy (deny warnings)". Consequence: a warning introduced in an example is
reported and ignored, in a recipe that `just verify` runs beside two that fail on
one. Source-established; the local run produced no warnings, so nothing is
currently hidden by it.

**KD-3. The four tracked example lockfiles are stale, and the build rewrites
them** (F-083). Running `just clippy-examples` on this tree modified all four
tracked `Cargo.lock` files. Each gained `constant_time_eq`, which `hiqlite` now
depends on, and `cache-only`'s gained thirteen packages including the whole
`rusqlite`, `libsqlite3-sys` and `serde_rusqlite` chain, so its lock predates a
change in what its feature set pulls in.

Consequence: the dependency set a reader, or a scanner, sees in a committed
example lockfile is not the set CI builds, and CI discards its own corrected
version every run. The inconsistency extends to which crates have a lockfile at
all: neither `derive-complex-types` nor `external-state-machine` has a tracked
one, the first having an untracked file on disk and the second none until it is
built, while `.gitignore:7` ignores `Cargo.lock` so the four that are tracked were
force-added and nothing in the repository records the rule.

**Observed by execution**: the rewrite was produced, its contents recorded above,
and then reverted, because updating dependency locks is not something an adoption
does (D-2). Same family as F-012 and F-071: artifacts that must agree, with no
check.

**Closed by this change.** F-015, whose visibility half was closed on 2026-09-20
and whose adoption half is closed here: all seven example `.rs` files are claimed.
The entry is retained as the record.

## 6. Resolved decisions

**D-1 (2026-09-21, no example is run by a control, and none is changed).**
Adding a CI step that runs the four argument-free examples would close KD-1 and is
the obvious next move. It is a workflow change, which is `000`'s territory and
W-19's row, and it would change what CI rejects, which is an enforcement decision.
This spec measures the gap and hands it over.

**D-2 (2026-09-21, the lockfile rewrite is reverted, not committed).** The build
produced a corrected lock for four crates and the diff is quoted in KD-3.
Committing it is a dependency update: it changes which versions the examples
build against, on a tree where nothing runs them, in a change whose subject is
ownership. Reverted, recorded, and left to the owner, who also has to decide the
tracking rule the second half of KD-3 is about.

**D-3 (2026-09-21, the READMEs and fixtures are not claimed).** B-2 and B-3
describe them; the `.sql` and `Cargo.toml` files are already in the governed scope
from the pilot's declaration. Claiming six README files would add territory whose
contract is "it says what the example does", which B-1 now states in one place.

## 7. The inventory declaration this spec requires

The six crates are declared in `layout.standalone_rust_workspaces`, which the
2026-09-20 configuration change added, so their seven `.rs` files are already in
the coverage denominator. That declaration does **not** hash them, which
`spec-spine.toml`'s own comment says in those words, and no glob reached them:
the example entries were `examples/*/migrations/*.sql` and
`examples/*/Cargo.toml`.

`spec-spine.toml` gains `examples/*/src/*.rs` in `index.extra_hashed_inputs`, so
a byte change in a claimed example stales the committed index. No
`[coverage] governed_scope` entry is needed.

**It is a freshness declaration, not an enforcement setting.**

## 8. Out of scope

- **Every repair**, including the one-flag fix for KD-2.
- **Running examples in CI.** D-1, W-19.
- **Updating or standardising the example lockfiles.** D-2.
- **The READMEs, migrations and `config` files.** D-3.
- **`hiqlite-derive`**, which is `016`'s, and the configuration contract, which
  is `009`'s.
- **Ratification, enforcement, and any tool or pin change.**

## Verification

Run with `just spine-verify 017`.

```verify:cli
test -f examples/walkthrough/src/main.rs
test -f examples/external-state-machine/src/main.rs
sh -c 'spec-spine index owner examples/walkthrough/src/main.rs | grep -q 017-examples-as-documentation'
sh -c 'spec-spine index owner examples/bench/src/bench.rs | grep -q 017-examples-as-documentation'
sh -c 'spec-spine index owner examples/external-state-machine/src/main.rs | grep -q 017-examples-as-documentation'
sh -c 'spec-spine registry relationships 017-examples-as-documentation | grep -q 016-derive-macros'
sh -c 'test "$(ls -d examples/*/ | wc -l | tr -d " ")" = "6"'
sh -c 'test "$(grep -h -c "assert" examples/*/src/*.rs | paste -sd+ - | bc)" = "54"'
sh -c 'grep -q "cargo clippy$" justfile'
sh -c 'grep -A8 "^clippy-examples:" justfile | grep -q "cargo clippy$"'
sh -c '! grep -A8 "^clippy-examples:" justfile | grep -q "D warnings"'
grep -q 'just clippy-examples' .github/workflows/code_style.yaml
grep -q 'Clippy (deny warnings)' .github/workflows/code_style.yaml
grep -q '^Cargo.lock$' .gitignore
sh -c 'git ls-files examples | grep -c "Cargo.lock" | grep -q "^4$"'
sh -c '! git ls-files examples/derive-complex-types | grep -q "Cargo.lock"'
sh -c '! git ls-files examples/external-state-machine | grep -q "Cargo.lock"'
sh -c 'grep -q "features = \[\"cast_ints\", \"macros\"\]" examples/derive-complex-types/Cargo.toml'
sh -c 'grep -A3 "^hiqlite = " examples/external-state-machine/Cargo.toml | grep -q "external-state-machine"'
sh -c 'grep -q "default-features = false" examples/cache-only/Cargo.toml'
test -f examples/cache-only/config
test -f examples/sqlite-only/config
sh -c 'test "$(ls examples/*/migrations/*.sql | wc -l | tr -d " ")" = "5"'
grep -q 'examples/\*/src/\*.rs' spec-spine.toml
grep -q 'standalone_rust_workspaces' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/017-examples-as-documentation'
```
