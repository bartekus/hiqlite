# N=1 upgrade exclusion: real-version harness

`035-n1-upgrade-exclusion` section 5, X-1 to X-5 and X-7: hiqlite 0.14 at upstream
`8f3b9bd` against the candidate in this repository, each node its own process,
under both consumers' feature sets, release builds, on Linux.

- `old/`, `new/`: two workspaces of their own, so hiqlite 0.14 never enters the
  library's graph. Both build `src/node.rs`; `new/` also builds with `faults`,
  which enables hiqlite's internal `__upgrade-fault-points` for X-5 only.
- `run.sh`: the scenarios and their bounds (three consecutive runs per scenario,
  60 s per run, 30 minutes per invocation, stop at the first failure and keep its
  directory). Assertions are on directory entries, inodes, sizes, content hashes
  and lock-file bytes, and on the old node writing, stopping `Ok` and restarting.
- `container.sh`, `in-container.sh`: build and run in `rust:1.95-bookworm`, 4 CPUs,
  8 GB, the runs on a named volume (a Linux filesystem, not a host bind mount).

```sh
./container.sh build                     # compile time is reported separately
./container.sh run "x1 x2 x3 x4 x5 x7"
./container.sh fetch <dest>              # copy the run logs out of the volume
```

`N1_PLATFORM=linux/amd64` selects the other architecture (emulated on an arm64
host, which exercises amd64 binaries on an arm64 kernel, not an amd64 kernel).
X-7 is recorded, never passed: the downgrade is unsupported (`035` B-4). Results
belong in `035` section 5.1, not here. Not part of `cargo test`.
