---
id: "009-configuration-contract"
title: "Adopt the node configuration contract"
status: draft
kind: "adoption"
created: "2026-09-20"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "001-wal-durability-and-completion"
origin:
  retroactive: true
  paths: ["hiqlite/src/config_toml.rs", "hiqlite.toml", "hiqlite.env"]
establishes:
  - "hiqlite/src/config_toml.rs"
  - "hiqlite.toml"
  - "hiqlite.env"
extends:
  - spec: "001-wal-durability-and-completion"
    unit: { kind: file, path: "hiqlite/src/config.rs" }
    nature: additive
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
summary: >
  Adopts hiqlite's node configuration as a contract rather than as a file.
  Claims the TOML loader, the two shipped reference files, and extends 001's
  claim on config.rs, so the two constructors and the documentation that
  describes them are owned together. Records the precedence rule, the
  per-variable inventory, and five defects found while reading the source.
  Repairs nothing and changes no runtime behavior.
---

# 009: Adopt the node configuration contract

## 1. Purpose

`001` claims `hiqlite/src/config.rs` because the WAL durability mode is read
there. That claim covers one file of a contract that spans four: the environment
constructor in `config.rs`, the TOML constructor in `config_toml.rs`, and the
two reference files, `hiqlite.toml` and `hiqlite.env`, that tell an operator
what exists. F-010 records the gap. This spec closes it by claiming the
contract, not by repairing it.

**It is an adoption.** Every behavior below is described as found. Five defects
are recorded under `known-defects` and left unfixed, per constitution VI. No
runtime file changes except the addition of characterization tests, and those
assert current behavior including the defective parts.

## 2. Territory

**Establishes**, none of which any spec claimed before:

- `hiqlite/src/config_toml.rs`, the TOML constructor and its typed helpers;
- `hiqlite.toml`, the reference TOML file;
- `hiqlite.env`, the reference environment file.

**Extends**, without re-establishing:

- `001` on `hiqlite/src/config.rs`, additive. `001` keeps the origin and its
  durability claims; this spec adds the configuration-surface reading of the
  same file. One origin per unit (`000` section 4).
- `000` on `spec-spine.toml`, additive. The claims above need freshness and
  denominator declarations, stated in section 6, and `000` establishes that
  file. Declaring `extends` is the route `AGENTS.md` names for touching a unit
  another spec owns, and it leaves `000` unedited.

**Not claimed**, deliberately: every other `HQL_*` read. `backup.rs`, `s3.rs`,
`tls.rs`, `init.rs`, `split_brain_check.rs`, `server/proxy/config.rs`,
`dashboard/mod.rs` and `dashboard/session.rs` each read the environment
directly, and each belongs to the spec that will claim its subject. This spec
records where they are (section 3.5) and claims none of them.

## 3. Behavior

### B-1. Two constructors, one struct, different failure modes

`NodeConfig` is built two ways, and the difference is not cosmetic.

- **`NodeConfig::from_env()` and `from_env_file()`** (`config.rs:225-382`) read
  `dotenvy` files and then the process environment. A missing mandatory value or
  an unparseable one **panics**: `expect("HQL_SECRET_RAFT not found")`,
  `expect("Cannot parse HQL_NODE_ID to u64")`, and fifteen more of the same
  shape. F-009 records the pattern; this spec records that the whole environment
  constructor is built on it.
- **`NodeConfig::from_toml()` and `from_toml_table()`** (`config_toml.rs:30`,
  `:75`) return `Result<Self, Error>` and produce `Error::Config` for the same
  conditions.

Validation is **not** asymmetric, and an earlier reading of this that said it
was is withdrawn: `NodeConfig::is_valid()` is called by `start.rs:28` for both
paths, and additionally by `from_env_parse` at `config.rs:379`. What differs is
when a bad value is detected and whether the process survives detecting it.

### B-2. Precedence is environment, then TOML, then default

Every typed helper in `config_toml.rs` (`t_bool`, `t_i64`, `t_u32`, `t_u64`,
`t_u16`, `t_str`, `t_str_vec`, and the two `_secret` variants) has the same
shape:

1. `map.remove(key)` takes the TOML value out of the table **first**, always;
2. if the helper was given a non-empty `env_var` and that variable is set, the
   environment value is parsed and returned, and the TOML value is discarded;
3. otherwise the TOML value is returned if present;
4. otherwise the caller applies a literal default.

Step 1 happening before step 2 is what makes the unknown-key check of B-3 work
when an environment override is in play: the key is consumed either way.

A helper called with an **empty** `env_var` has no environment route at all.
Two keys are in that state, `health_check_delay_secs` and
`prepared_statement_cache_capacity`, and one of them is documented as an
environment variable anyway (KD-2).

### B-3. Unknown keys are rejected, with a feature-shaped exception

After parsing, a table with keys left in it is an error:
`Error::Config("Unknown Config data in section: '<table>': ...")`
(`config_toml.rs:458-461`). Seventeen keys are exempt
(`config_toml.rs:431-450`) because they belong to features that may not be
compiled in, so a config written for a fuller build still loads in a narrower
one.

The exemption list is a literal, and a key that is neither parsed nor listed is
rejected even when it is documented. KD-1 is exactly that case.

### B-4. The reference files are the discovery surface, and they are incomplete

`hiqlite.toml` documents the TOML keys; `hiqlite.env` documents a subset of the
environment variables. Reconciled on 2026-09-20:

- **Every key documented in `hiqlite.toml` is parsed, except one.**
  `tls_api_danger_tls_no_verify` (KD-1).
- **`hiqlite.env` documents 15 variables, of which two are read nowhere**:
  `HQL_HEALTH_CHECK_DELAY_SECS` (KD-2) and `HQL_ENC_KEYS_FROM` (KD-3).
- **The environment surface is wider than `hiqlite.env`.** Thirty-three `HQL_*`
  variables are read somewhere in `hiqlite/src` and absent from it, including
  every `HQL_S3_*`, every `HQL_TLS_*`, `HQL_LOG_SYNC`, `HQL_WAL_SIZE` and the
  `HQL_DANGER_RAFT_STATE_RESET` escape hatch. Most are documented as TOML keys
  in `hiqlite.toml` instead, and `HQL_SPLIT_BRAIN_INTERVAL` is in neither
  (F-011).

### B-5. The environment constructor cannot select the WAL durability mode

`from_env_parse` hardcodes `wal_sync: LogSync::ImmediateAsync` and
`wal_size: 2 * 1024 * 1024` (`config.rs:346-347`), as does `Default`
(`config.rs:178-179`). `HQL_LOG_SYNC` and `HQL_WAL_SIZE` are read only by the
TOML constructor (`config_toml.rs:156`, `:164`).

A deployment configured through the environment therefore always runs the
asynchronous durability level, and cannot select the `Immediate` mode that `001`
names for acknowledged writes that must survive a power loss.

**This is recorded as a limit, not as a defect.** No authored text promises that
the environment constructor honors those two settings: `hiqlite.env` does not
list them, and the environment names exist only as overrides inside the TOML
loader. Nothing disagrees with anything, which is the test constitution VI and
the findings register apply. What is true is that the limit has never been
stated anywhere a reader would find it, and this section is where it now is.

### B-6. Encryption keys come from a dependency

`config.rs:356` calls `cryptr::EncKeys::from_env()`. The locked cryptr 0.10.0
reads `ENC_KEYS`, `ENC_KEY_ACTIVE` and `ENC_KEYS_SEALED`, and no `HQL_`-prefixed
variable. The TOML path instead parses `enc_key_active` and `enc_keys` keys, or
accepts a prepared `EncKeys` from the caller.

The consequence for this spec's boundary: part of hiqlite's configuration
contract is defined by a dependency, and this spec describes that fact rather
than claiming cryptr's behavior as its own.

## 4. Evidence and its limits

Four characterization tests were added to `config_toml.rs`. Each asserts current
behavior and would fail if that behavior changed, which is what M2 asks of an
adoption spec. None is a regression test for a repair, because nothing is
repaired here.

| test | what it establishes |
|---|---|
| `documented_tls_api_no_verify_key_is_rejected_as_unknown` | the documented key reaches the unknown-key check and the config is refused (KD-1, first half) |
| `tls_api_no_verify_stays_false_when_the_raft_key_is_set` | the API side is `false` even when the raft key is `true`, because the key is removed on the first read (KD-1, second half) |
| `prepared_statement_cache_capacity_default_differs_from_the_env_path` | 1000 from TOML against 1024 from `Default` (KD-4) |
| `health_check_delay_secs_is_settable_from_toml_only` | the TOML key works and defaults to 30 (the half of KD-2 that functions) |

**What the tests do not establish.**

- **Anything about the environment route.** `from_toml_table` calls
  `dotenvy::dotenv()` and every helper prefers an environment variable over the
  TOML key, so a test that set one would be racing every other test in the
  process. The keys exercised above either have no environment variable at all
  or are asserted through an error path that runs before any override. The
  claims in B-2, B-4 and B-5 about environment behavior are **read from source
  and not executed**.
- **That `hiqlite.env`'s documented values are correct**, only that two of them
  are read nowhere. Each remaining variable's default and validation rule was
  read at its call site, not asserted.
- **Anything at runtime.** No node was started, no cluster was run, and no
  configuration was loaded from a real file in these tests.
- **That the inventory is complete.** It was produced by grepping `env::var`
  across `hiqlite/src` and reconciling the two reference files against the
  parser. A variable read through a path that grep does not match, as
  `ENC_KEYS` is through cryptr, is found only by reading.

## 5. Known defects

Recorded as found, none repaired here. Each is also filed in
`standards/spec/findings-register.md` so later work can cite a stable id.

**KD-1. `tls_api_danger_tls_no_verify` is documented, unreachable, and fatal.**
`hiqlite.toml:194` documents it. `config_toml.rs:211-212` reads
`tls_raft_danger_tls_no_verify` a second time into `tls_api_danger_tls_no_verify`,
and because `t_bool` removes the key on the first read at `:194-195`, the second
read always yields `None` and the API side is always `false`. The documented key
itself is never consumed, so it survives to the unknown-key check of B-3 and the
whole configuration is refused. Observed by execution in both halves.

The effect is **fail-closed**: API certificate verification stays enabled, so
this weakens no security boundary. What it does is make a documented escape
hatch unusable and turn using it into a startup failure. The environment route
is unaffected: `tls.rs:62-69` formats `HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY`
per variant and works for both.

**KD-2. `HQL_HEALTH_CHECK_DELAY_SECS` is documented and read nowhere.**
`hiqlite.env:106` sets it with a documented default of 30.
`config_toml.rs:241-242` passes an empty `env_var` for
`health_check_delay_secs`, and the helpers skip the environment lookup when
`env_var` is empty. Both constructors hardcode 30 (`config.rs:199`, `:365`). The
TOML key works; the documented variable does nothing. `network/api.rs:65` names
the variable in a log line, which makes it look supported.

**KD-3. `HQL_ENC_KEYS_FROM` is documented and read nowhere.**
`hiqlite.env:118` documents it with `env` and `file:path/to/file` as the two
values. No code reads it: checked across `*.rs`, `*.toml` and `*.env`, where
only `hiqlite.env` and `CHANGELOG.md` mention it, and the locked cryptr 0.10.0
does not read it either (B-6). The `file:` alternative the comment offers does
not exist on the environment path. The TOML path has a separate and working
`secrets_file` / `HQL_SECRETS_FILE` mechanism, which is not the same thing.

**KD-4. One setting has two defaults and two documented defaults.**
`prepared_statement_cache_capacity` is 1000 in the TOML path
(`config_toml.rs:150-151`, and `hiqlite.toml:69` documents 1000) and 1024 in
`Default` and the environment path (`config.rs:176`, `:340`, and the doc comment
at `config.rs:58-60` says 1024). Two authored texts disagree, which is a
contradiction rather than a code mismatch. Observed by execution. The setting
also has no environment variable in either path.

**KD-5. A parse-error message names the wrong type.**
`config.rs:335-339` parses `log_statements`, a `bool`, with
`expect("Cannot parse HQL_LOG_STATEMENTS as u64")`. Cosmetic, operator-visible,
and left as found.

## 6. The inventory declarations this spec requires

The three established units are not in any content hash at the pinned revision.
Probed on 2026-09-20 in a throwaway worktree: a claim on them resolves and
`spec-spine check --fail-on-unresolved --fail-on-warn` exits `0`, but
`spec-spine lint --fail-on-warn` exits `1` with three `L-008` warnings, because
a file in no content hash can change without staling any shard. `just
spine-check` runs that lint, so the claim cannot stand without the declarations.

`spec-spine.toml` therefore gains:

- `[index] extra_hashed_inputs`: `hiqlite/src/config_toml.rs`, `hiqlite.toml`,
  `hiqlite.env`, so a byte change in any of them stales the committed index;
- `[coverage] governed_scope`: `hiqlite.toml`, `hiqlite.env`, so the two
  reference files enter the denominator. `config_toml.rs` needs no entry,
  because the `hiqlite` cargo package walk already counts it.

This is the pattern `000` section 12.1 documents and that the dashboard and
example declarations already use. **It is a freshness and denominator
declaration, not an enforcement setting**: `coupling.require_ownership`,
`coupling.bypass_prefixes` and `index coverage --fail-on-untraced` are
untouched, and the enforcement ladder is unchanged (constitution XII).

## 7. Out of scope

- **Every repair.** KD-1 through KD-5 are recorded and left. Each is a separate
  governed change with its own spec and its own evidence, and KD-1 in particular
  changes what a documented key does, which is a contract decision.
- **F-009's panic policy.** Whether a malformed configuration should abort or
  return an error is the owner decision W-22 carries. This spec describes the
  current behavior and proposes nothing.
- **The other 33 environment reads.** Named in B-4, claimed by the specs that
  will own `backup.rs`, `s3.rs`, `tls.rs`, `init.rs`, `split_brain_check.rs`,
  the server and the dashboard.
- **`HQL_SPLIT_BRAIN_INTERVAL`'s absence from both reference files** (F-011).
  Documenting it is a change to a file this spec now owns, but it is an addition
  to the contract rather than a description of it, and it belongs with the
  lifecycle adoption that owns the checker.
- **Ratification of anything, and any enforcement change.**

## 8. Resolved decisions

**D-1 (2026-09-20, `extends` on `spec-spine.toml` rather than editing `000`).**
Section 6's declarations need an authoring edit to an owning spec or an
`extends` edge from this one. `AGENTS.md` names `extends` as the route for
touching a unit another spec owns, and it keeps `000` unedited, which matters
while `000` is a ratification candidate. The alternative, recording the change
as a `000` decision the way PR #5 did, is equally governed; it was not chosen
because the declarations exist to serve this spec's claims and belong with them.

**D-2 (2026-09-20, B-5 is a limit, not a defect).** The environment constructor
ignoring `HQL_LOG_SYNC` looks like a defect and is not one under the register's
class test: a defect is a mismatch between what the code states or promises and
what it does, and nothing authored promises that route. Recording it as a defect
would be the error F-007 and F-008 were reclassified to correct. It is recorded
as a limit whose consequence, no `Immediate` durability from the environment,
had never been written down.

**D-3 (2026-09-20, the tests do not touch the environment).** Asserting B-2's
precedence directly would mean setting a process-wide variable inside a test
binary that runs its tests in parallel, and edition 2024 makes that `unsafe` for
good reason. The precedence claim is therefore source-read evidence, stated as
such in section 4, rather than a test that would be flaky or would force the
whole module to run serially.

## Verification

Run with `just spine-verify 009`.

```verify:cli
test -f hiqlite/src/config_toml.rs
test -f hiqlite.toml
test -f hiqlite.env
sh -c 'spec-spine index owner hiqlite/src/config_toml.rs | grep -q 009-configuration-contract'
sh -c 'spec-spine index owner hiqlite.toml | grep -q 009-configuration-contract'
sh -c 'spec-spine index owner hiqlite.env | grep -q 009-configuration-contract'
sh -c 'spec-spine registry relationships 009-configuration-contract | grep -q 001-wal-durability-and-completion'
cargo test -p hiqlite --lib config_toml::tests::documented_tls_api_no_verify_key_is_rejected_as_unknown -- --exact
cargo test -p hiqlite --lib config_toml::tests::tls_api_no_verify_stays_false_when_the_raft_key_is_set -- --exact
cargo test -p hiqlite --lib config_toml::tests::prepared_statement_cache_capacity_default_differs_from_the_env_path -- --exact
cargo test -p hiqlite --lib config_toml::tests::health_check_delay_secs_is_settable_from_toml_only -- --exact
grep -q 'tls_api_danger_tls_no_verify' hiqlite.toml
sh -c '! grep -q "\"tls_api_danger_tls_no_verify\"" hiqlite/src/config_toml.rs'
grep -q 'HQL_HEALTH_CHECK_DELAY_SECS' hiqlite.env
sh -c '! grep -rq "env::var(\"HQL_HEALTH_CHECK_DELAY_SECS\")" hiqlite/src'
grep -q '"health_check_delay_secs", ""' hiqlite/src/config_toml.rs
grep -q 'HQL_ENC_KEYS_FROM' hiqlite.env
sh -c '! grep -rq "HQL_ENC_KEYS_FROM" hiqlite/src'
grep -q 'hiqlite/src/config_toml.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/009-configuration-contract'
```
