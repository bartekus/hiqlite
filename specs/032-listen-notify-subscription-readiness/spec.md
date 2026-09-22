---
id: "032-listen-notify-subscription-readiness"
title: "A remote client is subscribed to the event bus before it is handed back"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "003-client-consistency-and-retry-outcomes"
  - "006-cache-state-machine"
  - "012-cluster-integration-evidence"
  - "031-downstream-release-qualification"
amends: ["031-downstream-release-qualification"]
# D-2: `031`'s block is `003`'s and `012`'s acceptance. This repair supersedes exactly one of
# its commands, so it takes the whole block rather than editing a line of a predecessor's
# acceptance to match new code.
amends_verification: ["031-downstream-release-qualification"]
extends:
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/client/" }
    nature: superseding
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/network/" }
    nature: superseding
  - spec: "006-cache-state-machine"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/memory/" }
    nature: superseding
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
summary: >
  Repairs F-051. A remote client's event subscription was established by a
  detached task that nobody waited for, so `Client::remote` returned before the
  server had registered the listener and an event published in that window was
  delivered to no one, leaving `listen()` waiting forever. The server now
  acknowledges the registration before it answers the HTTP request, and the
  client waits for that acknowledgement, bounded, before it returns. This is
  what the repository's full-suite gate was stalled on.
---

# 032: A remote client is subscribed to the event bus before it is handed back

## 1. Purpose

F-051 records the defect and F-017 records its consequence: the cluster
integration test never completed, and stopped at `Test Listen / Notify with
remote clients`. That is not a flake. The ordering in `remote_only.rs` is
deterministic, and the losing window was measured at 175 ms.

Two independent gaps produced it, and closing either alone leaves a race:

- **The server answered before it had subscribed the caller.** `api::listen`
  put a `NotifyRequest::Listen` on an unbounded channel and returned the SSE
  response. A send completes when the message is buffered, not when the handler
  has processed it, so the response head could reach the client while the
  listener list was still empty.
- **The client returned before the stream existed.** `RemoteListener::spawn`
  spawned a task and handed back the receiver. `Client::remote` awaited nothing,
  so a caller that published an event on the next line raced a connection that
  had not been made.

One responsibility: **a remote client that returns is a remote client that is
subscribed.**

## 2. Territory

**Extends**, all with nature `superseding`: `003`'s `hiqlite/src/client/` and
`hiqlite/src/network/` directory units, and `006`'s
`hiqlite/src/store/state_machine/memory/` unit for `notify_handler.rs`. The
surface is disjoint from what `022`, `023`, `027` and `029` changed in those
same units.

**Amends** `031` and carries its acceptance (D-2), which is `003`'s and `012`'s
through `031`.

**Ownership boundary.** This is a subscription-establishment repair, not a
delivery guarantee. Nothing here makes the event bus at-least-once, replayable,
or ordered against the SQL log. `006` KD-1 stands.

## 3. Behavior

### B-1. The subscription acknowledges itself

`NotifyRequest::Listen` carries a `oneshot::Sender<()>` alongside the event
sender. The handler pushes the listener onto the list and **then** sends the
acknowledgement, so the acknowledgement is proof of registration and not proof
of receipt of the request.

`api::listen` awaits that acknowledgement before it returns the SSE response. A
client that sees the stream open is therefore a client the handler will send to.

If the acknowledgement channel is closed because the caller went away, the
handler warns and carries on. It serves every listener on the node, and no
single caller may take it down: the same rule as `023` B-1.

### B-2. The client waits for the stream, bounded

`RemoteListener::spawn` returns the event receiver together with a readiness
receiver, signalled on the **first** `SSE::Connected` and not on later
reconnects, which `Client::remote` has long returned before.

`Client::remote` awaits readiness for at most `READY_TIMEOUT`, ten seconds.

### B-3. A timeout is a warning, not a failure

If readiness does not arrive in that window, construction still succeeds and the
caller is warned, in terms that say what it costs: the stream keeps retrying,
and events published before it connects are not delivered.

This is deliberate. The listener has always had a reconnect loop, which is the
existing design's statement that the server may not be reachable when the client
is built; a proxy-mode client and a client built during a rolling restart are
both ordinary. Turning that into a construction error would break callers who
are not using `listen()` at all, to buy a guarantee that B-1 and B-2 already
give in every case where the server is up.

## 4. Evidence and its limits

Three tests in `notify_handler.rs`:

- an acknowledged subscription receives an event published immediately
  afterwards;
- a caller that abandons its acknowledgement never kills the handler, proven by
  a second subscriber still being served afterwards;
- every acknowledged subscriber receives the same event.

Each waits on the acknowledgement or on the event itself, under a
`tokio::time::timeout`. Nothing here sleeps to let a state arrive.

**None of the three can be run against the unrepaired code, and they are not
offered as if they could be.** The acknowledgement did not exist before this
spec, so the enum they construct did not either. The failing observation is the
one that already stood: the cluster test's non-completion, recorded as F-017 and
diagnosed as F-051. The repair's evidence is that the same test now runs to
completion; before it, it did not, under the same command.

What the acceptance does **not** establish:

- **The client-side readiness wait is not unit tested.** It needs a listening
  server, which is the cluster test's job and not a unit's.
- **The timeout branch of B-3 is not exercised.** Reaching it deterministically
  means a server that accepts a connection and never answers, which is a
  fixture this corpus does not have.
- **No claim about delivery.** An event published while a client is between
  reconnects is still lost. B-3 names that, `006` KD-1 owns it.
- **One subscription per client.** Nothing tests many remote clients
  subscribing concurrently.

## 5. Known defects

**KD-1. The window is closed at construction, not reopened-safe.** After a
disconnect, the listener reconnects and re-subscribes, and events published
between the disconnect and the new registration are delivered to nobody. The
readiness signal is deliberately first-connection-only, so nothing tells the
caller that this happened. This is the same defect as F-051, on a path that has
no equivalent of `Client::remote` to block.

**KD-2. `READY_TIMEOUT` is a constant.** Ten seconds, not configurable. A very
slow first connection therefore produces a warning and a client whose early
events are lost, exactly as before this repair.

## 6. Resolved decisions

**D-1 (2026-09-21, acknowledge from the handler rather than making the channel
bounded).** Sizing the request channel at zero would also make the send wait for
the receive, but it would make every other `NotifyRequest::Notify` wait for the
handler too, turning the notify path into a synchronous one. The
acknowledgement costs one `oneshot` per subscription, which happens once per
client.

**D-2 (2026-09-21, this block is `031`'s acceptance).** Which is `003`'s and
`012`'s through `031`. All of `031`'s commands are carried forward, one of them
superseded and marked as such: `Client::remote` no longer binds the receiver
alone. Editing that line inside `031` would have been changing a live acceptance
to match new code, which the mechanism exists to prevent.

**D-3 (2026-09-21, a timeout warns instead of failing).** Section B-3 gives the
reason. The alternative, a `Error::Connect` from `Client::remote`, was declined
because it changes the outcome for callers who never call `listen()`.

## 7. Out of scope

- **Event delivery semantics, replay, and ordering.** `006`.
- **The local (`listen_notify_local`) path**, which shares the handler and has
  no HTTP or SSE stage, so no window to close.
- **The SSE stream's TLS verification.** The `TODO what about tls_no_verify`
  comment is untouched; `030` KD-2 covers the remote client's trust anchor.
- **Ratification, enforcement, publication and release.** `031`.

## Verification

Run with `just spine-verify 032`. **This block is `031`'s acceptance as well as
this spec's** (D-2), and `031`'s is `003`'s and `012`'s, so `spec-spine verify
003`, `verify 012` and `verify 031` all resolve here.

```verify:cli
# --- 003's acceptance, carried forward with the package name corrected and
# nothing else changed ---
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite client::stream::tests::reconnect_buffer_delivers_a_late_response_during_its_window -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite client::stream::tests::reconnect_buffer_expiry_reports_an_ambiguous_timeout -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features external-state-machine external_state_machine::tests::dense_frontier_exact_retries_conflicts_and_rollback -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features external-state-machine external_state_machine::tests::oversized_receipt_rolls_back_operation_and_frontier -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features external-state-machine external_state_machine::tests::snapshot_evidence_restore_receipts_and_staleness -- --exact

# --- 012's acceptance, carried forward with the package name corrected and
# nothing else changed ---
test -f hiqlite/tests/cluster/main.rs
test -f hiqlite/tests/cluster/learner_only.rs
sh -c 'spec-spine index owner hiqlite/tests/cluster/main.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine index owner hiqlite/tests/cluster/remote_only.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine index owner hiqlite/tests/cluster/learner_only.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine index owner hiqlite/tests/cluster/backup_restore.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine index owner hiqlite/tests/cluster/self_heal.rs | grep -q 002-snapshot-publication-and-recovery'
sh -c 'spec-spine registry relationships 012-cluster-integration-evidence | grep -q 002-snapshot-publication-and-recovery'
sh -c 'cargo test -p hiqlite-patched --features cache,counters,dlock,listen_notify,macros,toml,external-state-machine --test cluster -- --list | grep -q "^test_cluster: test$"'
sh -c 'cargo test -p hiqlite-patched --features cache,counters,dlock,listen_notify,macros,toml,external-state-machine --test cluster -- --list | grep -q "^learner_only::learner_only_node_stays_non_voter_and_becomes_ready: test$"'
sh -c 'test "$(cargo test -p hiqlite-patched --features cache,counters,dlock,listen_notify,macros,toml,external-state-machine --test cluster -- --list | grep -c ": test$")" = "2"'
grep -q 'process::exit(0);' hiqlite/tests/cluster/main.rs
grep -q 'TODO sometimes the test gets stuck here' hiqlite/tests/cluster/main.rs
grep -q 'process::exit(1);' hiqlite/tests/cluster/main.rs
grep -q 'TODO if this next action comes too fast, there will be a WAL log ID mismatch' hiqlite/tests/cluster/main.rs
grep -q 'TEST_SKIP_S3_RESTORE' hiqlite/tests/cluster/main.rs
grep -q 'config.tls_raft = None;' hiqlite/tests/cluster/start.rs
grep -q 'config.tls_api = None;' hiqlite/tests/cluster/start.rs
grep -q 'config.cache_storage_disk = false;' hiqlite/tests/cluster/start.rs
grep -q 'config.wal_size = 8 \* 1024;' hiqlite/tests/cluster/start.rs
sh -c 'grep -A2 "for i in 1..=3 {" hiqlite/tests/cluster/start.rs | grep -q "loop {"'
grep -q 'pub async fn wait_until_healthy_db' hiqlite/src/client/mgmt.rs
grep -q 'config.learner_only = node_id == 3;' hiqlite/tests/cluster/learner_only.rs
grep -q 'for _ in 0..30 {' hiqlite/tests/cluster/learner_only.rs
grep -q 'env::set_var("HQL_BACKUP_SKIP_VALIDATION", "true");' hiqlite/tests/cluster/backup_restore.rs
grep -q 'unsafe { env::remove_var("HQL_BACKUP_RESTORE") };' hiqlite/tests/cluster/main.rs
grep -q '// #\[derive(rust_embed::Embed)\]' hiqlite/tests/cluster/migration.rs
grep -q '// #\[folder = "tests/cluster/migrations/bad_1"\]' hiqlite/tests/cluster/migration.rs
grep -q '// #\[folder = "tests/cluster/migrations/bad_2"\]' hiqlite/tests/cluster/migration.rs
test -f hiqlite/tests/cluster/migrations/bad_1/no_leading_index.sql
test -f hiqlite/tests/cluster/migrations/bad_2/2_bad_start_index.sql
# D-2: the one command of `031`'s that this repair supersedes. `Client::remote` no longer
# takes the receiver alone, because it now waits for the subscription before returning.
sh -c 'grep -q "RemoteListener::spawn(leader_cache.clone(), tls, api_secret.clone())" hiqlite/src/client/create.rs'
sh -c 'grep -A2 "let (tx, rx) = flume::unbounded();" hiqlite/src/client/listen_notify.rs | grep -q "task::spawn(Self::handler"'
grep -q 'Connecting to listen SSE stream' hiqlite/src/client/listen_notify.rs
grep -q 'self.listen_rx().recv_async().await' hiqlite/src/client/listen_notify.rs
grep -q 'hiqlite/tests/cluster/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/012-cluster-integration-evidence'
# --- what `031` adds, unchanged ---
# the three packages are renamed and the three libraries are not
sh -c 'grep -q "^name = \"hiqlite-patched\"" hiqlite/Cargo.toml'
sh -c 'grep -q "^name = \"hiqlite-wal-patched\"" hiqlite-wal/Cargo.toml'
sh -c 'grep -q "^name = \"hiqlite-derive-patched\"" hiqlite-derive/Cargo.toml'
sh -c 'grep -A5 "^\[lib\]" hiqlite/Cargo.toml | grep -q "^name = \"hiqlite\"$"'
sh -c 'grep -A5 "^\[lib\]" hiqlite-wal/Cargo.toml | grep -q "^name = \"hiqlite_wal\"$"'
sh -c 'grep -A5 "^\[lib\]" hiqlite-derive/Cargo.toml | grep -q "^name = \"hiqlite_derive\"$"'
# one version, and it says what it is
sh -c 'test "$(grep -h "^version = " hiqlite/Cargo.toml hiqlite-wal/Cargo.toml hiqlite-derive/Cargo.toml | sort -u | wc -l | tr -d " ")" = "1"'
sh -c 'grep -q "^version = \"0.15.0-patched.1\"" hiqlite/Cargo.toml'
# the internal dependencies keep their keys, so the feature table needs no change
sh -c 'grep -q "hiqlite-wal = { package = \"hiqlite-wal-patched\", version = " hiqlite/Cargo.toml'
sh -c 'grep -q "hiqlite-derive = { package = \"hiqlite-derive-patched\", version = " hiqlite/Cargo.toml'
sh -c 'grep -q "auto-heal = \[\"hiqlite-wal/auto-heal\"\]" hiqlite/Cargo.toml'
sh -c 'grep -q "\"dep:hiqlite-wal\"," hiqlite/Cargo.toml'
# every manifest points at the fork and says it is not upstream
sh -c 'test "$(grep -hc "bartekus/hiqlite" hiqlite/Cargo.toml hiqlite-wal/Cargo.toml hiqlite-derive/Cargo.toml | awk '\''{s+=$1} END {print s}'\'')" -ge 3'
sh -c 'test "$(grep -l "Not affiliated with or endorsed by the upstream project" hiqlite/Cargo.toml hiqlite-wal/Cargo.toml hiqlite-derive/Cargo.toml | wc -l | tr -d " ")" = "3"'
sh -c 'grep -qi "not affiliated with or endorsed by the upstream project" hiqlite/README.md'
# the two leaf crates package, which is as far as this can be verified before publication
cargo package -p hiqlite-wal-patched --allow-dirty
cargo package -p hiqlite-derive-patched --allow-dirty
# the publication workflow: its own trigger, its own job boundary, and no `--no-verify`
test -f .github/workflows/publish.yaml
sh -c 'grep -q "      - \"v\\*-patched.\\*\"" .github/workflows/publish.yaml'
# the flag itself, not the two comments explaining why it is absent
sh -c '! grep -vE "^\\s*#" .github/workflows/publish.yaml | grep -q -- "--no-verify"'
sh -c '! grep -qE "^ *pull_request" .github/workflows/publish.yaml'
sh -c 'grep -q "CARGO_REGISTRY_TOKEN" .github/workflows/publish.yaml'
sh -c '! grep -q "secrets.CLAUDE_CODE_OAUTH_TOKEN" .github/workflows/publish.yaml'
sh -c 'grep -q "needs: build" .github/workflows/publish.yaml'
# the review workflow: a different trigger, a different credential, and no registry token
test -f .github/workflows/ai-review.yaml
sh -c 'grep -q "CLAUDE_CODE_OAUTH_TOKEN" .github/workflows/ai-review.yaml'
sh -c '! grep -q "secrets.CARGO_REGISTRY_TOKEN" .github/workflows/ai-review.yaml'
sh -c '! grep -qE "^ *pull_request" .github/workflows/ai-review.yaml'
sh -c 'grep -q "merge-base --is-ancestor" .github/workflows/ai-review.yaml'
# 031 KD-10: dispatch cannot reach this workflow, so a review-* tag can, and the ref is not interpolated
sh -c 'grep -q "      - \"review-\*\"" .github/workflows/ai-review.yaml'
sh -c '! grep -q "\${{ inputs.ref }}\"" .github/workflows/ai-review.yaml'
sh -c '! grep -qE "^ *pull_request" .github/workflows/ai-review.yaml'
sh -c '! grep -q "anthropics/claude-code-action" .github/workflows/ai-review.yaml'
sh -c 'grep -q "npm install -g @anthropic-ai/claude-code@2.1.280" .github/workflows/ai-review.yaml'
# 031 KD-11 / F-116: the cluster health check waits for the membership
sh -c 'grep -q "async fn wait_for_members" hiqlite/tests/cluster/check.rs'
sh -c '! grep -q "assert_eq!(members, 3);" hiqlite/tests/cluster/check.rs'
sh -c 'grep -q "\"Bash(git diff:\*)\"" .github/workflows/ai-review.yaml'
# the ledger
test -f standards/spec/release-ledger.md
sh -c 'grep -q "hiqlite-patched" standards/spec/release-ledger.md'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/031-downstream-release-qualification standards/spec/release-ledger.md'
# --- what this repair adds ---
cargo test -p hiqlite-patched --lib --no-default-features --features cache,listen_notify_local store::state_machine::memory::notify_handler::tests::an_acknowledged_subscription_receives_an_event_published_immediately_afterwards -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache,listen_notify_local store::state_machine::memory::notify_handler::tests::a_caller_that_abandons_its_acknowledgement_never_kills_the_handler -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features cache,listen_notify_local store::state_machine::memory::notify_handler::tests::every_acknowledged_subscriber_receives_the_same_event -- --exact
# B-1: the request carries an acknowledgement, and the handler answers it after the push
sh -c 'grep -q "tokio::sync::oneshot::Sender<()>" hiqlite/src/store/state_machine/memory/notify_handler.rs'
sh -c 'grep -A5 "listeners.push(tx);" hiqlite/src/store/state_machine/memory/notify_handler.rs | grep -q "ack.send(())"'
sh -c '! grep -q "ack.send(()).unwrap()" hiqlite/src/store/state_machine/memory/notify_handler.rs'
sh -c 'grep -q "ack_rx" hiqlite/src/network/api.rs'
# B-2: the client is handed a readiness signal and waits on it
sh -c 'grep -q "READY_TIMEOUT: Duration = Duration::from_secs(10)" hiqlite/src/client/listen_notify.rs'
sh -c 'grep -q "oneshot::Receiver<()>)" hiqlite/src/client/listen_notify.rs'
sh -c 'grep -A6 "SSE::Connected(c)" hiqlite/src/client/listen_notify.rs | grep -q "tx_ready.take()"'
sh -c 'grep -q "tokio::time::timeout(crate::client::listen_notify::remote::READY_TIMEOUT, ready)" hiqlite/src/client/create.rs'
# B-3: a timeout warns and construction still succeeds
sh -c 'grep -A6 "READY_TIMEOUT, ready" hiqlite/src/client/create.rs | grep -q "Ok(Ok(())) => {}"'
sh -c 'grep -q "Some(rx_notify)" hiqlite/src/client/create.rs'
# the register records the repair against the finding it closes
sh -c 'grep -q "032-listen-notify-subscription-readiness" standards/spec/findings-register.md'
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
sh -c '! grep -rl "$(printf "\342\200\224")" specs/032-listen-notify-subscription-readiness'
# --- 031 B-8 / F-108 / F-109, added 2026-09-22. This block replaces 031's, so 031's
# additions have to live here to run at all ---
# B-8 / F-108: the qualification graph is committed and enforced, and openraft is exact
git ls-files --error-unmatch Cargo.lock
cargo metadata --locked --format-version 1 --no-deps
sh -c 'grep -q "^openraft = { version = \"=0.9.25\"" Cargo.toml'
sh -c 'grep -A1 "^name = \"openraft\"$" Cargo.lock | grep -q "^version = \"0.9.25\"$"'
sh -c 'grep -q "cargo metadata --locked --format-version 1 > /dev/null" .github/workflows/code_style.yaml'
sh -c 'grep -q "git diff --exit-code -- Cargo.lock" .github/workflows/code_style.yaml'
sh -c 'grep -q "cargo metadata --locked --format-version 1 > /dev/null" .github/workflows/publish.yaml'
sh -c 'grep -q "git diff --exit-code -- Cargo.lock" .github/workflows/publish.yaml'
sh -c 'grep -q "^qualify:" justfile'
# F-109: the containerized jobs run bash, and git may read the checkout before it is asked about the lock
sh -c 'test "$(grep -c "shell: bash" .github/workflows/publish.yaml)" -eq 2'
sh -c 'grep -q "shell: bash" .github/workflows/code_style.yaml'
sh -c 'grep -q "shell: bash" .github/workflows/acceptance.yaml'
sh -c 'grep -q "safe.directory" .github/workflows/code_style.yaml'
sh -c 'grep -q "safe.directory" .github/workflows/publish.yaml'
```
