#!/usr/bin/env bash
# `035` section 5, X-1 to X-5 and X-7, against real hiqlite 0.14 and the candidate, each its
# own process. Runs inside the Linux container (`in-container.sh`).
#
# Bounds (`035` section 5, lane A): each scenario at most three consecutive runs, each run at
# most 60 s, stop at the first failure and keep that run's directory; the whole invocation at
# most 30 minutes. X-7 is recorded, never passed or failed.
#
# Usage: BIN=... ROOT=... run.sh [x1 x2 x3 x4 x5 x7]
set -uo pipefail

BIN="${BIN:?}"
ROOT="${ROOT:?}"
SCENARIOS="${*:-x1 x2 x3 x4 x5 x7}"
FEATURE_SETS="${N1_FEATURE_SETS:-rahi rauthy}"
RUNS="${N1_RUNS:-3}"
RUN_BOUND=60
TOTAL_BOUND=$((30 * 60))
PORT_API=38711
PORT_RAFT=38712
export N1_PORT_API=$PORT_API N1_PORT_RAFT=$PORT_RAFT
CONSENT=HQL_CACHE_LEGACY_MOVE_ASIDE
POINTS="after-db-lock after-cache-lock after-partial-created after-snapshots-moved after-staged after-legacy-log-moved after-log-moved"
T_START=$(date +%s)
LAUNCHES=0

log() { echo "$*"; }
die() { echo "FAIL: $*"; return 1; }

# Every entry under $1: type, inode, size, sha256 (files), and the bytes of each lock file.
snap() {
  (cd "$1" && find . -mindepth 1 | sed 's#^\./##' | LC_ALL=C sort | while read -r p; do
    if [ -d "$p" ]; then echo "D $(stat -c '%i' "$p") $p"
    else echo "F $(stat -c '%i %s' "$p") $(sha256sum "$p" | cut -c1-16) $p"; fi
  done)
}
lock_contents() {
  for f in hiqlite-owner.lock logs/lock.hql logs_cache/lock.hql state_machine/lock; do
    [ -f "$1/$f" ] && echo "content $f: [$(tr '\n' ' ' < "$1/$f" | cut -c1-80)]"
  done
  return 0
}
# Wait until $1 contains $2, bounded at $3 seconds; fail if the process $4 exited first.
waitfor() {
  local i=0
  while ! grep -q "$2" "$1" 2>/dev/null; do
    if [ -n "${4:-}" ] && ! kill -0 "$4" 2>/dev/null; then echo "process $4 exited before '$2'"; return 1; fi
    sleep 0.05; i=$((i + 1))
    if [ $i -gt $(($3 * 20)) ]; then echo "TIMEOUT waiting for '$2' in $1"; return 1; fi
  done
  return 0
}
# Run a node to completion, bounded; record its output and exit code.
node() { # name bin dir mode [env...]
  local name=$1 bin=$2 dir=$3 mode=$4; shift 4
  LAUNCHES=$((LAUNCHES + 1))
  local rc=0
  env "$@" timeout 30 "$BIN/$bin" "$dir" "$mode" > "$dir.$name.log" 2>&1 || rc=$?
  echo "$rc" > "$dir.$name.rc"
  echo "  $name ($bin $mode${*:+ $*}): exit $rc: $(grep -E '^(START_OK|START_ERR|SHUTDOWN|USE)' "$dir.$name.log" | cut -c1-300 | tr '\n' '|')"
  return 0
}
rc() { cat "$1.$2.rc"; }
port_free() {
  ! (exec 3<>/dev/tcp/127.0.0.1/$PORT_API) 2>/dev/null && ! (exec 3<>/dev/tcp/127.0.0.1/$PORT_RAFT) 2>/dev/null
}
no_panic() { ! grep -q "panicked" "$1" || die "panic in $1: $(grep -m1 panicked "$1")"; }
# Before and after differ only by the entries named in $3 (a regex of paths added).
same_except() {
  local d
  d=$(diff <(grep -vE " ($3)\$" "$1") <(grep -vE " ($3)\$" "$2"))
  [ -z "$d" ] || { echo "$d" | head -20; die "tree changed beyond $3"; }
}
# A legacy directory: real 0.14, written and stopped cleanly.
legacy_dir() { # fs dir
  mkdir -p "$2"
  node legacy "node-old-$1" "$2" write-stop
  [ "$(rc "$2" legacy)" = 0 ] || die "0.14 could not prepare $2"
  ls "$2"/logs_cache/*.wal > /dev/null 2>&1 || die "0.14 left no cache WAL in $2"
}
# Start 0.14 in `hold` mode in the background.
hold_old() { # fs dir
  LAUNCHES=$((LAUNCHES + 1))
  ( r=0; "$BIN/node-old-$1" "$2" hold "$2.ctl" > "$2.old.log" 2>&1 || r=$?; echo "old exit=$r" >> "$2.old.log" ) &
  OLD_PID=$!
  waitfor "$2.old.log" HOLDING 20 "$OLD_PID" || { cat "$2.old.log"; die "0.14 did not reach HOLDING"; }
}
old_use_and_stop() { # dir
  echo use > "$1.ctl"; waitfor "$1.old.log" USED 20 || die "0.14 did not answer use"
  grep -q "USE tag=after ok=true" "$1.old.log" || die "0.14 write after the refusal failed: $(grep 'USE tag=after' "$1.old.log")"
  echo stop > "$1.ctl"; waitfor "$1.old.log" "old exit" 30 || die "0.14 did not stop"
  grep -q "SHUTDOWN=Ok" "$1.old.log" || die "0.14 shutdown not Ok: $(grep SHUTDOWN "$1.old.log")"
  grep -q "old exit=0" "$1.old.log" || die "0.14 exit not 0: $(grep 'old exit' "$1.old.log")"
  no_panic "$1.old.log"
}

# X-1 and X-2: a live 0.14 node, then the candidate with ($3=true) or without consent.
x_live() { # fs dir consent
  local fs=$1 v=$2 consent=$3
  mkdir -p "$v"; hold_old "$fs" "$v" || return 1
  snap "$v" > "$v.A"; lock_contents "$v" > "$v.A.locks"
  if [ "$consent" = true ]; then node cand "node-new-$fs" "$v" read "$CONSENT=true"
  else node cand "node-new-$fs" "$v" read; fi
  [ "$(rc "$v" cand)" = 3 ] || die "the candidate was not refused (exit $(rc "$v" cand))"
  grep -q "StorageInUse" "$v.cand.log" || die "not StorageInUse: $(grep START_ERR "$v.cand.log")"
  grep -q "lock.hql" "$v.cand.log" || die "the refusal does not name the WAL lock"
  grep -q "hiqlite-owner.lock" "$v.cand.log" || die "the refusal does not name the owner lock"
  no_panic "$v.cand.log"
  snap "$v" > "$v.B"
  same_except "$v.A" "$v.B" "hiqlite-owner.lock" || return 1
  if compgen -G "$v/pre-upgrade-*" > /dev/null; then die "a pre-upgrade directory exists"; fi
  old_use_and_stop "$v" || return 1
  if [ -e "$v/logs/lock.hql" ]; then die "0.14 left logs/lock.hql after a clean stop"; fi
  node restart "node-old-$fs" "$v" read
  [ "$(rc "$v" restart)" = 0 ] || die "0.14 restart failed"
  grep -q "USE tag=read ok=true .* sql_rows=3" "$v.restart.log" || die "rows after restart: $(grep 'USE tag=read' "$v.restart.log")"
  no_panic "$v.restart.log"
}
x1() { x_live "$1" "$2" true; }
x2() { x_live "$1" "$2" false; }

# X-3: 0.14 killed with SIGKILL (its unclean marker left), then the candidate with consent.
x3() {
  local fs=$1 v=$2
  mkdir -p "$v"
  LAUNCHES=$((LAUNCHES + 1))
  # Directly, not in a subshell: `$!` must be the node, which is what gets SIGKILL.
  "$BIN/node-old-$fs" "$v" killme > "$v.old.log" 2>&1 &
  local pid=$!
  waitfor "$v.old.log" "KILL_ME ok=true" 20 "$pid" || die "0.14 did not get ready"
  kill -9 "$pid"; wait "$pid" 2>/dev/null || true
  [ -e "$v/state_machine/lock" ] || die "no unclean marker after SIGKILL"
  snap "$v" > "$v.A"; lock_contents "$v" > "$v.A.locks"
  node cand "node-new-$fs" "$v" read "$CONSENT=true"
  no_panic "$v.cand.log"
  if [ "$fs" = rahi ]; then
    # Without `auto-heal`: a refusal before any rename, and `logs/` byte-identical.
    [ "$(rc "$v" cand)" = 3 ] || die "not refused (exit $(rc "$v" cand))"
    grep -q "START_ERR.*Startup.*state_machine/lock" "$v.cand.log" || die "not the marker refusal: $(grep START_ERR "$v.cand.log")"
    snap "$v" > "$v.B"
    same_except "$v.A" "$v.B" "hiqlite-owner.lock" || return 1
    if compgen -G "$v/pre-upgrade-*" > /dev/null; then die "moved"; fi
    if [ -e "$v/logs/meta.hql~" ]; then die "logs/meta.hql~ created"; fi
  else
    # `auto-heal` (Rauthy's defaults): the marker is its policy, not a refusal. The move runs
    # under the locks and the legacy log is kept byte for byte.
    [ "$(rc "$v" cand)" = 0 ] || die "the auto-heal start failed (exit $(rc "$v" cand))"
    grep -q "USE tag=read ok=true" "$v.cand.log" || die "not usable after the start"
    local moved; moved=$(ls -d "$v"/pre-upgrade-* 2>/dev/null || true)
    [ "$(echo "$moved" | wc -w | tr -d " ")" = 1 ] || die "expected one pre-upgrade directory: $moved"
    case "$moved" in *.partial) die "incomplete move $moved";; esac
    for w in "$v"/pre-upgrade-*/logs_cache/*.wal; do
      local rel; rel=logs_cache/$(basename "$w")
      grep -q " $(sha256sum "$w" | cut -c1-16) $rel\$" "$v.A" || die "moved $rel differs from the original"
    done
  fi
}

# X-4: the race of B-4. Ten launches per run: candidate (with consent) and 0.14 on a legacy
# directory at offsets 0 to 50 ms, alternating which starts first.
x4() {
  local fs=$1 v=$2
  legacy_dir "$fs" "$v.base" || return 1
  local i
  for i in 0 1 2 3 4 5 6 7 8 9; do
    local d="$v/launch-$i" off; off=$(awk "BEGIN{printf \"%.3f\", $i * 0.050 / 9}")
    mkdir -p "$v"; cp -a "$v.base" "$d"
    local first second
    if [ $((i % 2)) = 0 ]; then first=new; second=old; else first=old; second=new; fi
    LAUNCHES=$((LAUNCHES + 2))
    launch() { # side
      local r=0
      if [ "$1" = new ]; then env "$CONSENT=true" timeout 30 "$BIN/node-new-$fs" "$d" race > "$d.new.log" 2>&1 || r=$?; echo $r > "$d.new.rc"
      else timeout 30 "$BIN/node-old-$fs" "$d" race > "$d.old.log" 2>&1 || r=$?; echo $r > "$d.old.rc"; fi
    }
    launch $first & local p1=$!
    sleep "$off"; launch $second & local p2=$!
    wait $p1 $p2 || true
    local ok_new ok_old
    ok_new=$(grep -c START_OK "$d.new.log" || true); ok_old=$(grep -c START_OK "$d.old.log" || true)
    echo "  launch $i offset ${off}s first=$first: new exit $(cat "$d.new.rc") ok=$ok_new | old exit $(cat "$d.old.rc") ok=$ok_old"
    [ $((ok_new + ok_old)) = 1 ] || die "launch $i: $((ok_new + ok_old)) proceeded"
    if [ "$ok_new" = 1 ]; then
      grep -q "SHUTDOWN=Ok" "$d.new.log" && [ "$(cat "$d.new.rc")" = 0 ] || die "launch $i: the candidate did not stop Ok"
      # The loser, 0.14, refused before writing anything: its marker is not there, and the
      # legacy log moved intact.
      if [ -e "$d/state_machine/lock" ]; then die "launch $i: an unclean marker is left"; fi
      for w in "$v.base"/logs_cache/*.wal; do
        cmp -s "$w" "$d"/pre-upgrade-*/logs_cache/"$(basename "$w")" || die "launch $i: moved $(basename "$w") differs"
      done
    else
      grep -q "SHUTDOWN=Ok" "$d.old.log" && [ "$(cat "$d.old.rc")" = 0 ] || die "launch $i: 0.14 did not stop Ok"
      grep -q "StorageInUse" "$d.new.log" || die "launch $i: the candidate was not refused as StorageInUse: $(grep START_ERR "$d.new.log")"
      if compgen -G "$d/pre-upgrade-*" > /dev/null || [ -e "$d/logs_cache.hiqlite-next" ]; then die "launch $i: the refused candidate moved something"; fi
      ls "$d"/logs_cache/*.wal > /dev/null 2>&1 || die "launch $i: the legacy log is gone"
    fi
    no_panic "$d.new.log"
    rm -rf "$d"
  done
}

# X-5: the candidate killed at each of B-5's points (the `faults` build aborts there); the next
# start without consent refuses; with consent it completes, and nothing legacy is left where the
# cache raft opens.
x5() {
  local fs=$1 v=$2
  legacy_dir "$fs" "$v.base" || return 1
  local p
  for p in $POINTS; do
    local d="$v/$p"; mkdir -p "$v"; cp -a "$v.base" "$d"
    node fault "node-new-$fs-faults" "$d" read "$CONSENT=true" "HQL_TEST_UPGRADE_FAULT=$p"
    grep -q "aborting at $p" "$d.fault.log" || die "$p: the fault point was not reached"
    [ "$(rc "$d" fault)" = 134 ] || die "$p: expected an abort, exit $(rc "$d" fault)"
    node refuse "node-new-$fs" "$d" read
    [ "$(rc "$d" refuse)" = 3 ] || die "$p: a start without consent was not refused (exit $(rc "$d" refuse))"
    grep -q "$CONSENT" "$d.refuse.log" || die "$p: the refusal does not name the consent"
    node finish "node-new-$fs" "$d" read "$CONSENT=true"
    [ "$(rc "$d" finish)" = 0 ] || die "$p: the start with consent failed (exit $(rc "$d" finish))"
    grep -q "USE tag=read ok=true .* sql_rows=2" "$d.finish.log" || die "$p: rows: $(grep 'USE tag=read' "$d.finish.log")"
    local moved; moved=$(cd "$d" && ls -d pre-upgrade-* 2>/dev/null || true)
    [ "$(echo "$moved" | wc -w | tr -d " ")" = 1 ] || die "$p: expected one pre-upgrade directory: $moved"
    case "$moved" in *.partial) die "$p: still partial";; esac
    for w in "$v.base"/logs_cache/*.wal; do
      cmp -s "$w" "$d/$moved/logs_cache/$(basename "$w")" || die "$p: moved $(basename "$w") differs"
    done
    diff -r "$v.base/state_machine_cache" "$d/$moved/state_machine_cache" > /dev/null || die "$p: moved snapshots differ"
    if [ -e "$d/logs_cache.hiqlite-next" ]; then die "$p: staging left"; fi
    no_panic "$d.fault.log"; no_panic "$d.refuse.log"; no_panic "$d.finish.log"
    rm -rf "$d"
  done
}

# X-7: recorded, not passed. 0.14 over a directory the candidate moved and wrote.
x7() {
  local fs=$1 v=$2
  legacy_dir "$fs" "$v" || return 1
  node up "node-new-$fs" "$v" read "$CONSENT=true"
  [ "$(rc "$v" up)" = 0 ] || die "the candidate could not upgrade the directory"
  node down "node-old-$fs" "$v" read
  echo "  RECORD 0.14 over 0.15: exit $(rc "$v" down); $(grep -m1 -E 'panicked|START_ERR|START_OK' "$v.down.log" | cut -c1-240)"
  echo "  RECORD sizes: logs/meta.hql=$(stat -c %s "$v/logs/meta.hql" 2>/dev/null) logs_cache/meta.hql=$(stat -c %s "$v/logs_cache/meta.hql" 2>/dev/null) marker=$([ -e "$v/state_machine/lock" ] && echo present || echo absent)"
  node again "node-new-$fs" "$v" read
  echo "  RECORD candidate afterwards: exit $(rc "$v" again); $(grep -m1 -E 'START_ERR|START_OK|panicked' "$v.again.log" | cut -c1-240)"
  return 0
}

log "N1 upgrade harness: $(uname -srm), $(nproc) cpus, bins $(ls "$BIN" | tr '\n' ' ')"
log "scenarios: $SCENARIOS; feature sets: $FEATURE_SETS; $RUNS runs each, ${RUN_BOUND}s per run, ${TOTAL_BOUND}s total"
port_free || { echo "PORTS $PORT_API/$PORT_RAFT BUSY"; exit 2; }
export -f snap lock_contents waitfor node rc no_panic same_except legacy_dir hold_old old_use_and_stop x_live x1 x2 x3 x4 x5 x7 die log
export BIN CONSENT POINTS

status=0
for fs in $FEATURE_SETS; do
  for sc in $SCENARIOS; do
    for run in $(seq 1 $RUNS); do
      if [ $(( $(date +%s) - T_START )) -ge $TOTAL_BOUND ]; then echo "TOTAL BOUND REACHED: stopping, incomplete"; exit 4; fi
      v="$ROOT/$sc-$fs-$run"
      t0=$(date +%s%N)
      echo "== $sc $fs run $run"
      # `set -e` inside the run: the first failed check ends it. The launch count is printed
      # on success; a failed run's count is in its log lines.
      timeout --kill-after=5 $RUN_BOUND bash -c "set -euo pipefail; LAUNCHES=0; $sc $fs $v; echo LAUNCHES=\$LAUNCHES" > "$v.out" 2>&1
      r=$?
      # Nothing of a run may outlive it, whatever ended it.
      pkill -9 -f "$BIN/node-" 2>/dev/null || true
      cat "$v.out"
      ms=$(( ($(date +%s%N) - t0) / 1000000 ))
      if [ $r -ne 0 ]; then
        [ $r = 124 ] && echo "RUN BOUND of ${RUN_BOUND}s exceeded"
        echo "RESULT $sc $fs run $run: FAIL (exit $r) in ${ms}ms; kept $v"
        status=1; break 3
      fi
      echo "RESULT $sc $fs run $run: $([ "$sc" = x7 ] && echo RECORDED || echo PASS) in ${ms}ms"
      # Keep evidence of passing runs small: the logs and listings, not the volumes.
      rm -rf "$v" "$v.base"
      port_free || { echo "PORTS BUSY after $sc run $run"; status=1; break 3; }
    done
  done
done
echo "TOTAL_SECONDS=$(( $(date +%s) - T_START ))"
exit $status
