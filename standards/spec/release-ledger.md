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
| R-01 | cache log store against the locked OpenRaft contract | merged | `020`; eleven tests, all observed failing against the unrepaired implementation |
| R-02 | truncated WAL append stream, and the terminal-writer call paths | merged | `021`; three tests, observed failing; adapter-level recovery across a WAL file boundary |
| R-03 | replicated cache commands this build cannot apply | merged | `022`; five tests, four observed failing |
| R-04 | distributed-lock handler liveness, and the lease limits it does not remove | merged | `023`; eight tests, four observed failing |
| R-05 | exclusive storage ownership | merged | `024`; ten tests, four using two real processes |
| R-06 | snapshot publication, installation and restart selection as one contract | merged | `025`; eight tests, five observed failing, deterministic injection |
| R-07 | backup retention, restore ordering, validation and configuration failure | merged | `026`; seven tests, four observed failing |
| R-08 | startup errors, background-failure observation, the lifecycle API | merged | `027`; three tests **plus** an abort-profile binary, observed catching the defect at exit `134` |
| R-09 | the coverage denominator, the enforcement rungs, post-merge acceptance | merged | `028`; probes at the pin, recorded with their numbers |
| R-10 | the defects that reach a consumer, and a gate for every exclusion | merged | `029`; F-098 found and repaired; five acceptance blocks carried |
| R-11 | transport security and the dashboard's pre-auth surface | merged | `030`; nine tests; two defects recorded rather than repaired, with the argument |
| R-12 | package rename, version, publication and review workflows | merged | `031`; this ledger |
| R-13 | independent AI review of the candidate | reviewed | `.github/workflows/ai-review.yaml`, run against the tagged commit; findings and their disposition in `031` section 4 |
| R-14 | publication to crates.io | published | the registry's own response per crate, recorded in `031` section 4 |
| R-15 | external consumer verification | consumer-verified | a build outside this checkout with no workspace patch and no path dependency, recorded in `031` section 4 |
| R-16 | the fork release and its artifacts | published | the GitHub release for the tag |
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
