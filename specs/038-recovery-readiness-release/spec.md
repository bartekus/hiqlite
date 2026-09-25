---
id: "038-recovery-readiness-release"
title: "Release the startup recovery readiness repair alone, as 0.15.0-patched.3"
status: draft
created: "2026-09-24"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "031-downstream-release-qualification"
  - "036-n1-repair-release"
  - "037-startup-recovery-readiness"
origin:
  retroactive: false
# D-1: `036` B-1 and B-2 name 0.15.0-patched.2; this spec moves them to 0.15.0-patched.3.
# `036`'s file is not edited.
amends: ["036-n1-repair-release"]
# D-2: `036`'s block pins the version it released. This spec carries that block forward
# whole, with the version commands moved and marked, rather than editing a predecessor's
# acceptance.
amends_verification: ["036-n1-repair-release"]
extends:
  - spec: "016-derive-macros"
    unit: { kind: crate, id: "hiqlite-derive-patched" }
    nature: additive
  - spec: "035-n1-upgrade-exclusion"
    unit: { kind: directory, path: "qualification/n1-upgrade/" }
    nature: additive
  # Section 13 is added; no earlier section is changed.
  - spec: "031-downstream-release-qualification"
    unit: { kind: file, path: "standards/spec/consumer-handoff.md" }
    nature: additive
  - spec: "017-examples-as-documentation"
    unit: { kind: directory, path: "examples/" }
    nature: additive
references:
  - unit: { kind: file, path: "hiqlite/Cargo.toml" }
    role: "context"
  - unit: { kind: file, path: "hiqlite-wal/Cargo.toml" }
    role: "context"
  - unit: { kind: file, path: "Cargo.lock" }
    role: "context"
  - unit: { kind: file, path: "hiqlite/README.md" }
    role: "context"
summary: >
  The release identity of `037`'s repair (F-134): all three packages at
  0.15.0-patched.3, in lockstep, with exact internal requirements and no other
  change to the dependency graph (openraft stays =0.9.25). Nothing else ships
  with it: the N=3 lane B change (#39) is left for a later release. States the
  consumer guidance (wait for each group before first use, report recovering
  rather than down, size liveness probes, pin exactly or build --locked), the
  qualification the release tree carries, which is 036's bar, and what stays
  separate: ratification, publication and each consumer's adoption. Carries
  036's acceptance forward with its version commands moved.
---

# 038: Release the startup recovery readiness repair alone, as 0.15.0-patched.3

## 1. Purpose

`037` repaired F-134: a node that reported healthy and ready before its
startup recovery had applied its log. The repair reaches no one until a release
carries it. This spec is that release's identity and its obligations. It changes
no behavior of the library: the tree differs from `037`'s merged tree only in
version fields, lockfile version lines, one README line, the consumer handoff's
new section 13 and this document.

The owner granted release authority for `0.15.0-patched.3` on 2026-09-24 (D-3),
on the qualification bar of `0.15.0-patched.2` (`036` B-5).

## 2. Territory

- **Amends** `036`: its B-1 and B-2 versions now read `0.15.0-patched.3`.
  **Carries** `036`'s acceptance, and through it `032`'s, `031`'s, `003`'s
  and `012`'s (`amends_verification`, D-2).
- **Extends** `016`'s derive crate (its version line), `035`'s harness
  directory (its lockfile), `017`'s examples (their lockfiles) and `031`'s
  consumer handoff (section 13, additive).
- **References**, without claiming, the unowned manifests, the root lockfile
  and the packaged README.

## 3. Behavior

### B-1. One version, three packages, in lockstep, exact

`hiqlite-patched`, `hiqlite-wal-patched` and `hiqlite-derive-patched` are all
`0.15.0-patched.3`, with `hiqlite-patched` requiring the other two at
`=0.15.0-patched.3` (`036` B-2's rule, the version moved). The root lockfile,
the examples' lockfiles and `qualification/n1-upgrade/new/Cargo.lock` change
only those three version lines; `openraft` stays `=0.9.25` (`036` B-3).

### B-2. This release carries `037` and nothing else

The library source is `037`'s merged source. Lane B of the N=3 proposal (#39,
`033`/`034`) is not in it: it is merged after this release's tree is fixed, and
ships in a later release once its N=3 qualification exists.

### B-3. The consumer handoff says what changes for a consumer

Section 13 of the handoff states: the recovery contract of `037`; that a
consumer waits for each group it uses before first use and treats
`Error::Recovering` as retryable; that it reports "recovering" rather than
"down"; that liveness probes on `/health` are sized for a replay; the exact
pin and `--locked`; the `0.15.0-patched.1` caret hazard, still open; and the
yanking of `0.15.0-patched.1` as guidance only, not done.

### B-4. The release tree is qualified as itself

`036` B-5, items 1 to 5, on this tree, plus `037`'s regression
(`hiqlite/tests/recovery_readiness.rs`) in a release build under Rauthy's and
Rahi's feature sets on native Linux arm64 and native Linux amd64. A missing leg
is stated, not waived; if any leg fails, nothing is published.

### B-5. Publication, adoption and ratification are separate acts

As `036` B-6. The tag is signed and on the merged commit whose tree was
qualified; the GitHub release is a pre-release; after publication the registry
checksums are read back anonymously, a fresh consumer resolving only from the
registry builds and runs an N=1 node under both feature sets, and the ledger
and handoff are updated on that evidence by a separate change.

## 4. Evidence and its limits

Recorded after the tag (`036` D-3), under
`~/DevDep/hiqlite-release-artifacts/0.15.0-patched.3/` (`CANDIDATE.md` is the
index), and in this section by the change that records the publication.

## 5. Known defects

**KD-1. The published caret requirements of 0.15.0-patched.1 stay** (`036`
KD-1). Only guidance, lockfiles and a possible yank protect its consumers.

**KD-2. A consumer that uses a group without waiting now gets errors** (`037`
KD-2). That is the contract, and section 13 says so.

## 6. Resolved decisions

**D-1 (2026-09-24, amend `036` rather than edit it).** `036` records the
published `0.15.0-patched.2`; rewriting it would erase that record.

**D-2 (2026-09-24, this block is `036`'s).** `036`'s block pins
`0.15.0-patched.2` in nine commands; the version commit would make them fail.
The block is carried forward whole with those commands moved to
`0.15.0-patched.3` and marked, and the check that no lockfile names an older
version widened to `patched.1` and `patched.2`.

**D-3 (2026-09-24, owner decision: release authority).** The owner granted
release authority for `0.15.0-patched.3` (all three crates, exact inter-crate
pins, openraft `=0.9.25`) on `036`'s qualification bar, and directed that a
failed leg stops publication. The owner also directed the handoff to keep the
exact-pin and `--locked` instruction and to consider yanking guidance for
`0.15.0-patched.1`, without yanking.

**D-4 (2026-09-24, lane B is not in this release).** B-2. Lane B changes node
1's bootstrap decision and the answer of a cluster-internal route that a mixed
repaired and unrepaired bootstrap depends on (`033` D-8), and the N=3 harness
has not qualified it. Releasing it under a recovery fix would ship an
unqualified N=3 change to consumers that asked for the recovery fix.

## 7. Out of scope

- Lane B and every N=3 change.
- Yanking `0.15.0-patched.1`.
- Any consumer's pin, lockfile, image or release.
- Ratification of `037` or this spec.

## Verification

Run with `RUSTUP_TOOLCHAIN=1.95.0 just spine-verify 038-recovery-readiness-release`.
**This block is `036`'s acceptance as well as this spec's** (D-2). It does not
run the harness or package `hiqlite-patched`; B-4 lists those.

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
# `036` D-2: was `^version = "0.15.0-patched.1"`; `038` D-2: then "0.15.0-patched.2"; the version this release carries.
sh -c 'grep -q "^version = \"0.15.0-patched.3\"" hiqlite/Cargo.toml'
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
# --- what 036 adds, with the version moved (038 D-2) ---
# B-1: all three at the new version, in lockstep, in the manifests and in the lock
sh -c 'test "$(grep -h "^version = \"0.15.0-patched.3\"" hiqlite/Cargo.toml hiqlite-wal/Cargo.toml hiqlite-derive/Cargo.toml | wc -l | tr -d " ")" = "3"'
sh -c 'test "$(grep -A1 -E "^name = \"hiqlite(-wal|-derive)?-patched\"$" Cargo.lock | grep -c "^version = \"0.15.0-patched.3\"$")" = "3"'
sh -c 'test "$(grep -A1 -E "^name = \"hiqlite(-wal|-derive)?-patched\"$" qualification/n1-upgrade/new/Cargo.lock | grep -c "^version = \"0.15.0-patched.3\"$")" = "3"'
sh -c '! git grep -q -E "^version = \"0.15.0-patched.[12]\"" -- "*Cargo.lock"'
# B-2: the internal requirements are exact
sh -c 'grep -q "hiqlite-derive = { package = \"hiqlite-derive-patched\", version = \"=0.15.0-patched.3\", path = " hiqlite/Cargo.toml'
sh -c 'grep -q "hiqlite-wal = { package = \"hiqlite-wal-patched\", version = \"=0.15.0-patched.3\", path = " hiqlite/Cargo.toml'
# B-4: the packaged README and the handoff give the exact pin and the downgrade rule
sh -c 'grep -q "version = \"=0.15.0-patched.3\"" hiqlite/README.md'
grep -q '^## 12\. ' standards/spec/consumer-handoff.md
grep -q 'Downgrade to 0.14.x is unsupported' standards/spec/consumer-handoff.md
# the phrase wraps in the document, so it is matched with line breaks folded to spaces
sh -c 'tr "\n" " " < standards/spec/consumer-handoff.md | grep -q "restore the verified pre-upgrade archive into a fresh volume"'
# every lockfile still satisfies its manifest
cargo metadata --locked --format-version 1 --no-deps --manifest-path qualification/n1-upgrade/new/Cargo.toml
sh -c '! grep -rl "$(printf "\342\200\224")" specs/036-n1-repair-release standards/spec/consumer-handoff.md hiqlite/README.md'
# --- what 038 adds ---
# B-3: the handoff states the recovery contract, the waits and the probe sizing
grep -q '^## 13\. 0.15.0-patched.3' standards/spec/consumer-handoff.md
sh -c 'tr "\n" " " < standards/spec/consumer-handoff.md | grep -q "Wait for each group it uses before first use"'
sh -c 'tr "\n" " " < standards/spec/consumer-handoff.md | grep -q "It is \*\*not\*\* done by this release"'
# B-2: the repair this release carries is present and its acceptance holds
sh -c 'grep -q "pub use recovery::{RecoveryProgress, RecoveryState};" hiqlite/src/lib.rs'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/038-recovery-readiness-release'
```
