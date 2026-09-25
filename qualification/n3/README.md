# qualification/n3: the real-node N=3 harness

Spec `033-n3-topology-qualification`, B-2, D-2; built as lane D of
`standards/spec/n3-topology-proposal.md` section 16.

**This is harness construction, not qualification.** Nothing this directory runs
today is an A-scenario of 033 B-6, and no result it produces is evidence of N=3
support. The one implemented scenario, `smoke`, exists to show that the harness
itself starts, observes, stops and reports real node processes within its
bounds. Every invocation report carries `"counts_as_qualification": false`.

## What is here

| path | what |
|---|---|
| `Cargo.toml` | its own `[workspace]`, so the root workspace never sees it; a `[profile.release]` mirroring the root's (`panic = "abort"`, `lto = true`, `codegen-units = 1`, `strip = true`) |
| `Cargo.lock` | committed (`git add -f`: the root ignores every `Cargo.lock`). Seeded from the root lockfile, so every package the two share resolves to the root's version |
| `proto/` | the file and wire formats the harness and the node share |
| `node/` | `n3-node`: one hiqlite node as its own OS process. Exactly one of the features `rahi` or `rauthy` |
| `harness/` | `n3-harness`: topology, ports, TCP proxy, processes, signals, bounded waits, scenarios, reports |

The library's `cargo test`, `just test` and the root workspace do not build or
run anything here.

## Feature sets

| feature | hiqlite features |
|---|---|
| `rahi` | `sqlite, cache, counters, dlock, listen_notify_local, backup, s3` (no defaults) |
| `rauthy` | defaults (`auto-heal, backup, sqlite, toml`) plus `cache, cast_ints, counters, dashboard, listen_notify_local, macros` |

Every node runs with `cache_storage_disk = true`. The `LogSync` mode is a
parameter (`--log-sync`), because the owner's choice (proposal D-4) is not
recorded yet.

## Build

Each feature set gets its own target directory so both binaries can coexist.
The harness finds them at these paths by default (`--node-bin-rahi`,
`--node-bin-rauthy` override).

```sh
cd qualification/n3
cargo +1.95.0 build -j 4 --release -p n3-node --features rahi   --target-dir target/rahi
cargo +1.95.0 build -j 4 --release -p n3-node --features rauthy --target-dir target/rauthy
cargo +1.95.0 build -j 4 --release -p n3-harness
cargo +1.95.0 test  -j 4 -p n3-harness -p n3-proto              # harness internals only
```

Do not run `cargo fmt --all` here: it follows the path dependency and would
reformat `hiqlite/`. Use `rustfmt --edition 2024` on this directory's files.

## Run

```sh
target/release/n3-harness list
target/release/n3-harness run --scenario smoke --layout split
target/release/n3-harness run --scenario smoke --layout co-located
target/release/n3-harness run --scenario smoke --layout split --inject-failure kill-node
```

Parameters (`run --help` has all of them):

| flag | default | meaning |
|---|---|---|
| `--layout` | `split` | `split`: one node per simulated pod; `co-located`: one node of each cluster per pod |
| `--clusters` | `2` | clusters of three voters each (033 D-4: a `rahi` and a `rauthy` StatefulSet) |
| `--feature-set` | `rahi` | one set for every cluster, or one per cluster, comma-separated |
| `--log-sync` | `immediate` | `immediate`, `immediate_async` or `interval_<ms>` |
| `--base-port` | `29100` | first of `3 x nodes` consecutive ports |
| `--seed` | `1` | start order, written values, secrets; each run derives its own seed from it |
| `--fault` | `none` | `sigkill:<pod>`, `sigterm:<pod>`, `isolate:<pod>`, `link-down:<pod>:<pod>`, `heal`, comma-separated |
| `--runs` | `1` | fixed run count; stops at the first failed run |
| `--run-bound-secs` | `300` | wall-clock bound of one run |
| `--max-total-secs` | runs x run bound + 30 | wall-clock bound of the invocation |
| `--ready-bound-secs` | `120` | bound of each formation or convergence wait |
| `--op-bound-secs` | `30` | bound of each write or read through a node |
| `--term-grace-secs` | `30` | after `SIGTERM`, the wait before the harness sends `SIGKILL` |
| `--root` | `qualification/n3/runs` | each invocation gets a private directory under it |
| `--inject-failure` | none | `kill-node` or `impossible-assert`: a failure the harness must detect |

Layouts with the default two clusters: `split` is six simulated pods
(`c1-0..c1-2`, `c2-0..c2-2`), one node each, which is D-4's N=3 shape;
`co-located` is three pods (`pod-0..pod-2`), each holding node `k` of both
clusters, so one pod-level signal reaches a voter of each cluster, which is the
N=1 profile's shape. `--clusters 1 --layout split` is three pods.

Exit codes: `0` every run passed; `1` a run failed; `2` refused before any run
(unknown or unimplemented scenario, fault given to a scenario that takes none,
missing binary, a port in use or reserved); `3` the invocation bound was
reached before the run count (not a pass); `130` interrupted.

## How it works

**Processes.** Each node is `n3-node --launch <dir>/launch.json`, spawned by the
harness with stdout and stderr in `<dir>/node.<incarnation>.log`. Every run has
its own directory, `<root>/<unix-ms>-<scenario>-<layout>-s<seed>/run-<k>/`, with
one subdirectory per node holding its `data/`, launch file, logs, status files
and shutdown records. A restart archives the previous incarnation's
`status.json` and `shutdown.json` as `status.<n>.json` and `shutdown.<n>.json`.

**Ports and the proxy.** A node with ports `api, raft, ctl` binds `api` and
`raft` on `[::1]` and advertises `127.0.0.1:api` and `127.0.0.1:raft`, where the
harness proxy listens and forwards to `[::1]`. hiqlite takes a listener's port
from the advertised address, so the proxy needs a second loopback address with
the same port; `::1` exists on every host without configuration. Every raft and
API link therefore passes through the harness. `ctl` (`127.0.0.1`) is the
harness's own control channel and is not proxied. Before a run, every port is
test-bound on both addresses and the run is refused if any is taken; the
harness never stops a process it did not start. Ports `8080`, `8100-8103`,
`8200-8203`, `18411`, `18412`, `31001-31003`, `32001-32003`, `35001-35003` and
`36001-36003` are refused outright.

**Partitions** are proxy states (`proxy.rs`): an endpoint down refuses and
severs every connection to it; a link down `(src, dst)` refuses and severs
connections from `src`'s process to `dst`. Every node dials `127.0.0.1`, so the
source is attributed by mapping the peer port to its process with `lsof` and the
process to a node through the harness's own table. Attribution runs only while a
link rule names the destination, and fails closed: an unattributable connection
under such a rule is refused and counted. The smoke applies no partition; the
proxy's rules are exercised by its unit tests.

**Observation.** Each node rewrites `status.json` atomically every
`--status-interval-ms` (250 ms) with both groups' raft metrics from its own
`Client`: server state, term, leader, last log index, last applied log id,
membership log id, voters and learners, plus its phase and any terminal node
failure. The harness accepts a status only when its `pid` is the current
process's. Writes and reads go through the node's own `Client` over the
control socket: a durable-group read is `query_consistent`, a cache read is the
node's local `get`.

**Waits.** Every wait polls observed state (`--poll-ms`, 100 ms) under a fixed
bound and fails at the bound with the last observation. Every poll first checks
that every node the scenario expects alive is still running, so an unexpected
exit fails the wait at once instead of at the bound. There is no fixed sleep
that stands in for an observation.

**Stops.** The harness sends `SIGTERM` or `SIGKILL` itself, to every node of a
stop at the same moment, and measures each node from the signal to the observed
exit (50 ms poll resolution). On `SIGTERM` the node calls `Client::shutdown()`
once and writes `shutdown.json` with the result and its own measurement, then
exits (`0` Ok, `3` Timeout, `4` other error). Each node's stop is classified:

| outcome | meaning |
|---|---|
| `confirmed_ok` | `shutdown()` returned `Ok(())`: confirmed graceful completion |
| `unconfirmed_timeout` | `shutdown()` returned `Err(Timeout)`: the sequence possibly still running |
| `shutdown_error` | `shutdown()` returned another error |
| `forced_kill` | the grace elapsed and the harness sent `SIGKILL`: a confirmed forced exit before completion |
| `exited_without_record` | the process ended before `shutdown()` returned (crash, abort) |
| `killed` | a deliberate `SIGKILL` (fault, injection, cleanup) |
| `not_reaped` | still not reaped 10 s after `SIGKILL` |

Each stop event also carries a per-pod aggregate: the pod's duration is its
slowest node's, its outcome its worst node's. That is the shape A-9 reports.

**Reports.** `run-<k>/report.json` holds the configuration, every event with
its time, every stop event, each node's process state, last exit, last status
and shutdown record, and the proxy's per-endpoint counters. The invocation's
`report.json` holds the platform, the Git revision and whether the tree was
dirty, the node binaries, each run's summary and the result. A failing run's
directory is always kept; passing ones are kept unless `--discard-passing`.

**Bounds.** Each run has a wall-clock bound, clipped to what is left of the
invocation bound; a run that reaches it fails, its nodes are killed and its
directory kept. The invocation stops at the first failed run.

## Scenarios

`smoke` (implemented; harness validation only):

1. start every node of every cluster, in an order shuffled by the seed;
2. wait until each group of each cluster reports voters `{1,2,3}`, no learners,
   one agreed leader and one agreed membership log id on every member;
3. create the table, then write one key through the durable group and one
   through the cache group via each node;
4. wait until every member has applied its leader's last log index, then read
   every acknowledged write back through every member of its cluster;
5. `SIGTERM` every node at once and classify every stop;
6. restart every node on its data, wait for the same formation, record any
   change of membership log id (reported, not asserted), converge, and read
   every acknowledged write back through every member again;
7. `SIGTERM` every node at once again.

The smoke fails on a missed bound, an unexpected exit, a refused or
mismatching read, or a stop classified `forced_kill`, `exited_without_record`
or `not_reaped`. It records `unconfirmed_timeout` and `shutdown_error` without
failing on them: it measures nothing, and A-9 is where those outcomes count.

`a1-bootstrap` to `a10-full-stop-start` are registered and refuse to run
(exit `2`). Implementing and running them is lane E's work, under its own
authorization.

## What a smoke pass does and does not mean

It means the harness, on the host it ran on, started real release-build node
processes with the named feature set and `LogSync`, routed their links through
its proxy, observed formation and convergence from their own metrics, wrote and
read through both groups, delivered and classified its own signals, and wrote
its reports, all within its bounds.

It does not mean anything in 033 B-6 passed. It is one run, not a fixed
consecutive count; it injects no partition, no leader loss and no membership
change; its restart is the smoke's, not A-10's; its shutdown timings are not
A-9's measurement. One host means no kernel crash and no real disk loss.
macOS is not the target platform. It is not a support claim, at N=3 or at
N=1.

## Observed smoke results

Recorded 2026-09-23 by the construction lane, which allowed three smoke runs in
total and used exactly three. Host: macOS 26 (Darwin 25.5.0), arm64, 10 cores,
one machine. Toolchain `1.95.0`. Feature set `rahi` for both clusters,
`LogSync::Immediate`, `cache_storage_disk = true`, two clusters, seed `1`, base
port `29100`, default bounds. The tree was the uncommitted construction tree on
branch `feat/033-n3-harness` (the reports say `git_dirty: true`). Run
directories were kept under `runs/`, which is not committed.

| # | command | wall | result |
|---|---|---|---|
| 1 | `run --scenario smoke --layout split` | 40.9 s | exit `0`, pass |
| 2 | `run --scenario smoke --layout co-located` | 40.7 s | exit `0`, pass |
| 3 | `run --scenario smoke --layout split --inject-failure kill-node` | 14.6 s | exit `1`, fail as intended |

Runs 1 and 2 observed the same sequence: formation of all four groups (voters
`{1,2,3}`, leader `1`, membership log id `T1-N1-7`) in 14.3 to 14.7 s from
spawn, 12 acknowledged writes, convergence within 0.4 s, 36 matching reads;
both full stops `confirmed_ok` on all six nodes, harness-measured 9.69 to
9.79 s per node (node-measured 9.65 to 9.76 s, which is the fixed 9.5 s
pre-shutdown delay plus the sequence); re-formation after the restart in
6.1 to 6.2 s with the membership log id unchanged, and 36 matching reads
again. The proxy counted every raft and API connection, including the
upstream failures of peers dialing node 1 before it had bound.

Run 3 sent `SIGKILL` to `c1-n1` after the writes while the scenario still
expected it alive. The next bounded wait failed at once with `convergence after
writes: c1-n1 exited unexpectedly: signal 9`; the harness killed its five
remaining nodes, kept the run directory, and exited `1`.

The `rauthy` node binary was built in release on the same host (10 min 14 s
at `-j 4`; `rahi` took 6 min 57 s, the harness 38 s) and was not run.

These three runs validate the harness on one macOS host. They are not an
A-scenario run, count toward no run total of 033 B-6, and support no claim
about N=3.
