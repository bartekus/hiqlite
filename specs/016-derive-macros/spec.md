---
id: "016-derive-macros"
title: "Adopt the derive macros"
status: draft
kind: "adoption"
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: medium
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "006-cache-state-machine"
origin:
  retroactive: true
  paths:
    - "hiqlite-derive/src/"
establishes:
  # The **package** was renamed to `hiqlite-derive-patched` by the downstream release (`031`
  # B-2); the directory and the library name are unchanged. A crate unit resolves by package
  # name, so this claim had to follow the rename or stop resolving. This is a correction to a
  # territory declaration, not a change to what this spec says: its text is unedited.
  - { kind: crate, id: "hiqlite-derive-patched" }
  - "hiqlite-derive/src/lib.rs"
  - "hiqlite-derive/src/from_row.rs"
  - "hiqlite-derive/src/into_cache_data.rs"
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
  Adopts the two derive macros: what FromRow accepts in a #[column] attribute
  and what each attribute expands to, and what CacheVariants requires of an
  enum. Four findings, all demonstrated: CacheVariants emits its generics in
  the wrong position and rejects data-carrying variants, FromRow does not
  recognise the core spelling of Option, and the trait it implements cannot
  report a conversion failure. Repairs no runtime behavior.
---

# 016: Adopt the derive macros

## 1. Purpose

`hiqlite-derive` is 362 lines that write code into every consumer that uses the
`macros` feature. Its contract is the set of `#[column]` attributes it accepts
and what each one expands to, and none of that was written down: the only
description was one example crate, and the only tests were two that the fork
added while repairing a silent integer clamp.

This spec states the contract and the shape of what it emits, and records what
it rejects.

**It is an adoption.** Nothing is repaired. The only change to a shipped file is
four tests added to the existing `#[cfg(test)]` module in `from_row.rs`.

## 2. Territory

**Establishes** the `hiqlite-derive` crate and its three files. Nothing claimed
them.

**Extends**, without re-establishing: `000` on `spec-spine.toml`; `005` on the
findings register and the adoption plan.

**Depends on** `006`, which owns the `CacheVariants` trait the second macro
implements.

**Describes without claiming** `hiqlite/src/macros.rs`, the re-export point, and
`examples/derive-complex-types`, which is `017`'s.

## 3. Behavior

### B-1. `FromRow` implements `From`, which is why nothing can fail

`impl_from_row` (`from_row.rs:5-102`) emits
`impl ::std::convert::From<&mut ::hiqlite::Row<'_>> for #name`. The trait is
`From` and not `TryFrom`, so the generated `from` has no way to return an error.
Every attribute that can meet bad data therefore expands to a panic: `flatten`
to `try_from(...).unwrap()` (`:28`), `parse` to `parse().unwrap()` (`:60`,
`:63`), and `from_i32` to `try_from(i).expect("column value does not fit into
i32")` (`:33-37`). KD-3 states the consequence.

The generics are emitted correctly here: `impl #impl_generics ... for #name
#ty_generics #where_clause` (`:92`). B-4 is about the other macro, which is not.

### B-2. The `#[column]` attribute grammar

`ColumnHandler::from` (`:136-255`) walks the attribute's token stream by hand.
The accepted forms are:

| attribute | expansion for `T` | expansion for `Option<T>` |
|---|---|---|
| none | `row.get(#name)` | the same; the `Option` is resolved by `row.get`'s type |
| `rename = "col"` | as above, with `col` as the column name | same |
| `from_i32` | `TryFrom<i64>` then `expect`, then `.into()` | `row.get::<Option<i64>>(..).map(...)` |
| `from_i64` | `row.get::<i64>(..).into()` | `row.get::<Option<i64>>(..).map(...)` |
| `from_string` | `row.get::<String>(..).into()` | mapped |
| `parse` | `row.get::<String>(..).parse().unwrap()`, and `use ::std::str::FromStr;` is emitted once into the function body | mapped |
| `flatten` | `TryFrom::try_from(&mut *row).unwrap()` | not special-cased |
| `skip` | `Default::default()` | same |

`rename` may be combined with exactly one of `from_i32`, `from_i64`,
`from_string` or `parse`, in **either** order (`:177-212` and `:213-249`), and
both orders produce identical output. Anything else reaches `do_panic`
(`:141-156`), which prints the whole accepted grammar. `skip` and `flatten`
cannot be combined with `rename`: the match arms for them consume no further
tokens, so a trailing `, rename = "x"` is silently ignored rather than rejected.

Attributes whose path does not start with `column` are skipped (`:162-166`), so
`#[serde(...)]` and friends pass through.

### B-3. `Option` is detected by name, from a three-segment path

`is_field_ty_opt` (`:104-119`) takes the type path's segments, strips a leading
`std`, then strips an `option`, then asks whether what remains is `Option`. So
`Option<T>`, `option::Option<T>` and `std::option::Option<T>` are recognised.
`core::option::Option<T>` is not, because `core` is not stripped. KD-2, and it is
executed.

A non-path type, for example a reference or a tuple, returns `Some(false)`.

### B-4. `CacheVariants` supports exactly one shape of enum

`impl_cache_variants` (`into_cache_data.rs:4-39`) walks an enum's variants in
declaration order, emitting `Self::#id => #idx` for `hiqlite_cache_index` and
`(#idx, #name)` for `hiqlite_cache_variants`. The index is the declaration
position, so **reordering the variants renumbers every cache**, which is the
migration hazard `006` and F-027 are about, stated here because this is where the
number comes from.

Two shapes it does not support, both demonstrated in section 4:

- a **generic** enum, because the implementation header is
  `impl ::hiqlite::CacheVariants for #impl_generics #name #ty_generics`
  (`:25`), with the generics after `for` instead of after `impl`. KD-1.
- an enum with a **data-carrying variant**, because `Self::#id` is a unit-variant
  pattern. KD-1's second half.

A struct or union reaches `unimplemented!()` (`:21`). KD-4.

## 4. Evidence and its limits

Six tests in `from_row.rs`, four added here. Two were already present, added by
the fork's `from_i32` clamp repair, and are retained.

| test | what it establishes |
|---|---|
| `from_i32_uses_try_from_and_never_clamps` | pre-existing: the `from_i32` expansion is a checked conversion |
| `basic_mapping_uses_row_get_by_column_name` | pre-existing: the `From` impl and `rename` |
| `the_core_spelling_of_option_is_not_recognised` | both halves of B-3: `std::option::Option` takes the optional branch and `core::option::Option` does not (KD-2) |
| `the_fallible_attributes_expand_to_unwrap_or_expect` | `flatten`, `parse` and `skip` expansions, the injected `use FromStr`, and that the trait is `From` (B-1, KD-3) |
| `rename_combines_with_a_conversion_in_either_order` | both orders of the combined form produce identical output (B-2) |
| `an_enum_input_panics_instead_of_emitting_a_diagnostic` | the non-struct path (KD-4) |

**KD-1 was observed by compiling probes, not by a committed test.** Both probes
are quoted in KD-1 with the compiler's exact output. They are not in the tree,
because a source file that fails to compile cannot live in a crate CI builds, and
adding `trybuild` to reach the same result is a new dependency, which an adoption
does not introduce (D-2).

**What is not established.**

- **That any generated code is correct at runtime.** Every test here asserts on
  the token stream `impl_from_row` returns. That the emitted `row.get` calls do
  the right thing against a real row is `003`'s, and the only place it is
  exercised is `017`'s `derive-complex-types` example and the cluster suite's
  type-conversion phase (`012` B-1 phase 7).
- **Anything about `impl_cache_variants` by unit test.** It returns
  `proc_macro::TokenStream`, which cannot be constructed or converted outside a
  proc-macro invocation, so it is not callable from a test in its own crate.
  `impl_from_row` returns `proc_macro2::TokenStream` and is, which is the only
  reason half this section exists.
- **The silently-ignored trailing tokens after `skip` and `flatten`** (B-2). Read
  from the match arms; no test asserts it, because asserting that something is
  ignored requires deciding it should not be, which is a repair.

## 5. Known defects

Recorded as found, none repaired. Each is also filed in
`standards/spec/findings-register.md`.

**KD-1. `CacheVariants` cannot be derived on a generic enum, or on one with
data-carrying variants** (F-077). `into_cache_data.rs:25` emits

```rust
impl ::hiqlite::CacheVariants for #impl_generics #name #ty_generics #where_clause
```

with `#impl_generics` after `for`. For a non-generic enum it is empty and the
header is accidentally correct; for `enum Generic<T>` it expands to
`impl ::hiqlite::CacheVariants for <T> Generic<T>`, which is not Rust. Observed:

```
error: expected `::`, found `Generic`
error: proc-macro derive produced unparsable tokens
```

Separately, `Self::#id => #idx` (`:17`) is a unit-variant pattern, so a
data-carrying variant is rejected. Observed:

```
error[E0533]: expected unit struct, unit variant or constant,
              found tuple variant `Self::One`
```

Consequence: the cache-variant enum must be a plain unit-variant, non-generic
enum, which every current consumer happens to be, and neither restriction is
documented or diagnosed. The first is a one-token fix; the second is a design
choice that should be a `compile_error!` rather than an E0533 pointing at the
derive. **Observed by execution**, both halves.

**KD-2. `core::option::Option` is treated as a non-optional type** (F-078).
`from_row.rs:104-119` strips a leading `std` and an `option` segment but not
`core`, so `core::option::Option<i64>` fails the check and the field takes the
non-optional branch: `row.get::<i64>("a").into()`. Consequence: in a `no_std`-
styled or `core`-preferring codebase, a nullable column mapped through
`from_i32`, `from_i64`, `from_string` or `parse` asks the row for a bare value
and fails at runtime rather than yielding `None`. **Observed by execution**, both
spellings.

**KD-3. The derived conversion cannot report a failure** (F-079). B-1. The macro
implements `From<&mut Row>`, so `flatten`, `parse` and `from_i32` all expand to
`unwrap` or `expect` (`:28`, `:60`, `:63`, `:35`). Consequence: a row whose value
does not fit, does not parse, or whose nested type rejects it, panics inside a
query result mapping rather than returning an error to the caller, and the panic
surfaces wherever `query_map` was called. Classed as a limit: the trait choice is
deliberate and `From` is what makes `query_map` ergonomic; what is missing is any
statement of the cost, and a `TryFromRow` alongside it. `lib.rs:12` carries a
`TODO` about returning a result, which is about the macro's own implementation
rather than about this. **Observed by execution** for all three expansions.

**KD-4. A non-struct input panics instead of producing a diagnostic** (F-080).
`from_row.rs:81-82` and `into_cache_data.rs:21` are `unimplemented!()`.
Consequence: `#[derive(FromRow)]` on an enum ends the compilation with
`proc-macro derive panicked: not implemented`, with the span on the derive and no
statement of what is supported, where a `syn::Error::to_compile_error` would
point at the item and say "FromRow supports structs with named fields". Classed
as a limit for the same reason as KD-3: nothing promises otherwise. **Observed by
execution** for the `FromRow` half.

## 6. Resolved decisions

**D-1 (2026-09-21, nothing here is repaired).** KD-1's first half is moving one
interpolation, and it is still a change to generated code with no compile-fail
coverage to regress against. KD-2 is one added segment strip. Both belong with a
change that also adds the compile-fail harness D-2 declines to add.

**D-2 (2026-09-21, no `trybuild` and no committed compile-fail case).** The
honest evidence for KD-1 is a probe compiled and its output quoted, which section
4 says in those words. Adding `trybuild` as a dev-dependency to reach the same
conclusion is a new dependency on a crate whose whole purpose is testing, taken
by an adoption that repairs nothing; the closing condition for W-18 asks for
compile-pass and compile-fail tests, and this spec delivers the compile-pass half
as token-stream assertions and states plainly that the compile-fail half is a
recorded probe.

**D-3 (2026-09-21, the crate is claimed as a crate unit and as three files).**
The crate unit is what makes `hiqlite-derive` attributable at all; the three file
units are what the coupling gate needs to see a change to any of them as owned.

## 7. The inventory declaration this spec requires

`hiqlite-derive/src/**/*.rs` is in no content hash at the pinned revision. The
package is in the cargo workspace, so its three files are already in the coverage
denominator; package membership does not feed the freshness hash, which is the
same gap `009` section 8 first recorded. `spec-spine.toml` gains one glob.

**It is a freshness declaration, not an enforcement setting.**

## 8. Out of scope

- **Every repair.** KD-1 to KD-4 are recorded and left.
- **The `CacheVariants` trait**, which is `006`'s, and the ordering hazard B-4
  names, which is F-027's.
- **`hiqlite/src/macros.rs`**, the re-export point, which no spec claims.
- **The examples**, including `derive-complex-types`, which are `017`'s.
- **Whether the public API is frozen.** W-17.
- **Ratification, enforcement, and any tool or pin change.**

## Verification

Run with `just spine-verify 016`.

```verify:cli
test -f hiqlite-derive/src/lib.rs
test -f hiqlite-derive/src/from_row.rs
test -f hiqlite-derive/src/into_cache_data.rs
sh -c 'spec-spine index owner hiqlite-derive/src/from_row.rs | grep -q 016-derive-macros'
sh -c 'spec-spine index owner hiqlite-derive/src/into_cache_data.rs | grep -q 016-derive-macros'
sh -c 'spec-spine registry relationships 016-derive-macros | grep -q 006-cache-state-machine'
cargo test -p hiqlite-derive --lib from_row::tests::the_core_spelling_of_option_is_not_recognised -- --exact
cargo test -p hiqlite-derive --lib from_row::tests::the_fallible_attributes_expand_to_unwrap_or_expect -- --exact
cargo test -p hiqlite-derive --lib from_row::tests::rename_combines_with_a_conversion_in_either_order -- --exact
cargo test -p hiqlite-derive --lib from_row::tests::an_enum_input_panics_instead_of_emitting_a_diagnostic -- --exact
cargo test -p hiqlite-derive --lib from_row::tests::from_i32_uses_try_from_and_never_clamps -- --exact
cargo test -p hiqlite-derive --lib from_row::tests::basic_mapping_uses_row_get_by_column_name -- --exact
grep -q 'proc_macro_derive(FromRow, attributes(column))' hiqlite-derive/src/lib.rs
grep -q 'proc_macro_derive(CacheVariants)' hiqlite-derive/src/lib.rs
sh -c 'grep -q "impl #impl_generics ::std::convert::From<&mut ::hiqlite::Row" hiqlite-derive/src/from_row.rs'
sh -c 'grep -q "impl ::hiqlite::CacheVariants for #impl_generics #name #ty_generics #where_clause" hiqlite-derive/src/into_cache_data.rs'
sh -c 'grep -q "index_matches.push(quote! {Self::#id => #idx,});" hiqlite-derive/src/into_cache_data.rs'
grep -q 'Data::Struct(_) | Data::Union(_) => unimplemented!(),' hiqlite-derive/src/into_cache_data.rs
grep -q 'Data::Enum(_) => unimplemented!(),' hiqlite-derive/src/from_row.rs
sh -c 'grep -q "if s == \"std\" {" hiqlite-derive/src/from_row.rs'
sh -c '! grep -q "if s == \"core\"" hiqlite-derive/src/from_row.rs'
grep -q 'column value does not fit into i32' hiqlite-derive/src/from_row.rs
sh -c 'grep -q "TryFrom::try_from(&mut \*row).unwrap()" hiqlite-derive/src/from_row.rs'
grep -q 'use ::std::str::FromStr;' hiqlite-derive/src/from_row.rs
grep -q 'ColumnAttr::Skip => quote! {#id: ::std::default::Default::default(),},' hiqlite-derive/src/from_row.rs
grep -q 'hiqlite-derive/src/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/016-derive-macros'
```
