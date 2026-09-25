# Consumer handoff: `hiqlite-patched` 0.15.0-patched.1

For the Rauthy and Rahi releases that will carry these packages, and for the
coordinator. Everything below can be checked without this checkout: the registry
serves the packages, the GitHub release carries the tag, and the CI runs are
linked in section 9.

Owned by `031-downstream-release-qualification`. The per-item release state is
in `standards/spec/release-ledger.md`. Each finding id is an entry in
`standards/spec/findings-register.md`.

**Status: published 2026-09-22.** Every coordinate below comes from the
registry's and the forge's own responses. **Section 12 covers
`0.15.0-patched.2`, published 2026-09-24**, which carries `035`'s N=1 repair.

---

## 1. Release identity

| | |
|---|---|
| tag | `v0.15.0-patched.1` (signed) |
| source commit | `3392c12033f42f571b806d9ec24c5c5c9c40999a` on `spec-spine` |
| GitHub release | https://github.com/bartekus/hiqlite/releases/tag/v0.15.0-patched.1 |
| upstream baseline | `sebadob/hiqlite` `v0.14.0` plus 19 commits, to `52122ae7163d051d6b488d751018f76596f7d8f7` |
| repository | `https://github.com/bartekus/hiqlite` |

## 2. Published packages

| package | version | library | checksum (sha256 of the `.crate`) | registry |
|---|---|---|---|---|
| `hiqlite-patched` | `0.15.0-patched.1` | `hiqlite` | `456c1c117e5c581f6638572f26d9ef7cd567738c578e42ff0c8e09300534e7ca` | https://crates.io/crates/hiqlite-patched/0.15.0-patched.1 |
| `hiqlite-wal-patched` | `0.15.0-patched.1` | `hiqlite_wal` | `024992a08719a870bcef79192ed392cbef758b39caf0a60167541df379a05a9b` | https://crates.io/crates/hiqlite-wal-patched/0.15.0-patched.1 |
| `hiqlite-derive-patched` | `0.15.0-patched.1` | `hiqlite_derive` | `e2380bba9eb80f5d7ecb5a59097b91bed1a3600f9d6e659ad37362e6cb2df077` | https://crates.io/crates/hiqlite-derive-patched/0.15.0-patched.1 |

## 3. What to put in `Cargo.toml`

**Rauthy** (its current feature list, unchanged):

```toml
hiqlite = { package = "hiqlite-patched", version = "=0.15.0-patched.1", features = [
    "cache", "cast_ints", "counters", "dashboard", "listen_notify_local", "macros",
] }
```

**Rahi / `rahi-store`** (its current feature list, unchanged):

```toml
hiqlite = { package = "hiqlite-patched", version = "=0.15.0-patched.1", default-features = false, features = [
    "sqlite", "cache", "counters", "dlock", "listen_notify_local", "backup", "s3",
] }
```

**The `package =` alias is required.** The derive macros expand to
`::hiqlite::CacheVariants` and `::hiqlite::Row`, and `params!` to
`::hiqlite::Param`. Those absolute paths resolve against the **dependency key**,
so writing `hiqlite-patched = ".."` breaks every expansion. The library names are
unchanged, so every `use hiqlite::..` in your source keeps working.

`hiqlite-wal-patched` and `hiqlite-derive-patched` come in through
`hiqlite-patched`. Declare one directly only if you use it directly, in the same
shape: `hiqlite-wal = { package = "hiqlite-wal-patched", version = "=0.15.0-patched.1" }`.

**Delete any `[patch.crates-io]` entry for `hiqlite` or `hiqlite-wal`.** A patch
section binds only the workspace that contains it, and **published libraries do
not inherit it**. See section 10.

## 4. API adaptations

**Rauthy:** none required to compile. **Rahi:** none required to compile.

Things to know:

- **`params!` is exported only with the `macros` feature.** Rahi does not enable
  `macros`, so it builds a parameter list as `vec![hiqlite::Param::from(..), ..]`,
  or enables `macros`.
- **`hiqlite::Error` gained variants** (`CacheIncompatible`, `StorageInUse`,
  `NodeFailed`, `Startup`), and `hiqlite_wal::Error` gained `IncompleteAppend`.
  An exhaustive `match` on either needs a `_` arm.
- **`Client::shutdown()` now returns the shutdown's own result.** It still waits
  at most fifteen seconds and still releases storage ownership last. It used to
  return `Ok` whatever happened. Now:
  - `Err(Timeout)`, nothing stopped: a membership change admitted before the
    shutdown did not finish within five seconds. Call `shutdown()` again, or end
    the process.
  - `Err(..)`, a component did not stop: storage ownership is kept, and ending
    the process releases it. A later `shutdown()` reports this too.
  - `Err(Timeout)`, "continues in the background": fifteen seconds passed. The
    sequence continues while the runtime lives; ending the process first is a
    crash for whatever had not stopped, which the logs are built to recover from.
  - `Err(..)` also when the node's WAL writer had already failed (the node was
    out of service): a failed writer does not acknowledge a clean stop, so the
    shutdown says it was not one.
  - `ShutdownHandle::wait()` returns the same results.
- **An out-of-service node refuses its embedded client** with `Error::NodeFailed`
  on every query, execute, cache operation, lock and listen (F-110). Before, only
  the health checks refused. This is wired from source and not fault-injected
  here. Rauthy's WAL-failure injection against the published build confirmed
  it: every operation after the failure, including the failing write and a
  read, returned `NodeFailed`, and readiness went 503.
- **`lock()` never waits more than one lease plus two seconds per await**, then
  re-requests. A caller queued behind a holder that died acquires after about
  one lease, where it used to fail after 120 seconds (F-102).
- New and optional: `Client::node_failure()`, `Client::ensure_node_available()`,
  `Client::wait_until_healthy_{db,cache}_timeout()`, `Migrations::try_build()`,
  `ServerTlsConfigCerts::ca`, `S3Config::{try_from_env_checked, from_lookup,
  verify_access}`, `hiqlite::lifecycle::*`, `hiqlite::CACHE_LEGACY_MOVE_ASIDE_ENV`.
- Changed signatures: `ServerTlsConfig::from_env` returns
  `Result<Option<Self>, Error>`, `ServerTlsConfigCerts` gained a field, and
  `hiqlite::tls::build_tls_config(tls_no_verify, ca_path)` gained its second
  parameter (pass `None` for the previous behavior).

## 5. Upgrade, downgrade and recovery

**Read this before the first start on an existing data directory.**

| from | to | database and SQLite raft log | cache raft log and snapshots |
|---|---|---|---|
| upstream 0.14.x | this release | carried | **not carried**: see below |
| this release | upstream 0.14.x | carried | **must be moved aside by hand first** |
| this release | this release | carried | carried |

**Why the cache is not carried.** Upstream PR #362, in this release's baseline,
changed the cache raft's replicated command layout. A 0.14.0 cache log fails to
decode here, or decodes some entries as different commands (F-111). A
disk-backed cache (`cache_storage_disk = true`, the default for both
applications) is therefore refused on its first start, with `Error::Startup`
naming both directories. The refusal happens before anything is decoded, and the
error ends with "Nothing was changed."

**Stop every node first.** The format marker guards a node's own disk, not
replication. A node of this release in a cluster whose leader still runs 0.14.x
would receive legacy cache entries over the network and decode them in the new
layout. A mixed-version cluster is not supported in either direction. At N=1,
the supported topology, this does not arise.

**The upgrade procedure.** Start once with `HQL_CACHE_LEGACY_MOVE_ASIDE=true`.
That moves `{data_dir}/logs_cache` and `{data_dir}/state_machine_cache` into
`{data_dir}/pre-upgrade-<unix seconds>/`, logs each move, and starts with an
empty cache. You can leave the variable set: it does nothing once the new format
marker exists, or when the cache is in memory. The alternative is to stop the
node and move both directories aside yourself.

`HQL_DANGER_RAFT_STATE_RESET` keeps working with the guard: a reset start
re-marks the cache directory it empties, and the next ordinary start proceeds.

**What is lost:** cache contents, meaning in-flight auth and device codes,
WebAuthn challenges, PoW, rate limits, IP blacklist entries, counters and
distributed locks. An in-memory cache loses all of that on every restart anyway.
Rauthy keeps sessions in SQLite, so users stay logged in.

**Downgrade to 0.14.x is unsupported** (corrected 2026-09-23 by `036`, after
`035`'s probes). A hiqlite 0.14 binary on a directory any 0.15 build has written
can panic and tear both raft groups' metadata (`035` P-6, X-7, F-129), and
moving `logs_cache` and `state_machine_cache` aside by hand before a 0.14 start
is **not** shown safe: the SQLite raft log's compatibility from 0.15 back to
0.14 was never probed. The supported way back is to restore the verified
pre-upgrade archive into a fresh volume. No 0.15 build makes a 0.14 binary
refuse its directory. The earlier text of this paragraph recommended the manual
move-aside; do not follow it. (0.14.0 also refuses the `tls_*_ca` configuration
keys, which matters only for a restored configuration file.)

**Evidence.** The cycle was run once, on the directory Rauthy v0.36.2 wrote, with
Rauthy's 22-variant cache enum and feature set: upgrade with the opt-in,
downgrade to a build of upstream `v0.14.0`, then re-upgrade. The database carried
in both directions, with rows written on each side of each transition read back
on the other. **Not established:** cross-version snapshot readability (no SQLite
snapshot existed in the run), and a 0.14.x build without the `backup` feature,
which has a different `QueryWrite` layout and is outside this contract.

**Recovery.**
- An interrupted restore is **rolled forward** on the next start from its
  committed staged image. Before, the node restarted on the old database without
  its WAL.
- A node whose newest SQLite snapshot is unreadable falls back to an older one
  only if the local WAL still holds the entries from it onwards. Otherwise it
  refuses to start and names each rejected candidate.
- A torn trailing WAL record past the header is dropped, and the complete prefix
  recovers. A torn record **inside** the header's range, which a power loss can
  produce, is rolled back with `auto-heal`. **Without `auto-heal` the node
  refuses to start** with `Integrity`. Rauthy has `auto-heal` through the default
  features. **Rahi does not; consider enabling it.** Under `LogSync::Immediate`
  the rolled-back records were never acknowledged. Under `ImmediateAsync`, the
  default, they may have been.

**Data format.** The SQLite schema, the snapshot format, the WAL record format,
and the `QueryWrite` layout for builds with `backup` are unchanged. The
`CacheRequest` layout is the one described above.

## 6. F-107: membership changes and shutdown

**Observed:** a leader that had just removed itself from the voters still
accepted a peer's leave request. In debug builds that hit openraft's
`Only leader is allowed to call update_effective_membership()` assertion. It was
seen once in CI and once locally, on openraft 0.9.25.

**Established from source:** the decision ran before `raft_lock` was taken.
`post_membership` and the raft stream's `RemoveMembershipCache` changed
membership with no decision at all, and shutdown stopped both raft groups without
the lock. In a release build, openraft 0.9.25 appends the membership and rebuilds
replication on a node that is no longer in the leader state.

**Not established:** what that does to commit and to cluster membership in a
release build. It was never executed.

**The repair** (`027` B-9, D-8) puts every change through one gate:

- **Admission closes first.** Shutdown closes the gate before anything else;
  after that, requests are refused at once with `409`, meaning "ask another node".
- **Every change is decided under the gate's lock.** It requires this node to be
  the reported leader, in the `Leader` state, and a voter. That covers both raft
  groups, all six mutation paths, and the shutdown self-leave.
- **Shutdown drains, bounded.** It waits at most five seconds for an admitted
  change. On timeout it **stops nothing** and returns `Err(Timeout)`; it never
  falls through to an unsynchronized stop.
- **Every stop runs under the gate**, in a task a caller's timeout cannot cancel
  while the runtime lives.
- **Every wait under the gate is bounded:** thirty seconds for openraft
  membership calls, ten for commit visibility, and ten for a remote leave.
- **The membership helpers take the gate's token**, so a new path that bypasses
  it does not compile.

**Tests:** six deterministic interleaving tests, run on tokio's paused clock with
the schedule driven explicitly. Four mutations were each observed failing:
deciding before the lock, dropping the recheck under the lock, letting a drain
timeout fall through to a stop, and draining without closing admission. **No
test starts a node and races an HTTP membership request against shutdown**
(`027` KD-7).

## 7. F-108: two graphs

**The qualification graph** is what local runs and CI build: the committed
`Cargo.lock`. Both CI workflows that qualify a tree refuse a lock the manifests
do not already satisfy (`cargo metadata --locked`), print the toolchain and the
lock's digest, and end by proving that no step changed it. `just qualify` is the
local equivalent.

| | |
|---|---|
| `Cargo.lock` sha256 at the tag | `bfb7fd206c41bd5590a41cd6e31a77434ae0286915009f561165224916685034` |
| local toolchain | `rustc 1.95.0` |
| CI toolchain | `rustc 1.95.0 (59807616e 2026-04-14)`, `cargo 1.95.0` |
| openraft | `0.9.25` |

**The consumer graph** is what `hiqlite-patched` resolves in a workspace with no
path, Git or `[patch]` override. It was resolved and run separately from the
registry: see section 9.

- `openraft` is pinned `=0.9.25`, the only version qualified. That is because
  F-107 showed a patch release of the consensus library changing what a race
  does.
- The qualification graph covers the published workspace crates. The example
  smoke builds CI also runs resolve their own graphs, because `examples/` is
  outside the workspace.
- Every other dependency keeps a compatible range, so a fresh resolution may pick
  newer versions than the qualified lock, and nothing claims every permitted
  version was tested (`031` KD-7).
- The pin also has a cost: an `openraft` 0.9 fix needs a new `-patched.N` release
  (KD-8).
- Neither consumer graph contains the upstream `hiqlite` package. Both are
  recorded in section 9.

## 8. Disposition of every finding relevant to Rauthy and Rahi

Supported topology: **N = 1**. N = 3 is not advertised. Supported platforms:
Linux and macOS, on a local filesystem. Network filesystems are not supported,
and are not detected.

**Repaired in this release** (each with a test that fails without it, unless
noted):

| finding | what |
|---|---|
| F-001, F-002, F-028 | WAL append completion through the adapter; a cut-off append stream was a false success |
| F-003, F-004, F-006 | snapshot publication, installation and restart selection as one crash-recovery contract |
| F-005 | two processes owning one data directory |
| F-021 to F-024, F-029, F-047 | cache log store against the openraft contract |
| F-027 | a cache command this build cannot apply |
| F-040, F-039, F-009, F-089, F-061 | startup failures that panicked or ended the process |
| F-041, F-043, F-044 | TLS: a trust anchor, half a certificate pair, the API setting read from the raft endpoint |
| F-051 | remote listen subscription readiness |
| F-056 to F-063, F-100 | backup retention, restore ordering and validation |
| F-098 | an oversized entry killed the WAL writer |
| F-101 | a clean shutdown recorded as a failure |
| F-102 | a lock waiter parked for 120 s; an await changing lock state outside Raft |
| F-107 | membership changes against shutdown |
| F-108 | the qualification graph |
| F-109 | the containerized CI jobs ran under `sh` |
| F-110 | an out-of-service node serving its embedded client (refusal wired from source; not injected here) |
| F-111, F-112, F-113 | the upgrade from 0.14.x: the refusal, the reader abort that hid it, and a misrecorded failure |
| F-114 | a terminated WAL writer stranded an append queued behind its failure, and its producer (or a shutdown) waited forever; found as a CI stall, reproduced deterministically |
| review of the candidate | every WAL acknowledgement is failure-tolerant; a failed `init_pristine` stops what it started; the move-aside syncs both directories |
| backup `026` B-8 to B-10 | restore roll-forward, a durable backup, a retention floor of one copy |
| `HQL_WAL_SIZE` | a malformed value panicked `NodeConfig::default()`; any `wal_size` under the WAL's 8 KiB minimum is now refused by `is_valid` |

**Known limitations on a supported path** (not fixed; the consequence is stated):

| finding | consequence |
|---|---|
| F-026 | **the lock is a lease, not a fence.** A holder past its 10 s lease is not told, so two holders can overlap. Rahi uses `dlock`. |
| F-025 | a dead cache handler thread still panics the applying task; this needs an earlier panic to happen first |
| F-087, F-091 | the dashboard's login cooldown is global, so anyone who can reach it can lock the operator out; sessions cannot be revoked. Rauthy enables `dashboard`. Do not expose the API port to untrusted networks. |
| F-035 | the environment route always uses `LogSync::ImmediateAsync`; set `wal_sync` in code or TOML if you need `Immediate` |
| F-079 | a `FromRow` type mismatch panics rather than returning an error, as in 0.14.0 |
| `026` KD-8 | nothing reports an S3 upload's outcome except the log. `Client::backup()` success means the **local** backup is durable. |
| F-059 residual | a valid backup from a different cluster is accepted by a restore |
| `021` B-8 | without `auto-heal`, a torn record inside the WAL header's range needs an operator after a power loss |
| F-053 | no test opens a TLS connection |
| F-037, F-038 | an IPv6 advertised address is a startup error; `node_id` means two things |
| F-054 | a restart race behind a 1 s sleep in the N=3 test; at N=1 an immediate restart and write passed 6 of 6 per feature set |

**Outside the supported scope, and why:**

- **N > 1:** F-058 (uncoordinated restore) and F-054 at N=3. The shutdown
  pre-delay also leaves little of the fifteen-second budget when there are peers.
- **The `server` binary and proxy:** F-070, F-072, F-073, F-076. Neither
  application uses them.
- **Test-harness and governance findings:** F-048, F-049, F-055, F-081, F-090,
  F-094, F-095, F-016, F-018 and F-099. None reaches a published artifact.

## 9. Evidence, tied to the final candidate

| | |
|---|---|
| CI `Check` on the candidate head `9fd491f`, tree identical to the source commit | https://github.com/bartekus/hiqlite/actions/runs/35789836641 |
| publish workflow, secret-free checks then publication, on the source commit | https://github.com/bartekus/hiqlite/actions/runs/35793410166 |
| post-merge acceptance, all 33 blocks, on the source commit | https://github.com/bartekus/hiqlite/actions/runs/35791749020 |
| AI review workflow, on `b5039d2`; its findings acted on in #33 (031 KD-12) | https://github.com/bartekus/hiqlite/actions/runs/35787298830 |
| pre-merge independent reviews | fresh-context reviews of `a51cb3f` and `34641b0`. The first found one release-blocking defect (a remote re-await aborting the node) and five should-fix issues; all were acted on in `d45826c`. The second found a documentation gap it rated release-blocking (mixed-version clusters), three should-fix defects and four minor ones; all were acted on in `23d1e62` and `aaf866a` except a malformed `wal_size` in TOML being silently replaced by the default, which is recorded in `029`. A third, of `e1e9135`, found one release-blocking defect (the reset path refused its next restart) and two medium ones (a terminated writer's shutdown reported clean; work queued behind that shutdown); all were acted on in `4d8948b` |
| local acceptance | all 33 blocks on `4d8948b` in an isolated worktree; 300 isolated runs of the WAL suite with no failure; the cluster suite on `34641b0` and `70b8260` |
| external consumer, from the registry, no path, Git or patch override | passed. Rauthy's and Rahi's feature sets each start an N=1 node, write SQLite and cache, use the derive macros (Rauthy) or `dlock` (Rahi), shut down, restart at once and write again; the union set exercises the alias and both derives. Resolved `hiqlite-*-patched 0.15.0-patched.1` with the checksums above and openraft 0.9.25; no upstream `hiqlite` in either graph; consumer lock sha256 `ace62686…` (node consumer, both sets) and `4cbbd7e5…` (union) |

## 10. What Rahi must do to carry this

`rahi-store 0.2.0` on crates.io depends on `hiqlite = "0.14"`, and a published
version's dependencies are immutable. **Publishing hiqlite-patched changes
nothing for anyone resolving `rahi-store 0.2.0`**, and published libraries do not
inherit a workspace's `[patch]`. Rahi carries this release only through:

1. `rahi-store`'s manifest changed to the aliased declaration in section 3;
2. the `[patch.crates-io]` section and its comment removed from Rahi's workspace
   root;
3. a new `rahi-store` published, then a new `rahi` that depends on it;
4. the upgrade procedure in section 5 in Rahi's release notes, since Rahi runs
   with a disk-backed cache;
5. optionally, `auto-heal` added to Rahi's hiqlite features (section 5).

An application that depends on `rahi` gets this release only once that new
`rahi` exists.

## 11. Upstream

Nothing here was proposed to, reviewed by, or accepted by the upstream project.
This fork's governance binds only this fork. Prefer upstream `hiqlite` once a
release of it carries what you need: these packages exist to be replaced. Note
that upstream's own next release will contain the same cache log format change
(F-111).

## 12. 0.15.0-patched.2 and pinning (published 2026-09-24)

Added by `036-n1-repair-release`; the coordinates below come from the
registry's and the forge's own responses (`036` section 4).

| | |
|---|---|
| tag | `v0.15.0-patched.2` (signed), on `5c2cdef6c4168aeaa322f1a24d2b30b6f5f9d518` |
| GitHub release | https://github.com/bartekus/hiqlite/releases/tag/v0.15.0-patched.2 |
| publish run | https://github.com/bartekus/hiqlite/actions/runs/35952175390 |
| `hiqlite-patched` | `0.15.0-patched.2`, sha256 `67ae1ca7cd5c601fc0176f5e6e15dfc480b088b048ed9d482add288f655c229d` |
| `hiqlite-wal-patched` | `0.15.0-patched.2`, sha256 `d65dd8c35c40f8204c64c62a549614da93078e12d290e7db48937bc6c828d290` |
| `hiqlite-derive-patched` | `0.15.0-patched.2`, sha256 `ce54d2189eadd47c368537b9a6687afef94df64a1eed0ae192614f350a57f2e2` |

Rauthy (hiqlite defaults plus its list) and Rahi (`default-features = false`
plus its list) keep their feature lists from section 3 and change only the
version to `=0.15.0-patched.2`.

**What 0.15.0-patched.2 changes** (`035`): a live hiqlite node of either version
is refused before anything is moved, with or without
`HQL_CACHE_LEGACY_MOVE_ASIDE`; the consent move is one resumable operation that
works in `pre-upgrade-<secs>.partial/` until it completes; refusals are errors
that say what they created; WAL lock files are removed at the end of a clean
stop and kept after a failed start (the next start runs the deep integrity
check). Scope the consent variable to the upgrade start and remove it after the
first successful start. The downgrade rule of section 5 is unchanged: restore
the verified pre-upgrade archive into a fresh volume.

**Pinning, because 0.15.0-patched.1's requirements are caret.** The published
`hiqlite-patched 0.15.0-patched.1` requires `hiqlite-wal-patched` and
`hiqlite-derive-patched` `^0.15.0-patched.1`, which `0.15.0-patched.2`
satisfies. A published manifest cannot be changed, so once the new WAL crate is
on the registry:

- **Moving to 0.15.0-patched.2:** pin
  `hiqlite = { package = "hiqlite-patched", version = "=0.15.0-patched.2" }`. Its
  internal requirements are exact, so the other two follow. Move all three
  together; never combine versions.
- **Staying on 0.15.0-patched.1:** commit the lockfile and build with
  `--locked`, or pin `hiqlite-wal-patched` and `hiqlite-derive-patched` to
  `=0.15.0-patched.1` directly. Without either, a fresh resolution or a
  `cargo update` can combine the old `hiqlite-patched` with the new WAL crate: a
  graph nobody qualified, which still has F-126 and F-130.

**Rahi:** its exact pin moves only by its own governed decision. **Rauthy:** an
image rebuilt on the published packages, with its own F-130 acceptance. Neither
is done by this repository.

## 13. 0.15.0-patched.3: readiness after startup recovery

Added by `038-recovery-readiness-release`. The registry coordinates are
recorded by the change that records the publication, from the registry's and
the forge's own responses, as section 12's were.

**What 0.15.0-patched.3 changes** (`037`, F-134): after a start, each Raft
group's state machine applies the log the node held, and until it has, the node
is up but does not serve. After an unclean stop under `auto-heal` that is a
full rebuild of the SQLite database from the log; a disk-backed cache replays
its log on every start. Until the group recovers:

- `/health` and `/ready` answer `503` with `Recovering: ...`, including inside
  `health_check_delay_secs`, which no longer masks it;
- `is_healthy_db` / `is_healthy_cache` return `Error::Recovering` for their own
  group, so `wait_until_healthy_db` / `wait_until_healthy_cache` wait through it;
- every operation of the embedded client on that group, and every new remote
  client or event stream, is refused with `Error::Recovering`;
- `Client::recovery_state()` returns `Some(RecoveryState::Recovering(..))`, one
  `RecoveryProgress` (group, applied, target, `unclean_stop`) per group.

**What a consumer does.**

- **Wait for each group it uses before first use.** `wait_until_healthy_db`
  before database work and `wait_until_healthy_cache` before cache work. A
  consumer that uses a group straight after `start_node` now gets
  `Error::Recovering` instead of a partial state; retry or wait, do not treat it
  as fatal.
- **Report "recovering", not "down".** `Error::is_recovering()` (or
  `recovery_state()`) distinguishes a node that is catching up from
  `Error::NodeFailed` or an unreachable node.
- **Size the liveness probe.** A liveness probe on `/health` now fails for the
  length of a replay. Use a startup probe, or a liveness threshold longer than
  the longest expected replay, so a node is not restarted mid-recovery.
- **Local event listeners are unchanged** (`037` KD-1): a replay still
  re-delivers notifications to `listen_notify_local` listeners.

**Pinning.** Pin
`hiqlite = { package = "hiqlite-patched", version = "=0.15.0-patched.3" }`; its
internal requirements are exact, so the other two follow. Move all three
together, and build with `--locked` from a committed lockfile. The section 12
hazard still applies to `0.15.0-patched.1`, whose caret requirements accept the
new WAL crate: a consumer still on it pins `hiqlite-wal-patched` and
`hiqlite-derive-patched` to `=0.15.0-patched.1` or builds `--locked`.

**Yanking 0.15.0-patched.1 (guidance, not done).** Yanking it would stop new
resolutions from choosing it, and so from forming the mixed graph, while
leaving every existing lockfile working. It is **not** done by this release;
it needs a separate owner decision.

**Rauthy:** its F22 wait (`bartekus/rauthy` #7) becomes redundant for the
database once it runs this release, since `wait_until_healthy_db` then returns
only after the replay; whether to drop it is Rauthy's decision, verified with
its own crash leg. Rauthy uses the cache after waiting only for the database,
so it should also wait for the cache. **Rahi:** already waits for both groups;
adopting this release is its own governed decision.
