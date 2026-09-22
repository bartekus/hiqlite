---
id: "031-downstream-release-qualification"
title: "Qualify and publish the downstream release under distinct package names"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "004-governance-harness"
  - "005-adoption-assessment-and-plan"
  - "029-consumer-surface-repairs"
  - "030-transport-security-and-dashboard-repairs"
amends:
  - "003-client-consistency-and-retry-outcomes"
  - "012-cluster-integration-evidence"
  - "016-derive-macros"
# D-6: the package rename invalidates every acceptance command that names a package, because
# `-p` takes a package name. These two are the specs whose live blocks name one and which this
# session did not author; the eight it did author were corrected in place, which is ordinary
# authoring on one's own spec.
amends_verification:
  - "003-client-consistency-and-retry-outcomes"
  - "012-cluster-integration-evidence"
establishes:
  - "standards/spec/release-ledger.md"
  - ".github/workflows/publish.yaml"
  - ".github/workflows/ai-review.yaml"
extends:
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "justfile" }
    nature: additive
  - spec: "017-examples-as-documentation"
    unit: { kind: directory, path: "examples/" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Renames the three packages so nothing resolves under an upstream name,
  versions them at 0.15.0-patched.1 because the public API changed, keeps the
  library names and the internal dependency keys so consumers and the feature
  table need no rewriting, and adds a publication workflow and an independent
  review workflow with separate triggers and separate credentials. Records the
  release work ledger, and the acceptance corrections the rename forced.
---

# 031: Qualify and publish the downstream release under distinct package names

## 1. Purpose

Ten specs repaired this fork. This one turns that into something two named
applications can depend on, without any of it resolving under a name that
belongs to the upstream project.

One responsibility: **the release identity, how it is published, and what
evidence stands behind each step.**

## 2. Territory

**Establishes** the release ledger and the two workflows.

**Extends** `spec-spine.toml` and the `justfile` (`000`'s), and `examples/`
(`017`'s), each of which names a package and therefore had to follow the rename.

**Amends** `003` and `012` and carries their acceptance, and corrects `016`'s
crate unit (B-2).

**Ownership boundary.** Publication is not adoption, ratification, or upstream
acceptance. The adoption plan says publication is deliberately not in its queue;
this spec is that queue and makes no adoption claim. Constitution X keeps the
five apart and this release performs exactly one of them.

## 3. Behavior

### B-1. One version, and it says what it is

`0.15.0-patched.1` for all three packages.

**The minor bump is the breaking-change signal.** The public API changed:
`hiqlite::Error` gains `CacheIncompatible`, `StorageInUse`, `NodeFailed` and
`Startup`; `hiqlite_wal::Error` gains `IncompleteAppend`; `writer::spawn`
returns a third value; `ServerTlsConfigCerts` gains `ca`;
`ServerTlsConfig::from_env` returns a `Result`; `Client` gains
`node_failure`, `ensure_node_available` and two bounded health waits;
`Migrations::try_build` is new. Under `0.x` a patch-level version would claim a
compatibility this release does not have.

**`-patched.N` is a numbered prerelease**, so `0.15.0-patched.2` orders after it
and a future upstream `0.15.0` orders after both. It is not a moving name: each
publication gets its own `N`.

### B-2. The packages are renamed and the libraries are not

| package | library | consumer writes |
|---|---|---|
| `hiqlite-patched` | `hiqlite` | `hiqlite = { package = "hiqlite-patched", version = "0.15.0-patched.1" }` |
| `hiqlite-wal-patched` | `hiqlite_wal` | pulled in by the above |
| `hiqlite-derive-patched` | `hiqlite_derive` | pulled in by the above |

The alias is **required, not a convenience**, and the reason is in the generated
code: `hiqlite-derive` emits `::hiqlite::CacheVariants` and
`::hiqlite::Row`, and `params!` emits `::hiqlite::Param`. Those are absolute
paths and they resolve against the **dependency key** in the consumer's
manifest. A consumer who writes `hiqlite-patched = "..."` without the alias gets
an extern named `hiqlite_patched` and every macro expansion fails to resolve.
The handoff says so in the first block a reader sees.

**Internally the dependency keys are kept**: `hiqlite/Cargo.toml` declares
`hiqlite-wal = { package = "hiqlite-wal-patched", ... }`. The keys are what
`dep:hiqlite-wal`, `dep:hiqlite-derive` and `hiqlite-wal/auto-heal` in the
feature table refer to, so the feature table needs no change and cannot drift
from the dependency names. Sixteen feature entries did not have to be touched.

**One claim had to follow the rename.** `016` claims
`{ kind: crate, id: "hiqlite-derive" }`, and a crate unit resolves by package
name, so it stopped resolving and the index reported `I-003`. The claim now
names `hiqlite-derive-patched`. That is a correction to a territory declaration
forced by a rename, not a change to what `016` says; its text is unedited.

### B-3. Every manifest says it is not upstream

All three carry the fork repository, and a description that says they are a
downstream build of Hiqlite from `bartekus/hiqlite`, not affiliated with or
endorsed by the upstream project. `hiqlite/README.md`, which is what crates.io
renders, opens with the same statement, the upstream provenance to the commit,
the required alias, and a line saying to prefer upstream when a release of it
carries what you need.

`authors` is unchanged and remains the upstream author's. The licence requires
attribution and the code is his; the fork's identity is carried by the package
name, the repository and the description, which is where a reader looks.

### B-4. Publication is dependency-first, staged, and never `--no-verify`

`hiqlite-patched`'s published manifest resolves its two dependencies **by
version**, so it cannot be packaged until they are on the registry. That is not
a limitation to work around; it is the check. `cargo package -p hiqlite-patched`
fails today with "no matching package named `hiqlite-derive-patched` found", and
that failure is the thing that proves the published manifest does not silently
depend on the workspace.

So the order is `hiqlite-wal-patched`, then `hiqlite-derive-patched`, then
`hiqlite-patched`, with a bounded wait between steps until the registry actually
**serves** each version. `--no-verify` appears nowhere: it skips the build of the
packaged tree, which is the only thing that catches a manifest that resolves in
the workspace and not from the registry.

Re-running the workflow is safe by **inspection**, not by `|| true`: each step
asks the registry whether that exact version exists and skips it if so, and any
other failure still fails the job. A published version is never overwritten and
never moved.

### B-5. The registry token and the review credential never meet

Three workflows, three triggers, three privilege levels:

| workflow | trigger | holds | may run on a pull request |
|---|---|---|---|
| `acceptance.yaml` | push to the integration branch | nothing | no |
| `publish.yaml` | push of a `v*-patched.*` tag | `CARGO_REGISTRY_TOKEN` | no |
| `ai-review.yaml` | `workflow_dispatch` with a ref | `CLAUDE_CODE_OAUTH_TOKEN` | no |

`publish.yaml` is split so the token is referenced only in the second job: the
checks run in a job that holds nothing, and the publishing job runs only after
they pass. The review workflow refuses a ref that is not an ancestor of the
integration branch, so it reviews merged code rather than an arbitrary tree.

**The review is evidence, not an approval.** Nothing gates on it, and its output
is treated as untrusted input: findings are read and judged, not applied.

### B-6. The tag and the manifests must agree

The publishing workflow's first check compares the tag against all three
manifests and refuses if any disagrees. A tag is the only thing that triggers
publication, so a tag that means something different from what it publishes is
the one mistake that cannot be corrected afterwards.

### B-7. The lint matrix compiles the `server` feature

`server` is the only feature that compiles `hiqlite/src/server/`, and `full`
does not imply it: `server` is `full` **plus** four dependencies,
`listen_notify` and `tokio/macros`. Nothing in the matrix reached that tree, and
`just test-no-s3` does not enable it either.

That is not a theoretical gap. A change to a shared enum was applied to
`network::api::listen` and not to the proxy's copy of the same endpoint;
`cargo clippy -- -D warnings`, the whole feature matrix, the full test suite and
both CI workflows passed, and **nine acceptance blocks then failed on the same
compile error**, because a `verify` command in each enables `server`. The only
gate that saw it runs after the merge. F-104.

Two `server` combinations are added, on library targets. `--all-targets` there
pulls in `hiqlite-wal`'s test modules, which carry pre-existing lints unrelated
to this release, and silencing those to widen a gate would be changing unrelated
code to make it pass.

### B-8. The qualification graph is committed, and the consumer graph is qualified separately

Two graphs, and F-108 was the cost of treating them as one.

**The qualification graph** is what local runs and CI build. It is the workspace
`Cargo.lock`, now tracked. It is force-added rather than removed from
`.gitignore`, because that file is `000`'s explicit unit and the tracked example
lockfiles already work the same way. Both CI workflows that qualify a tree, the
pull-request `Check` and the publish workflow's secret-free `build` job, begin by
printing `rustc -Vv`, `cargo -V` and the lock's SHA-256 and refusing a lock the
manifests do not already satisfy (`cargo metadata --locked`), and end by proving
that no step changed it. `just qualify` is the local equivalent; `just check`
still runs `cargo update` first and is a maintenance sweep, not a qualification.
The toolchain is the one printed: CI's container and a local `+1.95.0` are not
asserted to be identical, and the printed versions are the record.

**The consumer graph** is what someone who adds `hiqlite-patched` to their own
manifest resolves. The committed lock does not constrain it; nothing a library
ships can. It is qualified after publication, from the registry, in a workspace
with no `path`, Git or `[patch]` override (R-15), and recorded separately.

**The containerized jobs run bash (F-109).** Their image's default shell is
`sh`, which rejects `set -o pipefail`; `publish.yaml` and `acceptance.yaml` both
depended on it in steps that had never run.

**One range is narrowed.** `openraft` is `=0.9.25`, the only version this release
was qualified on. F-107 showed a patch release of the consensus library changing
what a membership race does, and an ordering defect in consensus code is the
kind a caret range would let a consumer resolve into silently. Every other
dependency keeps its range: a consumer may resolve newer compatible versions than
the ones qualified, and the handoff lists what was qualified.

## 4. Evidence and its limits

**Release state is in the ledger**, `standards/spec/release-ledger.md`, one row
per work item with the state it has actually reached and the evidence for it.
The seven states are ordered and a row never skips one; publication and consumer
verification are the last two on purpose.

What is established at the time this spec was written:

- both leaf crates package cleanly, with `cargo package` and no `--no-verify`;
- `hiqlite-patched` does **not** package, with the error quoted in B-4, which is
  the staged verification working;
- all three names were checked available on crates.io on 2026-09-21;
- the six examples build against the renamed packages through the same alias a
  consumer uses, which is the alias mechanism exercised by six real crates;
- the whole corpus's acceptance passes, all thirty-one blocks.

What the acceptance does **not** establish, and these are the ones that matter:

- **Nothing is published by this spec.** Publication happens on a tag push and
  its evidence is the registry's own response, recorded in the ledger when it
  exists. Until then R-14 is not `published`.
- **No consumer outside this checkout has built against it.** The examples are
  in this repository and use `path`. R-15 is the row for a build with no
  workspace patch and no path dependency, and it is the only thing that proves
  the published manifests resolve.
- **No workflow in this spec has run.** `publish.yaml` needs a tag and
  `ai-review.yaml` needs a dispatch. Their YAML is asserted by greps, which is
  not the same as a run.
- **The alias requirement is reasoned from the emitted paths, not from a failing
  build.** B-2 says a consumer without the alias fails to resolve; that is read
  from the `::hiqlite::` paths the macros emit. R-15 is where it is demonstrated.
- **`cargo package` is not `cargo publish`.** It builds the packaged tree; it
  does not prove crates.io accepts it.

## 5. Known defects

**KD-1. The version is asserted against the tag and not against a changelog.**
B-6 compares three manifests to a tag. Nothing checks that `0.15.0-patched.1`
is the right number for the API change it carries; B-1 argues it and no control
enforces it.

**KD-2. `authors` names only the upstream author.** That is accurate about who
wrote the code and it means crates.io shows no maintainer for the fork. The
repository and description carry that instead.

**KD-3. Two of the three published crates have no README.** Only
`hiqlite-patched` sets `readme`. The other two carry the fork notice in their
`description` and nothing else, so a reader landing on their crates.io page sees
one sentence.

**KD-4. The rename forced acceptance edits in eight specs this session
authored.** Ordinary authoring, and worth recording: a package rename is not a
behavioral change and it invalidated forty-odd acceptance commands across the
corpus, because `-p` takes a package name. A block that named a manifest path
instead would have survived it. Nothing was changed except the package name, and
the sweep passes.

**KD-5. The examples' lockfiles are updated only for `openraft`.** B-8's pin
made every tracked example lockfile (`bench`, `cache-only`, `sqlite-only`,
`walkthrough`) resolve a version the pin forbids, so each was updated with
`cargo update -p openraft --precise 0.9.25`, which also records the renamed
packages. Nothing else in them was refreshed, and CI builds them without
`--locked`. What follows is the original entry.
 F-083 records that four
tracked example lockfiles are stale and that the documented build rewrites them;
the rename changes the package names those lockfiles contain. They are left as
they are, so the first build of an example after this change rewrites them.

**KD-7. Only `openraft` is pinned.** Every other dependency is a compatible
range, so a fresh consumer can resolve versions newer than the committed lock's,
and the consumer qualification covers the graph it resolved on the day it ran,
not every graph the ranges admit.

**KD-8. The pin has a maintenance cost.** A consumer cannot take an `openraft`
0.9 patch release, including a security fix, without a new `-patched.N` release
of this crate.

**KD-10. `ai-review.yaml` could never be dispatched.** `workflow_dispatch`
reaches only a workflow that exists on the repository's default branch, and that
is `main`, which tracks upstream and carries none of this corpus. The first
attempt to dispatch it after the merge returned "workflow not found on the
default branch". It now also runs on a pushed `review-*` tag, a maintainer act
with the same write access, from the tagged commit, with the same ancestor
check. Changing the default branch was the other option; it is a repository
setting and was not taken.

**KD-9. This spec's own `## Verification` block does not run.** `032` declares
`amends_verification` on it, so `just spine-verify 031` executes `032`'s block.
The F-108 and F-109 checks were first written here, where they never ran, and
were moved into `032`'s block on 2026-09-22 once the run's command count showed
it. A check added to this block is inert.

**KD-6. Nothing prevents a second publication of a different tree under the same
version.** The workflow skips a version the registry already serves, which makes
a re-run safe; it does not compare what is on the registry with what is in the
tree. crates.io refuses an overwrite, so the failure mode is a refused publish
rather than a moved release.

## 6. Resolved decisions

**D-1 (2026-09-21, `0.15.0-patched.1`).** Owner direction was a coherent
numbered prerelease containing `-patched.N`, with the numeric base taken from the
actual baseline and compatibility delta. The baseline is `0.14.0` plus nineteen
commits and the delta is breaking, so `0.15.0` is the base and `patched.1` is the
first publication of it.

**D-2 (2026-09-21, rename the package, keep the library).** The alternative was
renaming the library too, which would have meant every consumer editing every
`use` and the derive macros emitting a different path. Keeping the library name
means the only thing a consumer writes differently is one `package = ` key.

**D-3 (2026-09-21, keep the internal dependency keys).** Renaming the keys would
have meant editing sixteen feature-table entries that refer to them by name, each
an opportunity for the feature table and the dependency list to disagree. The
`package =` form makes the rename invisible to everything above it.

**D-4 (2026-09-21, stage publication rather than use `--no-verify`).** Reaching
for `--no-verify` is the obvious way to publish a crate whose dependencies are
not on the registry yet, and it removes the only check that distinguishes a
manifest that resolves from the registry from one that resolves from the
workspace. That check is the reason the order exists.

**D-5 (2026-09-21, three workflows rather than one).** A single workflow with
conditional steps would put the registry token in the same job as a build of a
tree, and the review credential in the same job as the registry token. Separate
triggers and separate jobs are what make "a pull request cannot reach either" a
property of the file rather than of a condition someone has to get right.

**D-6 (2026-09-21, this block carries `003`'s and `012`'s acceptance, and the
other eight were corrected in place).** Thirty specs' blocks name a package.
Twenty of them are already held by a later spec, whose block was corrected as
ordinary authoring; eight of those holders are specs this session authored. Two,
`003` and `012`, have live blocks this session did not author, so they go through
`amends_verification`. Nothing in either block changed except the package name.

**D-7 (2026-09-22, commit the qualification lock and pin exactly one
dependency).** The earlier reading of F-108 declined to track the lock because it
pins nothing for consumers. That is true and beside the point: the qualification
has to be reproducible whatever consumers resolve, and consumers are qualified on
their own graph. Pinning every dependency exactly was declined as well; it would
make the crate hard to combine with anything, and the evidence that a patch
release changes behavior exists only for `openraft`.

## 7. Out of scope

- **Ratification.** `028` B-5, and it is not an agent's act.
- **Upstream reporting.** `000`'s fork-governance boundary.
- **Enabling an enforcement rung.** `028` D-3.
- **Container images and the `build-image` recipe**, which still name the
  upstream registry and are not part of this release.
- **The remaining unrepaired findings**, each of which has a recorded reason in
  `029` section 4, `030` section 5, or the register.

## Verification

Run with `just spine-verify 031`. **This block is the acceptance for `003` and
`012` as well as this spec's** (D-6).

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
grep -q 'let rx_notify = Some(RemoteListener::spawn(' hiqlite/src/client/create.rs
sh -c 'grep -A2 "let (tx, rx) = flume::unbounded();" hiqlite/src/client/listen_notify.rs | grep -q "task::spawn(Self::handler"'
grep -q 'Connecting to listen SSE stream' hiqlite/src/client/listen_notify.rs
grep -q 'self.listen_rx().recv_async().await' hiqlite/src/client/listen_notify.rs
grep -q 'hiqlite/tests/cluster/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/012-cluster-integration-evidence'
# --- what this release adds ---
# B-8 / F-108 / F-109: carried in `032`'s block, which replaces this one (`amends_verification`)
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
# B-7 / F-104: the matrix compiles the server tree, and the proxy carries the same contract
sh -c 'grep -q -- "--features server -- -D warnings" justfile'
sh -c 'grep -q -- "--features server,cast_ints -- -D warnings" justfile'
sh -c 'grep -q "NotifyRequest::Listen((tx, ack))" hiqlite/src/server/proxy/handlers.rs'
cargo clippy --no-default-features --features server -- -D warnings
sh -c 'grep -q "\"dep:hiqlite-wal\"," hiqlite/Cargo.toml'
# every manifest points at the fork and says it is not upstream
sh -c 'test "$(grep -hc "bartekus/hiqlite" hiqlite/Cargo.toml hiqlite-wal/Cargo.toml hiqlite-derive/Cargo.toml | paste -sd+ - | bc)" -ge 3'
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
# the ledger
test -f standards/spec/release-ledger.md
sh -c 'grep -q "hiqlite-patched" standards/spec/release-ledger.md'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/031-downstream-release-qualification standards/spec/release-ledger.md'
```
