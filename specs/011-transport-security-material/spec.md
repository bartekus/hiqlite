---
id: "011-transport-security-material"
title: "Adopt the transport security material and its consumers"
status: draft
kind: "adoption"
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "009-configuration-contract"
  - "010-node-lifecycle-and-split-brain"
origin:
  retroactive: true
  paths:
    - "hiqlite/src/tls.rs"
    - "hiqlite/src/http_client.rs"
establishes:
  - "hiqlite/src/tls.rs"
  - "hiqlite/src/http_client.rs"
  - "hiqlite/tests/tls_env.rs"
extends:
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
  - spec: "009-configuration-contract"
    unit: { kind: file, path: "hiqlite.env" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Adopts hiqlite's TLS material as a contract traced end to end: how the two
  endpoints select certificates, what each dangerous override actually disables,
  which client honours which override, and what a verifying client can trust.
  Six defects are recorded, two of them executed. The most consequential are
  that the API channel's no-verify flag is read from the raft configuration by
  two of its four consumers, that a verifying client has no configurable trust
  anchor, and that the documented justification for not verifying does not cover
  the endpoints that send a bearer secret. Documents HQL_TLS_AUTO_CERTS.
  Repairs no runtime behavior.
---

# 011: Adopt the transport security material and its consumers

## 1. Purpose

`009` adopted the configuration keys. `010` adopted the lifecycle that consumes
them. Neither says what `tls_raft` and `tls_api` actually do once a node is
running, and the answer is not recoverable from either: the material is built in
one file, the client configurations in a second, and the four call sites that
decide which override applies to which channel are in three more.

This spec traces that path end to end. It stops at no constructor field, because
the defects are not in the constructors. Three of the six are only visible when
the flag is followed from the configuration to the socket.

**It is an adoption.** Every behavior below is described as found. Six defects
are recorded under `known-defects` and left unfixed, per constitution VI. The
only non-test change to a shipped file is one comment block added to
`hiqlite.env` (section 5, KD-6). No runtime behavior changes, and no security
control is altered in either direction.

## 2. Territory

**Establishes**, none of which any spec claimed before:

- `hiqlite/src/tls.rs`, the material, the two client configurations, the
  self-signed generator, and the verifier that verifies nothing;
- `hiqlite/src/http_client.rs`, the REST client every cluster-management request
  is made with, which is where the `danger_tls_no_verify` flag lands;
- `hiqlite/tests/tls_env.rs`, this spec's environment-route evidence, which
  exists as its own target for the reason D-2 records.

**Extends**, without re-establishing: `000` on `spec-spine.toml` for the
freshness declarations of section 8; `009` on `hiqlite.env` for KD-6; `005` on
the findings register and the adoption plan.

**Depends on** `010` and describes, without claiming, four call sites inside its
units (`start.rs:37-43`, `:114-118`, `:254-284`, `:291-295`) and one inside
`003`'s (`client/mgmt.rs:186-190`). B-5 is about which flag each of them passes,
so it cannot be written without naming them. No line in either unit is modified,
so no edge on them is taken.

## 3. Behavior

### B-1. Two endpoints, three ways to end up without verification

`ServerTlsConfig` (`tls.rs:27-30`) has two variants. `TlsAutoCertificates`
generates a self-signed certificate per process; `Specific(ServerTlsConfigCerts)`
loads a PEM key and certificate chain from disk and carries one boolean,
`danger_tls_no_verify`.

`danger_tls_no_verify()` (`:50-55`) returns `true` unconditionally for
`TlsAutoCertificates` and the stored boolean for `Specific`.
`ServerTlsConfigCerts::new` (`:40-46`) starts from `false`. So certificate
verification is off whenever auto-certificates are selected, on by default when
specific certificates are given, and off when the operator asks for that
explicitly. `auto_certificates_always_disable_verification` asserts the first two.

The third route is KD-1: a configuration that was meant to be `Specific` and is
not.

### B-2. `client_config` and `build_http_client` are two different clients

`client_config` (`:144-149`) produces a rustls `ClientConfig` used for the raw
WebSocket transport (`into_tls_stream`, `:172-181`). `build_http_client`
(`http_client.rs:7-25`) produces a `reqwest::Client` used for every REST request
in the join, leave, metrics and membership paths. Both take the same boolean and
apply it through different mechanisms: `build_tls_config` swaps in
`NoTlsVerifier` (`tls.rs:158-167`, `:186-230`), a `ServerCertVerifier` whose
three verification methods return success unconditionally; `build_http_client`
sets `tls_danger_accept_invalid_certs` (`http_client.rs:11`).

Both also share a trust-anchor problem, from opposite directions. B-4 states it.

### B-3. `from_env` reads three variables per endpoint and one shared switch

`ServerTlsConfig::from_env(variant)` (`tls.rs:57-83`) is called twice,
`from_env("RAFT")` and `from_env("API")` (`config.rs:351-352`). It reads
`HQL_TLS_AUTO_CERTS` once, then `HQL_TLS_{variant}_KEY`,
`HQL_TLS_{variant}_CERT` and `HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY`.

The selection is: key **and** cert both present gives `Specific`; otherwise
`HQL_TLS_AUTO_CERTS` parsed as `true` gives `TlsAutoCertificates`; otherwise
`None`, which means that endpoint runs without TLS. `from_env_branches_including_the_silent_downgrade`
asserts every branch, including the two that make KD-1.

The TOML route is `009`'s and reaches the same struct through
`config_toml.rs:180-215`, with one difference `009` recorded and this spec
inherits: the API side's `danger_tls_no_verify` cannot be set from TOML at all
(F-031). Combined with B-5, the API flag is unreachable from one route and
ignored by two of its four consumers.

### B-4. A verifying client has no configurable trust anchor

`build_tls_config` (`tls.rs:152-170`) builds `RootCertStore::empty()` and adds
certificates to it only under `#[cfg(feature = "webpki-roots")]` (`:155-156`).
That feature is not in `default` (`hiqlite/Cargo.toml:22`). `reqwest` is built
with `default-features = false` and `rustls-no-provider`
(`Cargo.toml:80-85`), with no roots feature, and `build_http_client` likewise
merges the webpki bundle only under the same `cfg` (`http_client.rs:15-22`).

`ServerTlsConfigCerts` has three fields and none of them is a CA
(`the_tls_material_type_has_no_field_for_a_trust_anchor`). There is no other
API, environment variable or TOML key for one. So a verifying client can trust
exactly one thing, the bundled public web PKI, and only in a build that opted
into a non-default feature. An internal CA, which is the normal way to run a
private cluster over TLS, cannot be trusted at all.

The practical consequence is stated in KD-3 rather than here, because it is a
consequence and not a description.

### B-5. Four consumers, three different pairings of scheme and verification

Every REST request in the cluster paths targets a peer's **`addr_api`**. Which
endpoint's TLS configuration decides the scheme, and which one decides
verification, differs by caller.

| caller | scheme from | verification from | client |
|---|---|---|---|
| `store/mod.rs:98-109`, `:195-206` to `should_node_1_skip_init` | `tls_api` | `tls_api` | `build_http_client` |
| `start.rs:254-284` to `become_cluster_member` | **`tls_raft`** | **`tls_raft`** | `build_http_client` |
| `start.rs:114-118` to `split_brain_check::spawn` | `tls_api` | **neither** | `reqwest::Client::new()` |
| `start.rs:291-295` then `client/mgmt.rs:186-190` on shutdown | `tls_api` | **`tls_raft`** | `build_http_client` |

`start.rs:38-43` computes both `tls_raft` and `tls_no_verify` from
`node_config.tls_raft` and passes the pair down; `init.rs:275-276` turns the
first into `https` or `http` against `node.addr_api`. `split_brain_check.rs:135`
constructs a default `reqwest::Client`, so it honours no override at all.

In a cluster where both endpoints are configured the same way, which is the only
shape the reference files illustrate, all four agree. KD-4 records what happens
when they are configured differently, and what the third row does even when they
are not.

### B-6. The self-signed path names the listen address and lives for three years

`server_config_self_signed` (`tls.rs:97-142`) generates one key pair per process
into a `OnceLock`, then issues a certificate whose single SAN and common name
come from `url.rsplit_once(":")` applied to `listen_addr_raft` or
`listen_addr_api`, which is the **listen** address and not the advertised one.
Validity runs from 60 seconds ago to three years ahead. The inline comment
(`:126-129`) states the reasoning: the certificate is never verified, so the name
and the lifetime do not matter, and the encryption is what is wanted.

That reasoning is sound for what it covers. KD-5 is about what it does not cover.

### B-7. Every failure in this module is a panic, and one of them is silent

`server_config` `expect`s the PEM load (`:93`), and it is awaited in the caller's
own task (`start.rs:140`, `:226`), so a missing or malformed certificate file
panics out of `start_node_inner` rather than through the detached-task route
F-040 describes. The self-signed path `unwrap`s key generation, the `OnceLock`
set and the certificate build (`:102-105`, `:134`, `:141`). `into_tls_stream`
`expect`s the host name (`:177`). `from_env` `expect`s the per-variant override
(`:68`).

The exception is `HQL_TLS_AUTO_CERTS` (`:58-60`), which uses
`parse().unwrap_or(false)` and therefore treats any unparsable value as "off".
Two booleans read four lines apart in the same function disagree about whether a
typo is fatal. KD-2 records both halves; the panic half is executed.

## 4. Evidence and its limits

Six characterization tests: two in `tls.rs` and four assertions grouped into two
tests in `hiqlite/tests/tls_env.rs`. Each asserts current behavior and would fail
if that behavior changed. None is a regression test for a repair.

| test | what it establishes |
|---|---|
| `tls::tests::auto_certificates_always_disable_verification` | auto-certificates disable verification for whichever endpoint selects them; `Specific` starts verifying (B-1) |
| `tls::tests::the_tls_material_type_has_no_field_for_a_trust_anchor` | the material type carries a key, a chain and one boolean, and no CA (B-4) |
| `tls_env::from_env_branches_including_the_silent_downgrade` | every branch of `from_env`, including both silent downgrades (B-3, KD-1) and the unparsable `HQL_TLS_AUTO_CERTS` (KD-2) |
| `tls_env::a_malformed_no_verify_override_panics` | the per-variant override panics on the input the shared switch tolerates (KD-2) |

**What the tests do not establish.**

- **That any of this is what happens on a wire.** No TLS handshake was performed,
  no certificate was validated or rejected, and no connection was made. B-1's
  effect on a real peer, B-2's two mechanisms, B-4's empty root store and the
  whole of B-5 are **read from source and not executed**.
- **B-4's consequence.** That a default build cannot verify anything follows from
  `RootCertStore::empty()` plus a `cfg` and a feature list. The test reaches the
  material type's shape, not the store's contents: `ClientConfig` exposes no
  accessor for its trust anchors, so there is nothing to assert against. The
  acceptance block pins the `cfg` and the feature list instead.
- **B-5 entirely.** It is an argument-passing claim about five call sites in
  three files this spec does not own. The acceptance block pins each site's exact
  text; that proves the wiring still reads as described, not that a mismatched
  deployment fails the way KD-4 says.
- **Nothing about `reqwest`'s own defaults.** This spec states what hiqlite adds
  to the client and what it does not. What `reqwest` 0.13 trusts when given
  `rustls-no-provider` and no roots feature was not executed and is not claimed.
- **Nothing about the abort profile,** for the same reason `010` section 4 gives.
  KD-2's panic was observed under unwinding, in a test process that survived it.

## 5. Documenting `HQL_TLS_AUTO_CERTS`, and what the entry deliberately omits

`HQL_TLS_AUTO_CERTS` is documented in `hiqlite.toml:170-182` as the override for
`tls_auto_certificates` and appears in neither the env reference nor anywhere
else an operator would look. It is the single switch that turns verification off
on both endpoints at once, which makes it the one variable in this module whose
absence matters most. That is KD-6, and this spec closes it by adding it to
`hiqlite.env`.

The new entry states what the switch does and that clients do not verify the
resulting certificates. It **deliberately does not repeat** the justification
`hiqlite.toml:173-175` gives for that, "They do a 3-way handshake anyway, which
validates both client and server without the secret ever being sent over the
network", because KD-5 records that this is true of one channel and false of the
others. Copying it into a second reference file would propagate the claim; this
spec neither propagates nor corrects it, and says so here so the omission is not
mistaken for an oversight.

## 6. Known defects

Recorded as found, none repaired here. Each is also filed in
`standards/spec/findings-register.md`.

**KD-1. A half-configured endpoint downgrades silently** (F-041).
`from_env` (`tls.rs:71-82`) requires key **and** cert. If exactly one is set, the
other is not reported missing: the branch simply fails and the function falls
through. With `HQL_TLS_AUTO_CERTS` off the endpoint runs in **plaintext**; with
it on the endpoint runs with a self-signed certificate that no client verifies.
Either way an operator who misspelled one variable name gets a working node with
weaker transport than they configured, and no log line, warning or error says so.
`NodeConfig::is_valid` does not look at TLS material. Observed by execution, both
halves.

**KD-2. Two booleans four lines apart disagree about whether a typo is fatal**
(F-042). `HQL_TLS_AUTO_CERTS` (`tls.rs:58-60`) is `parse().unwrap_or(false)`, so
`HQL_TLS_AUTO_CERTS=ture` silently means "off", which via KD-1 can mean
plaintext. `HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY` (`:64-69`) is
`parse().expect(...)`, so the same class of typo ends the process. The rest of
the module is uniformly fatal: the PEM load (`:93`), key generation and the
`OnceLock` set (`:102-105`), certificate construction (`:134`, `:141`) and the
host name in `into_tls_stream` (`:177`). The PEM load is the likeliest operator
mistake and it panics out of `start_node_inner` directly rather than through a
detached task, so unlike F-040 it does fail startup, which is the better of the
two outcomes and is nowhere written down. The panic half is observed by
execution; the rest is source-established. Same class as F-009.

**KD-3. A verifying client has nothing to verify against** (F-043). Per B-4 the
root store is `RootCertStore::empty()` plus, only under the non-default
`webpki-roots` feature, the public web PKI bundle; and no API, variable or key
supplies a CA. Consequence: in a default build, `danger_tls_no_verify = false`
with `Specific` certificates is not "verification enabled", it is "verification
enabled against an empty trust store", which no peer certificate can satisfy.
With the feature enabled it is "verification against the public web PKI", which
an internally issued certificate also cannot satisfy. The only configuration in
which specific certificates and a working connection coexist is
`danger_tls_no_verify = true`, which is the flag named `danger`. This is
fail-closed, so it weakens nothing; what it does is leave the safe setting with
no reachable use. Source-established, with the material type's shape executed.

**KD-4. The API channel's no-verify flag is read from the raft configuration**
(F-044). Per B-5's table, four consumers reach a peer's `addr_api` and use three
different pairings. `start.rs:38-43` derives `tls_no_verify` from
`node_config.tls_raft` and `start.rs:254-284` hands it to
`become_cluster_member`, which uses it for both the scheme and the client against
`addr_api` (`init.rs:275-276`); the shutdown leave takes the scheme from
`tls_api` and the verification from `tls_raft` (`client/mgmt.rs:186-190`); and
`split_brain_check.rs:135` builds a default `reqwest::Client` that honours
neither. Only `store/mod.rs` uses the API configuration for the API endpoint.

Consequences, in order of how likely they are to be met. A cluster with TLS on
one endpoint and not the other has a join sequence that speaks the wrong scheme
to `addr_api` and cannot form. A cluster with auto-certificates has a split-brain
checker whose every request fails verification, so the observability path of
`010` B-8 reports connection errors on an interval instead of memberships. And
the API side's own `danger_tls_no_verify` is doubly dead: unreachable from TOML
(F-031) and ignored by two of the four consumers that would honour it.
Source-established; the acceptance block pins each site.

**KD-5. The stated reason for not verifying does not cover the endpoints that
send a bearer secret** (F-045). `tls.rs:20-23` and `hiqlite.toml:173-175` both
justify unverified certificates with a 3-way handshake that validates both
parties "without the secret ever being sent over the network". That describes
`HandshakeSecret` (`network/handshake.rs`, used at `network/api.rs:488` and
`network/raft_server.rs`), a challenge-response over the WebSocket channels, and
for those channels it is accurate.

It is not accurate for the REST endpoints. `validate_secret`
(`network/mod.rs:64-76`) compares the `X-API-SECRET` **request header** against
`secret_api`, and that header is set on every `/cluster/*`, `/listen` and
`/backup` request (`init.rs:186`, `:458`, `:583`, `:683`, `:742`,
`split_brain_check.rs:140`, `client/mgmt.rs:73`, `client/listen_notify.rs:57`).
Those requests carry the secret in cleartext inside a TLS session whose server
certificate, under auto-certificates or any `danger_tls_no_verify`, is not
checked. An attacker positioned to intercept the connection can present any
certificate and read `secret_api`, which is the credential for the whole
management surface.

Recorded as a contradiction between authored text and behavior, not as a
proposal. Whether the fix is to narrow the sentence, to verify the API channel,
or to move the REST endpoints onto the challenge-response is a security design
decision with three different costs. Source-established.

**KD-6. `HQL_TLS_AUTO_CERTS` is documented in one reference file only** (F-046),
found and closed in this change. Section 5 states what was added and what was
deliberately not.

**Retained without change.** F-031 (`009` KD-1: the TOML route re-reads the raft
key into the API variable, so the documented API key is both unreachable and
fatal to set) is in `009`'s unit and is cited here, not adopted. KD-4 is the
reason it matters more than `009` could say: the flag F-031 makes unreachable is
also the flag two of its four consumers ignore.

## 7. Resolved decisions

**D-1 (2026-09-21, nothing here is repaired, including KD-5).** KD-1 and KD-2
have one-line fixes and KD-4 has a four-line one. Each changes what a
misconfigured node does, which is W-22's decision, and KD-4's also changes
behavior inside `010`'s and `003`'s units. KD-5 is not a code change at all in
its cheapest form, but narrowing a security claim in two authored texts is still
a statement about what the product guarantees, and an adoption spec is the wrong
instrument for it. All six are recorded and left.

**D-2 (2026-09-21, the environment tests are their own target).** `hiqlite` is
`#![forbid(unsafe_code)]` (`lib.rs:4`), which edition 2024 makes fatal for
`env::set_var`, so these assertions cannot live in the library's unit tests at
all. `009` D-3 had already refused the environment route for the separate reason
that a process-wide variable races every other test in the binary. A dedicated
integration target answers both: the `forbid` does not apply to it, and the only
tests sharing its process are the two that cooperate by design. This closes the
evidence gap `009` D-3 left open for the TLS variables specifically; it does not
close it for the rest of the configuration surface.

**D-3 (2026-09-21, B-5 is described although its call sites are not claimed).**
Four of the five sites are in `010`'s units and one is in `003`'s. Claiming them
would mean an `extends` edge for a spec that modifies no line of either, and
`extends` is the edge for adding surface to a unit, not for reading it. The
finding is recorded against this spec because this is where the flag it is about
is defined, and the acceptance block pins the call sites by assertion so the
description cannot silently stop matching them.

**D-4 (2026-09-21, KD-5 is a contradiction and not a defect).** The register's
class test asks whether code fails to do what authored text says. Here the code
does exactly what it was written to do; an authored sentence describes a
narrower channel than the one it is printed next to. That is the same shape as
F-032 to F-034 and it is classed the same way, which also keeps the defect count
honest.

## 8. The inventory declarations this spec requires

Neither `hiqlite/src/tls.rs` nor `hiqlite/src/http_client.rs` is in any content
hash at the pinned revision, for the reason `010` section 8 gives: the existing
globs reach the query, client, network and sqlite state-machine trees but not
`hiqlite/src/*.rs`. `hiqlite/tests/tls_env.rs` is likewise outside
`hiqlite/tests/cluster/**/*.rs`. Without declarations, `spec-spine lint
--fail-on-warn` exits `1` with `L-008` warnings and `just spine-check` fails.

`spec-spine.toml` gains three `extra_hashed_inputs` entries:
`hiqlite/src/tls.rs`, `hiqlite/src/http_client.rs` and
`hiqlite/tests/tls_env.rs`. No `[coverage] governed_scope` entry is needed; all
three are inside the `hiqlite` cargo package the walk already counts.

**It is a freshness declaration, not an enforcement setting.**
`coupling.require_ownership`, `coupling.bypass_prefixes` and `index coverage
--fail-on-untraced` are untouched.

## 9. Out of scope

- **Every repair, and every security change.** KD-1 through KD-6 are recorded
  and left, F-031 is retained, and no control is tightened or loosened.
- **The startup-error policy.** W-22, which KD-2 joins.
- **`network/handshake.rs` and `network/challenge_response.rs`.** KD-5 names them
  to say what they cover; neither is claimed here, and the WebSocket
  authentication contract is unadopted.
- **`hiqlite/src/server/proxy/`.** It has its own `HEADER_NAME_SECRET` copy
  (`server/proxy/handlers.rs:21`) and its own TLS handling; W-11 owns it.
- **`reqwest`'s and `rustls`'s own defaults.** Named as pinned dependency
  behavior, not specified.
- **Ratification of anything, and any enforcement change.**

## Verification

Run with `just spine-verify 011`.

```verify:cli
test -f hiqlite/src/tls.rs
test -f hiqlite/src/http_client.rs
test -f hiqlite/tests/tls_env.rs
sh -c 'spec-spine index owner hiqlite/src/tls.rs | grep -q 011-transport-security-material'
sh -c 'spec-spine index owner hiqlite/src/http_client.rs | grep -q 011-transport-security-material'
sh -c 'spec-spine index owner hiqlite/tests/tls_env.rs | grep -q 011-transport-security-material'
sh -c 'spec-spine registry relationships 011-transport-security-material | grep -q 010-node-lifecycle-and-split-brain'
cargo test -p hiqlite --lib tls::tests::auto_certificates_always_disable_verification -- --exact
cargo test -p hiqlite --lib tls::tests::the_tls_material_type_has_no_field_for_a_trust_anchor -- --exact
cargo test -p hiqlite --test tls_env -- --test-threads=1
grep -q 'ServerTlsConfig::TlsAutoCertificates => true' hiqlite/src/tls.rs
grep -q 'danger_tls_no_verify: false' hiqlite/src/tls.rs
grep -q 'if key.is_some() && cert.is_some()' hiqlite/src/tls.rs
sh -c 'grep -A1 "env::var(\"HQL_TLS_AUTO_CERTS\")" hiqlite/src/tls.rs | grep -q "parse::<bool>().unwrap_or(false)"'
grep -q 'DANGER_TLS_NO_VERIFY to bool' hiqlite/src/tls.rs
grep -q 'RootCertStore::empty()' hiqlite/src/tls.rs
grep -q 'cfg(feature = "webpki-roots")' hiqlite/src/tls.rs
grep -q 'cfg(feature = "webpki-roots")' hiqlite/src/http_client.rs
grep -q 'tls_danger_accept_invalid_certs(tls_no_verify)' hiqlite/src/http_client.rs
sh -c '! sed -n "s/^default = //p" hiqlite/Cargo.toml | grep -q "webpki-roots"'
grep -q 'rustls-no-provider' Cargo.toml
sh -c 'grep -A1 "let tls_no_verify = node_config" hiqlite/src/start.rs | grep -q "\.tls_raft"'
grep -q 'let client = reqwest::Client::new();' hiqlite/src/split_brain_check.rs
grep -q 'let client = build_http_client(tls_no_verify);' hiqlite/src/init.rs
grep -q 'let scheme = if tls { "https" } else { "http" };' hiqlite/src/init.rs
grep -q 'node_config.tls_api.is_some(),' hiqlite/src/store/mod.rs
grep -q 'fn validate_secret' hiqlite/src/network/mod.rs
grep -q 'HEADER_NAME_SECRET' hiqlite/src/network/mod.rs
grep -q 'HandshakeSecret::server' hiqlite/src/network/api.rs
grep -q 'without the secret ever being' hiqlite/src/tls.rs
grep -q 'without the secret ever being' hiqlite.toml
grep -q 'HQL_TLS_AUTO_CERTS' hiqlite.env
sh -c '! grep -q "3-way handshake" hiqlite.env'
grep -q 'forbid(unsafe_code)' hiqlite/src/lib.rs
grep -q 'hiqlite/src/tls.rs' spec-spine.toml
grep -q 'hiqlite/src/http_client.rs' spec-spine.toml
grep -q 'hiqlite/tests/tls_env.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/011-transport-security-material'
```
