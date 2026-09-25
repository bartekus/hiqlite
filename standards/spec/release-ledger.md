# Downstream release work ledger

The state of one release: `hiqlite-patched`, `hiqlite-wal-patched` and
`hiqlite-derive-patched` at `0.15.0-patched.1`, published from
`bartekus/hiqlite` so that patched Rauthy and Rahi builds can consume the
repairs in it.

Owned by `031-downstream-release-qualification`. The adoption plan's section 8
is the **adoption** queue and says publication and release are deliberately not
in it; this is the release queue and says nothing about adoption.

## How to read a row

Seven states, in order, and a row never skips one:

| state | what it means |
|---|---|
| proposed | written down here, authorized by nothing |
| authorized | the owner directed it |
| implemented | the code exists |
| tested | a regression exists that fails without it |
| reviewed | an independent reviewer looked at it |
| merged | it is on the integration branch |
| published | the registry serves it |
| consumer-verified | a consumer outside this checkout built against it |

**A row moves on evidence, and the evidence is named.** "Tested" means a named
test; "published" means a registry response; "consumer-verified" means a build
outside this repository. Anything else stays where it is.

## Release identity

| | |
|---|---|
| packages | `hiqlite-patched`, `hiqlite-wal-patched`, `hiqlite-derive-patched` |
| version | `0.15.0-patched.1` |
| upstream baseline | `v0.14.0` plus 19 commits, to `52122ae7163d051d6b488d751018f76596f7d8f7` |
| integration branch | `spec-spine` on `bartekus/hiqlite` |
| supported topology | `N = 1`. See `026` B-7 |

**Why `0.15.0` and not `0.14.x`.** The public API changed: `Error` gains four
variants across the two crates, `writer::spawn` returns a third value,
`ServerTlsConfigCerts` gains a field, `ServerTlsConfig::from_env` returns a
`Result`, and `Client` gains five methods. Under `0.x` a minor bump is the
breaking-change signal, so a patch-level version would claim a compatibility
this release does not have.

**Why the names are distinct.** The three names are not upstream's. Nothing here
was proposed to, reviewed by or accepted upstream, and a crate that resolves
under upstream's name would say otherwise. All three were checked available on
2026-09-21 and are owned by this publication.

## Work

| id | subject | state | evidence |
|---|---|---|---|
| R-01 | cache log store against the locked OpenRaft contract | published | `020`; eleven tests, all observed failing against the unrepaired implementation |
| R-02 | truncated WAL append stream, and the terminal-writer call paths | published | `021`; three tests, observed failing; adapter-level recovery across a WAL file boundary |
| R-03 | replicated cache commands this build cannot apply | published | `022`; five tests, four observed failing |
| R-04 | distributed-lock handler liveness, and the lease limits it does not remove | published | `023`; eight tests, four observed failing |
| R-05 | exclusive storage ownership | published | `024`; ten tests, four using two real processes |
| R-06 | snapshot publication, installation and restart selection as one contract | published | `025`; eight tests, five observed failing, deterministic injection |
| R-07 | backup retention, restore ordering, validation and configuration failure | published | `026`; eight tests, five observed failing, the eighth against this spec's own first implementation (F-100) |
| R-08 | startup errors, background-failure observation, the lifecycle API | published | `027`; three tests **plus** an abort-profile binary, observed catching the defect at exit `134` |
| R-09 | the coverage denominator, the enforcement rungs, post-merge acceptance | published | `028`; probes at the pin, recorded with their numbers |
| R-10 | the defects that reach a consumer, and a gate for every exclusion | published | `029`; F-098 found and repaired; five acceptance blocks carried |
| R-11 | transport security and the dashboard's pre-auth surface | published | `030`; nine tests; two defects recorded rather than repaired, with the argument |
| R-12 | package rename, version, publication and review workflows | published | `031`; this ledger |
| R-18 | the remote client's event subscription, and the full-suite gate it was blocking | published | `032`; three tests; the cluster integration suite runs to completion for the first time in this repository |
| R-19 | F-107: one membership gate, decided under its lock, and a shutdown that drains before it stops | published | `027` B-9, D-8; six interleaving tests, four mutations each observed failing; fresh-context review of `a51cb3f`, findings acted on in `d45826c` |
| R-20 | F-108: the committed qualification graph, and `openraft = "=0.9.25"` | published | `031` B-8; `cargo metadata --locked` in both qualifying workflows; same review |
| R-21 | F-102: a lock waiter bounded by the lease, and an await that changes nothing | published | `023` B-6; six tests, each observed failing against the handler before it; the review's release-blocking finding was in this work and is repaired |
| R-22 | backup: restore roll-forward, a durable backup, a retention floor of one | published | `026` B-8 to B-10; three tests, each observed failing under mutation; same review |
| R-23 | F-110: an out-of-service node refuses its embedded client | implemented | `027` B-9; shipped in `0.15.0-patched.1`. **Confirmed by the Rauthy integration's fault injection against the published crates** (logs directory made read-only under a live node): the lifecycle recorded the WAL writer failure and every later operation, including the failing write and a read, returned `Error::NodeFailed`; readiness went 503; the process did not abort. The state stays `implemented` because no regression in this repository drives it |
| R-24 | F-109: the containerized workflows run bash | published | `031` B-8; observed failing in CI on `a51cb3f`, passing on every `Check` from `d45826c` on |
| R-25 | W-07: torn versus complete trailing WAL records | published | `021` B-8; three tests, with and without `auto-heal`; evidence for existing behavior |
| R-26 | the upgrade from 0.14.x: a legacy cache refused before anything opens, a move-aside opt-in, the reader that no longer aborts | published | `027` B-10, KD-11; F-111 to F-113; four tests; upgrade, downgrade and re-upgrade run on Rauthy's directory; a refused start leaves it byte-identical apart from the owner lock; fresh-context review of `34641b0`, findings acted on in `23d1e62` |
| R-27 | F-114: a terminated WAL writer refuses queued work instead of stranding it | published | `021` B-9; a deterministic test that fails without the repair; 300 isolated runs of the WAL suite without a failure; fresh-context review of `e1e9135` found a clean shutdown reported for a terminated writer and a rollover failure that answered nobody, both acted on in the next commit |
| R-13 | independent AI review of the candidate | reviewed | run 35787298830 on `b5039d2`, through the CLI (031 KD-10); findings acted on in #33 (031 KD-12); the #33 delta reviewed fresh-context before merge |
| R-14 | publication to crates.io | published | publish run 35793410166 on `3392c120`; the registry serves all three at `0.15.0-patched.1`, checksums in the handoff |
| R-15 | external consumer verification | consumer-verified | a consumer outside this checkout, no path, Git or patch override, resolved the three packages from the registry and ran a node under Rauthy's and Rahi's feature sets; the handoff records the graph |
| R-16 | the fork release and its artifacts | published | https://github.com/bartekus/hiqlite/releases/tag/v0.15.0-patched.1 |
| R-17 | the consumer handoff | published | `standards/spec/consumer-handoff.md` |

## What this release does not do

Stated here so a reader does not have to infer it from an absence.

- **It does not ratify anything.** Every spec in the corpus is `draft`, which
  `spec-spine registry list` prints. Ratification is a human act and this
  release performs none (`028` B-5).
- **It does not enable an enforcement rung.** `028` B-3 measured both and
  enabled neither.
- **It does not support a multi-node restore.** `026` B-7.
- **It does not report anything upstream.** `000`'s fork-governance boundary
  stands: nothing here was proposed to or accepted by the upstream project.
- **It does not claim the cluster suite passes.** The remote-client stall
  (F-051) is unrepaired and `012`'s evidence limits are unchanged.

## 0.15.0-patched.2 (published 2026-09-24)

Added by `036-n1-repair-release` (`036` D-4). The rows above describe
`0.15.0-patched.1` and are not edited. This release carries one repair, `035`,
alone; its identity and evidence are `036` section 4.

| | |
|---|---|
| version | `0.15.0-patched.2`, all three packages; `hiqlite-patched` requires the other two at `=0.15.0-patched.2` |
| release commit | `5c2cdef6c4168aeaa322f1a24d2b30b6f5f9d518` on `spec-spine` (squash of #38), tree `1c3c09bc` |
| supported topology | `N = 1`, unchanged |

| id | subject | state | evidence |
|---|---|---|---|
| R-28 | `035`: exclusion of every live node before the legacy cache move, the lock handoff, the resumable consent move | published | `035` U-1 to U-7 and the regression file (observed failing on the published source); real-version X-1 to X-5 30 of 30 on native Linux arm64 and native Linux amd64, X-7 recorded; `035` D-11's regression in a release build on both |
| R-29 | exact internal requirements (`=0.15.0-patched.2`) | published | `036` B-2; the registry's `hiqlite-patched` manifest; a registry-only consumer resolved all three at `0.15.0-patched.2` |
| R-30 | independent review | reviewed | the independent review of `3b11e4a` (`035` D-11), acted on in `787e6aa`. The AI review run 35953622705 on `5c2cdef` completed but produced no report (the CLI printed only its last message); it is not counted as a review |
| R-31 | publication to crates.io | published | signed tag `v0.15.0-patched.2`; publish run 35952175390; checksums in the handoff's section 12; each download matched its checksum |
| R-32 | external consumer verification | consumer-verified | a consumer outside this checkout, registry only (no path, Git or patch), both applications' feature sets: built, exercised the alias and both derives, started, restarted and wrote an N=1 node |
| R-33 | the fork release | published | https://github.com/bartekus/hiqlite/releases/tag/v0.15.0-patched.2 |
| R-34 | the consumer handoff | published | `standards/spec/consumer-handoff.md` section 12; section 5's downgrade paragraph corrected to `035` B-4 |

**Not established by this release:** the Rauthy image, a Rahi cell, N>1. Each
consumer's re-pin, rebuild and release is its owner's act.

## 0.15.0-patched.3 (published 2026-09-25)

Added by `038-recovery-readiness-release`. The rows above are not edited. This
release carries one repair, `037` (F-134), alone; its identity and evidence are
`038` section 4.

| | |
|---|---|
| version | `0.15.0-patched.3`, all three packages; `hiqlite-patched` requires the other two at `=0.15.0-patched.3` |
| release commit | `6c8db22014bafd79ec1cff2ec91526e816d5f75c` on `spec-spine` (squash of #42), tree `713f4a6b` |
| supported topology | `N = 1`, unchanged |

| id | subject | state | evidence |
|---|---|---|---|
| R-35 | `037`: not healthy, not ready and serving nothing until startup recovery completes; `Error::Recovering`, `Client::recovery_state` | published | `037`'s regression observed failing on the published `0.15.0-patched.2` library and passing on this tree, on macOS arm64, native Linux arm64 and native Linux amd64, under both applications' feature sets |
| R-36 | `036` B-5 on this tree | qualified | X-1 to X-5 30 of 30 and the failed-start regression 3 of 3 on native Linux arm64 and amd64, release builds |
| R-37 | exact internal requirements (`=0.15.0-patched.3`) | published | the registry's `hiqlite-patched` manifest; a registry-only consumer resolved all three at `0.15.0-patched.3` |
| R-38 | AI review | reviewed | run 36089797652 on `6c8db22`; its findings predate this release and are F-135 to F-137 (`038` D-5), not repaired here |
| R-39 | publication to crates.io | published | signed tag `v0.15.0-patched.3`; publish run 36118177148; checksums in the handoff's section 13 |
| R-40 | external consumer verification | consumer-verified | a consumer outside this checkout, registry only, both applications' feature sets: built, exercised the alias and both derives, started, restarted and wrote an N=1 node |
| R-41 | the fork release | published | https://github.com/bartekus/hiqlite/releases/tag/v0.15.0-patched.3 |
| R-42 | the consumer handoff | published | `standards/spec/consumer-handoff.md` section 13 |

**Not established by this release:** the Rauthy image, a Rahi cell, N>1, a
real `SIGKILL` leg. Each consumer's re-pin, rebuild and release is its owner's
act.
