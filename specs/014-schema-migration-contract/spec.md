---
id: "014-schema-migration-contract"
title: "Adopt the schema migration contract and its fixtures"
status: draft
kind: "adoption"
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "002-snapshot-publication-and-recovery"
  - "012-cluster-integration-evidence"
origin:
  retroactive: true
  paths:
    - "hiqlite/src/migration.rs"
establishes:
  - "hiqlite/src/migration.rs"
  - "hiqlite/tests/cluster/migrations/"
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
  Adopts the migration file-name contract, the set it builds, and the six
  fixtures that exist to test it. Closes F-052 by making the two disabled
  fixture cases executable at the layer that actually rejects them, which is
  where #[should_panic] works and where the cluster test could not reach.
  Records three findings, one of them executed: the panic a malformed name
  produces names the wrong rule. Repairs no runtime behavior.
---

# 014: Adopt the schema migration contract and its fixtures

## 1. Purpose

Migrations are validated in two places that do not know about each other.
`Migrations::build` (`migration.rs:8-65`) validates the embedded **files** at
the caller, before anything is proposed to Raft. `migrate`
(`store/state_machine/sqlite/writer.rs:848-883`) and `last_applied_migration`
(`:906-986`) validate the **applied** set against the embedded one, on the
writer thread of every node, after the entries are committed.

`002` owns the second. Nothing owned the first, and nothing tested it: the
repository ships three `bad_*` fixture directories, two of which have their
assertions commented out with an authored reason (F-052, `012` KD-5).

This spec claims the first layer and the fixture set, states both contracts and
where they differ, and makes the two disabled cases executable.

## 2. Territory

**Establishes** `hiqlite/src/migration.rs` and the fixture subtree
`hiqlite/tests/cluster/migrations/`, which is six `.sql` files in four
directories. Neither was claimed: `012` established the fifteen `.rs` files of
`hiqlite/tests/cluster/` and explicitly left the fixtures to this row.

**Extends**, without re-establishing: `000` on `spec-spine.toml` for the
declarations of section 6; `005` on the findings register and the adoption plan.

**Depends on** `002`, which owns the second validation layer, and `012`, which
owns the test that consumes these fixtures through a live cluster.

**Describes without claiming** `client/migrate.rs:20-90`, which is `003`'s, and
`writer.rs:848-986`, which is `002`'s. B-3 is about how the two layers divide the
work, so it cannot be written without naming them. No line in either is
modified.

## 3. Behavior

### B-1. The file-name contract, stated

`Migrations::build::<T: RustEmbed>()` reads every file in the embedded folder
and requires, in this order:

1. the name contains at least one `_`, and the text before the first one parses
   as `u32` (`:11-17`);
2. after sorting by that integer, the lowest is exactly `1` (`:27-31`);
3. every name ends in `.sql` (`:40-42`);
4. the sorted ids form a run with no gaps: each id is the previous plus one
   (`:52-59`).

What it produces per file is `{ id, name, hash, content }`, where `name` is
everything after the **first** underscore with `.sql` stripped, `hash` is the
hex-encoded sha256 of the file bytes as `rust_embed` computed it, and `content`
is the raw bytes. So `3_types_conversion.sql` is id `3`, name
`types_conversion`. An empty folder returns an empty `Vec` and validates
nothing (`:22-24`).

Every one of the four rules is enforced by a panic or an `expect`. There is no
error return: the signature is `-> Vec<Migration>`. KD-3.

### B-2. The applied-set contract, stated, because it is what the hash is for

`last_applied_migration` (`writer.rs:906-986`) reads `_migrations` and compares
it to the embedded set row by row: ids must be contiguous from the first
embedded id, and for each already-applied row the **name** and the **hash** must
equal the embedded migration's. A mismatch panics with a message naming which
of the three diverged.

That is the contract the hash exists for: a migration file that is edited after
being applied is detected on the next start, on every node, and refused. The
cluster suite asserts the hashes of the first two `good` migrations against the
`_migrations` table (`012` B-1 phase 3); the test added here asserts the same two
constants at the layer that computes them, so the two ends of that contract are
pinned to the same values in the same tree.

`migration_ts` is the Raft log index, not the wall clock (`writer.rs:853-855`),
deliberately, so the `ts` column is identical on every node.

### B-3. Two gap checks, two messages, two layers

`build` panics with `"Migration index has a gap: {} does not follow {}"`
(`migration.rs:53-59`) over the embedded files. `migrate` panics with
`"Migration index has a gap between {} and {}"` (`writer.rs:872-878`) over the
set that survived `retain(|m| m.id > last_applied)`.

They are not redundant. The first catches a broken build; the second catches a
client that sent an optimized subset starting above `last_applied + 1`, which
`client/migrate.rs` does on purpose to reduce request size. The wording of the
two is close enough to be mistaken for the same check in a log, and only one of
them can be reached from the fixtures.

### B-4. The fixture set, and what each fixture is for

| fixture | contract it exists for | reachable |
|---|---|---|
| `good/1_init.sql`, `2_another_migration.sql`, `3_types_conversion.sql` | the valid path: ordering, naming, hashing | yes, by the cluster suite and by this spec's test |
| `bad_1/no_leading_index.sql` | rule 1, a name with no numeric index | **now**, by this spec's test; the cluster assertion is commented out |
| `bad_2/2_bad_start_index.sql` | rule 2, a set that does not start at 1 | **now**, by this spec's test; the cluster assertion is commented out |
| `bad_3/1_filename_ok.sql` | a valid name whose SQL is invalid, so the failure is at apply time | yes, by the cluster suite |

Rules 3 and 4, the `.sql` suffix and the gap, have **no fixture at all**, and
neither does a duplicate id. Section 4 says so rather than implying the set is
complete.

## 4. Evidence and its limits

Four characterization tests, appended to `migration.rs`. They are ordinary
`#[test]` functions, which is the whole point: `migration.rs:29-31` in the
cluster suite records that `#[should_panic]` "does not work in this context",
because the cluster assertions live in an async helper called from another
test. At this layer the context is a synchronous unit test and the annotation
works, so the two fixtures that have been sitting unused are now executed.

| test | what it establishes |
|---|---|
| `migration::tests::a_valid_set_is_ordered_by_index_and_hashed_by_content` | B-1's whole happy path over `good/`, including both hash constants the cluster suite asserts from the other end (B-2) |
| `migration::tests::a_syntactically_invalid_migration_still_builds` | `build` validates names and indices only: `bad_3` builds cleanly and fails later |
| `migration::tests::a_name_without_a_numeric_index_panics_with_the_other_rules_message` | rule 1 over `bad_1`, and KD-1: the message names rule 2 |
| `migration::tests::an_index_set_that_does_not_start_at_one_panics` | rule 2 over `bad_2` |

**What the tests do not establish.**

- **Rule 3 and rule 4.** No fixture has a non-`.sql` file or a gap, so the
  `.sql` `expect` (`:40-42`) and the gap panic (`:52-59`) are source-established
  only. No fixture was added to reach them: D-2.
- **The duplicate-id case**, which is KD-2 and has no fixture either.
- **Anything in B-2.** `last_applied_migration` is `002`'s unit and needs a
  database; the cluster suite reaches its success path only.
- **The `split_once` message** at `:11-13`, which KD-1 is about. It is
  unreachable from any fixture, because reaching it needs a file name with no
  underscore at all.
- **`client/migrate.rs`'s optimization**, which is what makes B-3's second gap
  check necessary. It is `003`'s and is not exercised here.

## 5. Known defects

Recorded as found, none repaired. Each is also filed in
`standards/spec/findings-register.md`.

**KD-1. The panic for a malformed name names the wrong rule** (F-064).
`migration.rs:11-17`. The `split_once('_')` expect says migration file names must
start with `<integer>_<migration_name>`; the `parse::<u32>()` expect says they
must start with an increasing integer with no gaps starting at index 1. For any
name that contains an underscore but whose first token is not a number, which is
the ordinary malformed case and exactly what the repository's own
`no_leading_index.sql` fixture is, the second message fires. So an operator whose
file is named `create_users.sql` is told about gaps and start indices, and the
message that would tell them the actual rule is reachable only for a name with no
underscore at all. **Observed by execution**:
`a_name_without_a_numeric_index_panics_with_the_other_rules_message` asserts the
message that fires, with the finding id in its doc comment.

**KD-2. A duplicate index is reported as a gap** (F-065). `migration.rs:52-59`.
The check is `migration.id != res[len - 1].id + 1`, which a second file with an
already-seen id fails, so `1_a.sql` and `1_b.sql` panic with
`"Migration index has a gap: 1 does not follow 1"`. A gap and a duplicate are
different deployment mistakes with different fixes, and the message describes the
one that did not happen. Source-established; no fixture reaches it (D-2).

**KD-3. Every validation failure is a panic, and the signature cannot carry an
error** (F-066). `Migrations::build` returns `Vec<Migration>` and enforces all
four rules with `expect` and `panic!` (`:13`, `:16`, `:30`, `:42`, `:54`). It is
called from `Client::migrate` (`client/migrate.rs:32`, `:81`), which returns
`Result<(), Error>`, so a caller that handles migration errors correctly still
cannot handle a malformed migration set: the process ends instead.

Consequence, and the reason this is recorded rather than filed as a style note:
it is why the repository's own bad-fixture assertions were commented out.
`hiqlite/tests/cluster/migration.rs:29-31` states that `#[should_panic]` does not
work in that context, which is true of an async helper, and the underlying cause
is that the failure is expressed as a panic rather than as the `Err` the
surrounding test already knows how to assert. Same class as F-009 and F-042;
also a public-API question, which is W-17's. Source-established, with three of
the five panic sites executed.

**Closed by this change.** F-052 (`012` KD-5), which recorded that two of the
three `bad_*` fixtures had their assertions commented out and therefore no
evidence. Both are now executed, at `Migrations::build` rather than through
`Client::migrate`. The entry is retained as the record of the gap; section 4
states what is still unevidenced, which is rules 3 and 4 and the duplicate case,
none of which ever had a fixture.

## 6. The inventory declarations this spec requires

`hiqlite/src/migration.rs` is in no content hash at the pinned revision, for the
reason `010`, `011` and `013` sections give: the existing globs do not reach
`hiqlite/src/*.rs`. The six `.sql` fixtures are in neither the hash nor the
coverage denominator, because the Rust package walk counts `.rs` only.

`spec-spine.toml` therefore gains `hiqlite/src/migration.rs` and
`hiqlite/tests/cluster/migrations/**/*.sql` in `index.extra_hashed_inputs`, and
the same fixture glob in `[coverage] governed_scope`, so the files this spec
claims are in the denominator it is measured against. The fixture glob is the
second entry of its kind, after the example `.sql` files the pilot declared for
the same reason.

**These are inventory declarations, not enforcement settings.**

## 7. Resolved decisions

**D-1 (2026-09-21, nothing here is repaired).** KD-1 is two swapped strings and
KD-3 is a signature change. The first is not worth a behavioral change on its
own and belongs with whichever repair addresses the second; the second changes a
public API's failure mode, which is W-17's frozen-API question.

**D-2 (2026-09-21, no fixture is added for the untested rules).** Rules 3 and 4
and the duplicate case could each be reached by adding a directory under
`tests/cluster/migrations/`. Adding fixtures is adding evidence surface, which is
a reasonable thing to do and is not an adoption: this spec claims what exists and
says precisely what it does not cover. The four existing fixtures are all
exercised, which is what W-10's closing condition asks for, and section 4 names
the three cases that would need new ones.

**D-3 (2026-09-21, the tests live in `migration.rs` and not in a new test
target).** `Migrations` is not exported from the crate root: `lib.rs:47` exports
`AppliedMigration` only, and `mod migration` is private. An integration test
cannot reach `Migrations::build`, so the assertions have to be unit tests. That
also removes the need for the separate target `011` D-2 needed, because nothing
here touches process-wide state.

## 8. Out of scope

- **Every repair.** KD-1 to KD-3 are recorded and left.
- **`migrate`, `last_applied_migration` and `apply_migration`**, which are
  `002`'s. B-2 and B-3 describe them to state where the contract continues.
- **`Client::migrate` and its request optimization**, which are `003`'s.
- **Migration-failure restart behavior**, which needs the cluster surface and is
  reachable only through `012`'s phase 3.
- **Whether the public API is frozen.** W-17, which KD-3 joins.
- **Ratification, enforcement, and any tool or pin change.**

## Verification

Run with `just spine-verify 014`.

```verify:cli
test -f hiqlite/src/migration.rs
test -f hiqlite/tests/cluster/migrations/bad_1/no_leading_index.sql
test -f hiqlite/tests/cluster/migrations/bad_2/2_bad_start_index.sql
test -f hiqlite/tests/cluster/migrations/bad_3/1_filename_ok.sql
test -f hiqlite/tests/cluster/migrations/good/1_init.sql
sh -c 'spec-spine index owner hiqlite/src/migration.rs | grep -q 014-schema-migration-contract'
sh -c 'spec-spine index owner hiqlite/tests/cluster/migrations/good/1_init.sql | grep -q 014-schema-migration-contract'
sh -c 'spec-spine index owner hiqlite/tests/cluster/migration.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine registry relationships 014-schema-migration-contract | grep -q 012-cluster-integration-evidence'
cargo test -p hiqlite --lib migration::tests::a_valid_set_is_ordered_by_index_and_hashed_by_content -- --exact
cargo test -p hiqlite --lib migration::tests::a_syntactically_invalid_migration_still_builds -- --exact
cargo test -p hiqlite --lib migration::tests::a_name_without_a_numeric_index_panics_with_the_other_rules_message -- --exact
cargo test -p hiqlite --lib migration::tests::an_index_set_that_does_not_start_at_one_panics -- --exact
grep -q 'Migration file names must start with `<integer>_<migration_name>' hiqlite/src/migration.rs
grep -q 'Migration scripts must start with an increasing integer' hiqlite/src/migration.rs
grep -q 'panic!("Migrations must start at index 1");' hiqlite/src/migration.rs
grep -q 'Migration scripts must always end with .sql' hiqlite/src/migration.rs
grep -q 'Migration index has a gap: {} does not follow {}' hiqlite/src/migration.rs
grep -q 'Migration index has a gap between {} and {}' hiqlite/src/store/state_machine/sqlite/writer.rs
sh -c 'grep -q "pub fn build<T: RustEmbed>() -> Vec<Migration>" hiqlite/src/migration.rs'
sh -c '! grep -q "pub use migration::Migrations" hiqlite/src/lib.rs'
grep -q 'pub use migration::AppliedMigration;' hiqlite/src/lib.rs
grep -q 'let mut migrations = Migrations::build::<T>();' hiqlite/src/client/migrate.rs
sh -c 'grep -q "if applied.hash != migration.hash {" hiqlite/src/store/state_machine/sqlite/writer.rs'
grep -q '46a52cfa9b2532439423fe769a3a75aa17e8690ee98b2c1b7c5c21560702e2aa' hiqlite/src/migration.rs
grep -q '46a52cfa9b2532439423fe769a3a75aa17e8690ee98b2c1b7c5c21560702e2aa' hiqlite/tests/cluster/migration.rs
grep -q 'hiqlite/src/migration.rs' spec-spine.toml
grep -q 'hiqlite/tests/cluster/migrations/\*\*/\*.sql' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/014-schema-migration-contract'
```
