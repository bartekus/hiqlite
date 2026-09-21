---
id: "015-server-binary-and-proxy"
title: "Adopt the server binary and the proxy"
status: draft
kind: "adoption"
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "003-client-consistency-and-retry-outcomes"
  - "009-configuration-contract"
  - "010-node-lifecycle-and-split-brain"
  - "011-transport-security-material"
origin:
  retroactive: true
  paths:
    - "hiqlite/src/main.rs"
    - "hiqlite/src/server/"
establishes:
  - "hiqlite/src/main.rs"
  - "hiqlite/src/server/mod.rs"
  - "hiqlite/src/server/args.rs"
  - "hiqlite/src/server/cache.rs"
  - "hiqlite/src/server/config.rs"
  - "hiqlite/src/server/logging.rs"
  - "hiqlite/src/server/password.rs"
  - "hiqlite/src/server/proxy/mod.rs"
  - "hiqlite/src/server/proxy/config.rs"
  - "hiqlite/src/server/proxy/handlers.rs"
  - "hiqlite/src/server/proxy/notify.rs"
  - "hiqlite/src/server/proxy/state.rs"
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
  Adopts the three subcommands of the hiqlite binary and the proxy that one of
  them starts. The proxy does not start: it registers an axum 0.7 route literal
  against the pinned axum 0.8, which panics at router construction before the
  bind, and nothing caught it because CI never enables the server feature for a
  test. Ten findings recorded, four executed, including a non-constant-time
  comparison of the API secret in the proxy's copy of a function the node
  hardened. Repairs no runtime behavior.
---

# 015: Adopt the server binary and the proxy

## 1. Purpose

`hiqlite` is also a binary with three subcommands, and one of them starts a
second HTTP service that reimplements pieces of the node's own API surface.
Nothing claimed any of it, and F-019 records why that matters more here than
elsewhere: the `server` feature is not enabled for any CI test run, so this
entire tree compiles and lints and is never executed.

This spec claims it. Section 5's first finding is what that gap costs: the proxy
panics on its first route registration, at the pinned axum, and has done since
the 0.8 upgrade.

**It is an adoption.** Nothing is repaired. The only changes to shipped files are
three `#[cfg(test)]` modules.

## 2. Territory

**Establishes** `hiqlite/src/main.rs` and eleven files of `hiqlite/src/server/`:
the subcommand surface (`mod.rs`, `args.rs`), the configuration and generator
(`config.rs`), logging (`logging.rs`), the dashboard password hasher
(`password.rs`), a dead module (`cache.rs`, KD-9), and the five proxy files that
were unclaimed (`proxy/mod.rs`, `config.rs`, `handlers.rs`, `notify.rs`,
`state.rs`).

**Does not claim** `hiqlite/src/server/proxy/stream.rs`, which `003` establishes.

**Extends**, without re-establishing: `000` on `spec-spine.toml`; `005` on the
findings register and the adoption plan.

**Depends on** `003` for `Client::remote`, which the proxy is built on; `009` for
`NodeConfig::from_toml` and the reference file the generator duplicates; `010`
for the node the `serve` subcommand starts; and `011` for `ServerTlsConfig`,
which the proxy reads with `from_env("API")`.

**Describes without claiming** `app_state.rs:28-36` (`RaftType`),
`helpers.rs:33-156`, and `network/mod.rs:63-76`. KD-2 and KD-3 are comparisons
against those, and no line in any of them is modified.

## 3. Behavior

### B-1. Three subcommands, and what each one does

`main.rs` is thirteen lines: under `server` it awaits `hiqlite::server::server()`,
and without the feature it is a `panic!` telling you to enable it. `server()`
(`server/mod.rs:17-47`) parses `Args` and branches:

- **`serve`** initialises logging, builds a `NodeConfig` from TOML, starts a node
  with an `Empty` cache, and blocks on a shutdown handle. Everything after
  `start_node_with_cache` is `010`'s.
- **`proxy`** initialises logging, parses `Config` from environment files,
  validates it, and calls `start_proxy`.
- **`generate-config`** initialises logging and writes a default configuration.

The three do not share a configuration route: `serve` reads TOML through `009`'s
`from_toml`, `proxy` reads environment files through `dotenvy`, and
`generate-config` writes a TOML template embedded in the source.

### B-2. The `$HOME` default is a sentinel string, expanded on one of two paths

`ArgsConfig::config_file` defaults to the literal `"$HOME/.hiqlite/hiqlite.toml"`
(`args.rs:24-25`) and `ArgsProxy::config_file` to
`"$HOME/.hiqlite/hiqlite.env"` (`:39-40`). `clap` does not expand shell
variables, so both are literals.

`build_node_config` (`server/config.rs:9-13`) handles this by comparing the
argument against that exact literal and substituting `default_config_file_path()`
when it matches. `Config::parse` (`proxy/config.rs:16-22`) does not: it passes
the string straight to `dotenvy::from_filename_override`, which fails to find a
relative path called `$HOME/.hiqlite/hiqlite.env` and logs at debug. KD-4.

### B-3. `generate-config` writes a second copy of the reference configuration

`generate` (`server/config.rs:26-69`) creates `~/.hiqlite` at `0700`, prompts
before overwriting, takes or generates a dashboard password, prints it to stdout,
hashes it with argon2 through `password::hash_password_b64`, writes the file at
`0600`, and generates fresh `secret_raft`, `secret_api` and `EncKeys` values.

The file it writes is `default_config` (`:89-433`), a 344-line `format!`
template. It is a second reference configuration alongside `hiqlite.toml`, which
`009` claims, with no mechanism keeping the two in step. They have already
diverged by eight keys. KD-5, and it is executed.

### B-4. The proxy is a remote client behind four routes

`start_proxy` (`proxy/mod.rs:17-87`) installs the ring crypto provider if TLS is
configured, builds a `Client::remote` against the configured `addr_api` list,
spawns the notify listener, and serves four routes on `0.0.0.0:{listen_port}`:
`/cluster/metrics/{raft_type}`, `/listen`, `/stream` and `/ping`.

`handlers.rs` authenticates `listen` and `metrics` with its own `validate_secret`
and its own `HEADER_NAME_SECRET` constant (`:21`, `:84-96`), both private copies
of the ones in `network/mod.rs`. `stream` does not call `validate_secret`, and
that is correct rather than a hole: `proxy/stream.rs:22` runs
`HandshakeSecret::server` on the upgraded socket, which is the same
challenge-response the node uses and the one `011` KD-5 describes. The two copies
of `validate_secret` are **not** equivalent, though. KD-2.

`notify.rs` spawns two tasks: a router that forwards every event from
`client.listen_bytes()` into the shared notify handler, and a drain that keeps
the channel from filling. The router's loop `if let Ok(msg) = ...` discards a
receive error and loops immediately, so a persistent client failure is a spin.

### B-5. Logging is set once from the flag, and ignores the environment

`init_logging` (`logging.rs:3-27`) builds a `tracing_subscriber::fmt` with
`with_env_filter(level.as_str())`, which constructs the filter from that string.
`RUST_LOG` is not read, so the `--log-level` flag is the only control, and the
per-target filtering an operator would expect from `RUST_LOG` is unavailable.
KD-10.

### B-6. The proxy's validation is two fields, and one of them is misnamed

`Config::is_valid` (`proxy/config.rs:50-62`) checks that `nodes` is non-empty
and that `secret_api` is at least 16 characters. Nothing validates the port, the
TLS material, or that any node is reachable. The message for a short secret names
`'secret_raft'` as well as `'secret_api'`, and the proxy has no `secret_raft`.
KD-8, and both halves are executed.

`Config::parse` itself (`:16-48`) has no error return: it `expect`s
`LISTEN_PORT`'s parse, `EncKeys::from_env`, the key initialisation, and
`HQL_SECRET_API`. So every configuration failure the proxy can have is either a
panic before `is_valid` runs or is not checked at all.

## 4. Evidence and its limits

Four characterization tests, in three `#[cfg(test)]` modules. All require
`--features server`, which is exactly the feature CI does not enable (F-019), so
the acceptance block runs them explicitly.

| test | what it establishes |
|---|---|
| `server::proxy::tests::the_proxy_metrics_route_is_rejected_by_the_pinned_axum` | the route literal `start_proxy` registers is rejected by axum 0.8 at construction, with the real handler and the same `nest` (KD-1) |
| `server::proxy::tests::the_same_capture_in_zero_eight_syntax_is_accepted` | the node's spelling of the same capture is accepted, so KD-1 is a divergence and not a version gap |
| `server::config::tests::the_generated_config_omits_keys_the_reference_file_documents` | the exact eight-key divergence between the generated template and `hiqlite.toml`, in both directions (KD-5) |
| `server::proxy::config::tests::proxy_validation_covers_two_fields_and_names_a_third` | both branches of `is_valid`, the 16-character boundary, and that the message names `secret_raft` (KD-8) |

**What the tests do not establish.**

- **That the proxy ever serves a request.** No proxy was started, no socket was
  bound, and no request was made. KD-1 is why: the router cannot be built. Every
  claim in B-4 beyond the route table is read from source.
- **KD-2's exploitability.** A non-constant-time comparison is a source fact; no
  timing measurement was taken and none is claimed.
- **KD-3 by execution.** That `/cluster/metrics/unknown` reaches the `panic!` is
  read from `RaftType`'s `#[serde(rename_all = "lowercase")]` and the match arm.
  It cannot be executed here for the same reason as the first bullet.
- **`serve` and `generate-config` end to end.** No node was started through the
  binary and no configuration file was written to a home directory. The generated
  template is exercised as a string, not as a file a node then loads.
- **Anything about argon2 parameters or the dashboard password**, which are
  W-12's.

## 5. Known defects

Recorded as found, none repaired. Each is also filed in
`standards/spec/findings-register.md`.

**KD-1. The proxy panics on its first route and never binds** (F-067).
`proxy/mod.rs:60` registers `"/metrics/:raft_type"`. That is axum 0.7 path
syntax. The pinned axum is 0.8.9 (`Cargo.toml:31`, `Cargo.lock`), which rejects a
segment beginning with `:` at `Router::route` with
`"Path segments must not start with `:`. For capture groups, use `{capture}`."`.
`Router::route` panics rather than returning an error, so `start_proxy` ends the
process before `Client::remote`, before the notify tasks, and before the bind.

Consequence: `hiqlite proxy` does not run at all on this tree. The node
registers the same capture correctly (`start.rs:166-180`, `{raft_type}`), so this
is one call site left behind by the 0.8 upgrade, not an unmigrated codebase. It
was not caught because the `server` feature is linted but never tested (F-019),
and `Router::route`'s panic is a runtime one that no compile check reaches.

**Observed by execution**, twice: the literal panics with the real handler and
the same `nest`, and the `{raft_type}` spelling of the same route is accepted.

**KD-2. The proxy compares the API secret in non-constant time** (F-068).
`proxy/handlers.rs:84-96` is a private copy of `network/mod.rs:63-76`, and the
two differ in one line. The node's is
`!constant_time_eq(state.secret_api.as_bytes(), secret.as_bytes())`; the proxy's
is `state.secret_api.as_bytes() != secret.as_bytes()`, which is a byte-slice
comparison that returns on the first differing byte.

Consequence: on `/listen` and `/cluster/metrics/*`, the proxy's rejection time
varies with how long a prefix of the supplied header matched, which is the
condition the node's `constant_time_eq` exists to remove. The header constant
`HEADER_NAME_SECRET` is also duplicated (`:21`), which is how the two copies came
to diverge: the hardening landed on one of them. Source-established; no timing
measurement was taken. Reachable only once KD-1 is fixed, which is the order a
repair has to consider.

**KD-3. A valid path value reaches an unconditional panic** (F-069).
`RaftType` (`app_state.rs:28-36`) derives `Deserialize` with
`#[serde(rename_all = "lowercase")]` and has an `Unknown` variant, so the path
segment `unknown` deserializes successfully. `proxy/handlers.rs:68` then matches
`RaftType::Unknown => panic!("neither `sqlite` nor `cache` feature enabled")`.
The panic message describes a build configuration, and the input that reaches it
is a request.

The same arm appears seven times in `helpers.rs` (`:33`, `:56`, `:69`, `:94`,
`:124`, `:156`), which this spec does not claim, and the node routes
`/cluster/add_learner/{raft_type}`, `/become_member/{raft_type}`,
`/membership/{raft_type}`, `/metrics/{raft_type}` and `/stream/{raft_type}`
(`start.rs:166-180`) into handlers that take `Path<RaftType>`. So the shape is
not confined to the proxy.

Mitigation, stated so the severity is not overstated: `validate_secret` runs
before the match on both the proxy and the node, so the caller must already hold
`secret_api`. Consequence under unwinding is a dropped connection; under
`panic = abort` it is the process, which is `010` B-7's split. Source-established.

**KD-4. The proxy's documented default configuration file can never be loaded**
(F-070). `args.rs:39-40` documents `$HOME/.hiqlite/hiqlite.env` as the default
`--config-file`, and `proxy/config.rs:20-22` passes it to `dotenvy` unexpanded.
`server/config.rs:9-13` special-cases the identical sentinel for the `serve`
path, so the mechanism exists and was applied to one of the two. Consequence: an
operator who follows the help text and puts a file at
`~/.hiqlite/hiqlite.env` gets a debug-level "config file not found", then a
panic from `HQL_SECRET_API not found` (`proxy/config.rs:45`) with no indication
that the file was looked for in the wrong place. Source-established.

**KD-5. The generated configuration is a second reference file, and it has
drifted** (F-071). `server/config.rs:89-433` embeds a full TOML template that
duplicates `hiqlite.toml`, which `009` claims. Nothing compares them. They
currently differ by eight keys, all present in the reference and absent from the
generated file: `listen_addr_raft`, `listen_addr_api`, `tls_auto_certificates`,
`secrets_file`, and the four `rate_limit_*` keys.

Consequence: an operator who starts from `hiqlite generate-config` never sees
that the listen addresses can be set separately from the advertised ones, which
is `010` B-4's distinction; never sees `tls_auto_certificates`, which is the
switch `011` KD-6 was filed about and which `011` had just finished documenting
in the other reference file; and never sees `secrets_file` or the rate limits.
**Observed by execution**: the test pins the exact eight, in both directions, so
the drift cannot widen or silently close unnoticed. Same family as F-012, the
dashboard build-drift finding: two artifacts that must agree, with no check.

**KD-6. `start_proxy` panics where it promises an error** (F-072).
`proxy/mod.rs` returns `Result<(), Error>` and then `expect`s the crypto provider
installation (`:19-21`), `expect`s the socket address parse (`:70`), and
`unwrap`s the serve future in both the TLS and plaintext branches (`:78`, `:83`).
So a port already in use, a malformed listen address, or a TLS material failure
ends the process rather than returning through `server()` to `main`. Same class
as F-038 and F-040, and part of W-22. Source-established.

**KD-7. The proxy binds `0.0.0.0` with no way to change it** (F-073).
`proxy/mod.rs:68`: `format!("0.0.0.0:{}", config.listen_port)`. The port is
configurable through `LISTEN_PORT`; the interface is not. The node it fronts has
`listen_addr_api` for exactly this (`010` B-4, `009`), so an operator who binds
the node to a private interface cannot do the same for the proxy. Classed as a
limit, because nothing claims otherwise; recorded because the asymmetry with the
node is undocumented. Source-established.

**KD-8. The proxy's validation message names a secret the proxy does not have**
(F-074). `proxy/config.rs:55-59` rejects a `secret_api` shorter than 16
characters with `"'secret_raft' and 'secret_api' should be at least 16
characters long"`. The proxy's `Config` has four fields and none of them is
`secret_raft`; the message is the node's, copied. Consequence: an operator is
told to fix a setting that does not exist in the file they are editing. Classed
as a contradiction. **Observed by execution.**

**KD-9. A declared module contains nothing but commented-out code** (F-075).
`server/cache.rs` is 21 lines, every one of them a comment, and `server/mod.rs:9`
declares `mod cache;`. It compiles to nothing. Recorded rather than deleted,
because deleting it is a change and this is an adoption, and because the commented
type it holds is a two-variant cache enum that would answer what the server
binary's `Empty` cache (`server/mod.rs:24`) was meant to become.
Source-established.

**KD-10. The server binary ignores `RUST_LOG`** (F-076). `logging.rs:14-26` calls
`with_env_filter(level.as_str())`, which builds the filter from that string
rather than from the environment. Consequence: `--log-level` is the only control,
and the per-target directives an operator would reach for, including silencing
`openraft`, are unavailable. Nothing claims `RUST_LOG` works, so this is a limit
and not a contradiction; it is recorded because every other Rust service an
operator runs does read it. Source-established.

**Retained without change.** F-019, which records that the `server` feature is
never enabled for a CI test run. KD-1 is the first demonstrated consequence of
that gap, and section 4 states that this spec's own tests need the feature
passed explicitly.

## 6. Resolved decisions

**D-1 (2026-09-21, nothing here is repaired, including KD-1).** KD-1 is one
character in one string and it makes a broken subcommand work. It is still a
behavioral repair to a surface with no test coverage at all, and fixing it
immediately exposes KD-2 and KD-3 on a reachable network path, which is an order
a repair has to reason about and an adoption must not decide by accident. The
repair belongs with a change that also enables the `server` feature in CI, which
is F-019's and W-19's.

**D-2 (2026-09-21, the route test asserts the panic rather than the fix).** It
uses the real handler and the same `nest`, and a sibling test asserts that the
`{raft_type}` spelling is accepted, so the pair says both what is broken and what
correct looks like without changing either.

**D-3 (2026-09-21, `stream` is not reported as an authentication hole).** It is
the one handler that does not call `validate_secret`, which reads like a gap
until `proxy/stream.rs:22` is followed. It runs `HandshakeSecret::server` on the
upgraded socket. Recorded here because the absence is conspicuous and the next
reader deserves the answer rather than the question.

**D-4 (2026-09-21, `main.rs` is claimed although it is thirteen lines).** It is
the only entry point the binary has, and leaving it unowned would leave the
feature gate that decides whether the whole of this tree exists unattributed.

## 7. The inventory declarations this spec requires

Of the twelve files, only `hiqlite/src/server/proxy/stream.rs` is in a content
hash at the pinned revision, declared by the pilot for `003`. The other eleven
are reached by no glob: `hiqlite/src/server/**/*.rs` does not exist in
`extra_hashed_inputs`, and `hiqlite/src/*.rs` is not covered either, which is
what `010`, `011` and `013` each had to work around individually.

`spec-spine.toml` gains `hiqlite/src/main.rs` and `hiqlite/src/server/**/*.rs`,
the second of which subsumes the existing `proxy/stream.rs` entry; that entry is
kept, because removing it would change what `000`'s declaration list says about
`003`'s territory and this spec does not own that statement. No
`[coverage] governed_scope` entry is needed; all twelve are inside the `hiqlite`
cargo package.

**These are freshness declarations, not enforcement settings.**

## 8. Out of scope

- **Every repair.** KD-1 to KD-10 are recorded and left.
- **`proxy/stream.rs`**, which is `003`'s, and the WebSocket protocol it speaks.
- **`helpers.rs`**, which KD-3 names for the seven further copies of the same
  match arm and which no spec claims.
- **The dashboard**, including the password this binary generates and hashes.
  W-12.
- **Enabling the `server` feature in CI.** F-019, W-19.
- **The startup-error policy.** W-22, which KD-6 joins.
- **Whether the public API is frozen.** W-17.
- **Ratification, enforcement, and any tool or pin change.**

## Verification

Run with `just spine-verify 015`.

```verify:cli
test -f hiqlite/src/main.rs
test -f hiqlite/src/server/proxy/mod.rs
sh -c 'spec-spine index owner hiqlite/src/main.rs | grep -q 015-server-binary-and-proxy'
sh -c 'spec-spine index owner hiqlite/src/server/proxy/handlers.rs | grep -q 015-server-binary-and-proxy'
sh -c 'spec-spine index owner hiqlite/src/server/config.rs | grep -q 015-server-binary-and-proxy'
sh -c 'spec-spine index owner hiqlite/src/server/proxy/stream.rs | grep -q 003-client-consistency-and-retry-outcomes'
sh -c 'spec-spine registry relationships 015-server-binary-and-proxy | grep -q 011-transport-security-material'
cargo test -p hiqlite --features server --lib server::proxy::tests::the_proxy_metrics_route_is_rejected_by_the_pinned_axum -- --exact
cargo test -p hiqlite --features server --lib server::proxy::tests::the_same_capture_in_zero_eight_syntax_is_accepted -- --exact
cargo test -p hiqlite --features server --lib server::config::tests::the_generated_config_omits_keys_the_reference_file_documents -- --exact
cargo test -p hiqlite --features server --lib server::proxy::config::tests::proxy_validation_covers_two_fields_and_names_a_third -- --exact
grep -q '.route("/metrics/:raft_type", get(handlers::metrics)),' hiqlite/src/server/proxy/mod.rs
grep -q '"/metrics/{raft_type}", get(management::metrics)' hiqlite/src/start.rs
grep -q 'axum = { version = "0.8' Cargo.toml
sh -c 'grep -q "constant_time_eq(state.secret_api.as_bytes(), secret.as_bytes())" hiqlite/src/network/mod.rs'
sh -c 'grep -q "if state.secret_api.as_bytes() != secret.as_bytes() {" hiqlite/src/server/proxy/handlers.rs'
sh -c '! grep -q "constant_time_eq" hiqlite/src/server/proxy/handlers.rs'
grep -q 'static HEADER_NAME_SECRET: &str = "X-API-SECRET";' hiqlite/src/server/proxy/handlers.rs
grep -q 'HandshakeSecret::server(&mut ws, state.secret_api.as_bytes())' hiqlite/src/server/proxy/stream.rs
sh -c 'grep -q "serde(rename_all = \"lowercase\")" hiqlite/src/app_state.rs'
grep -q 'RaftType::Unknown => panic!("neither `sqlite` nor `cache` feature enabled"),' hiqlite/src/server/proxy/handlers.rs
sh -c 'test "$(grep -c "RaftType::Unknown => panic!" hiqlite/src/helpers.rs)" -ge 6'
sh -c 'grep -q "if args.config_file == \"\$HOME/.hiqlite/hiqlite.toml\"" hiqlite/src/server/config.rs'
sh -c 'grep -q "default_value = \"\$HOME/.hiqlite/hiqlite.env\"" hiqlite/src/server/args.rs'
sh -c '! grep -q "HOME" hiqlite/src/server/proxy/config.rs'
grep -q "'secret_raft' and 'secret_api' should be at least 16 characters long" hiqlite/src/server/proxy/config.rs
sh -c '! grep -q "secret_raft:" hiqlite/src/server/proxy/config.rs'
grep -q 'let addr_str = format!("0.0.0.0:{}", config.listen_port);' hiqlite/src/server/proxy/mod.rs
grep -q 'with_env_filter(level.as_str())' hiqlite/src/server/logging.rs
sh -c '! grep -q "RUST_LOG" hiqlite/src/server/logging.rs'
sh -c '! grep -vq "^\s*//" hiqlite/src/server/cache.rs || test "$(grep -cv "^\s*//\|^\s*$" hiqlite/src/server/cache.rs)" = "0"'
grep -q 'mod cache;' hiqlite/src/server/mod.rs
grep -q 'hiqlite/src/main.rs' spec-spine.toml
grep -q 'hiqlite/src/server/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/015-server-binary-and-proxy'
```
