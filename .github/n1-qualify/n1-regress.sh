#!/usr/bin/env bash
# `035` D-11's failed-start shared-descriptor regression, in a release build, on native Linux.
# Three test-process launches (approved increment over the harness's 330): the hiqlite-wal
# test once, the hiqlite test under Rahi's and under Rauthy's feature set. Stops at the first
# failure; no retry.
#
# usage: n1-regress.sh <candidate source dir, read-only> <arm64|amd64>
set -euo pipefail
src="$1"
arch="$2"
image="rust:1.95-bookworm"
common=(--rm --platform "linux/$arch" --cpus 4 --memory 8g --memory-swap 8g)
vols=(-v "$src:/src:ro"
  -v "n1-regress-work-$arch:/work"
  -v "n1-regress-target-$arch:/target"
  -v "n1-upgrade-cargo-$arch:/usr/local/cargo/registry"
  -v "n1-upgrade-git-$arch:/usr/local/cargo/git")
RAHI=sqlite,cache,counters,dlock,listen_notify_local,backup,s3
RAUTHY=cache,cast_ints,counters,dashboard,listen_notify_local,macros

# Build (network allowed; not part of the run budget). The source is copied to a Linux
# filesystem (a named volume), because the tests write relative to their crate directory.
docker run "${common[@]}" "${vols[@]}" "$image" bash -c "
set -euo pipefail
rm -rf /work/src && cp -a /src /work/src && cd /work/src
export CARGO_TARGET_DIR=/target
start=\$(date +%s)
exe() { cargo test --release --locked \"\$@\" --no-run --message-format=json \
  | grep -o '\"executable\":\"[^\"]*\"' | cut -d'\"' -f4 | grep -v '^\$' | tail -1; }
w=\$(exe -p hiqlite-wal-patched --lib)
a=\$(exe -p hiqlite-patched --lib --no-default-features --features $RAHI)
b=\$(exe -p hiqlite-patched --lib --features $RAUTHY)
printf '%s\n%s\n%s\n' \"\$w\" \"\$a\" \"\$b\" > /work/exes
echo BUILD_SECONDS=\$((\$(date +%s) - start))
rustc --version; uname -m
sha256sum \"\$w\" \"\$a\" \"\$b\"
"

# Run: no network, each test binary launched directly, once, bounded at 60 s.
docker run "${common[@]}" --network none "${vols[@]}" "$image" bash -c "
set -uo pipefail
mapfile -t e < /work/exes
L=0
one() { # dir exe test
  L=\$((L + 1))
  echo \"== launch \$L: \$(basename \$2) \$3\"
  sha256sum \"\$2\"
  (cd \"/work/src/\$1\" && timeout 60 \"\$2\" --exact \"\$3\" --test-threads 1)
  rc=\$?
  echo \"RC=\$rc\"
  return \$rc
}
start=\$(date +%s)
one hiqlite-wal \"\${e[0]}\" writer::tests::a_shared_lock_is_held_until_the_writer_stops \
  && one hiqlite \"\${e[1]}\" upgrade_exclusion::tests::a_failed_start_keeps_the_wal_lock_until_the_writer_stops \
  && one hiqlite \"\${e[2]}\" upgrade_exclusion::tests::a_failed_start_keeps_the_wal_lock_until_the_writer_stops
rc=\$?
echo LAUNCHES=\$L
echo RUN_SECONDS=\$((\$(date +%s) - start))
[ \$rc = 0 ] && echo RESULT=PASS || echo RESULT=FAIL
exit \$rc
"
