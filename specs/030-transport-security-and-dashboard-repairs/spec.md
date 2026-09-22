---
id: "030-transport-security-and-dashboard-repairs"
title: "Make certificate verification reachable, stop the API endpoint taking the raft channel's answers, and repair the dashboard's pre-auth surface"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "011-transport-security-material"
  - "018-dashboard-service-and-ui"
  - "027-node-lifecycle-and-startup-errors"
  - "029-consumer-surface-repairs"
amends:
  - "011-transport-security-material"
  - "018-dashboard-service-and-ui"
# D-7: this spec's `## Verification` block is the acceptance for both.
amends_verification:
  - "011-transport-security-material"
  - "018-dashboard-service-and-ui"
amends_sections:
  - "3-behavior"
  - "5-known-defects"
extends:
  - spec: "011-transport-security-material"
    unit: { kind: file, path: "hiqlite/src/tls.rs" }
    nature: superseding
  - spec: "011-transport-security-material"
    unit: { kind: file, path: "hiqlite/src/http_client.rs" }
    nature: superseding
  - spec: "011-transport-security-material"
    unit: { kind: file, path: "hiqlite/tests/tls_env.rs" }
    nature: superseding
  - spec: "018-dashboard-service-and-ui"
    unit: { kind: directory, path: "hiqlite/src/dashboard/" }
    nature: superseding
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite/src/config_toml.rs" }
    nature: additive
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite.toml" }
    nature: additive
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite.env" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/start.rs" }
    nature: additive
  - spec: "010-node-lifecycle-and-split-brain"
    unit: { kind: file, path: "hiqlite/src/split_brain_check.rs" }
    nature: additive
  - spec: "015-server-binary-and-proxy"
    unit: { kind: directory, path: "hiqlite/src/server/" }
    nature: additive
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/client/" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Repairs the transport-security and dashboard defects that reach a consumer of
  this release. Certificate verification becomes reachable at all, which it was
  not: the client trust store was built empty and there was no way to add to it.
  The API endpoint stops taking the raft channel's answers to its own questions.
  The claim that the secret is never sent over the network is narrowed to the
  channel it is true of. And the dashboard's unauthenticated surface stops
  panicking, stops allocating 32 MiB per concurrent attempt, and stops
  replicating reads as writes.
---

# 030: Make certificate verification reachable, stop the API endpoint taking the raft channel's answers, and repair the dashboard's pre-auth surface

## 1. Purpose

`011` and `018` adopted the transport security material and the dashboard as
found, and recorded between them eleven defects. This spec repairs the ones that
reach a consumer of this release and states the two it does not.

There is a theme in the transport half and it is worth naming, because it
explains why these were not noticed. **Each of them makes the safe configuration
harder to reach than the unsafe one.** Verification against an empty trust store
fails every handshake, so an operator turns verification off. Half a certificate
pair falls through to plaintext silently, so nobody learns they misconfigured it.
A malformed boolean means "off" in one place and ends the process in another. The
documentation says the secret is never sent over the network, which is true of
one channel and false of the one that carries it.

One responsibility: **make the secure configuration expressible, and stop the
insecure one being the path of least resistance.**

## 2. Territory

Four units are extended as `superseding` and nine additively. `011` and `018`
are amended and this spec carries both acceptances.

**Ownership boundary.** This spec does not make hiqlite's transport secure; it
removes the reasons an operator could not configure it securely, and corrects
what the documentation claims. Whether a deployment is secure depends on what
that operator does with the options, and section 5 says which options still do
not exist.

## 3. Behavior

### B-1. Verification has something to verify against

`ServerTlsConfigCerts` gains a `ca` field: a PEM file of trust anchors this
node's clients check peers against, readable from `HQL_TLS_{RAFT,API}_CA` or
`tls_{raft,api}_ca`.

This is F-043 and it is the load-bearing one. The client trust store was
`RootCertStore::empty()`, filled only under the `webpki-roots` feature, which
neither consumer of this release enables **and which would not help if they did**:
the public web roots do not sign an internal cluster's certificates. So
`danger_tls_no_verify = false` with specific certificates verified against
nothing and failed every handshake. The safe setting was not merely
inconvenient, it was unreachable, which is an excellent reason for an operator
to reach for the unsafe one.

The PEM reader is a few dozen lines rather than a new dependency: it extracts
`CERTIFICATE` blocks and hands the DER to `rustls` and `reqwest`, which are the
things that judge it. A trust anchor file that cannot be read is logged as an
error and leaves the store as it was, which fails closed.

### B-2. Half a certificate pair is an error, and so is a malformed boolean

F-041: `if key.is_some() && cert.is_some()` fell through to `None` when exactly
one was set, and logged nothing. An operator who set `HQL_TLS_API_CERT` and
misspelled `HQL_TLS_API_KEY` got a **plaintext** endpoint with no indication of
it. It is now an error naming both variables, in both orders.

F-042: `HQL_TLS_AUTO_CERTS` was `parse().unwrap_or(false)`, so a typo meant
"off", while `HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY` four lines below was
`.expect(..)`, so the same typo ended the process. Two booleans, four lines
apart, opposite answers. Both are configuration errors now, which is the policy
`027` B-1 states once for the whole crate.

### B-3. The API endpoint answers its own questions

F-044: `tls_no_verify` was derived from `node_config.tls_raft` and then handed to
every caller that talks to `addr_api`, and the scheme came from the same line. So
a node with verification off on the raft side and a properly verified API side
did not verify the API side either, and the reverse could not be expressed at
all. The split-brain checker was worse: `reqwest::Client::new()`, which honours
no override of any kind, against a URL whose scheme came from the API
configuration.

Both the no-verify flag and the trust anchor are now node-level facts, set once
at startup from `tls_api` and read by every REST client in the process. Set
together, so no caller can take one endpoint's answer to the other's question,
which is what the defect was.

### B-4. The claim about the secret is narrowed to the channel it is true of

F-045. The authored text says clients need not verify certificates because "they
do a 3-way handshake anyway, which validates both client and server without the
secret ever being sent over the network."

That is true of the **raft WebSocket channel**. It is false of the REST surface:
every `/cluster/*`, `/listen` and `/backup` call sends `secret_api` in an
`X-API-SECRET` header, in cleartext inside the TLS session, and no handshake
protects a header. With a self-signed certificate nobody verifies, an on-path
attacker terminates the session and reads the secret.

The claim is kept where it is true and the exception is stated beside it, in the
type's documentation, in `hiqlite.toml` and in `hiqlite.env`, each pointing at
the trust anchor B-1 added as what to do instead.

**No code changed for this one.** It is a documentation repair, and recording
that plainly matters: the behavior is unchanged and what was wrong was the
sentence a reader used to decide it was acceptable.

### B-5. The dashboard's unauthenticated surface

Three of these need no credential at all, which is what makes them the priority.

- **F-085.** The static-file fallback sliced the path four bytes from the end
  without asking whether that was a character boundary, and `http::Uri` accepts
  raw UTF-8. It is mounted **outside** the `Session` extractor, so any request
  reached it. The check is about the extension, so it asks for the extension.
- **F-084.** The single-flight hashing lock opened with `let _ = ..write().await`,
  which drops the guard at the end of the statement, so the lock serialized
  nothing while the comment above it described a brute-force defence. With
  32 MiB and two threads per hash, N concurrent unauthenticated attempts
  allocated N times that. One binding.
- **F-089.** A malformed `HQL_PASSWORD_DASHBOARD` ended the process at startup,
  while an **absent** one disabled the dashboard and carried on. Both now
  disable it, and say which it was.

Two more need a credential but are still wrong:

- **F-086 and F-088 are one line.** `sql[..7]` is a byte slice over a body from
  `String::from_utf8_lossy`, so it panics off a boundary; and a seven-byte prefix
  misclassifies every read that does not start with one of three keywords, so a
  CTE, a bare `VALUES`, or anything behind a comment took the **raft write**
  path. A read charged as a replicated write, and then refused by the
  non-determinism guard. Both go away by asking for the first keyword token.

## 4. Evidence and its limits

Nine tests, replacing four that pinned defects.

The TLS environment route is a **single** test rather than three, and that is
deliberate: `HQL_TLS_AUTO_CERTS` is shared by every variant and the environment
is process-wide, so two tests that both set it race each other. The first draft
of this spec split them and they did race, which is how that was established
rather than assumed. It walks every branch in sequence: no TLS, a full pair, the
trust anchor, the per-variant override, a malformed override, half a pair in
both orders, auto-certificates, half a pair with auto-certificates on, and a
malformed `HQL_TLS_AUTO_CERTS`.

The dashboard tests drive the real handlers: six paths through the static-file
fallback including three multi-byte ones, the single-flight lock in both forms
and through `verify_password` itself, eighteen inputs to the keyword classifier,
and the two decode failures behind F-089.

What the acceptance does **not** establish:

- **No handshake happens anywhere.** Not one test opens a TLS connection. B-1's
  trust anchor is asserted at the configuration boundary and by the PEM reader;
  that `rustls` and `reqwest` then verify against it is their contract. `011`
  section 4 already stated that no test here reaches a socket, and that is
  unchanged.
- **The PEM reader's test uses framing, not real certificates.** The bodies are
  not valid X.509. What is asserted is that two blocks produce two entries and
  that a non-PEM file is an error; `add_parsable_certificates` judges the DER.
- **F-044's repair is not observed end to end.** No node starts, so what is
  asserted is that the flag is derived from `tls_api` and that the split-brain
  checker uses the shared builder.
- **No request is served over the dashboard's middleware stack**, which `018`
  section 4 already recorded: the `Session` extractor, the CSRF check and the
  middleware are executed by nothing. The handlers here are called directly.
- **Nothing measures the memory F-084 is about.** The lock is asserted to be
  held; the 32 MiB is read from the parameters.

## 5. Known defects

**KD-1. F-087 is recorded and not repaired: the login cooldown is a lockout.**
A failed login blocks the **next** login globally for five seconds, with no
per-client state. Anyone who can reach the dashboard can keep it permanently
unloginable with one wrong password every five seconds, including the operator.

It is not repaired because the alternative is worse in the way the authored
comment says: per-client state needs a client identity, and behind a proxy that
is a header anyone can set, so keying on it buys spoofable state and an
unbounded map in exchange for a DoS that is already bounded by "the attacker can
reach the dashboard". Repairing it properly means a design decision about
dashboard authentication, which this release does not take. **An operator who
exposes the dashboard to an untrusted network should expect this.**

**KD-2. F-091 is recorded and not repaired: a session cannot be revoked.**
The cookie carries `{created, expires}` and validity is `expires < now`, with a
one hour lifetime and no logout route. Rotating `HQL_PASSWORD_DASHBOARD` does not
invalidate a live session, and nothing else does either.

**KD-3. A remote client has no trust anchor.** `Client::remote` builds its TLS
configuration without one, because it has no node configuration to take it from.
B-1 makes the trust anchor a node-level fact, and a remote client is not a node.
So a remote client still verifies against the `webpki-roots` bundle or against
nothing.

**KD-4. The trust anchor is process-wide.** One API trust anchor and one API
no-verify flag per process, set by the first node constructed. A process running
two nodes with different API TLS configurations would have the second take the
first's. That is the same configuration a `OnceLock` makes visible rather than
silently wrong, and `024` refuses two nodes over one data directory, but nothing
refuses two nodes with different transport configurations.

**KD-5. Nothing verifies the certificate chain against the hostname a peer is
reached by.** That is `rustls`'s job and it is now given the anchors to do it
with; no test in this repository establishes that it does.

**KD-6. `hiqlite/static` is embedded and its provenance is unestablished.** The
dashboard's compiled bytes are served by the handler B-5 repairs, and `019`
measured that the committed artifact is not what the current source builds (64
emitted files against 55 committed, F-093). For a UI that handles an operator
credential that is a supply-chain question, and it is `019`'s.

## 6. Resolved decisions

**D-1 (2026-09-21, a trust anchor field rather than enabling `webpki-roots`).**
The feature exists and neither consumer enables it, so enabling it is the
obvious-looking move. It is the wrong one: the public web roots do not sign an
internal cluster's certificates, so it would make the store non-empty without
making verification work. What was missing is the anchor, not a bundle.

**D-2 (2026-09-21, a hand-written PEM reader rather than `rustls-pemfile`).**
One file format, a few dozen lines, against a new dependency in a crate this
release is about to publish. The DER is still judged by `rustls` and `reqwest`.

**D-3 (2026-09-21, half a pair is an error, not a warning).** A warning is read
by nobody at three in the morning, and the outcome it warns about is a plaintext
endpoint an operator believes is encrypted. Refusing is the only version of this
that cannot be missed.

**D-4 (2026-09-21, the API TLS facts are process-wide rather than threaded).**
Threading a trust anchor through `become_cluster_member`,
`should_node_1_skip_init`, the shutdown path and the remote constructor is seven
signature changes for a value that is a node-level fact. A `OnceLock` set once at
startup cannot be inconsistent between those callers, which is exactly what F-044
was. KD-4 records the cost.

**D-5 (2026-09-21, F-045 is a documentation repair and nothing else).** The
behavior is not wrong: sending the secret in a header inside TLS is fine when the
TLS is verified. What was wrong is a sentence telling an operator that verifying
it does not matter. Changing behavior here would mean refusing to start without a
verified API endpoint, which is a deployment decision this release does not make
for anyone.

**D-6 (2026-09-21, F-087 and F-091 are recorded, not repaired).** Both need a
design decision about dashboard authentication. KD-1 gives the argument for the
first; the second needs server-side session state the dashboard does not have.
Recording them with their exact consequence is what a consumer needs; guessing at
a design during a release is not.

**D-7 (2026-09-21, this block is the acceptance for `011` and `018`).** Fourteen
of their commands asserted defective expressions or named replaced tests. Each is
replaced below and marked with what it was.

## 7. Out of scope

- **Dashboard authentication design.** KD-1, KD-2.
- **The dashboard build's provenance.** `019`, KD-6.
- **A TLS integration test.** `012`, and `011` section 4 already records that no
  cluster test has ever run with TLS (F-053).
- **The node and library surface.** `029`.
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 030`. **This block is the acceptance for `011` and
`018` as well as this spec's** (D-7).

```verify:cli
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
cargo test -p hiqlite-patched --features dashboard --lib dashboard::password::tests::the_single_flight_lock_is_held_for_the_whole_hashing -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::password::tests::verify_password_holds_the_lock_while_it_runs -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::password::tests::the_hasher_is_argon2id_with_recorded_parameters -- --exact
# was a_multibyte_path_panics_the_fallback, which pinned F-085
cargo test -p hiqlite-patched --features dashboard --lib dashboard::static_files::tests::a_multibyte_path_is_served_or_missing_but_never_a_panic -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::static_files::tests::an_already_compressed_type_is_not_encoded_again -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::static_files::tests::a_known_asset_is_served_with_its_cache_headers -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::static_files::tests::an_unknown_asset_is_a_plain_404 -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::query::tests::forbidden_fn_scan_catches_only_real_calls -- --exact
cargo test -p hiqlite-patched --features dashboard --lib dashboard::session::tests -- --test-threads=1
cargo test -p hiqlite-patched --features dashboard --lib dashboard::handlers::tests
# was the `let _ =` that dropped the guard at the end of the statement. It survives in the
# test that asserts the difference, and not in the function.
sh -c '! grep -q "^    let _ = IS_HASHING.write().await;" hiqlite/src/dashboard/password.rs'
sh -c 'grep -q "let _guard = IS_HASHING.write().await;" hiqlite/src/dashboard/password.rs'
# the comment described a rate limit that did not exist; it does now
grep -q 'prevents brute-fore effectively' hiqlite/src/dashboard/password.rs
grep -q 'the single-flight lock in `password::verify_password`' hiqlite/src/dashboard/session.rs
# was the byte slice four bytes from the end, which panics off a character boundary (F-085)
sh -c '! grep -q "let path_ending = &path\[path.len().saturating_sub(4)..\];" hiqlite/src/dashboard/static_files.rs'
sh -c 'grep -q "\.extension()" hiqlite/src/dashboard/static_files.rs'
# was the seven-byte prefix: a boundary panic (F-086) and a read classified as a write
# (F-088), in one line
sh -c '! grep -q "let sql_start = sql\[..7\].to_lowercase();" hiqlite/src/dashboard/query.rs'
sh -c 'grep -q "fn first_keyword" hiqlite/src/dashboard/query.rs'
cargo test -p hiqlite-patched --features dashboard --lib dashboard::query::tests::the_first_keyword_is_found_past_whitespace_and_comments -- --exact
sh -c '! grep -q "if sql.len() < 8 {" hiqlite/src/dashboard/query.rs'
grep -q 'There is no per-client state' hiqlite/src/dashboard/session.rs
grep -q 'static NEXT_LOGIN_ALLOWED: Mutex<Option<Instant>> = Mutex::new(None);' hiqlite/src/dashboard/session.rs
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
sh -c 'grep -q "There is no per-client state" hiqlite/src/dashboard/session.rs'
sh -c 'grep -q "const SESSION_LIFETIME: i64 = 3600;" hiqlite/src/dashboard/session.rs'
sh -c '! grep -rq "logout" hiqlite/src/dashboard/'
# and the generated server config documents the trust anchor alongside the reference file
sh -c 'grep -q "tls_api_ca" hiqlite/src/server/config.rs'
cargo test -p hiqlite-patched --features server --lib server::config::tests::the_generated_config_omits_keys_the_reference_file_documents -- --exact
sh -c '! grep -rl "$(printf "\342\200\224")" specs/030-transport-security-and-dashboard-repairs'
```
