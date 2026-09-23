#!/usr/bin/env bash
# Host side of `035`'s real-version harness: build the two node binaries and run the scenarios
# in a Linux container, with release builds, on a Linux filesystem (a named volume, never a
# host bind mount, whose locking is the host's), inside the declared budget.
#
#   ./container.sh build            compile (timed separately; not part of the run budget)
#   ./container.sh run [SCENARIOS]  run, e.g. "x1 x2 x3 x4 x5 x7" (the default)
#   ./container.sh fetch DEST       copy the run directories out of the volume
#
# N1_PLATFORM selects linux/arm64 (default) or linux/amd64.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
platform="${N1_PLATFORM:-linux/arm64}"
arch="${platform#linux/}"
image="rust:1.95-bookworm"
common=(--rm --platform "$platform" --cpus 4 --memory 8g --memory-swap 8g
  -v "$repo:/src:ro"
  -v "n1-upgrade-build-$arch:/build"
  -v "n1-upgrade-cargo-$arch:/usr/local/cargo/registry"
  -v "n1-upgrade-git-$arch:/usr/local/cargo/git"
  -v "n1-upgrade-runs-$arch:/runs")
case "${1:-}" in
  build) docker run "${common[@]}" "$image" bash /src/qualification/n1-upgrade/in-container.sh build ;;
  run) docker run "${common[@]}" --network none "$image" bash /src/qualification/n1-upgrade/in-container.sh run "${2:-x1 x2 x3 x4 x5 x7}" ;;
  fetch)
    mkdir -p "$2"
    docker run --rm --platform "$platform" -v "n1-upgrade-runs-$arch:/runs" "$image" tar -C /runs -cf - . | tar -C "$2" -xf - ;;
  *) echo "usage: $0 build | run [scenarios] | fetch DEST" >&2; exit 2 ;;
esac
