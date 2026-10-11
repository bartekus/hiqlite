# Upstream reconciliation, 2026-10-10

This is the implementation and review matrix for spec 045, not a release or support declaration.
The target is a smaller, justified patch set atop upstream behavior. A patch is removable only
when equivalent behavior passes the fork's regression under both consumers' feature sets.

## Exact starting points

- **Released fork and integration branch**: `df9023c6f3bc59c076dbd0a594f32e051ed3e186`,
  `v0.15.0-patched.4`
- **Existing draft PR 62, spec 044**: `0d11c08779af29ceb6bcfea0e1991c5adc5655c4`
- **Latest upstream release**: `v0.15.2`, `18e3e71be1b40465f82e1f26f17b7d1bfcc6995d`
- **Upstream main including subsequent fixes**: `83a9680a35860b83405c42284739a1761cd8111c`
- **Common ancestor**: `52122ae7163d051d6b488d751018f76596f7d8f7`

At these identities, upstream has 13 commits absent from the fork, and the fork has 135 commits
absent from upstream. This includes governance, tests, and release evidence. It is not a count of
independent runtime patches. Local `spec-spine` and `upstream/main` refs were stale; use the exact
identities above rather than assuming those local names identify the evaluated trees.

## Upstream commit disposition

- **`9824bcfc` robustness and security (#368)**: Selective port from 044 retained; incompatible
  surfaces below remain separate work.
- **`cc0c64c9` prepare 0.15.0**: Do not copy upstream package/release identity into the patched
  distribution.
- **`e0a6a8e9` server Dockerfile**: Independent packaging work; no consumer image built by this
  candidate.
- **`f8391c89` socket-task cancellation (#373)**: Ported with cancellation regression and real
  socket resource checks.
- **`80a3c2c6` TCP keepalives (#374)**: Ported into the fork's common listener and lifecycle
  helpers.
- **`eedcf61e` dashboard UTF-8 panic (#375)**: Existing fork token parser already avoids byte
  slicing and covers comments/CTEs. Retain its regression.
- **`45996fa6` WAL and startup robustness (#376)**: Retain stronger WAL/recovery/derive repairs.
  Cache cleanup during restore needs separate integration.
- **`e1548dd5` prepare 0.15.1**: Release metadata only; not a patched release.
- **`82826c52` source-map-js update (#378)**: Dashboard dependency maintenance remains separate;
  no UI build in this candidate.
- **`80baf897` remove automatic cluster wait (#379)**: Preserve the fork's local replay barrier.
  Optional distributed catch-up is a different API.
- **`18e3e71b` prepare 0.15.2**: Release metadata only; not a patched release.
- **`b8642d67` external engine dependencies (#382)**: Move SQLite extension features to `sqlite`;
  make constant-time comparison cluster-only.
- **`83a9680a` split-brain API TLS (#384)**: Fork already uses API trust configuration. Port
  one-client-per-checker reuse.

These dispositions describe hunk-level reconciliation. They do not make upstream's commits
ancestors of the fork, and do not claim full 0.15.2 API or on-disk compatibility.

## Necessary retained patches and removal criteria

- **WAL append completion, terminal writer failure, vote/remove ordering**: Why retain it:
  Incomplete work must not be acknowledged as successful. Criterion before removing its
  implementation: Run existing WAL fault regressions against the replacement.
- **WAL creation file and parent-directory sync**: Why retain it: Upstream file sync does not
  establish directory-entry durability. Criterion before removing its implementation: Equivalent
  durable creation sequence plus 040 regressions.
- **Exclusive ownership before migration/restore/reset**: Why retain it: Concurrent mutation can
  corrupt consumer storage. Criterion before removing its implementation: Negative second-opener
  and startup mutation-order regressions.
- **Crash-safe snapshots and restore staging**: Why retain it: Published bytes and replay state
  must agree after interruption. Criterion before removing its implementation: Snapshot/install
  and restore crash-transition acceptance.
- **Local replay readiness and `Recovering`**: Why retain it: Rauthy bootstrap and Rahi startup
  must not read a transient empty store. Criterion before removing its implementation: Unclean
  restart through public readiness and query surfaces.
- **`NodeFailed`, failure observation, truthful shutdown**: Why retain it: Consumers must refuse
  terminal storage and distinguish failed shutdown. Criterion before removing its implementation:
  Terminal-operation and shutdown-error regressions.
- **API CA configuration and subscription acknowledgement**: Why retain it: Consumers use internal
  trust and local subscription readiness. Criterion before removing its implementation: Verified
  CA-path and subscribe-before-publish acceptance.
- **Bounded lock waits plus Rahi SQL fencing**: Why retain it: A cache lease alone cannot fence
  stale workers. Criterion before removing its implementation: Rahi stale-holder, restart, and
  supersession tests with proposed replacement.
- **Explicit consent for legacy cache exclusion**: Why retain it: Cache loss has identity and
  security consequences for Rauthy. Criterion before removing its implementation: Versioned
  migration, refusal, rollback, and consumer impact qualification.

Similarity of function names is insufficient evidence for removing a patch. In particular,
upstream's optional cluster catch-up wait does not replace a startup local replay barrier, and
automatic lock heartbeats do not replace SQL fencing.

## Changes needing a migration contract

Upstream inserts `LockAlive` at command index 10, moving `LockAwait`, `LockRelease`, and counters.
The fork retains the older command discriminants. Upstream also changes the cache snapshot shape.
Do not roll mixed fork/upstream members or reuse persisted cache bytes without a qualified decoder
or explicit reset. The bincode package change alone does not establish incompatibility or safety.

Before adopting that surface, define the accepted source versions, format marker, refusal before
mutation, authenticated export or reset consent, and full-archive rollback. Test both directions
where supported; explicitly refuse downgrade where it cannot be safe. Preserve SQL and Raft state
together. Cache cleanup in upstream #376 must integrate with the fork's staged restore and storage
lock, rather than copying destructive cleanup before its transactional boundary.

Keep the public configuration renames, remote notification WebSocket redesign, derive rewrite,
`/version` API, and lock heartbeat adoption in individually reviewable slices. Preserve aliases
needed by current Rust consumers. Upstream's 0.15.0 `Option` derive regression was fixed in 0.15.1;
the rationale for retaining the fork derive is now behavioral parity testing, not an assertion
that current upstream still has that regression.

## Downstream sequence

1. Review and integrate spec 044 independently, then this candidate. Neither draft is ratified here.
2. Qualify a new, explicitly named patched release, including native Linux arm64 and amd64.
   Do not republish patched.4 with changed bytes.
3. In Rahi, explicitly select `LogSync::Immediate` for the durability-sensitive store, rather than
   inheriting `ImmediateAsync`. Test actual startup configuration, acknowledged receipt/claim/fence
   preservation on restart, and failure propagation. Measure write latency before deployment.
   Synchronous mode strengthens WAL acknowledgement; it alone does not prove a multi-node support
   claim or compensate for storage that ignores synchronization.
4. Adopt the qualified registry release in Rahi in its own governed change. The evaluated release
   pins `hiqlite-patched =0.15.0-patched.3`; patched.4 is not already adopted there.
5. Build and qualify Rauthy against that named release independently. Rauthy
   `0.36.2-patched.3` embeds Hiqlite `0.15.0-patched.2`. Preserve its pre-bootstrap replay wait,
   terminal readiness response, bounded shutdown attempt, and upgrade consent behavior.
6. Qualify any cache migration separately for both consumers. Rauthy cache reset drops temporary
   authentication challenges, DPoP nonces, IP bans, and abuse counters; SQL sessions surviving does
   not make the reset security-neutral. Its DPoP enforcement change is not a Hiqlite patch.
7. Treat N=3/lane-B as a separate support decision with the existing scenario matrix and consumer
   deployment evidence. A three-node ordinary test is not N=3 release qualification.

This sequence makes the next review concrete while leaving publication, consumer adoption, and
distributed support as distinct acts with their own evidence. No Rahi or Rauthy pins change here.

## Local validation

Validated with Rust 1.95.0 on macOS arm64, with S3 restore explicitly skipped:

- `cargo test --locked --workspace --features full,external-state-machine`: 253 tests passed.
  This includes recovery readiness, upgrade exclusion, lease wake, derive, and WAL regressions.
- The cluster integration executable passed with
  `full,counters,listen_notify,external-state-machine` and `TEST_SKIP_S3_RESTORE=true`,
  within the 300-second bound. It covered remote operations, notifications, local backup restore,
  stale membership, state-machine crash recovery, and full-volume-loss self-healing.
  Cache storage was in memory as configured by the existing test harness. This is ordinary
  integration evidence, not N=3 release qualification.
- `just spine-verify 045-upstream-reconciliation`: all four commands passed, including the
  cancellation regression, socket-option tests, dashboard parser tests, and external-engine build.
- Cancellation negative control: removing the guard's drop cancellation while retaining the
  regression made it fail at its two-second bound. Restoring cancellation made it pass.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- Library Clippy with warnings denied: passed for `server,external-state-machine`,
  Rahi's `--no-default-features` with `sqlite,cache,counters,dlock,listen_notify_local,backup,s3`,
  Rauthy's defaults with `cache,cast_ints,counters,dashboard,listen_notify_local,macros`,
  and `--no-default-features` with `external-state-machine`.
- The external-only normal dependency tree excludes OpenRaft, reqwest, socket2, and
  constant_time_eq. Fork-required SQLite backup, functions, and hooks remain enabled.
- Formatting, whitespace checks, pinned spec-spine 0.26.0 freshness/lint, and pre-commit coupling
  passed. Coupling was checked against both spec 044's head and the fork integration base.

These checks build Hiqlite with the consumers' feature selections. They do not build or run the
consumers themselves. Native Linux release qualification, S3 restore, publication, consumer pin
updates, and N=3 release qualification remain outstanding.

Raw logs are local ignored evidence under `.statecraft/state/evidence/045/` in this worktree.
