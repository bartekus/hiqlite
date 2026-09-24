---
id: "036-n1-repair-release"
title: "Release the N=1 exclusion repair alone, as 0.15.0-patched.2, with exact internal requirements"
status: draft
created: "2026-09-23"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "016-derive-macros"
  - "031-downstream-release-qualification"
  - "032-listen-notify-subscription-readiness"
  - "035-n1-upgrade-exclusion"
# D-1: `031` B-1 names the version all three packages carry. This spec moves it to
# `0.15.0-patched.2` and makes the internal requirements exact; `031`'s file is not edited.
# `032` is amended only in its acceptance block (D-2); its text is unchanged.
amends: ["031-downstream-release-qualification", "032-listen-notify-subscription-readiness"]
# D-2: `032`'s block is `031`'s acceptance (and through it `003`'s and `012`'s). It pins
# `^version = "0.15.0-patched.1"`. This spec carries that block forward whole, with that one
# command replaced and marked, rather than editing a predecessor's acceptance.
amends_verification: ["032-listen-notify-subscription-readiness"]
extends:
  # The version line of the derive crate's manifest (a crate unit resolves by package name).
  - spec: "016-derive-macros"
    unit: { kind: crate, id: "hiqlite-derive-patched" }
    nature: additive
  # The harness workspace's lockfile follows the version of the path dependency it builds.
  - spec: "035-n1-upgrade-exclusion"
    unit: { kind: directory, path: "qualification/n1-upgrade/" }
    nature: additive
  # Section 5's downgrade paragraph is corrected to `035` B-4, and section 12 is added.
  - spec: "031-downstream-release-qualification"
    unit: { kind: file, path: "standards/spec/consumer-handoff.md" }
    nature: superseding
  # The ledger's section for this release, added by the change that recorded the publication.
  - spec: "031-downstream-release-qualification"
    unit: { kind: file, path: "standards/spec/release-ledger.md" }
    nature: additive
  # The examples' lockfiles follow the workspace version (`031` KD-5's shape).
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
  - unit: { kind: file, path: "standards/spec/n3-topology-proposal.md" }
    role: "context"
summary: >
  The release identity of the standalone N=1 repair of `035`: all three
  packages at 0.15.0-patched.2, in lockstep, with the internal requirements on
  hiqlite-wal-patched and hiqlite-derive-patched made exact, and no other change
  to the dependency graph (openraft stays =0.9.25). States the exposure the
  published 0.15.0-patched.1 keeps (its caret requirements accept the new WAL
  crate), the pin and lock guidance consumers need because of it, the
  qualification the release tree must carry, and what stays separate:
  ratification, merge, tag, publication and each consumer's adoption. Carries
  `032`'s acceptance forward with its version command replaced.
---

# 036: Release the N=1 exclusion repair alone, as 0.15.0-patched.2

## 1. Purpose

`035` repaired the N=1 upgrade exclusion (F-126 to F-131, F-133). The repair
reaches no one until a release carries it (`035` B-7). This spec is that
release's identity and its obligations. It changes no behavior of the library:
the tree it describes differs from `035`'s reviewed candidate only in version
fields, two requirement operators, lockfile version lines, one README paragraph,
the consumer handoff and this document.

It is prepared under proposal decision D-18's recommended wording ("the `035`
repair ships alone, as `0.15.0-patched.2`"), which the owner selected for
preparation on 2026-09-23. Preparation is not publication: the tag, the upload
and the handoff of immutable identities each wait for a separate publication
authorization (B-6). D-17 (the exclusion handle) is deferred until a consumer
asks and is not in this release. D-19 is recorded as "state the unsupported
downgrade only": B-4's downgrade paragraph is that statement.

## 2. Territory

- **Amends** `031`: B-1's version and B-2's consumer line now name
  `0.15.0-patched.2`; the internal requirements become exact (B-2 here).
  **Amends** `032` in its acceptance block only (D-2).
- **Carries** `032`'s acceptance, which is `031`'s, `003`'s and `012`'s
  (`amends_verification`, D-2).
- **Extends** `016`'s derive crate (its version line), `035`'s harness directory
  (its lockfile), `017`'s examples (their lockfiles) and `031`'s consumer
  handoff (section 5's downgrade paragraph and a new section 12).
- **References**, without claiming, the unowned manifests and root lockfile it
  edits, the packaged README and the proposal's decision table.
- **Extends** `031`'s release ledger with this release's section, written by the
  change that recorded the publication (D-4).

**Boundaries.** hiqlite owns the three packages' identity, their requirements
and the guidance it gives. Each consumer owns its own pin, lockfile, rebuild
and release (`035` B-7): nothing here edits Rauthy or Rahi. crates.io owns what
it serves; a published version is immutable and never reused.

## 3. Behavior

### B-1. One version, three packages, in lockstep

`hiqlite-patched`, `hiqlite-wal-patched` and `hiqlite-derive-patched` are all
`0.15.0-patched.2`. `hiqlite-derive-patched` has no source change since
`0.15.0-patched.1`; it moves because `031` B-1 keeps one version for all three.
`0.15.0-patched.2` orders after `0.15.0-patched.1` and before any upstream
`0.15.0` (`031` B-1). The label was checked unused on 2026-09-23 and is checked
again immediately before the tag (B-6).

### B-2. The internal requirements are exact

`hiqlite/Cargo.toml` declares `version = "=0.15.0-patched.2"` for
`hiqlite-wal-patched` and `hiqlite-derive-patched`. A bare `"0.15.0-patched.1"`
is a caret requirement, and Cargo's caret on a prerelease of `0.15.0` is
satisfied by every later `0.15.0-patched.N` (and by `0.15.0` itself). With an
exact requirement, a resolver cannot combine this `hiqlite-patched` with a
different WAL or derive crate. The dependency keys (`hiqlite-wal`,
`hiqlite-derive`) are unchanged, so the feature table is untouched (`031` B-2).

### B-3. No other change to the graph

The root `Cargo.lock` changes only the three workspace packages' version lines.
No third-party package is added, removed or moved; `openraft` stays `=0.9.25`
(`031` B-8). The same holds for the lockfiles of `examples/` and of
`qualification/n1-upgrade/new/`, which name the workspace packages by path.
The graph comparison is part of the release evidence (section 4).

### B-4. What the published 0.15.0-patched.1 keeps, and what consumers do

**The caret exposure cannot be repaired from here.** `hiqlite-patched
0.15.0-patched.1` is published with caret requirements on
`hiqlite-wal-patched` and `hiqlite-derive-patched` `0.15.0-patched.1`. Exact
requirements in `0.15.0-patched.2` do not change a published manifest. Once
`hiqlite-wal-patched 0.15.0-patched.2` is on the registry, a consumer that keeps
`hiqlite-patched =0.15.0-patched.1` and **re-resolves** (a fresh resolution,
`cargo update`, or a lockfile that is not committed) can get the old
`hiqlite-patched` with the new WAL and derive crates. That mixed graph is not a
qualified configuration: it carries `hiqlite-wal`'s new lock lifecycle under
the old start order, and still has F-126 and F-130.

Three graphs, kept apart in every statement of evidence:

| graph | how a consumer gets it | status |
|---|---|---|
| all-old | `=0.15.0-patched.1` with a committed lockfile that already holds `hiqlite-wal-patched 0.15.0-patched.1` | qualified as `0.15.0-patched.1` (`031`), with F-126 to F-131 and F-133 open |
| all-new | `=0.15.0-patched.2` | the graph this release qualifies (section 4) |
| mixed | `=0.15.0-patched.1` without a lock, or updated after the new WAL crate is published | **unqualified**; any compile check of it is recorded as that and nothing more |

Consumer guidance, stated in the handoff (section 12):

- Pin `hiqlite = { package = "hiqlite-patched", version = "=0.15.0-patched.2" }`.
  The exact internal requirements then fix the other two.
- A consumer that stays on `0.15.0-patched.1` commits its lockfile and checks
  it (`cargo ... --locked`), or pins `hiqlite-wal-patched` and
  `hiqlite-derive-patched` to `=0.15.0-patched.1` directly. Moving means moving
  all three together.
- Downgrade: a hiqlite 0.14 binary on a directory any 0.15 build has written is
  unsupported. The supported way back is to restore the verified pre-upgrade
  archive into a fresh volume. A manual move-aside before a 0.14 start is not
  shown safe (`035` B-4, KD-1, X-7). Nothing in this release makes old binaries
  refuse a new directory; no universal old-binary fence is claimed (D-19).

### B-5. The release tree is qualified as itself

Evidence counts for this release only when it is bound to the tree it was
produced on. The release commit's tree is the one that is tagged. A squash merge
into `spec-spine` produces a new commit name with an identical tree; evidence
is bound to the tree hash, and the merged tree is compared before the tag.
Required on that tree:

1. The governance gates (pinned `spec-spine`), this block, and
   `spine-verify 035`.
2. The three `.crate` files rebuilt from the tree, with their file lists,
   manifests (the normalized `Cargo.toml` each contains), the lock graph, the
   feature sets, the toolchain and the SHA-256 of each. Unpublished
   interdependencies are resolved through a local overlay for this purpose
   only; the published files are built by `publish.yaml` from the tag, not
   reused from here.
3. `035`'s real-version harness (X-1 to X-5, X-7 recorded), release builds,
   `panic = "abort"`, both consumer feature sets, three runs each, within
   `035`'s bounds, on native Linux arm64, plus the failed-start shared
   descriptor regression (`035` D-11) in a release build on the same platform.
   Binaries built from the tree, and each binary's digest recorded with the
   run.
4. A native Linux amd64 leg of item 3, when a native amd64 runner is available.
   Emulated amd64 is not accepted evidence. If no native runner is available,
   the release record says the leg is missing; it is not waived silently.
5. CI `Check` and `govern` on the pull request that carries the tree.

The evidence from `3b11e4a` (`035` section 5.1) is evidence about that tree,
which is not this one, and is cited as such.

### B-6. Publication, adoption and ratification are separate acts

Merging this spec's pull request does not ratify it or `035`, and does not
publish. Publication needs its own authorization, presented with: the exact
release source and tag, the three package inventories and checksums, the
requirements and graph, every mandatory acceptance item and every missing leg,
the final review, CI and the consumer compatibility evidence. After
publication, `031`'s workflow and its external registry-only consumer check
(no path, Git or patch substitution) run, the registry checksums are compared
with the candidate's, and the ledger and handoff are updated on that evidence.
Rahi's re-pin and Rauthy's rebuild are their owners' acts.

## 4. Evidence and its limits

Recorded after the tag (D-3). The raw logs, binaries' digests, package
inventories and the consumer build are outside the repository under
`~/DevDep/hiqlite-release-artifacts/0.15.0-patched.2/` (`CANDIDATE.md` is the
index), as `031`'s were.

**Trees.** The runs used the frozen candidate `f5352a6` (tree `4fce93b9`). The
release commit is `5c2cdef` on `spec-spine` (tree `1c3c09bc`), the squash of
#38. The two trees differ only in one acceptance command of this spec (the
handoff sentence it greps wraps across a line) and two `.derived` shards; the
subtrees `hiqlite/`, `hiqlite-wal/`, `hiqlite-derive/`,
`qualification/n1-upgrade/` and `Cargo.lock`, `Cargo.toml` are byte-identical,
so every package and harness input is the one qualified.

| item (B-5) | where | result |
|---|---|---|
| 1. gates and acceptance | macOS arm64, pinned `spec-spine` 0.20.0 | `spine-check` 0; `couple` OK; `spine-verify 036` passed (128 commands) on `5c5f11c` (tree `1c3c09bc`); `spine-verify 035` passed on `f5352a6` |
| 2. packages | local, from `f5352a6` | three `.crate` files, 172, 17 and 7 files; exact internal requirements; `openraft =0.9.25` |
| 3. harness, arm64 | native Linux arm64 (Docker Desktop VM), 4 CPUs, 8 GB, named volume, release builds, `panic = "abort"` | X-1 to X-5: 30 of 30 pass; X-7: 6 recorded; 330 launches; 500 s |
| 3. D-11 regression, arm64 | same, release build | 3 of 3 launches pass (the WAL test; the start test under Rahi's and Rauthy's sets) |
| 4. harness, amd64 | native Linux amd64, GitHub-hosted runner, run 35947633017 (tree hash checked in the job) | X-1 to X-5: 30 of 30 pass; X-7: 6 recorded; 330 launches; 521 s |
| 4. D-11 regression, amd64 | same | 3 of 3 pass |
| 5. CI | PR #38 | `Check` and `govern` pass on `5c5f11c`; `Acceptance` passes on `5c2cdef` (run 35950937754) |

**X-7, recorded on both architectures.** hiqlite 0.14 over an upgraded
directory aborted in all twelve runs (in `hiqlite-wal`'s reader or its store
setup); neither group's `meta.hql` was torn; `state_machine/lock` was left; the
release refused afterwards under Rahi's set and started under Rauthy's
(`auto-heal`). The downgrade stays unsupported (B-4).

**Graph.** `cargo tree` for both consumer feature sets, against the published
`0.15.0-patched.1` tree: identical except the three workspace versions. The
**mixed graph** (B-4) was reproduced with a local overlay standing in for the
registry: `hiqlite-patched =0.15.0-patched.1` resolved to
`hiqlite-wal-patched` and `hiqlite-derive-patched` `0.15.0-patched.2` and
compiled under both feature sets. That is recorded as a compile check of an
unqualified configuration and nothing more.

**Publication** (2026-09-24). Signed tag `v0.15.0-patched.2` on `5c2cdef`
(GitHub: verified); publish run 35952175390, both jobs passed. The registry
serves:

| package | registry sha256 |
|---|---|
| `hiqlite-patched` | `67ae1ca7cd5c601fc0176f5e6e15dfc480b088b048ed9d482add288f655c229d` |
| `hiqlite-wal-patched` | `d65dd8c35c40f8204c64c62a549614da93078e12d290e7db48937bc6c828d290` |
| `hiqlite-derive-patched` | `ce54d2189eadd47c368537b9a6687afef94df64a1eed0ae192614f350a57f2e2` |

Each was downloaded anonymously and matched its checksum. The two leaf crates'
contents, apart from `.cargo_vcs_info.json` (which names the packaging commit),
are identical to the candidate's. `hiqlite-patched` differs from the candidate
in one file, its packaged `Cargo.lock`, which carries the registry `source` and
`checksum` lines of the two leaf crates that the candidate's local overlay could
not; those checksums are the published ones. A fresh consumer outside this
checkout, resolving only from the registry (no path, Git or patch), built,
exercised the alias and both derives, and started, wrote, stopped and restarted
an N=1 node under Rauthy's and Rahi's feature sets, resolving all three at
`0.15.0-patched.2` and `openraft 0.9.25`.

What this release does not establish: the Rauthy image, a Rahi cell, N>1, a
kernel crash or power loss at the move's fault points, and anything `035`
section 5 lists as not executed.

## 5. Known defects

**KD-1. The published caret requirements stay.** B-4. `0.15.0-patched.1`'s
manifest is immutable; only guidance and lockfiles protect its consumers from
the mixed graph.

**KD-2. Exact requirements cost a coordinated upgrade.** A consumer can no
longer take a later WAL crate without a new `hiqlite-patched`. That is the
intent (one qualified graph), and it is recorded because it is a constraint.

**KD-3. The label check is point-in-time.** B-1's check says what crates.io
served on 2026-09-23. It is repeated before the tag, and `publish.yaml` refuses
nothing by itself if the label were taken (`031` KD-6).

## 6. Resolved decisions

**D-1 (2026-09-23, amend `031` rather than edit it).** `031` records the
published `0.15.0-patched.1` and its evidence; its B-1 is true of that release.
Rewriting it would erase the record the ledger cites. This spec amends it for
the next version instead.

**D-2 (2026-09-23, this block is `032`'s).** `032` took `031`'s block
(`032` D-2), and one of its commands pins `^version = "0.15.0-patched.1"`. The
version commit would make it fail. The block is carried forward whole with that
command replaced and marked, and the release's own commands appended, as `032`
and `035` did.

**D-3 (2026-09-23, results after the tag, not in the release tree).** Recording
the harness results, checksums and CI runs in this spec before the tag would
change the tree after it was qualified, so either the evidence would describe a
different tree or the qualification would have to run again. They are kept
outside the tree, bound to its hash, and recorded here by the post-publication
change, as `031`'s results were (`72e09a6`).

**D-4 (2026-09-24, the ledger records this release in a section of its own).**
The ledger's rows describe `0.15.0-patched.1` and cite its evidence. This
release's rows are added in a separate section instead of editing those rows,
so each release's record stays true of that release.

## 7. Out of scope

- The N=3 repairs (proposal lane B), `034`'s restore, D-17's handle and
  D-19's fence.
- Any consumer's pin, lockfile, image or release.
- Ratification of `031`, `035` or this spec.

## Verification

Run with `RUSTUP_TOOLCHAIN=1.95.0 just spine-verify 036-n1-repair-release`.
**This block is `032`'s acceptance as well as this spec's** (D-2), and `032`'s
is `031`'s, `003`'s and `012`'s, so each of those resolves here. It does not
run the harness or package `hiqlite-patched` (whose published manifest resolves
the other two from the registry); B-5 lists those.

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
# `036` D-2: was `^version = "0.15.0-patched.1"`; the version this release carries.
sh -c 'grep -q "^version = \"0.15.0-patched.2\"" hiqlite/Cargo.toml'
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
# --- what 036 adds ---
# B-1: all three at the new version, in lockstep, in the manifests and in the lock
sh -c 'test "$(grep -h "^version = \"0.15.0-patched.2\"" hiqlite/Cargo.toml hiqlite-wal/Cargo.toml hiqlite-derive/Cargo.toml | wc -l | tr -d " ")" = "3"'
sh -c 'test "$(grep -A1 -E "^name = \"hiqlite(-wal|-derive)?-patched\"$" Cargo.lock | grep -c "^version = \"0.15.0-patched.2\"$")" = "3"'
sh -c 'test "$(grep -A1 -E "^name = \"hiqlite(-wal|-derive)?-patched\"$" qualification/n1-upgrade/new/Cargo.lock | grep -c "^version = \"0.15.0-patched.2\"$")" = "3"'
sh -c '! git grep -q "^version = \"0.15.0-patched.1\"" -- "*Cargo.lock"'
# B-2: the internal requirements are exact
sh -c 'grep -q "hiqlite-derive = { package = \"hiqlite-derive-patched\", version = \"=0.15.0-patched.2\", path = " hiqlite/Cargo.toml'
sh -c 'grep -q "hiqlite-wal = { package = \"hiqlite-wal-patched\", version = \"=0.15.0-patched.2\", path = " hiqlite/Cargo.toml'
# B-4: the packaged README and the handoff give the exact pin and the downgrade rule
sh -c 'grep -q "version = \"=0.15.0-patched.2\"" hiqlite/README.md'
grep -q '^## 12\. ' standards/spec/consumer-handoff.md
grep -q 'Downgrade to 0.14.x is unsupported' standards/spec/consumer-handoff.md
# the phrase wraps in the document, so it is matched with line breaks folded to spaces
sh -c 'tr "\n" " " < standards/spec/consumer-handoff.md | grep -q "restore the verified pre-upgrade archive into a fresh volume"'
# every lockfile still satisfies its manifest
cargo metadata --locked --format-version 1 --no-deps --manifest-path qualification/n1-upgrade/new/Cargo.toml
sh -c '! grep -rl "$(printf "\342\200\224")" specs/036-n1-repair-release standards/spec/consumer-handoff.md hiqlite/README.md'
```
