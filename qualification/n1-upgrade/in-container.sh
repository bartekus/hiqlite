#!/usr/bin/env bash
# Container side. `/src` is the repository, read-only; `/build` and `/runs` are volumes.
set -euo pipefail
cmd="$1"
src=/src/qualification/n1-upgrade
case "$cmd" in
  build)
    mkdir -p /build/bin
    start=$(date +%s)
    for side in old new; do
      for fs in rahi rauthy; do
        for extra in "" faults; do
          [ "$side" = old ] && [ -n "$extra" ] && continue
          feats="$fs${extra:+,$extra}"
          name="$side-$fs${extra:+-$extra}"
          # A private copy of the manifest directory: `/src` is read-only and cargo wants to
          # write nothing there, but it resolves `../src/node.rs` and the path dependency from
          # the manifest's location, so the copy keeps the same shape.
          echo "== building $name ($feats)"
          CARGO_TARGET_DIR="/build/target-$side" cargo build --release --locked \
            --manifest-path "$src/$side/Cargo.toml" --features "$feats" -j 4
          cp "/build/target-$side/release/node-$side" "/build/bin/node-$name"
        done
      done
    done
    echo "BUILD_SECONDS=$(( $(date +%s) - start ))"
    rustc --version; uname -m
    sha256sum /build/bin/* ;;
  run)
    stamp=$(date -u +%Y%m%dT%H%M%SZ)
    root="/runs/$stamp"
    mkdir -p "$root"
    BIN=/build/bin ROOT="$root" bash "$src/run.sh" $2 2>&1 | tee "$root/report.txt"
    echo "DISK_USED=$(du -sh /runs | cut -f1)" | tee -a "$root/report.txt" ;;
esac
