---
id: "041-wal-creation-durability-release"
title: "Release the WAL file-creation durability repair as 0.15.0-patched.4"
status: draft
created: "2026-09-25"
owner: "hiqlite maintainers"
risk: critical
implementation: in-progress
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "031-downstream-release-qualification"
  - "038-recovery-readiness-release"
  - "039-lock-after-restart-evidence"
  - "040-wal-file-creation-durability"
origin:
  retroactive: false
# D-1: `038` B-1 names 0.15.0-patched.3; this spec moves it to 0.15.0-patched.4.
amends: ["038-recovery-readiness-release"]
# D-2: `038`'s block, which is also `036`'s, pins the version it released. It is carried
# forward whole with the version commands moved and marked.
amends_verification: ["038-recovery-readiness-release"]
extends:
  - spec: "016-derive-macros"
    unit: { kind: crate, id: "hiqlite-derive-patched" }
    nature: additive
  - spec: "035-n1-upgrade-exclusion"
    unit: { kind: directory, path: "qualification/n1-upgrade/" }
    nature: additive
  # Section 14 is added; no earlier section is changed.
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
  The release identity of `040`'s repair (F-135): all three packages at
  0.15.0-patched.4, in lockstep, with exact internal requirements and openraft
  still =0.9.25. It also carries what merged after patched.3: `039`'s
  lock-after-restart evidence and the F-136 and F-137 corrections. Lane B of
  the N=3 proposal (#39) is not in it: the owner reverted it on trunk before the
  release commit (D-4). Publication waits for the owner's ratification of every
  spec the release contains (D-3).
---

# 041: Release the WAL file-creation durability repair as 0.15.0-patched.4

## 1. Purpose

`040` repaired F-135: a new WAL file's header, length and name were not made
durable at creation, so under `LogSync::Immediate` an append acknowledged just
after a rollover could be lost to a power cut. This spec is the release that
carries it, and its obligations. It changes no library behavior of its own: the
tree differs from the merged trunk only in version fields, lockfile version
lines, one README line, the consumer handoff's new section 14 and this document.

The owner granted release authority for `0.15.0-patched.4` on 2026-09-25 (D-3)
on the qualification bar of `0.15.0-patched.3` (`038` B-4 and B-5),
**conditional on the owner having ratified every spec the release contains**.

## 2. Territory

- **Amends** `038`: its B-1 version now reads `0.15.0-patched.4`. **Carries**
  `038`'s acceptance, and through it `036`'s, `032`'s, `031`'s, `003`'s and
  `012`'s (`amends_verification`, D-2).
- **Extends** `016`'s derive crate (its version line), `035`'s harness
  directory (its lockfile), `017`'s examples (their lockfiles) and
  `031`'s consumer handoff (section 14, additive).
- **References**, without claiming, the unowned manifests, the root lockfile
  and the packaged README.

## 3. Behavior

### B-1. One version, three packages, in lockstep, exact

`hiqlite-patched`, `hiqlite-wal-patched` and `hiqlite-derive-patched` are all
`0.15.0-patched.4`, with `hiqlite-patched` requiring the other two at
`=0.15.0-patched.4`. The root lockfile, the examples' lockfiles and the N=1
qualification harness's lockfile change only those three version lines;
`openraft` stays `=0.9.25`.

### B-2. What this release carries

Everything merged to `spec-spine` after `6c8db22` (patched.3):

| change | spec | library behavior |
|---|---|---|
| F-135: a new WAL file is synced, with its directory, before use | `040` | yes: one `sync_all` and one directory sync per WAL file created |
| lock-after-restart evidence (F-138, rahi `045` D-22) | `039` | none (a test) |
| F-136: the `hiqlite::tls` changes in `CHANGELOG.md` | `027` D-9 | none |
| F-137: the publish job and the `Dockerfile` inside the lockfile bracket | `031` D-8 | none (build recipes) |
| lane B of the N=3 proposal: merged as #39, reverted by #52 (`2d5fa30`) before the release commit | `033` D-12, `034` D-10 | none: not in this release (D-4) |
| toolchain pin, security policy, spec-spine `=0.26.0` (#43, #45) | `004`, `000` | none |
| Statecraft CI profile and its format, feature-gating and clippy debt repairs (#47, #53, #56, #57, #60) | `004`, `042` | none (CI, formatting and test code) |
| isolated test scratch roots and non-ephemeral integration-test ports, F-139 to F-141 (#54, #59) | `043` | none (test code) |

### B-3. The consumer handoff says what changes for a consumer

Section 14 states: the F-135 repair and the configuration it matters for
(`LogSync::Immediate`); that a distributed lock left held by a restart is
granted to the next caller within one lease plus two seconds on every
`0.15.0-patched.*` (F-138), so rahi's one-TTL wait after a restart is only
needed while it runs upstream 0.14; the `hiqlite::tls` changes; that lane B
is not in this release; the exact pin and `--locked`; and that
`0.15.0-patched.1` is not yanked.

### B-4. The release tree is qualified as itself

`038` B-4 on this tree, plus `040`'s and `039`'s regressions in a release
build under Rauthy's and Rahi's feature sets, on native Linux arm64 and native
Linux amd64. A missing leg is stated, not waived; if any leg fails, nothing is
published.

### B-5. Publication, adoption and ratification are separate acts

As `038` B-5. In addition, nothing is tagged or published until every spec
named in D-3 reads `status: approved` at the release commit.

## 4. Evidence and its limits

Not yet produced. This change prepares the release; the qualification legs of
B-4 run on the frozen candidate once the owner's condition (D-3) is met,
and their results are recorded here by that change. The raw logs will be under
`~/DevDep/hiqlite-release-artifacts/0.15.0-patched.4/` (`CANDIDATE.md`).

Local, on this branch (macOS arm64, spec-spine 0.26.0): every lockfile still
satisfies its manifest (`cargo metadata --locked`, root, four examples, the N=1
harness; the N=3 harness left the tree with lane B, D-4).

## 5. Known defects

**KD-1. The published caret requirements of 0.15.0-patched.1 stay** (`036`
KD-1, `038` KD-1).

**KD-2. Withdrawn.** It read "Lane B ships without N=3 qualification unless
the owner decides otherwise"; the owner reverted lane B instead (D-4), so
nothing unqualified at N=3 ships.

## 6. Resolved decisions

**D-1 (2026-09-25, amend `038` rather than edit it).** `038` records the
published `0.15.0-patched.3`.

**D-2 (2026-09-25, this block is `038`'s).** `038`'s block pins
`0.15.0-patched.3` in seven commands. It is carried forward whole with those
moved to `0.15.0-patched.4` and marked, the check that no lockfile names an
older version widened to `patched.3` for every current-generation lockfile,
with the N=1 harness's intentionally old lockfile excluded, and `041`'s own
commands appended. `038`'s section-13 checks stay, since section 13 stays.

**D-3 (2026-09-25, owner decision: release authority and its precondition).**
The owner granted `0.15.0-patched.4`, containing the F-135 repair and the
lease-wake repair (and F-136/F-137 if merged), on patched.3's qualification
bar, with no yank of `0.15.0-patched.1`, and only once the owner has ratified
every spec the release contains. At preparation, `033`, `034`, `039`, `040` and
this spec are `draft` (`033` and `034` are `implementation: in-progress`), so
this change stops before qualification and publication. After D-4's resolution
the specs the release contains that need ratification are `037` and `038`
(ratified by the owner, #46, 2026-09-25) and `039` through `043`; `033` and
`034` are not in it. Specs `042` and `043` entered the candidate through the
CI-debt and test-flake repairs merged before the final trunk refresh. They
change no library behavior, but they are still contained specs and therefore
remain inside the owner's precondition.

**D-4 (2026-09-25, lane B was in the tree; resolved below).** `038` D-4 kept lane B out of
patched.3 because it changes node 1's bootstrap decision and a cluster-internal
route that a mixed repaired and unrepaired bootstrap depends on (`033` D-8),
and the N=3 harness has not qualified it. It merged afterwards (#39,
`ce04e64`), so any release cut from `spec-spine` now carries it. The owner
decides between: shipping it with this release under KD-2, reverting it on
`spec-spine` before the release commit, or holding the release for `033`'s N=3
qualification. This spec takes none of the three.

**Resolution (2026-09-25, owner decision).** The owner decided, verbatim:
"Revert lane B for patched.4". #52 reverted #39 on `spec-spine` (merge
`2d5fa30`), so trunk equals what ships and lane B is not in this release;
`033` D-12 and `034` D-10 record it. The reland (the revert of that revert) is
a draft pull request that waits for `033`'s N=3 qualification. `033` and `034`
need no ratification for this release. KD-2 is withdrawn.

## 7. Out of scope

- Yanking `0.15.0-patched.1`.
- Any consumer's pin, lockfile, image or release (rahi adopts through its own
  governed change).
- Ratification of any spec.

## Verification

Run with `RUSTUP_TOOLCHAIN=1.95.0 just spine-verify 041-wal-creation-durability-release`.
**This block is `038`'s and `036`'s acceptance as well as this spec's** (D-2).
It does not run the harnesses or package `hiqlite-patched`; B-4 lists those.

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
# `036` D-2: was `^version = "0.15.0-patched.1"`; `038` D-2: then "0.15.0-patched.2"; `041` D-2: then
# "0.15.0-patched.4"; the version this release carries.
sh -c 'grep -q "^version = \"0.15.0-patched.4\"" hiqlite/Cargo.toml'
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
# --- what 036 adds, with the version moved (038 D-2, 041 D-2) ---
# B-1: all three at the new version, in lockstep, in the manifests and in the lock
sh -c 'test "$(grep -h "^version = \"0.15.0-patched.4\"" hiqlite/Cargo.toml hiqlite-wal/Cargo.toml hiqlite-derive/Cargo.toml | wc -l | tr -d " ")" = "3"'
sh -c 'test "$(grep -A1 -E "^name = \"hiqlite(-wal|-derive)?-patched\"$" Cargo.lock | grep -c "^version = \"0.15.0-patched.4\"$")" = "3"'
sh -c 'test "$(grep -A1 -E "^name = \"hiqlite(-wal|-derive)?-patched\"$" qualification/n1-upgrade/new/Cargo.lock | grep -c "^version = \"0.15.0-patched.4\"$")" = "3"'
sh -c '! git grep -q -E "^version = \"0.15.0-patched.[123]\"" -- "*Cargo.lock" ":(exclude)qualification/n1-upgrade/old/Cargo.lock"'
# B-2: the internal requirements are exact
sh -c 'grep -q "hiqlite-derive = { package = \"hiqlite-derive-patched\", version = \"=0.15.0-patched.4\", path = " hiqlite/Cargo.toml'
sh -c 'grep -q "hiqlite-wal = { package = \"hiqlite-wal-patched\", version = \"=0.15.0-patched.4\", path = " hiqlite/Cargo.toml'
# B-4: the packaged README and the handoff give the exact pin and the downgrade rule
sh -c 'grep -q "version = \"=0.15.0-patched.4\"" hiqlite/README.md'
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
# --- what 041 adds ---
# D-4: lane B is not in the release tree
test ! -e qualification/n3
sh -c '! grep -q "init_peer_wait_secs" hiqlite/src/config.rs'
# B-2: the repairs this release carries are present
sh -c 'grep -A20 "pub fn create_file" hiqlite-wal/src/wal.rs | grep -q "sync_dir(dir)?;"'
test -f hiqlite/tests/lease_wake.rs
sh -c 'test "$(grep -c "cargo publish --locked" .github/workflows/publish.yaml)" = "2"'
grep -q 'cargo build --locked --features server --release' Dockerfile
grep -q '^### Breaking: `hiqlite::tls` against 0.14.0' CHANGELOG.md
# B-3: the handoff's section for this release
grep -q '^## 14\. 0.15.0-patched.4' standards/spec/consumer-handoff.md
sh -c '! grep -rl "$(printf "\342\200\224")" specs/041-wal-creation-durability-release standards/spec/consumer-handoff.md'
```
