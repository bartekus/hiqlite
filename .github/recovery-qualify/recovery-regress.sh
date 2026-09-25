#!/usr/bin/env bash
# `037`'s regression, `hiqlite/tests/recovery_readiness.rs`, in a release build on native Linux,
# under Rauthy's and Rahi's feature sets: one launch each, bounded, no retry.
#
# usage: recovery-regress.sh <candidate source dir, read-only> <arm64|amd64> <fail|pass>
#   fail: the candidate is the unrepaired source; every launch must fail on its assertion.
#   pass: the candidate is the repair; every launch must pass.
set -euo pipefail
src="$1"
arch="$2"
expect="$3"
image="rust:1.95-bookworm"
common=(--rm --platform "linux/$arch" --cpus 4 --memory 8g --memory-swap 8g)
vols=(-v "$src:/src:ro"
  -v "recovery-work-$arch:/work"
  -v "recovery-target-$arch:/target"
  -v "n1-upgrade-cargo-$arch:/usr/local/cargo/registry"
  -v "n1-upgrade-git-$arch:/usr/local/cargo/git")
RAUTHY="--features cache,cast_ints,counters,dashboard,listen_notify_local,macros"
RAHI="--no-default-features --features sqlite,cache,counters,dlock,listen_notify_local,backup,s3"

docker run "${common[@]}" "${vols[@]}" "$image" bash -c "
set -euo pipefail
rm -rf /work/src && cp -a /src /work/src && cd /work/src
export CARGO_TARGET_DIR=/target
start=\$(date +%s)
exe() { cargo test --release --locked -p hiqlite-patched \"\$@\" --test recovery_readiness --no-run --message-format=json \
  | grep -o '\"executable\":\"[^\"]*\"' | cut -d'\"' -f4 | grep -v '^\$' | tail -1; }
a=\$(exe $RAUTHY)
b=\$(exe $RAHI)
printf '%s\n%s\n' \"\$a\" \"\$b\" > /work/exes
echo BUILD_SECONDS=\$((\$(date +%s) - start))
rustc --version; uname -m
sha256sum \"\$a\" \"\$b\"
"

docker run "${common[@]}" --network none "${vols[@]}" "$image" bash -c "
set -uo pipefail
mapfile -t e < /work/exes
ok=0
one() { # label exe
  echo \"== launch \$1: \$(basename \$2)\"
  sha256sum \"\$2\"
  (cd /work/src/hiqlite && timeout 300 \"\$2\" --test-threads 1 2>&1 | grep -E '^test |panicked at|healthy \\(|the cache was|test result')
  rc=\$?
  echo \"RC=\$rc\"
  if [ $expect = fail ]; then [ \$rc = 101 ] || ok=1; else [ \$rc = 0 ] || ok=1; fi
}
one rauthy \"\${e[0]}\"
one rahi \"\${e[1]}\"
[ \$ok = 0 ] && echo RESULT=AS_EXPECTED_$expect || echo RESULT=UNEXPECTED
exit \$ok
"
