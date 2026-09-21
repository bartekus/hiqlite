---
id: "018-dashboard-service-and-ui"
title: "Adopt the dashboard service and its UI contract"
status: draft
kind: "adoption"
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "003-client-consistency-and-retry-outcomes"
  - "010-node-lifecycle-and-split-brain"
  - "011-transport-security-material"
  - "015-server-binary-and-proxy"
origin:
  retroactive: true
  paths:
    - "hiqlite/src/dashboard/"
establishes:
  - "hiqlite/src/dashboard/mod.rs"
  - "hiqlite/src/dashboard/handlers.rs"
  - "hiqlite/src/dashboard/middleware.rs"
  - "hiqlite/src/dashboard/password.rs"
  - "hiqlite/src/dashboard/query.rs"
  - "hiqlite/src/dashboard/session.rs"
  - "hiqlite/src/dashboard/static_files.rs"
  - "hiqlite/src/dashboard/table.rs"
  - "dashboard/src/lib/utils/fetch.ts"
  - "dashboard/src/lib/stores/session.ts"
  - "dashboard/src/lib/types/session.ts"
  - "dashboard/src/lib/types/error.ts"
  - "dashboard/src/lib/types/query.ts"
  - "dashboard/src/lib/types/query_results.ts"
  - "dashboard/src/lib/types/raft_metrics.ts"
  - "dashboard/src/lib/types/table.ts"
  - "dashboard/src/lib/components/Login.svelte"
  - "dashboard/src/routes/+page.svelte"
  - "dashboard/tests/smoke.spec.ts"
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
  Adopts the dashboard's authorization boundary as one contract across both
  sides: the six API routes, the session cookie, the login cooldown and
  proof-of-work, the SQL guard, and the browser code that is the only client of
  any of it. Nine findings, five executed, including a single-flight lock that
  is dropped before it guards anything and an unauthenticated fallback that
  panics on a multi-byte path. Repairs no runtime behavior.
---

# 018: Adopt the dashboard service and its UI contract

## 1. Purpose

The dashboard is the one surface in hiqlite where a human password, rather than
a shared secret, authorizes arbitrary SQL against the cluster. Its boundary is
spread across eight Rust files and a Svelte application, and F-019 records that
none of it is executed by CI: the `dashboard` feature is linted and never tested.

This spec states the boundary in one place, on both sides, and records what
reading it carefully turned up. Five of the nine findings are executed, and three
of those were found by writing the first test the file had ever had, or by
running the ones it already had eight times.

**It is an adoption.** Nothing is repaired. The only changes to shipped files are
two `#[cfg(test)]` modules.

## 2. Territory

**Establishes** the eight files of `hiqlite/src/dashboard/`, and the eleven
browser files that carry the contract with them: the fetch layer, the session
store and the six API type definitions, the login component, the page that
consumes the session, and the one test the UI has.

**Does not claim** the remaining browser files, which are presentational
components (`Button.svelte`, `Pagination.svelte`, and so on), the vendored
`spow` WASM bindings, or the build configuration. They carry no contract with
the service: a change to any of them cannot make the boundary in section 3 wrong.
The build artifact is W-13's and `hiqlite/static` is claimed by nobody yet. D-3
records this and it is deliberate migration state, not an oversight.

**Extends**, without re-establishing: `000` on `spec-spine.toml`; `005` on the
findings register and the adoption plan.

**Depends on** `003` for the client stream a dashboard write travels on; `010`
for the router the six routes are nested into (`start.rs:186-218`); `011`,
because `is_api_tls_enabled` is read from `tls_api` and decides whether a
proof-of-work is required; and `015`, which owns the `generate-config` command
that produces the password hash.

## 3. Behavior

### B-1. Six routes, one fallback, and what each requires

`start.rs:189-217` mounts the dashboard **only if**
`state.dashboard.password_dashboard.is_some()`. With no password configured the
entire tree, including `/` and the static fallback, does not exist. That is the
first and strongest control, and `DashboardState::from_env` (`mod.rs:38-53`)
warns when it takes that branch.

| route | method | authorization |
|---|---|---|
| `/dashboard/api/session` | `POST` | none; this **is** the login. CSRF check, then cooldown, then proof-of-work if TLS, then argon2 |
| `/dashboard/api/pow` | `GET` | none. Issues a challenge at difficulty 20, or 10 when `HQL_INSECURE_COOKIE` is set |
| `/dashboard/api/session` | `GET` | `Session` extractor |
| `/dashboard/api/metrics` | `GET` | `Session` extractor |
| `/dashboard/api/tables`, `/tables/{filter}` | `GET` | `Session` extractor |
| `/dashboard/api/query` | `POST` | `Session` extractor, then B-4's guard |
| `/dashboard/*` fallback | `GET` | **none**. `static_files::handler`, KD-2 |

`Session` is an axum extractor (`session.rs:82-104`), so authorization happens
during extraction: it runs `check_csrf` and then decrypts the cookie. A handler
that takes `_: Session` cannot forget to check.

The middleware stack (`middleware.rs:27-59`) applies to the whole `/dashboard`
nest: `Authorization`, `Cookie` and `X-API-SECRET` are marked sensitive so they
are not logged, and five response headers are set, including
`frame-ancestors 'none'` and `X-Frame-Options: SAMEORIGIN`.

### B-2. The session is an encrypted timestamp pair and nothing else

`Session` is `{ created, expires }` (`session.rs:76-80`), serialized, encrypted
with the process's active `EncKeys`, base64-encoded, and set as a cookie named
`__Host-Hiqlite-Session`, or `Hiqlite-Session` without `Secure` and without
`Path=/` when `HQL_INSECURE_COOKIE` is on (`:113-129`). Lifetime is 3600 seconds
and validity is `expires < now` (`:157-164`).

There is no identity in it, no nonce, no server-side record, no rotation and no
logout route. Anyone holding the cookie is authorized until it expires, and the
only way to revoke every live session is to rotate the encryption keys. KD-8.

`check_csrf` (`:234-284`) is the other half of the extractor: `sec-fetch-site:
same-origin` always passes; for `GET`, `none` passes, an image fetch with
`no-cors` passes, and a `navigate` that is not an embed passes; everything else
is rejected. A **missing** `sec-fetch-site` is rejected unless
`HQL_INSECURE_COOKIE` is set, which is stated in the code as the plain-HTTP
allowance.

### B-3. Login is cooldown, then optional proof-of-work, then argon2

`post_session` (`handlers.rs:41-52`) validates a proof-of-work **only when the
API listens on TLS** (`:57-62`, `mod.rs:14-22`), because the WASM solver needs a
secure context; the browser mirrors that with `window.isSecureContext`
(`Login.svelte:30`). Over plain HTTP the `pow` field defaults to empty and is
ignored.

`set_session_verify` (`session.rs:167-191`) then checks a **global** cooldown
(`:38-54`): after any failed login, every login is rejected with `429` and a
`Retry-After` for five seconds. The authored comment explains the choice as
having "no per-client state to spoof or exhaust". KD-4 is what that reasoning
does not cover.

`verify_password` (`password.rs:12-23`) is argon2id, `m=32768, t=2, p=2,
out=32`, on a blocking task, opening with what is meant to be a single-flight
lock. KD-1 is that the lock is not held.

### B-4. Dashboard SQL is classified by the first seven bytes

`dashboard_query_dynamic` (`query.rs:10-103`) rejects anything shorter than 8
bytes, lowercases `sql[..7]`, and treats the statement as a read if that prefix
starts with `select`, `explain` or `pragma`. A read runs locally on the read
pool. Anything else goes through the Raft as a write, after
`find_forbidden_non_det_fn` (`:161-218`) scans for a non-deterministic function
call outside string literals and comments, which is a real parser for that
narrow job and has its own test.

Two consequences the classification has: KD-3, the slice, and KD-5, what counts
as a read.

### B-5. The browser is the only client, and it holds no credential

`fetch.ts` prefixes every call with `/dashboard/api`, sends no headers, and
relies entirely on the cookie. `handleRes` (`:35-40`) clears the session store on
any `401`, which is the whole of the client's session handling: `session.ts` is a
three-line writable store. The six type files mirror the service's JSON exactly,
`ISession` being `{ created, expires }`, the same two fields B-2 describes.

The login form posts `application/x-www-form-urlencoded` to the same path the
server serves, adding `pow` only in a secure context.

## 4. Evidence and its limits

Thirteen tests, five added here, all requiring `--features dashboard`, which is
the feature F-019 says CI never enables.

| test | what it establishes |
|---|---|
| `password::tests::the_single_flight_lock_is_released_before_any_hashing` | KD-1, by acquiring the lock exactly as `verify_password` does and then acquiring it again |
| `password::tests::the_hasher_is_argon2id_with_recorded_parameters` | the four argon2 parameters (B-3) |
| `static_files::tests::a_known_asset_is_served_with_its_cache_headers` | the embedded-asset path and its cache header |
| `static_files::tests::an_unknown_asset_is_a_plain_404` | the miss path |
| `static_files::tests::a_multibyte_path_panics_the_fallback` | KD-2, by executing it |
| `session::tests::*` (3, pre-existing) | the cooldown lock, the `429`, and the `Retry-After` value |
| `handlers::tests::*` (3, pre-existing) | the proof-of-work round trip, the `pow` default, and that it is checked only under TLS |
| `query::tests::forbidden_fn_scan_catches_only_real_calls` (pre-existing) | the SQL guard's literal and comment handling |
| `dashboard::tests::api_tls_flag_set_and_read` (pre-existing) | the TLS flag |

**What is not established.**

- **Any route, end to end.** No router was built, no request was served, and no
  cookie was round-tripped through a real response. B-1's table is read from
  `start.rs` and the handler signatures. The `Session` extractor, `check_csrf`
  and the middleware stack are **not executed by any test**.
- **KD-3, KD-4, KD-5, KD-6 and KD-8**, which are source-established. KD-3 needs an
  `AppStateExt`; KD-4 is an availability argument, not a behavior a unit test
  asserts; KD-5 needs a live Raft.
- **The browser side entirely.** The one test the UI has is a Playwright smoke
  test that loads the login page against a preview server with no backend, and
  KD-7 records that nothing runs it either.
- **Encryption.** `EncValue` is `cryptr`'s and is named as pinned dependency
  behavior, not specified.
- **That the pre-existing cooldown tests pass reliably.** They do not, under the
  default parallel harness. KD-9, and the acceptance block runs that group with
  `--test-threads=1` so this spec's own verification is not itself flaky.

## 5. Known defects

Recorded as found, none repaired. Each is also filed in
`standards/spec/findings-register.md`.

**KD-1. The single-flight hashing lock is dropped before it guards anything**
(F-084). `password.rs:13` is `let _ = IS_HASHING.write().await;`. A `_` pattern
drops its value at the end of the statement, so the write guard is released
immediately and the hashing that follows is not serialized. The comment three
lines above states the opposite, "only a single password hash at a time is
allowed ... prevents brute-fore effectively", and `session.rs:36-37` relies on it
in writing, "the single-flight lock in `password::verify_password` still
serializes the actual hashing".

Consequence: concurrent login attempts each start their own argon2 at
`m=32768, t=2, p=2`, so N simultaneous requests cost N times 32 MiB and N hashing
threads. The control that was supposed to bound that does nothing, and the
cooldown KD-4 describes only engages **after** a failure has been computed, so
the first burst is unbounded. **Observed by execution.** The fix is `let _guard =`.

**KD-2. The unauthenticated fallback panics on a multi-byte path** (F-085).
`static_files.rs:28`: `let path_ending = &path[path.len().saturating_sub(4)..];`.
That slices at a byte index without asking whether it is a character boundary.
`http::Uri` accepts raw UTF-8 in a path, so `GET /dashboard/\u{20ac}abc` reaches
`static_files::handler`, which is the `/dashboard` **fallback** and therefore
requires no session, and panics with `byte index 2 is not a char boundary`.

Consequence: any client that can reach the API port of a node with the dashboard
enabled can panic a request task without authenticating. Under unwinding the
connection dies; under `panic = abort` the node does, which is `010` B-7's split.
**Observed by execution.**

**KD-3. The same mistake in the query classifier, behind the session**
(F-086). `query.rs:24`: `let sql_start = sql[..7].to_lowercase();`, guarded at
`:14` by `sql.len() < 8`. The guard stops the out-of-range case and not the
boundary one: a statement whose seventh byte falls inside a multi-byte character,
for example `aaaaaa\u{20ac}`, panics. `post_query` builds the string with
`String::from_utf8_lossy` over the raw body (`handlers.rs:112`), so the input is
whatever was sent. This one is behind the `Session` extractor, so the caller must
be logged in; it is recorded because it is the same defect as KD-2 in the same
module, which is what makes it a pattern rather than a slip.
Source-established.

**KD-4. The global login cooldown is a denial of service against the operator**
(F-087). `session.rs:33-54` and `:174-183`. After any failed password, **every**
login is rejected for five seconds, with no client identity involved. The
authored justification is that "there is no per-client state to spoof or
exhaust".

That is true and it is not the exposure. The global lock is itself the
exhaustible resource: an unauthenticated client that sends one wrong password
every five seconds keeps the dashboard permanently unloginable for the real
operator, at a cost of one request per five seconds and with no authentication.
The cost asymmetry runs the wrong way, because the attacker's request is rejected
at `:175` before any hashing while the operator is locked out.

Recorded as a defect rather than a design preference because the code states its
own threat model in a comment and the stated model does not cover this.
Source-established; no availability test was written.

**KD-5. A read that does not start with the three keywords is replicated as a
write** (F-088). B-4. The classifier looks only at the first seven bytes, so
`WITH x AS (SELECT 1) SELECT * FROM x`, `VALUES (1)`, and any statement preceded
by a comment such as `/* note */ SELECT 1` are classified as writes and sent
through `client_write` to be applied on every node. Consequence: a read issued
from the dashboard can take the write path, occupy the Raft, and be rejected by
the non-deterministic-function guard for containing a function that would have
been fine in a read. Nothing is corrupted; a read is charged as a cluster-wide
write. Source-established.

**KD-6. A malformed dashboard password ends the process at startup** (F-089).
`mod.rs:41`: `String::from_utf8(b64_decode(&b64).unwrap()).unwrap()`. A
`HQL_PASSWORD_DASHBOARD` that is not base64, or that decodes to non-UTF-8,
panics inside `DashboardState::from_env`, which runs on the configuration path.
The `Err` arm four lines below handles the variable being **absent** gracefully,
by disabling the dashboard with a warning, so the two adjacent cases are handled
in opposite ways. Same class as F-009, F-042 and F-061, and part of W-22.
Source-established.

**KD-7. The UI has one test and nothing runs it** (F-090).
`dashboard/tests/smoke.spec.ts` loads the login page against a preview server
with no backend and asserts it hydrates without page errors, which is a
reasonable smoke test. `package.json` exposes it as `npm run test:smoke`, and
that string appears **nowhere else**: no `just` recipe, no workflow step. The
`justfile` touches `dashboard` only to build it. Consequence: the dashboard's
single automated check is opt-in and, on the evidence of the repository, opted
out of. Same shape as F-081 for the examples. Source-established.

**KD-8. A session cannot be revoked** (F-091). B-2. The cookie carries only two
timestamps, there is no server-side session record, no logout route and no
rotation. Consequences: changing `HQL_PASSWORD_DASHBOARD` does not invalidate a
live session; a leaked cookie is valid for up to an hour and cannot be withdrawn;
and the only revocation available is rotating the encryption keys, which
invalidates every encrypted value in the process, not only sessions. Classed as a
limit: a stateless one-hour session on an ops surface is a defensible design, and
what is missing is the statement of what it costs. Source-established.

**KD-9. The three cooldown tests race each other** (F-095). `session.rs:197-231`
are three pre-existing tests over one process-global `NEXT_LOGIN_ALLOWED`
(`:39`), run by libtest in parallel threads by default. `cooldown_locks_and_unlocks`
unlocks and asserts unlocked while `cooldown_response_reports_remaining_wait`
locks, so their assertions contradict each other whenever they interleave.

**Observed by execution**: eight consecutive runs of
`cargo test --features dashboard --lib dashboard::session::tests` produced three
failures, once with two of the three failing. Consequence: enabling the
`dashboard` feature in CI, which F-019 and several findings here point towards,
introduces a flaky test on the first day. This spec's acceptance block passes
`--test-threads=1` for that group, which is correct for an acceptance block and
is **not** a fix: `cargo test` does not. Recorded rather than repaired, because
serializing them is a change to tests this adoption did not write and the right
fix is a per-test lock or a reset fixture, which is a small design choice.

**Retained without change.** F-019, which records that the `dashboard` feature is
never enabled for a CI test run. KD-2, KD-7 and KD-9 are three more consequences
of it.

## 6. Resolved decisions

**D-1 (2026-09-21, nothing here is repaired, and KD-1 and KD-2 are one-line
fixes).** Both are tempting: `let _guard =` and a `char_indices` bound. Both are
also security-relevant behavior changes to a surface with no end-to-end test at
all, where the next change should be the one that enables the `dashboard`
feature in CI so a fix can be regression-tested. KD-4 is a policy question with
at least three answers, per-client cooldown, exponential backoff, or accepting
the exposure, and choosing among them is not an adoption's to do.

**D-2 (2026-09-21, the two panics are asserted as panics).**
`a_multibyte_path_panics_the_fallback` is `#[should_panic]` with the exact
message. It is a characterization test that a repair is expected to invert, the
same shape `013` D-2 records.

**D-3 (2026-09-21, the presentational browser files are not claimed).** Eleven of
the browser files carry the contract: the fetch layer, the session store, the six
API types, the login form, the page that reads the session, and the smoke test. A
change to `Pagination.svelte` cannot make section 3 wrong, and claiming forty
files to say so would put ownership where no statement lives. The rest stays
migration debt, and W-13 owns the built artifact.

**D-4 (2026-09-21, `/stream`-style handshake authentication is not proposed for
the fallback).** KD-2 is a panic, not an authorization hole: serving the login
page's assets without a session is required for the login page to load. The
finding is the slice, and it is stated that way so the fix is bounded.

## 7. The inventory declarations this spec requires

`hiqlite/src/dashboard/**/*.rs` is in no content hash at the pinned revision: the
existing globs reach `query/`, `client/`, `network/`, `server/` and the sqlite
state machine, and not this tree. The eight files are already in the coverage
denominator through the `hiqlite` package walk.

The browser files are the opposite case. `dashboard/src/**/*.ts` and
`dashboard/src/**/*.svelte` are already hashed and already in `governed_scope`,
declared by the pilot's dashboard block, and `dashboard/tests/**/*.ts` with them.
So the eleven claimed browser files need **no** new declaration, which is worth
saying because it is the first time the two sides of a claim have needed
different treatment.

`spec-spine.toml` gains one glob, `hiqlite/src/dashboard/**/*.rs`.

**It is a freshness declaration, not an enforcement setting.**

## 8. Out of scope

- **Every repair.** KD-1 to KD-8 are recorded and left.
- **The built dashboard artifact** in `hiqlite/static`, its determinism and its
  drift check. W-13, F-012.
- **Enabling the `dashboard` feature in CI**, and running the smoke test. F-019,
  W-19.
- **`cryptr`'s encryption and `spow`'s proof-of-work**, named as pinned
  dependency behavior.
- **The presentational browser components and the vendored WASM bindings.** D-3.
- **The startup-error policy.** W-22, which KD-6 joins.
- **Ratification, enforcement, and any tool or pin change.**

## Verification

Run with `just spine-verify 018`.

```verify:cli
test -f hiqlite/src/dashboard/session.rs
test -f dashboard/src/lib/utils/fetch.ts
test -f dashboard/tests/smoke.spec.ts
sh -c 'spec-spine index owner hiqlite/src/dashboard/password.rs | grep -q 018-dashboard-service-and-ui'
sh -c 'spec-spine index owner hiqlite/src/dashboard/static_files.rs | grep -q 018-dashboard-service-and-ui'
sh -c 'spec-spine index owner dashboard/src/lib/utils/fetch.ts | grep -q 018-dashboard-service-and-ui'
sh -c 'spec-spine index owner dashboard/tests/smoke.spec.ts | grep -q 018-dashboard-service-and-ui'
sh -c 'spec-spine registry relationships 018-dashboard-service-and-ui | grep -q 015-server-binary-and-proxy'
cargo test -p hiqlite --features dashboard --lib dashboard::password::tests::the_single_flight_lock_is_released_before_any_hashing -- --exact
cargo test -p hiqlite --features dashboard --lib dashboard::password::tests::the_hasher_is_argon2id_with_recorded_parameters -- --exact
cargo test -p hiqlite --features dashboard --lib dashboard::static_files::tests::a_multibyte_path_panics_the_fallback -- --exact
cargo test -p hiqlite --features dashboard --lib dashboard::static_files::tests::a_known_asset_is_served_with_its_cache_headers -- --exact
cargo test -p hiqlite --features dashboard --lib dashboard::static_files::tests::an_unknown_asset_is_a_plain_404 -- --exact
cargo test -p hiqlite --features dashboard --lib dashboard::query::tests::forbidden_fn_scan_catches_only_real_calls -- --exact
cargo test -p hiqlite --features dashboard --lib dashboard::session::tests -- --test-threads=1
cargo test -p hiqlite --features dashboard --lib dashboard::handlers::tests
sh -c 'grep -q "let _ = IS_HASHING.write().await;" hiqlite/src/dashboard/password.rs'
grep -q 'prevents brute-fore effectively' hiqlite/src/dashboard/password.rs
grep -q 'the single-flight lock in `password::verify_password`' hiqlite/src/dashboard/session.rs
sh -c 'grep -q "let path_ending = &path\[path.len().saturating_sub(4)..\];" hiqlite/src/dashboard/static_files.rs'
sh -c 'grep -q "let sql_start = sql\[..7\].to_lowercase();" hiqlite/src/dashboard/query.rs'
sh -c 'grep -q "if sql.len() < 8 {" hiqlite/src/dashboard/query.rs'
grep -q 'There is no per-client state' hiqlite/src/dashboard/session.rs
grep -q 'static NEXT_LOGIN_ALLOWED: Mutex<Option<Instant>> = Mutex::new(None);' hiqlite/src/dashboard/session.rs
sh -c 'grep -q "String::from_utf8(b64_decode(&b64).unwrap()).unwrap()" hiqlite/src/dashboard/mod.rs'
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
```
