#!/usr/bin/env bash
# `041` B-4: `040`'s and `039`'s regressions in a release build on native Linux, one launch each,
# bounded, no retry. `040`'s test is in `hiqlite-wal-patched`, which has no feature sets. `039`'s
# `lease_wake` needs `cache` and `dlock`: it runs under Rahi's set; Rauthy's set has no `dlock`,
# so the test target is empty there and that is recorded, not waived.
#
# usage: p4-regress.sh <candidate source dir, read-only> <arm64|amd64>
set -euo pipefail
src="$1"
arch="$2"
image="rust:1.95-bookworm"
common=(--rm --platform "linux/$arch" --cpus 4 --memory 8g --memory-swap 8g)
vols=(-v "$src:/src:ro"
  -v "p4-regress-work-$arch:/work"
  -v "p4-regress-target-$arch:/target"
  -v "n1-upgrade-cargo-$arch:/usr/local/cargo/registry"
  -v "n1-upgrade-git-$arch:/usr/local/cargo/git")
RAHI="--no-default-features --features sqlite,cache,counters,dlock,listen_notify_local,backup,s3"
RAUTHY="--features cache,cast_ints,counters,dashboard,listen_notify_local,macros"

docker run "${common[@]}" "${vols[@]}" "$image" bash -c "
set -euo pipefail
rm -rf /work/src && cp -a /src /work/src && cd /work/src
export CARGO_TARGET_DIR=/target
start=\$(date +%s)
exe() { cargo test --release --locked \"\$@\" --no-run --message-format=json \
  | grep -o '\"executable\":\"[^\"]*\"' | cut -d'\"' -f4 | grep -v '^\$' | tail -1; }
w=\$(exe -p hiqlite-wal-patched --lib)
l=\$(exe -p hiqlite-patched $RAHI --test lease_wake)
r=\$(exe -p hiqlite-patched $RAUTHY --test lease_wake)
printf '%s\n%s\n%s\n' \"\$w\" \"\$l\" \"\$r\" > /work/exes
echo BUILD_SECONDS=\$((\$(date +%s) - start))
rustc --version; uname -m
sha256sum \"\$w\" \"\$l\" \"\$r\"
"

docker run "${common[@]}" --network none "${vols[@]}" "$image" bash -c "
set -uo pipefail
mapfile -t e < /work/exes
ok=0
one() { # label dir exe args...
  label=\$1; dir=\$2; exe=\$3; shift 3
  echo \"== launch \$label: \$(basename \$exe) \$*\"
  sha256sum \"\$exe\"
  (cd \"/work/src/\$dir\" && timeout 300 \"\$exe\" --test-threads 1 \"\$@\")
  rc=\$?
  echo \"RC=\$rc\"
  [ \$rc = 0 ] || ok=1
}
start=\$(date +%s)
one 040 hiqlite-wal \"\${e[0]}\" --exact wal::tests::creating_a_wal_file_syncs_its_directory
one 039-rahi hiqlite \"\${e[1]}\"
echo '== 039 under Rauthy (no dlock): the target must list zero tests'
n=\$(cd /work/src/hiqlite && \"\${e[2]}\" --list 2>/dev/null | grep -c ': test\$' || true)
echo \"RAUTHY_LEASE_WAKE_TESTS=\$n\"
[ \"\$n\" = 0 ] || ok=1
echo RUN_SECONDS=\$((\$(date +%s) - start))
[ \$ok = 0 ] && echo RESULT=PASS || echo RESULT=FAIL
exit \$ok
"
