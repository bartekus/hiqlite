---
id: "044-port-upstream-015-fixes"
title: "Port the upstream 0.15.0 fixes the fork lacked, and accept upstream's health-check delay names"
status: draft
created: "2026-09-27"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "005-adoption-assessment-and-plan"
  - "009-configuration-contract"
  - "010-node-lifecycle-and-split-brain"
  - "015-server-binary-and-proxy"
  - "016-derive-macros"
  - "018-dashboard-service-and-ui"
  - "027-node-lifecycle-and-startup-errors"
  - "029-consumer-surface-repairs"
  - "030-transport-security-and-dashboard-repairs"
  - "032-listen-notify-subscription-readiness"
  - "035-n1-upgrade-exclusion"
  - "041-wal-creation-durability-release"
origin:
  retroactive: false
# The behavior these specs recorded as known defects is repaired here; their files are not
# edited, with the one exception D-3 records (`015`'s `establishes` list).
amends:
  - "009-configuration-contract"
  - "010-node-lifecycle-and-split-brain"
  - "015-server-binary-and-proxy"
  - "016-derive-macros"
  - "018-dashboard-service-and-ui"
  - "029-consumer-surface-repairs"
  - "030-transport-security-and-dashboard-repairs"
  - "035-n1-upgrade-exclusion"
# D-2: this spec's `## Verification` block is the acceptance for `029`, `030` and `035` (and
# through them for `005`, `009`, `010`, `011`, `014`, `015`, `016`, `018` and `027`). Their
# blocks pinned behavior this spec repairs; each replaced command is marked `044:` below.
amends_verification:
  - "029-consumer-surface-repairs"
  - "030-transport-security-and-dashboard-repairs"
  - "035-n1-upgrade-exclusion"
extends:
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/start.rs" }
    nature: superseding
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/init.rs" }
    nature: superseding
  - spec: "001-wal-durability-and-completion"
    unit: { kind: file, path: "hiqlite/src/config.rs" }
    nature: additive
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite/src/config_toml.rs" }
    nature: superseding
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite.env" }
    nature: superseding
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite.toml" }
    nature: superseding
  - spec: "015-server-binary-and-proxy"
    unit: { kind: directory, path: "hiqlite/src/server/" }
    nature: superseding
  - spec: "016-derive-macros"
    unit: { kind: file, path: "hiqlite-derive/src/from_row.rs" }
    nature: superseding
  - spec: "018-dashboard-service-and-ui"
    unit: { kind: directory, path: "hiqlite/src/dashboard/" }
    nature: superseding
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/network/" }
    nature: additive
  - spec: "006-cache-state-machine"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/memory/" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
summary: >
  Ports, by hand, the fixes upstream hiqlite 0.15.0 made for ten findings
  this fork still carried open: an IPv6 advertised address, `node_id` read as a
  position, the proxy's unloadable default file and two of its panics, a dead
  module, a panicking derive, the dashboard's global login cooldown that let an
  unauthenticated client lock the operator out, a notify channel that could
  park the handler thread, and `HQL_LOG_SYNC` on the environment route. Accepts
  upstream's `HQL_HEALTH_CHECK_DELAY` / `health_check_delay` names and duration
  syntax, keeping the old `_SECS` spellings as deprecated aliases and the public
  Rust field unchanged. Must not merge before `v0.15.0-patched.4` is tagged.
---

# 044: Port the upstream 0.15.0 fixes the fork lacked

## 1. Purpose

Upstream hiqlite 0.15.0 (`e0a6a8e9`) fixed, among much else, ten findings this
fork's register still carried open or had repaired only in part. This spec ports
those fixes, hunk by hunk, into the fork's own shape: startup failures stay
returned errors (`027`), `listen_notify` readiness stays `032`'s, and F-078 stays
repaired where upstream regressed it.

It also accepts upstream's renamed health-check delay configuration, so a
deployment written for upstream 0.15.0 configures this fork the same way, without
breaking a consumer's Rust code.

**Merge order.** This spec must not be merged into `spec-spine` before
`v0.15.0-patched.4` is tagged. `041` B-2 fixes that release's contents as
"everything merged to `spec-spine` after `6c8db22`"; a merge before the tag would
put these behavior changes into a release whose contract does not list them.

## 2. Territory

Additive or superseding `extends` on the units the repairs touch: `start.rs` and
`init.rs` (`010`), `config.rs` (`001`), `config_toml.rs`, `hiqlite.env` and
`hiqlite.toml` (`009`), the server tree (`015`), `from_row.rs` (`016`), the
dashboard tree (`018`), the network tree (`003`), the cache state machine tree
(`006`) and the findings register (`005`). `CHANGELOG.md` is unclaimed and is
updated. `specs/015-server-binary-and-proxy/spec.md` loses one `establishes`
entry, for the file this spec deletes (D-3).

**Ownership boundary.** Nothing here touches consensus, which is OpenRaft's, or a
caller's state machine. The node-identity repair (B-2) changes how hiqlite picks
its own addresses from its configuration; which nodes form the cluster is
unchanged.

**Upstream source.** Each item names the upstream file it follows; line numbers
are at `e0a6a8e9`. Upstream's change is one squashed commit (#368), so nothing
was cherry-picked: each hunk was read and re-applied by hand.

## 3. Behavior

### B-1. A bracketed IPv6 advertised address yields a parsable listen address (F-037)

`build_listen_addr` split the advertised address at the first `:`, so
`[fd00::1]:8100` gave the "port" `:1]:8100`. The port is now what follows the
last `:` outside the brackets (upstream `hiqlite/src/start.rs:395`,
`rsplit_once(':')`). Two things upstream does not do are added because the same
function is otherwise still wrong for IPv6: a bracketed address with no port
takes the scheme default (`rsplit_once` would take `1]` as the port), and a bare
IPv6 listen host such as `::` is bracketed (`:::8100` is not a socket address).

### B-2. `node_id` is an id everywhere (F-038)

`start.rs` took this node's addresses from `nodes[node_id - 1]`, a position;
`init.rs` registered the node whose `id` is `node_id`. Following upstream
(`start.rs:79-93`, `init.rs:148`, `config.rs:470`):

- `start.rs` finds the node by id. It does so right after `is_valid` and before
  either raft group starts, so it never needs a partial-start teardown, and an
  absent id is a returned `Error::Config`, not an `expect`.
- `init::get_this_node` returns `Result<Node, Error>`; its three callers
  propagate the error.
- `NodeConfig::is_valid` checks that `node_id` is present in `nodes`. It used to
  bound `node_id` by the list's length, which accepted an id nothing carries
  (`node_id = 1`, ids `2,3`) and refused one that is present (`node_id = 4`, ids
  `2,4`).

### B-3. The proxy finds its documented default file (F-070)

`hiqlite proxy --help` documents `$HOME/.hiqlite/hiqlite.env` as the default
`--config-file`, and `clap` does not expand it. The proxy now resolves that
literal against the real home directory, as `serve` already did for its own
default. Upstream fixed this inside a redesign that moved the proxy to a TOML
file (`server/mod.rs:31`, `server/args.rs:42`); that redesign is not ported
(section 6), so the proxy keeps reading an env file at the documented path.

### B-4. `start_proxy` returns the errors upstream returns (F-072, in part)

Following upstream `server/proxy/mod.rs`: the rustls crypto provider
installation is no longer `expect`ed (it fails only when a provider is already
installed, which is the wanted state), and the listen address parse returns
`Error::Config`. The two `unwrap`s on the serve futures remain, because upstream
0.15.0 keeps them too (KD-3).

### B-5. The dead `server/cache.rs` module is removed (F-075)

The file, all comments, and its `mod cache;` line are deleted, as upstream did.

### B-6. `#[derive(FromRow)]` on an enum or a union is a diagnostic (F-080)

The two `unimplemented!()` arms emit `syn::Error::to_compile_error` naming the
supported shape, as upstream `hiqlite-derive` does, so the compiler points at the
item instead of reporting that a proc macro panicked. Upstream's
`is_field_ty_opt` is **not** ported (KD-2).

### B-7. No global login cooldown; one hashing permit and a per-failure delay (F-087, F-095, F-084)

The dashboard answered every login `429` for five seconds after any failed one,
with no client identity involved, so one unauthenticated request every five
seconds kept the operator out indefinitely (F-087). Its three tests raced over the
process-global it used (F-095). Both are removed.

The rate limit is upstream's (`dashboard/password.rs`): a one-permit semaphore
around the argon2 hash, acquired with a five-second timeout; and a failed attempt
is answered only after a three-second delay that starts **after** the permit is
released, so it costs the failing caller its own time and nobody else's. This
replaces the fork's `RwLock` guard (`030`'s repair of F-084) rather than sitting
beside it: one mechanism, and F-084 stays repaired because the permit is a named
binding held across the hash. A permit timeout is `Error::RateLimit` (D-4).

### B-8. A remote listener cannot park the notify handler (F-117)

`api::listen` created each remote subscriber's channel with capacity one, and the
notify handler forwards with flume's blocking `send`, so a subscriber that had not
drained its first event parked the OS thread the handler runs on. The channel is
unbounded, as upstream has it (`network/api.rs:368`), and the handler
`debug_assert`s that every listener is (`notify_handler.rs:52-53`). `032`'s
acknowledgement is unchanged: the handler still pushes, then acknowledges.

### B-9. The environment constructor reads `HQL_LOG_SYNC` and `HQL_WAL_SIZE` (F-035)

`NodeConfig::from_env` hardcoded `wal_sync: ImmediateAsync` and `wal_size:
2 MiB`. It reads `HQL_LOG_SYNC` as upstream does (`config.rs:372-381`), with the
fork's policy for a bad value instead of upstream's `expect`: a value that does
not parse is logged and leaves `IntervalMillis(0)`, which `is_valid` refuses
naming the variable, the same sentinel shape `029` gave `HQL_WAL_SIZE`.
`is_valid` refuses `IntervalMillis(0)` on every route, because a zero-period
ticker panics the WAL's sync task; `log_sync = "interval_0"` in TOML reached that
panic before.

Porting this found that `029` KD-4 overstated its own repair: `Default` reads
`HQL_WAL_SIZE`, but `from_env` built its own value and never did. It does now,
through the same function `Default` uses.

### B-10. `HQL_HEALTH_CHECK_DELAY` and `health_check_delay` are accepted

Upstream renamed `HQL_HEALTH_CHECK_DELAY_SECS` to `HQL_HEALTH_CHECK_DELAY` and the
TOML key `health_check_delay_secs` to `health_check_delay`, as a duration: an
integer is seconds, or a whole number with one unit of `s`, `m`, `h`, `d`, `w`,
`y` (`120s`, `2m`). In this fork:

- the upstream names are the preferred spelling, with upstream's value syntax, on
  both the environment and the TOML route;
- the old names keep working, with their old syntax (an integer number of
  seconds), and log a deprecation warning naming the replacement;
- when both spellings are set, the new one wins and a warning says so (D-1 for
  the order across the environment and the file);
- the value is stored in the unchanged `NodeConfig::health_check_delay_secs: u32`
  (KD-1); a duration longer than `u32::MAX` seconds is a configuration error.

The duration parser takes the unit as the last `char`, where upstream splits at
`len - 1` bytes and panics on a multi-byte last character. `hiqlite.env`,
`hiqlite.toml`, the `generate-config` template and `CHANGELOG.md` document the
new names and the deprecated ones.

## 4. Evidence and its limits

Measured on macOS arm64, `cargo +1.95.0`, from this spec's worktree.

- Every behavior above has a lib test named in the acceptance block. Five
  replace tests that pinned the defect (the F-037 one, the three F-038 ones, the
  F-080 one), so each of those fails against the old code by construction.
- **F-117 was observed, not only read.** With `subscribe()` switched back to the
  one-slot channel and the new `debug_assert` removed,
  `a_subscriber_that_never_drains_does_not_stall_the_handler` failed on its second
  event ("the handler stalled before local event 1"); with the change it passes.
  The register had recorded F-117 at `medium`, from source.
- **F-087 is demonstrated at the function, not over HTTP.**
  `a_failed_attempt_is_delayed_without_locking_anyone_else_out` shows a correct
  password verifying while a wrong one serves its delay, on a semaphore of the
  test's own. No test drives `set_session_verify` through the router.
- **B-3 and B-4 are asserted at the resolving function and at source.** Neither
  test starts the proxy, which needs a live upstream client. The F-072 test is a
  source assertion and says so.
- **B-9's environment route is tested through its value parsers**, not by setting
  process variables, for `009` D-3's reason (the lib test binary shares one
  environment).
- **B-2 changes startup.** The `cluster` suite was run over it (section 7 of the
  pull request records the result); ids in that suite are `1..=3` in order, so it
  exercises the unchanged case, not the repaired one.

## 5. Known defects

**KD-1. The `NodeConfig` field is not renamed or retyped.** Upstream's field is
`health_check_delay: Duration`; the fork keeps `health_check_delay_secs: u32`,
because renaming a public field breaks every consumer that sets it (two of this
repository's own integration tests do). Sub-second durations cannot be expressed
and are not accepted by the parser either. The rename is deferred to a release
that is allowed to break the configuration surface.

**KD-2. F-078 is intentionally not ported back to upstream's form.** Upstream
0.15.0 rewrote `from_row.rs`; its `is_field_ty_opt` again takes the non-optional
branch for `core::option::Option<T>`, which is the defect `029` B-6 repaired. The
fork's function is kept, and the rest of upstream's derive rewrite (a new
`TryFromRow`, duplicate-column detection) is not ported.

**KD-3. `start_proxy` still ends the process when serving fails.** The two
`.unwrap()`s on `axum_server::bind_rustls(..).serve(..)` and
`axum_server::bind(..).serve(..)` remain, as they do upstream, so a port already in
use or a TLS failure panics instead of returning to `main`. F-072 is partly
repaired, not closed.

**KD-4. The dashboard login form keeps its `429` handler.**
`dashboard/src/lib/components/Login.svelte` locks the form on a `429` with
`Retry-After`. The server no longer sends one for a failed password; a permit
timeout is still `Error::RateLimit`, which the UI then handles with its five-second
default. The UI source and its built assets are not changed here (`019` owns the
build).

**KD-5. The per-failure delay is not a brute-force bound.** A client can run
attempts concurrently: each waits out its own delay, but only the hash itself is
serialized. That is upstream's stated trade-off for a debugging surface; the
proof of work the dashboard already requires, and not exposing it, remain the
controls.

**KD-6. `015`'s file was edited** to drop the deleted unit (D-3). This is the only
edit to another spec's file in this change.

## 6. Resolved decisions

**D-1 (2026-09-27, precedence across names and routes).** The preferred name wins
wherever it is set: `HQL_HEALTH_CHECK_DELAY`, then `health_check_delay`, then
`HQL_HEALTH_CHECK_DELAY_SECS`, then `health_check_delay_secs`. Within one name the
environment overrides the file, as it does for every other key. The alternative,
environment-over-file first, would let a stale `HQL_HEALTH_CHECK_DELAY_SECS` in an
environment override a `health_check_delay` an operator just wrote into the file,
which contradicts "the new name wins".

**D-2 (2026-09-27, carrying three acceptance blocks).** `029`, `030` and `035` hold
the acceptance that pinned F-037, F-038, F-075, F-080, F-084's guard form and
F-087's cooldown. Whole-block replacement is the mechanism's unit, so this block
carries all three, and every command it changes is marked with `044:` and what it
was. The amended specs' files are not edited.

**D-3 (2026-09-27, removing a unit another spec establishes).** F-075's repair
deletes `hiqlite/src/server/cache.rs`, which `015` lists under `establishes`. A
claimed unit that no longer exists is an unresolved claim (`I-004`), and the
pinned `spec-spine` has no edge by which a later spec retires another spec's
unit. The entry is removed from `015`'s frontmatter and replaced by a comment
naming this spec. The owner may prefer to keep the file and remove only the `mod`
line instead; that would leave `015` untouched.

**D-4 (2026-09-27, the permit-timeout error).** Upstream answers a permit timeout
with `Error::Request`. This fork uses `Error::RateLimit`, which is what the
condition is, and which the dashboard's login form already handles.

**D-5 (2026-09-27, IPv6 beyond upstream).** B-1 also brackets a bare IPv6 listen
host and handles a bracketed address with no port. Porting `rsplit_once` alone
leaves both cases producing an unparsable address, which is F-037 by another
input.

## 7. Out of scope

- **Upstream's proxy TOML redesign** (`hiqlite-proxy.toml`, `generate-proxy-config`,
  per-proxy rate limits and a stream-connection cap). B-3 is the minimal fix.
- **The rest of upstream 0.15.0**: the cache WAL auto-migration, the `/version`
  routes, `backup_keep_for_local`, the `IntervalMillis(200)` default, the
  bincode change, and upstream's listener binding rework (the fork's `027` already
  binds before reporting a start).
- **Ratification, merge, publication and release.** This spec is `draft`.

## Verification

Run with `just spine-verify 044-port-upstream-015-fixes`. **This block is the
acceptance for `029`, `030` and `035` as well as this spec's** (D-2), and through
them for `005`, `009`, `010`, `011`, `014`, `015`, `016`, `018` and `027`. The
abort-profile probe `035` carries is built in release mode.

```verify:cli
# --- 029-consumer-surface-repairs's acceptance (and through it 005, 009, 014, 015 and 016), carried forward ---
# Package names, not library names: the downstream release renamed the three packages
# (`031` B-2), and `-p` takes a package name. `use hiqlite::..` is unaffected.
# --- 005-adoption-assessment-and-plan's acceptance, carried forward, with the defect-pinning commands replaced ---
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
# was `Status, 2026-09-21: proposed, not authorized`, which `020` replaced when it delivered
# the repair that document proposed. F-096's second occurrence.
grep -q 'Status, 2026-09-21: delivered' standards/spec/cache-log-repair-proposal.md
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

# --- 009-configuration-contract's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/config_toml.rs
test -f hiqlite.toml
test -f hiqlite.env
sh -c 'spec-spine index owner hiqlite/src/config_toml.rs | grep -q 009-configuration-contract'
sh -c 'spec-spine index owner hiqlite.toml | grep -q 009-configuration-contract'
sh -c 'spec-spine index owner hiqlite.env | grep -q 009-configuration-contract'
sh -c 'spec-spine registry relationships 009-configuration-contract | grep -q 001-wal-durability-and-completion'
# was documented_tls_api_no_verify_key_is_rejected_as_unknown, which pinned F-031
cargo test -p hiqlite-patched --lib --features sqlite config::tests::a_wal_size_of_zero_is_refused_by_name -- --exact
sh -c '! grep -q "Cannot parse HQL_WAL_SIZE as u32" hiqlite/src/config.rs'
cargo test -p hiqlite-patched --lib --features toml config_toml::tests::the_documented_tls_api_no_verify_key_is_consumed_and_honoured -- --exact
cargo test -p hiqlite-patched --lib --features toml config_toml::tests::tls_api_no_verify_stays_false_when_the_raft_key_is_set -- --exact
# was prepared_statement_cache_capacity_default_differs_from_the_env_path, which pinned F-034
cargo test -p hiqlite-patched --lib --features toml config_toml::tests::the_prepared_statement_cache_default_is_the_same_on_every_route -- --exact
# was health_check_delay_secs_is_settable_from_toml_only, which pinned F-032
cargo test -p hiqlite-patched --lib --features toml config_toml::tests::health_check_delay_secs_is_settable_from_toml_and_from_the_environment -- --exact
grep -q 'tls_api_danger_tls_no_verify' hiqlite.toml
# was an assertion that the parser never reads the key `hiqlite.toml` documents
sh -c 'grep -q "\"tls_api_danger_tls_no_verify\"" hiqlite/src/config_toml.rs'
grep -q 'HQL_HEALTH_CHECK_DELAY_SECS' hiqlite.env
# was an assertion that the documented variable is read nowhere
# `044`: was `env::var("HQL_HEALTH_CHECK_DELAY_SECS")` read literally; it is read through a named
# constant now, as the deprecated alias of `HQL_HEALTH_CHECK_DELAY`
sh -c 'grep -q "HEALTH_CHECK_DELAY_SECS_ENV: &str = \"HQL_HEALTH_CHECK_DELAY_SECS\"" hiqlite/src/config.rs'
sh -c 'grep -q "env::var(HEALTH_CHECK_DELAY_SECS_ENV)" hiqlite/src/config.rs'
sh -c 'grep -q "env::var(HEALTH_CHECK_DELAY_SECS_ENV)" hiqlite/src/config_toml.rs'
# was the empty `env_var` argument that made the variable a documentation-only fiction
grep -q 'HEALTH_CHECK_DELAY_ENV' hiqlite/src/config_toml.rs
grep -q 'HQL_ENC_KEYS_FROM' hiqlite.env
sh -c '! grep -rq "HQL_ENC_KEYS_FROM" hiqlite/src'
grep -q 'hiqlite/src/config_toml.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/009-configuration-contract'

# --- 014-schema-migration-contract's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/migration.rs
test -f hiqlite/tests/cluster/migrations/bad_1/no_leading_index.sql
test -f hiqlite/tests/cluster/migrations/bad_2/2_bad_start_index.sql
test -f hiqlite/tests/cluster/migrations/bad_3/1_filename_ok.sql
test -f hiqlite/tests/cluster/migrations/good/1_init.sql
sh -c 'spec-spine index owner hiqlite/src/migration.rs | grep -q 014-schema-migration-contract'
sh -c 'spec-spine index owner hiqlite/tests/cluster/migrations/good/1_init.sql | grep -q 014-schema-migration-contract'
sh -c 'spec-spine index owner hiqlite/tests/cluster/migration.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine registry relationships 014-schema-migration-contract | grep -q 012-cluster-integration-evidence'
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::a_valid_set_is_ordered_by_index_and_hashed_by_content -- --exact
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::a_syntactically_invalid_migration_still_builds -- --exact
# was a_name_without_a_numeric_index_panics_with_the_other_rules_message, which pinned F-064
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::a_name_without_a_numeric_index_names_that_rule -- --exact
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::an_index_set_that_does_not_start_at_one_is_an_error -- --exact
# was the `expect` whose message fired for the wrong rule
grep -q 'no `_` separating the index from the name' hiqlite/src/migration.rs
grep -q 'must start with an integer index' hiqlite/src/migration.rs
grep -q 'migrations must start at index 1' hiqlite/src/migration.rs
grep -q 'must end with `.sql`' hiqlite/src/migration.rs
grep -q 'migration index has a gap: {} does not follow {}' hiqlite/src/migration.rs
grep -q 'Migration index has a gap between {} and {}' hiqlite/src/store/state_machine/sqlite/writer.rs
sh -c 'grep -q "pub fn build<T: RustEmbed>() -> Vec<Migration>" hiqlite/src/migration.rs'
sh -c '! grep -q "pub use migration::Migrations" hiqlite/src/lib.rs'
grep -q 'pub use migration::AppliedMigration;' hiqlite/src/lib.rs
# was `Migrations::build`, which panics, inside a function that returns `Result`
grep -q 'let mut migrations = Migrations::try_build::<T>()?;' hiqlite/src/client/migrate.rs
sh -c 'grep -q "if applied.hash != migration.hash {" hiqlite/src/store/state_machine/sqlite/writer.rs'
grep -q '46a52cfa9b2532439423fe769a3a75aa17e8690ee98b2c1b7c5c21560702e2aa' hiqlite/src/migration.rs
grep -q '46a52cfa9b2532439423fe769a3a75aa17e8690ee98b2c1b7c5c21560702e2aa' hiqlite/tests/cluster/migration.rs
grep -q 'hiqlite/src/migration.rs' spec-spine.toml
grep -q 'hiqlite/tests/cluster/migrations/\*\*/\*.sql' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/014-schema-migration-contract'

# --- 015-server-binary-and-proxy's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/main.rs
test -f hiqlite/src/server/proxy/mod.rs
sh -c 'spec-spine index owner hiqlite/src/main.rs | grep -q 015-server-binary-and-proxy'
sh -c 'spec-spine index owner hiqlite/src/server/proxy/handlers.rs | grep -q 015-server-binary-and-proxy'
sh -c 'spec-spine index owner hiqlite/src/server/config.rs | grep -q 015-server-binary-and-proxy'
sh -c 'spec-spine index owner hiqlite/src/server/proxy/stream.rs | grep -q 003-client-consistency-and-retry-outcomes'
sh -c 'spec-spine registry relationships 015-server-binary-and-proxy | grep -q 011-transport-security-material'
# was the_proxy_metrics_route_is_rejected_by_the_pinned_axum, which pinned F-067 by asserting
# that the literal the proxy used panics. It does; the proxy no longer uses it.
cargo test -p hiqlite-patched --features server --lib server::proxy::tests::the_proxy_route_table_can_be_constructed -- --exact
cargo test -p hiqlite-patched --features server --lib server::proxy::tests::the_zero_seven_spelling_is_still_rejected -- --exact
cargo test -p hiqlite-patched --features server --lib server::proxy::tests::the_same_capture_in_zero_eight_syntax_is_accepted -- --exact
cargo test -p hiqlite-patched --features server --lib server::config::tests::the_generated_config_omits_keys_the_reference_file_documents -- --exact
# was proxy_validation_covers_two_fields_and_names_a_third, which pinned F-074
cargo test -p hiqlite-patched --features server --lib server::proxy::config::tests::proxy_validation_covers_two_fields_and_names_the_right_one -- --exact
# was the axum 0.7 spelling that panics at router construction under the pinned axum 0.8
grep -q '.route("/metrics/{raft_type}", get(handlers::metrics)),' hiqlite/src/server/proxy/mod.rs
grep -q '"/metrics/{raft_type}", get(management::metrics)' hiqlite/src/start.rs
grep -q 'axum = { version = "0.8' Cargo.toml
sh -c 'grep -q "constant_time_eq(state.secret_api.as_bytes(), secret.as_bytes())" hiqlite/src/network/mod.rs'
# was the plain `!=` over the secret bytes, which returns as soon as they differ
sh -c '! grep -q "if state.secret_api.as_bytes() != secret.as_bytes() {" hiqlite/src/server/proxy/handlers.rs'
sh -c 'grep -q "constant_time_eq" hiqlite/src/server/proxy/handlers.rs'
grep -q 'static HEADER_NAME_SECRET: &str = "X-API-SECRET";' hiqlite/src/server/proxy/handlers.rs
grep -q 'HandshakeSecret::server(&mut ws, state.secret_api.as_bytes())' hiqlite/src/server/proxy/stream.rs
sh -c 'grep -q "serde(rename_all = \"lowercase\")" hiqlite/src/app_state.rs'
grep -q 'RaftType::Unknown => panic!("neither `sqlite` nor `cache` feature enabled"),' hiqlite/src/server/proxy/handlers.rs
sh -c 'test "$(grep -c "RaftType::Unknown => panic!" hiqlite/src/helpers.rs)" -ge 6'
sh -c 'grep -q "if args.config_file == \"\$HOME/.hiqlite/hiqlite.toml\"" hiqlite/src/server/config.rs'
sh -c 'grep -q "default_value = \"\$HOME/.hiqlite/hiqlite.env\"" hiqlite/src/server/args.rs'
sh -c '! grep -q "HOME" hiqlite/src/server/proxy/config.rs'
# was a message naming a secret the proxy has no concept of
grep -q "'secret_api' should be at least 16 characters long" hiqlite/src/server/proxy/config.rs
sh -c '! grep -q "secret_raft:" hiqlite/src/server/proxy/config.rs'
grep -q 'let addr_str = format!("0.0.0.0:{}", config.listen_port);' hiqlite/src/server/proxy/mod.rs
grep -q 'with_env_filter(level.as_str())' hiqlite/src/server/logging.rs
sh -c '! grep -q "RUST_LOG" hiqlite/src/server/logging.rs'
# `044`: was an assertion that `server/cache.rs` holds only comments and that `mod cache;`
# declares it (F-075); both are removed
test ! -e hiqlite/src/server/cache.rs
sh -c '! grep -q "mod cache;" hiqlite/src/server/mod.rs'
grep -q 'hiqlite/src/main.rs' spec-spine.toml
grep -q 'hiqlite/src/server/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/015-server-binary-and-proxy'

# --- 016-derive-macros's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite-derive/src/lib.rs
test -f hiqlite-derive/src/from_row.rs
test -f hiqlite-derive/src/into_cache_data.rs
sh -c 'spec-spine index owner hiqlite-derive/src/from_row.rs | grep -q 016-derive-macros'
sh -c 'spec-spine index owner hiqlite-derive/src/into_cache_data.rs | grep -q 016-derive-macros'
sh -c 'spec-spine registry relationships 016-derive-macros | grep -q 006-cache-state-machine'
# was the_core_spelling_of_option_is_not_recognised, which pinned F-078
cargo test -p hiqlite-derive-patched --lib from_row::tests::both_spellings_of_option_take_the_optional_branch -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::the_fallible_attributes_expand_to_unwrap_or_expect -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::rename_combines_with_a_conversion_in_either_order -- --exact
# `044`: was an_enum_input_panics_instead_of_emitting_a_diagnostic, which pinned F-080
cargo test -p hiqlite-derive-patched --lib from_row::tests::a_non_struct_input_emits_a_diagnostic_instead_of_panicking -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::from_i32_uses_try_from_and_never_clamps -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::basic_mapping_uses_row_get_by_column_name -- --exact
grep -q 'proc_macro_derive(FromRow, attributes(column))' hiqlite-derive/src/lib.rs
grep -q 'proc_macro_derive(CacheVariants)' hiqlite-derive/src/lib.rs
sh -c 'grep -q "impl #impl_generics ::std::convert::From<&mut ::hiqlite::Row" hiqlite-derive/src/from_row.rs'
# was the generics emitted after `for`, which is not where they go
sh -c 'grep -q "impl #impl_generics ::hiqlite::CacheVariants for #name #ty_generics #where_clause" hiqlite-derive/src/into_cache_data.rs'
sh -c 'grep -q "index_matches.push(quote! {Self::#id => #idx,});" hiqlite-derive/src/into_cache_data.rs'
# was `unimplemented!()` for a non-enum input, which is a panic with no diagnostic
sh -c 'grep -q "can only be applied to an enum" hiqlite-derive/src/into_cache_data.rs'
# `044`: was `Data::Enum(_) => unimplemented!(),`, the panic F-080 recorded
sh -c '! grep -q "unimplemented!()" hiqlite-derive/src/from_row.rs'
grep -q 'Data::Enum(_) | Data::Union(_) => {' hiqlite-derive/src/from_row.rs
grep -q '.to_compile_error();' hiqlite-derive/src/from_row.rs
sh -c 'grep -q "if s == \"std\" || s == \"core\" {" hiqlite-derive/src/from_row.rs'
sh -c 'grep -q "s == \"core\"" hiqlite-derive/src/from_row.rs'
grep -q 'column value does not fit into i32' hiqlite-derive/src/from_row.rs
sh -c 'grep -q "TryFrom::try_from(&mut \*row).unwrap()" hiqlite-derive/src/from_row.rs'
grep -q 'use ::std::str::FromStr;' hiqlite-derive/src/from_row.rs
grep -q 'ColumnAttr::Skip => quote! {#id: ::std::default::Default::default(),},' hiqlite-derive/src/from_row.rs
grep -q 'hiqlite-derive/src/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/016-derive-macros'

# --- what this repair adds ---
# an oversized entry is a rejected append, not a dead writer
cargo test -p hiqlite-wal-patched --lib writer::tests::oversized_entry_errors_without_killing_writer -- --exact
cargo test -p hiqlite-wal-patched --lib log_store_impl::tests::append_adapter_reports_a_rejected_append_as_an_error -- --exact
sh -c '! grep -q "`data` length must not exceed `wal_size` -> data length" hiqlite-wal/src/writer.rs || ! grep -q "panic!" hiqlite-wal/src/writer.rs'
sh -c 'grep -q "No-op, kept so a consumer that enables it still builds" hiqlite-wal/Cargo.toml'
# and the ceiling it implies is selectable from the environment
sh -c 'grep -q "HQL_WAL_SIZE" hiqlite/src/config.rs'
sh -c 'grep -q "HQL_WAL_SIZE" hiqlite.env'
# an unknown raft type is a bad request, not a panic about a build configuration
sh -c 'grep -q "pub fn selected" hiqlite/src/app_state.rs'
sh -c 'test "$(grep -c "raft_type.selected()?;" hiqlite/src/network/management.rs)" -eq 6'
sh -c 'grep -q "raft_type.selected()?;" hiqlite/src/network/api.rs'
# the migration rules are named errors, and the duplicate case says so
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::a_duplicate_index_says_so -- --exact
cargo test -p hiqlite-patched --lib --features sqlite migration::tests::the_panicking_wrapper_still_panics_for_source_compatibility -- --exact
test -f hiqlite/tests/cluster/migrations/duplicate/1_first.sql
test -f hiqlite/tests/cluster/migrations/duplicate/1_second.sql
sh -c 'grep -q "pub fn try_build<T: RustEmbed>() -> Result<Vec<Migration>, crate::Error>" hiqlite/src/migration.rs'
# the derive accepts both spellings of Option and both shapes of generics
cargo test -p hiqlite-derive-patched --lib from_row::tests::both_spellings_of_option_take_the_optional_branch -- --exact
sh -c 'grep -q "needs unit variants" hiqlite-derive/src/into_cache_data.rs'
# the public health waits can be bounded
sh -c 'grep -q "pub async fn wait_until_healthy_db_timeout" hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "pub async fn wait_until_healthy_cache_timeout" hiqlite/src/client/mgmt.rs'
# the cache TTL handler does not panic its own task
sh -c '! grep -q "expect(\"kv handler to always be running\")" hiqlite/src/store/state_machine/memory/cache_ttl_handler.rs'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/029-consumer-surface-repairs'

# --- 030-transport-security-and-dashboard-repairs's acceptance (and through it 011 and 018), carried forward ---
# Package names, not library names: the downstream release renamed the three packages
# (`031` B-2), and `-p` takes a package name. `use hiqlite::..` is unaffected.
# --- 011's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/tls.rs
test -f hiqlite/src/http_client.rs
test -f hiqlite/tests/tls_env.rs
sh -c 'spec-spine index owner hiqlite/src/tls.rs | grep -q 011-transport-security-material'
sh -c 'spec-spine index owner hiqlite/src/http_client.rs | grep -q 011-transport-security-material'
sh -c 'spec-spine index owner hiqlite/tests/tls_env.rs | grep -q 011-transport-security-material'
sh -c 'spec-spine registry relationships 011-transport-security-material | grep -q 010-node-lifecycle-and-split-brain'
cargo test -p hiqlite-patched --lib --features sqlite tls::tests::auto_certificates_always_disable_verification -- --exact
# was the_tls_material_type_has_no_field_for_a_trust_anchor, which pinned F-043
cargo test -p hiqlite-patched --lib --features sqlite tls::tests::the_tls_material_type_carries_a_trust_anchor -- --exact
cargo test -p hiqlite-patched --lib --features sqlite tls::tests::the_trust_anchor_reader_accepts_a_chain_and_rejects_anything_else -- --exact
cargo test -p hiqlite-patched --test tls_env --features sqlite -- --test-threads=1
grep -q 'ServerTlsConfig::TlsAutoCertificates => true' hiqlite/src/tls.rs
grep -q 'danger_tls_no_verify: false' hiqlite/src/tls.rs
# was the `is_some() && is_some()` that fell through to `None` for half a pair (F-041)
sh -c '! grep -q "^        if key.is_some() && cert.is_some()" hiqlite/src/tls.rs'
sh -c 'grep -q "TLS needs both" hiqlite/src/tls.rs'
# was `parse::<bool>().unwrap_or(false)`, so a typo meant "off" while the override four
# lines below ended the process for the same typo (F-042)
sh -c '! grep -q "parse::<bool>().unwrap_or(false)" hiqlite/src/tls.rs'
sh -c 'grep -q "must be .true. or .false." hiqlite/src/tls.rs'
# was the `expect` message for the per-variant override
sh -c '! grep -q "DANGER_TLS_NO_VERIFY to bool" hiqlite/src/tls.rs'
# the store still starts empty; what changed is that it can be filled (F-043)
grep -q 'RootCertStore::empty()' hiqlite/src/tls.rs
sh -c 'grep -q "fn load_pem_certs" hiqlite/src/tls.rs'
sh -c 'grep -q "pub ca: Option<Cow" hiqlite/src/tls.rs'
grep -q 'cfg(feature = "webpki-roots")' hiqlite/src/tls.rs
grep -q 'cfg(feature = "webpki-roots")' hiqlite/src/http_client.rs
grep -q 'tls_danger_accept_invalid_certs(tls_no_verify)' hiqlite/src/http_client.rs
sh -c 'grep -q "api_trust_anchor()" hiqlite/src/http_client.rs'
sh -c '! sed -n "s/^default = //p" hiqlite/Cargo.toml | grep -q "webpki-roots"'
grep -q 'rustls-no-provider' Cargo.toml
# was the API endpoint's no-verify flag read from the **raft** configuration (F-044)
sh -c '! grep -q "let tls_no_verify = node_config" hiqlite/src/start.rs'
sh -c 'grep -A1 "let tls_api_no_verify = node_config" hiqlite/src/start.rs | grep -q "\.tls_api"'
# was `reqwest::Client::new()`, which honours no override at all (F-044)
sh -c '! grep -q "let client = reqwest::Client::new();" hiqlite/src/split_brain_check.rs'
sh -c 'grep -q "build_http_client(crate::tls::api_no_verify())" hiqlite/src/split_brain_check.rs'
grep -q 'let client = build_http_client(tls_no_verify);' hiqlite/src/init.rs
grep -q 'let scheme = if tls { "https" } else { "http" };' hiqlite/src/init.rs
grep -q 'node_config.tls_api.is_some(),' hiqlite/src/store/mod.rs
grep -q 'fn validate_secret' hiqlite/src/network/mod.rs
grep -q 'HEADER_NAME_SECRET' hiqlite/src/network/mod.rs
grep -q 'HandshakeSecret::server' hiqlite/src/network/api.rs
# the claim is kept and narrowed to the channel it is true of (F-045)
grep -q 'without the secret ever being sent over' hiqlite/src/tls.rs
sh -c 'grep -q "does not cover the REST surface" hiqlite/src/tls.rs'
grep -q 'without the secret ever being sent over the network' hiqlite.toml
sh -c 'grep -q "It is NOT fine for the API endpoint" hiqlite.toml'
grep -q 'HQL_TLS_AUTO_CERTS' hiqlite.env
sh -c '! grep -q "3-way handshake" hiqlite.env'
sh -c 'grep -q "no handshake" hiqlite.env'
sh -c 'grep -q "HQL_TLS_API_CA" hiqlite.env'
sh -c 'grep -q "tls_api_ca" hiqlite.toml'
grep -q 'forbid(unsafe_code)' hiqlite/src/lib.rs
grep -q 'hiqlite/src/tls.rs' spec-spine.toml
grep -q 'hiqlite/src/http_client.rs' spec-spine.toml
grep -q 'hiqlite/tests/tls_env.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/011-transport-security-material'

# --- 018's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/dashboard/session.rs
test -f dashboard/src/lib/utils/fetch.ts
test -f dashboard/tests/smoke.spec.ts
sh -c 'spec-spine index owner hiqlite/src/dashboard/password.rs | grep -q 018-dashboard-service-and-ui'
sh -c 'spec-spine index owner hiqlite/src/dashboard/static_files.rs | grep -q 018-dashboard-service-and-ui'
sh -c 'spec-spine index owner dashboard/src/lib/utils/fetch.ts | grep -q 018-dashboard-service-and-ui'
sh -c 'spec-spine index owner dashboard/tests/smoke.spec.ts | grep -q 018-dashboard-service-and-ui'
sh -c 'spec-spine registry relationships 018-dashboard-service-and-ui | grep -q 015-server-binary-and-proxy'
# was the_single_flight_lock_is_released_before_any_hashing, which pinned F-084
# `044`: was the_single_flight_lock_is_held_for_the_whole_hashing and
# verify_password_holds_the_lock_while_it_runs, over the `RwLock` that upstream's semaphore replaced
cargo test -p hiqlite-patched --features dashboard --lib dashboard::password::tests::the_hashing_permit_is_held_while_the_hash_runs -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::password::tests::a_failed_attempt_is_delayed_without_locking_anyone_else_out -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::password::tests::the_hasher_is_argon2id_with_recorded_parameters -- --exact
# was a_multibyte_path_panics_the_fallback, which pinned F-085
cargo test -p hiqlite-patched --features dashboard --lib dashboard::static_files::tests::a_multibyte_path_is_served_or_missing_but_never_a_panic -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::static_files::tests::an_already_compressed_type_is_not_encoded_again -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::static_files::tests::a_known_asset_is_served_with_its_cache_headers -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::static_files::tests::an_unknown_asset_is_a_plain_404 -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::query::tests::forbidden_fn_scan_catches_only_real_calls -- --exact
# `044`: was `dashboard::session::tests -- --test-threads=1`, the three cooldown tests that raced
# (F-095); they are removed with the cooldown (F-087)
cargo test -p hiqlite-patched --features dashboard --lib dashboard::password::tests::there_is_no_global_login_cooldown -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::handlers::tests
# was the `let _ =` that dropped the guard at the end of the statement. It survives in the
# test that asserts the difference, and not in the function.
sh -c '! grep -q "^    let _ = IS_HASHING.write().await;" hiqlite/src/dashboard/password.rs'
# `044`: was `let _guard = IS_HASHING.write().await;`; F-084 stays repaired in upstream's form,
# a named permit held across the hash
sh -c 'grep -q "let permit = tokio::time::timeout(PERMIT_TIMEOUT, permits.acquire())" hiqlite/src/dashboard/password.rs'
sh -c 'grep -q "static HASH_PERMITS: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));" hiqlite/src/dashboard/password.rs'
# `044`: was the comment that described the rate limit and the session comment that relied on it
grep -q 'password::verify_password(password, pwd).await?;' hiqlite/src/dashboard/session.rs
# was the byte slice four bytes from the end, which panics off a character boundary (F-085)
sh -c '! grep -q "let path_ending = &path\[path.len().saturating_sub(4)..\];" hiqlite/src/dashboard/static_files.rs'
sh -c 'grep -q "\.extension()" hiqlite/src/dashboard/static_files.rs'
# was the seven-byte prefix: a boundary panic (F-086) and a read classified as a write
# (F-088), in one line
sh -c '! grep -q "let sql_start = sql\[..7\].to_lowercase();" hiqlite/src/dashboard/query.rs'
sh -c 'grep -q "fn first_keyword" hiqlite/src/dashboard/query.rs'
cargo test -p hiqlite-patched --features dashboard --lib dashboard::query::tests::the_first_keyword_is_found_past_whitespace_and_comments -- --exact
sh -c '! grep -q "if sql.len() < 8 {" hiqlite/src/dashboard/query.rs'
# `044`: was the global cooldown and its justification (F-087), both removed
sh -c '! grep -q "NEXT_LOGIN_ALLOWED" hiqlite/src/dashboard/session.rs'
sh -c '! grep -q "There is no per-client state" hiqlite/src/dashboard/session.rs'
# was two `unwrap`s that ended the process for a malformed value, while the absent case
# four lines below merely disabled the dashboard (F-089)
sh -c '! grep -q "String::from_utf8(b64_decode(&b64).unwrap()).unwrap()" hiqlite/src/dashboard/mod.rs'
sh -c 'grep -q "is not valid base64 and the dashboard will be disabled" hiqlite/src/dashboard/mod.rs'
cargo test -p hiqlite-patched --features dashboard --lib dashboard::tests::a_malformed_dashboard_password_is_a_value_not_a_panic -- --exact
grep -q 'HQL_PASSWORD_DASHBOARD has not been set and the dashboard will be disabled' hiqlite/src/dashboard/mod.rs
sh -c 'grep -q "const COOKIE_NAME: &str = \"__Host-Hiqlite-Session\";" hiqlite/src/dashboard/session.rs'
grep -q 'const SESSION_LIFETIME: i64 = 3600;' hiqlite/src/dashboard/session.rs
sh -c '! grep -rq "logout" hiqlite/src/dashboard/'
grep -q 'fn check_csrf' hiqlite/src/dashboard/session.rs
grep -q 'frame-ancestors' hiqlite/src/dashboard/middleware.rs
grep -q 'fallback(dashboard::static_files::handler)' hiqlite/src/start.rs
sh -c 'grep -q "if state.dashboard.password_dashboard.is_some()" hiqlite/src/start.rs'
grep -q '"test:smoke": "playwright test"' dashboard/package.json
sh -c '! grep -rq "test:smoke" justfile .github/workflows/'
grep -q "API_PREFIX = '/dashboard/api'" dashboard/src/lib/utils/fetch.ts
sh -c 'grep -q "if (res.status === 401)" dashboard/src/lib/utils/fetch.ts'
grep -q 'window.isSecureContext' dashboard/src/lib/components/Login.svelte
grep -q 'hiqlite/src/dashboard/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/018-dashboard-service-and-ui'
# --- what this repair adds ---
# the two limits this spec records rather than repairs are still exactly as described
# `044`: was the cooldown's "no per-client state" text; the cooldown is removed (F-087)
sh -c '! grep -q "LOGIN_COOLDOWN" hiqlite/src/dashboard/session.rs'
sh -c 'grep -q "const SESSION_LIFETIME: i64 = 3600;" hiqlite/src/dashboard/session.rs'
sh -c '! grep -rq "logout" hiqlite/src/dashboard/'
# and the generated server config documents the trust anchor alongside the reference file
sh -c 'grep -q "tls_api_ca" hiqlite/src/server/config.rs'
cargo test -p hiqlite-patched --features server --lib server::config::tests::the_generated_config_omits_keys_the_reference_file_documents -- --exact
sh -c '! grep -rl "$(printf "\342\200\224")" specs/030-transport-security-and-dashboard-repairs'

# --- 035-n1-upgrade-exclusion's acceptance (and through it 027 and 010), carried forward ---
# --- 027's acceptance (and through it 010's), carried forward ---
# Package names, not library names: the downstream release renamed the three packages
# (`031` B-2), and `-p` takes a package name. `use hiqlite::..` is unaffected.
# --- 010's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/start.rs
test -f hiqlite/src/init.rs
test -f hiqlite/src/app_state.rs
test -f hiqlite/src/split_brain_check.rs
sh -c 'spec-spine index owner hiqlite/src/start.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine index owner hiqlite/src/init.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine index owner hiqlite/src/app_state.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine index owner hiqlite/src/split_brain_check.rs | grep -q 010-node-lifecycle-and-split-brain'
sh -c 'spec-spine registry relationships 010-node-lifecycle-and-split-brain | grep -q 009-configuration-contract'
cargo test -p hiqlite-patched --lib start::tests::listen_port_comes_from_the_advertised_address -- --exact
cargo test -p hiqlite-patched --lib start::tests::missing_advertised_port_falls_back_to_the_scheme_default -- --exact
# `044`: was ipv6_advertised_address_produces_an_unparsable_listen_address, which pinned F-037
cargo test -p hiqlite-patched --lib start::tests::an_ipv6_advertised_address_yields_a_parsable_listen_address -- --exact
# `044`: were node_identity_is_resolved_by_id_here_and_by_position_in_start,
# is_valid_accepts_a_nodes_list_whose_ids_are_not_positions and
# get_this_node_panics_when_the_id_is_absent, which pinned F-038
cargo test -p hiqlite-patched --lib init::tests::node_identity_is_resolved_by_id_on_both_routes -- --exact
cargo test -p hiqlite-patched --lib init::tests::is_valid_checks_that_the_node_id_exists_rather_than_bounding_it_by_length -- --exact
cargo test -p hiqlite-patched --lib init::tests::get_this_node_returns_a_config_error_when_the_id_is_absent -- --exact
# `044`: was `.get(node_config.node_id as usize - 1)` and `expect("this node to always exist in
# all nodes")`, the two routes F-038 recorded
sh -c '! grep -q "node_id as usize - 1" hiqlite/src/start.rs'
grep -q '.find(|node| node.id == node_config.node_id)' hiqlite/src/start.rs
sh -c '! grep -q "this node to always exist in all nodes" hiqlite/src/init.rs'
grep -q 'let (tx_shutdown, rx_shutdown) = tokio::sync::watch::channel(false);' hiqlite/src/start.rs
# was two `with_graceful_shutdown` and two `axum_server::bind_rustls`, of which only the
# plaintext pair ever received a shutdown future. Both endpoints now go through one server
# helper that takes an already-bound listener and a receiver of its own.
sh -c 'grep -q "async fn serve_router" hiqlite/src/start.rs'
sh -c 'grep -q "axum_server::Handle::new()" hiqlite/src/start.rs'
sh -c 'grep -q "graceful_shutdown(Some(Duration::from_secs(10)))" hiqlite/src/start.rs'
sh -c '! grep -q "axum_server::bind_rustls" hiqlite/src/start.rs'
grep -q 'expect("The global Hiqlite shutdown handler to always listen")' hiqlite/src/client/mgmt.rs
# was `assert!(!handle.is_finished())`, the watchdog F-014 showed is unreachable under abort
# and silent under unwind
sh -c '! grep -q "assert!(!handle.is_finished())" hiqlite/src/split_brain_check.rs'
# was `expect("Cannot parse HQL_SPLIT_BRAIN_INTERVAL as u64")`. The string survives in the doc
# comment that records what it used to be, so this asserts the absence of the call rather than
# of the words.
sh -c '! grep -q "^ *\.expect(\"Cannot parse HQL_SPLIT_BRAIN_INTERVAL as u64\")" hiqlite/src/split_brain_check.rs'
sh -c 'grep -q "fn split_brain_interval_from" hiqlite/src/split_brain_check.rs'
grep -q 'HQL_SPLIT_BRAIN_INTERVAL' hiqlite.env
sh -c '! grep -q "split_brain" hiqlite.toml'
grep -q 'panic = "abort"' Cargo.toml
grep -q 'hiqlite/src/start.rs' spec-spine.toml
grep -q 'hiqlite/src/init.rs' spec-spine.toml
grep -q 'hiqlite/src/app_state.rs' spec-spine.toml
grep -q 'hiqlite/src/split_brain_check.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/010-node-lifecycle-and-split-brain'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/027-node-lifecycle-and-startup-errors'
# --- what this repair adds ---
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache split_brain_check::tests::a_malformed_split_brain_interval_is_a_startup_error -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache start::tests::a_listener_that_cannot_start_is_a_startup_error -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::the_first_failure_is_the_one_that_is_kept -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_failed_node_refuses_with_an_account_of_why -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_writer_thread_that_ends_without_a_reason_still_fails_the_node -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_reported_writer_failure_names_its_cause -- --exact
# F-101: a deliberate shutdown is not a failure, and a real one still survives it
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_deliberate_shutdown_is_not_a_failure -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache lifecycle::tests::a_failure_recorded_before_the_shutdown_survives_it -- --exact
sh -c 'grep -q "state.lifecycle.begin_shutdown();" hiqlite/src/client/mgmt.rs'
sh -c 'grep -A3 "fn fail(&self" hiqlite/src/lifecycle.rs | grep -q "self.shutting_down.load"'
# B-9 / F-107: a node that is not a voter may not commit a membership change
cargo test -p hiqlite-patched --lib --no-default-features --features cache network::management::tests::a_node_leaving_its_own_cluster_may_not_commit_a_membership_change -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache network::management::tests::every_refusal_tells_the_caller_to_try_elsewhere -- --exact
sh -c 'grep -q "fn membership_change_allowed" hiqlite/src/network/management.rs'
sh -c 'grep -q "Some(_) if !this_node_is_voter" hiqlite/src/network/management.rs'
sh -c 'grep -q "membership_change_allowed(" hiqlite/src/network/management.rs'
# D-8: one gate, decided under its lock, and a bounded shutdown drain that stops nothing
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::a_decision_taken_before_the_lock_acts_on_a_membership_it_did_not_see -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::shutdown_waits_for_an_admitted_change_and_admits_nothing_after -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::a_shutdown_that_cannot_drain_stops_nothing_and_can_be_retried -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::a_cancelled_shutdown_wait_holds_nothing_and_keeps_admission_closed -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::admission_waits_for_a_bounded_time_across_both_raft_groups -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache membership_gate::tests::a_refused_decision_holds_nothing -- --exact
sh -c '! grep -rnE "\.raft_lock|raft_lock:" hiqlite/src'
sh -c 'test "$(grep -c "_held: &crate::membership_gate::MembershipHeld" hiqlite/src/helpers.rs)" -eq 4'
sh -c 'grep -q "state.membership.drain(SHUTDOWN_DRAIN)" hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "state.membership.close();" hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "admit_membership_change(&state, &raft_type)" hiqlite/src/network/management.rs'
sh -c 'grep -q "admit_membership_change(" hiqlite/src/network/raft_server.rs'
# review of the candidate: results reach the caller, everything under the gate is bounded
sh -c 'test "$(grep -c "bounded_membership_op(" hiqlite/src/helpers.rs)" -eq 8'
sh -c 'grep -q "Ok(res) => res," hiqlite/src/client/shutdown_handle.rs'
sh -c 'grep -q "Ok(res) => res," hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "REMOTE_LEAVE_BOUND," hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "mark_stopped(&held, first_err.is_none())" hiqlite/src/client/mgmt.rs'
# B-10 / F-111 to F-113: the upgrade guard, the reader that no longer aborts
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::tests::a_legacy_cache_log_is_refused_and_left_untouched -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::tests::the_opt_in_moves_the_legacy_cache_aside_and_marks_the_new_one -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::tests::a_fresh_directory_is_marked_and_a_foreign_marker_is_refused -- --exact
cargo test -p hiqlite-wal-patched --lib reader::tests::a_requester_that_stops_reading_does_not_end_the_reader -- --exact
sh -c '! grep -q "ack.send(.*).unwrap()" hiqlite-wal/src/reader.rs'
# the guard runs before the restore and before either raft group, so a refusal opens nothing
# `035` D-9: was the order of `store::logs::ensure_cache_log_format`; the legacy check now runs
# inside `upgrade_exclusion::acquire_storage`, under the WAL locks, and keeps its place: before
# the restore and before either raft group.
sh -c 'a=$(grep -n "upgrade_exclusion::acquire_storage(" hiqlite/src/start.rs | head -1 | cut -d: -f1); b=$(grep -n "backup::restore_backup_start(&node_config)" hiqlite/src/start.rs | head -1 | cut -d: -f1); c=$(grep -n "store::start_raft_db(" hiqlite/src/start.rs | head -1 | cut -d: -f1); test -n "$a" && test -n "$b" && test -n "$c" && test "$a" -lt "$b" && test "$a" -lt "$c"'
grep -q "fn cache_log_format" hiqlite/src/upgrade_exclusion.rs
sh -c '! grep -q "ensure_cache_log_format" hiqlite/src/store/mod.rs'
# `035` D-9: was two calls of `ensure_cache_log_format` in `start.rs`; the second, after the reset,
# is now the re-lock and re-mark, still after `check_execute_reset` and before the SQLite raft.
sh -c 'r=$(grep -n "init::check_execute_reset(" hiqlite/src/start.rs | head -1 | cut -d: -f1); m=$(grep -n "mark_cache_log_if_unmarked()" hiqlite/src/start.rs | head -1 | cut -d: -f1); c=$(grep -n "store::start_raft_db(" hiqlite/src/start.rs | head -1 | cut -d: -f1); test -n "$r" && test -n "$m" && test -n "$c" && test "$r" -lt "$m" && test "$m" -lt "$c"'
sh -c 'test "$(grep -c "lifecycle.begin_shutdown();" hiqlite/src/store/mod.rs)" -ge 4'
sh -c '! grep -q "holds_files(&dir_snapshots" hiqlite/src/store/logs/mod.rs'
sh -c 'grep -q "if let Err(err) = init::init_pristine_node_1_db(" hiqlite/src/store/mod.rs'
sh -c 'grep -q "if let Err(err) = init::init_pristine_node_1_cache(" hiqlite/src/store/mod.rs'
# F-110: an out-of-service node refuses its embedded client
sh -c 'test "$(grep -c "self.ensure_node_available()?;" hiqlite/src/client/rate_limit.rs)" -eq 2'
sh -c 'test "$(grep -c "self.ensure_node_available()?;" hiqlite/src/client/query.rs)" -eq 7'
# the abort-profile half. The build is what puts it under `panic = "abort"`; these two are a
# pair and running a stale binary would prove nothing.
cargo build --release -p hiqlite-patched --features __abort-probe,s3 --bin hiqlite-abort-probe
./target/release/hiqlite-abort-probe
# the lifecycle is consulted before any raft metric, on both endpoints and in the client
sh -c 'grep -q "state.lifecycle.ensure_available()?" hiqlite/src/network/api.rs'
sh -c 'grep -q "pub fn node_failure" hiqlite/src/client/mgmt.rs'
sh -c 'grep -q "pub fn writer_failure" hiqlite-wal/src/log_store.rs'
sh -c 'grep -q "NodeFailed" hiqlite/src/error.rs'
sh -c 'grep -q "Startup" hiqlite/src/error.rs'
# --- what 035 adds ---
test -f standards/spec/findings-register.md
grep -q '^### F-126 ' standards/spec/findings-register.md
grep -q '^### F-131 ' standards/spec/findings-register.md
grep -q '^### F-133 ' standards/spec/findings-register.md
grep -q 'CACHE_LEGACY_MOVE_ASIDE_ENV' hiqlite/src/store/logs/mod.rs
grep -q '035-n1-upgrade-exclusion' standards/spec/n3-topology-proposal.md
test -f hiqlite/src/upgrade_exclusion.rs
test -f hiqlite/tests/upgrade_exclusion.rs
test -x qualification/n1-upgrade/run.sh
test -f qualification/n1-upgrade/old/Cargo.lock
test -f qualification/n1-upgrade/new/Cargo.lock
cargo test -p hiqlite-wal-patched --lib lockfile::tests
cargo test -p hiqlite-wal-patched --lib writer::tests::a_clean_stop_unlinks_the_lock_file_while_holding_it -- --exact
cargo test -p hiqlite-wal-patched --lib writer::tests::a_caller_held_lock_outlives_the_writer -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,cache,counters,dlock,listen_notify_local,backup,s3 upgrade_exclusion::tests
cargo test -p hiqlite-patched --no-default-features --features sqlite,cache,counters,dlock,listen_notify_local,backup,s3 --test upgrade_exclusion
cargo test -p hiqlite-patched --lib --features cache,cast_ints,counters,dashboard,listen_notify_local,macros upgrade_exclusion::tests
cargo test -p hiqlite-patched --features cache,cast_ints,counters,dashboard,listen_notify_local,macros --test upgrade_exclusion
cargo test -p hiqlite-patched --lib --no-default-features --features cache store::logs::tests
sh -c '! grep -rl --exclude-dir=target "$(printf "\342\200\224")" specs/035-n1-upgrade-exclusion qualification/n1-upgrade hiqlite/src/upgrade_exclusion.rs hiqlite/tests/upgrade_exclusion.rs'

# --- what 044 adds ---
# B-1 / F-037: a bracketed IPv6 advertised address yields a parsable listen address
cargo test -p hiqlite-patched --lib start::tests::an_ipv6_advertised_address_yields_a_parsable_listen_address -- --exact
sh -c 'grep -q "rsplit_once(.:.)" hiqlite/src/start.rs'
# B-2 / F-038: this node is resolved by id everywhere, and an absent id is a configuration error
cargo test -p hiqlite-patched --lib init::tests -- node_identity_is_resolved_by_id_on_both_routes is_valid_checks_that_the_node_id_exists get_this_node_returns_a_config_error
grep -q 'fn get_this_node(this_node: u64, nodes: &\[Node\]) -> Result<Node, Error>' hiqlite/src/init.rs
sh -c '! grep -q "self.node_id as usize > self.nodes.len()" hiqlite/src/config.rs'
# B-3 / F-070: the proxy's documented default file is found
cargo test -p hiqlite-patched --features server --lib server::config::tests::the_proxy_default_config_file_resolves_under_the_home_directory -- --exact
# B-4 / F-072: the errors upstream returns are returned; the unwraps upstream keeps are kept (KD-3)
cargo test -p hiqlite-patched --features server --lib server::proxy::tests::start_proxy_returns_the_errors_upstream_returns_and_keeps_the_unwraps_it_keeps -- --exact
# B-5 / F-075: the dead module is gone, and so is `015`'s claim on it
test ! -e hiqlite/src/server/cache.rs
sh -c '! grep -q "\"hiqlite/src/server/cache.rs\"" specs/015-server-binary-and-proxy/spec.md'
# B-6 / F-080: FromRow on an enum or a union is a diagnostic; F-078's repair is kept (KD-2)
cargo test -p hiqlite-derive-patched --lib from_row::tests::a_non_struct_input_emits_a_diagnostic_instead_of_panicking -- --exact
cargo test -p hiqlite-derive-patched --lib from_row::tests::both_spellings_of_option_take_the_optional_branch -- --exact
# B-7 / F-087, F-095: no global login cooldown; one hashing permit and a per-failure delay
cargo test -p hiqlite-patched --features dashboard --lib dashboard::password::tests
sh -c '! grep -q "TOO_MANY_REQUESTS" hiqlite/src/dashboard/session.rs'
# B-8 / F-117: listener channels are unbounded and the handler asserts it
cargo test -p hiqlite-patched --lib --no-default-features --features cache,listen_notify_local store::state_machine::memory::notify_handler::tests
sh -c 'grep -q "let (tx, rx) = flume::unbounded();" hiqlite/src/network/api.rs'
grep -q 'debug_assert!(tx.capacity().is_none());' hiqlite/src/store/state_machine/memory/notify_handler.rs
# B-9 / F-035: the environment constructor reads HQL_LOG_SYNC and HQL_WAL_SIZE
cargo test -p hiqlite-patched --lib --features toml config::tests::the_env_route_reads_log_sync_and_refuses_a_malformed_value_by_name -- --exact
grep -q 'HQL_LOG_SYNC' hiqlite.env
# B-10: HQL_HEALTH_CHECK_DELAY / health_check_delay, with the _secs spellings as deprecated aliases
cargo test -p hiqlite-patched --lib --features toml config::tests::the_health_check_delay_accepts_upstreams_duration_syntax -- --exact
cargo test -p hiqlite-patched --lib --features toml config::tests::the_preferred_health_check_delay_name_wins_over_the_deprecated_one -- --exact
cargo test -p hiqlite-patched --lib --features toml config_toml::tests::health_check_delay_takes_upstreams_name_and_syntax_and_wins_over_the_old_key -- --exact
grep -q '^#HQL_HEALTH_CHECK_DELAY=30s' hiqlite.env
grep -q '^#HQL_HEALTH_CHECK_DELAY_SECS=30' hiqlite.env
grep -q '^#health_check_delay = "30s"' hiqlite.toml
grep -q '#health_check_delay = "30s"' hiqlite/src/server/config.rs
# KD-1: the public field is not renamed or retyped in this change
grep -q 'pub health_check_delay_secs: u32,' hiqlite/src/config.rs
cargo test -p hiqlite-patched --features server --lib server::config::tests::the_generated_config_omits_keys_the_reference_file_documents -- --exact
grep -q '### F-117 ' standards/spec/findings-register.md
sh -c 'test "$(grep -c "by .044-port-upstream-015-fixes." standards/spec/findings-register.md)" -ge 10'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/044-port-upstream-015-fixes specs/015-server-binary-and-proxy standards/spec/findings-register.md CHANGELOG.md hiqlite.env hiqlite.toml'
```
