---
id: "012-cluster-integration-evidence"
title: "Adopt the cluster integration evidence surface"
status: draft
kind: "adoption"
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "002-snapshot-publication-and-recovery"
  - "003-client-consistency-and-retry-outcomes"
origin:
  retroactive: true
  paths:
    - "hiqlite/tests/cluster/"
establishes:
  - "hiqlite/tests/cluster/main.rs"
  - "hiqlite/tests/cluster/start.rs"
  - "hiqlite/tests/cluster/check.rs"
  - "hiqlite/tests/cluster/migration.rs"
  - "hiqlite/tests/cluster/execute_query.rs"
  - "hiqlite/tests/cluster/transaction.rs"
  - "hiqlite/tests/cluster/batch.rs"
  - "hiqlite/tests/cluster/type_conversions.rs"
  - "hiqlite/tests/cluster/cache.rs"
  - "hiqlite/tests/cluster/listen_notify.rs"
  - "hiqlite/tests/cluster/dlock.rs"
  - "hiqlite/tests/cluster/remote_only.rs"
  - "hiqlite/tests/cluster/learner_only.rs"
  - "hiqlite/tests/cluster/backup.rs"
  - "hiqlite/tests/cluster/backup_restore.rs"
extends:
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Adopts the fifteen unclaimed files of hiqlite/tests/cluster/ and maps each
  phase to the guarantee it establishes. Diagnoses F-017's recorded
  non-completion by execution rather than restating it: the suite is a single
  sequential test whose self-healing phase runs last, and the stall that
  prevents it from being reached is a connect-versus-publish race in
  Client::remote, localised here to the microsecond. Records eight defects and
  limits, including an unbounded wait, a success path that exits the process
  while a second test is running, and the fact that no cluster test has ever run
  with TLS enabled. Repairs no runtime behavior and modifies no test.
---

# 012: Adopt the cluster integration evidence surface

## 1. Purpose

Every spec in this corpus so far has stated, in its own section 4, some version
of the same sentence: the claim needs a running cluster and no test here reaches
one. `002` deferred live peer transfer, `009` deferred every environment route,
`010` deferred the join and shutdown sequences, `011` deferred the whole of its
wire behavior. All four were pointing at the same 2,295 lines, and none of them
owned it.

This spec claims those lines and says what they actually establish. It is not a
repair and it changes no test: the value of an adoption here is that the
evidence surface stops being an unexamined backstop that other specs gesture at.

**It also discharges the second half of F-017.** W-15 asks for the remote-client
stall to be "diagnosed or restated honestly". Section 5 diagnoses it, by
execution, with timestamps, and names the library defect that causes it. The
diagnosis is recorded; the repair is not attempted, for the reason D-1 gives.

## 2. Territory

**Establishes** the fifteen files of `hiqlite/tests/cluster/` that no spec
claimed: `main.rs`, `start.rs`, `check.rs`, `migration.rs`, `execute_query.rs`,
`transaction.rs`, `batch.rs`, `type_conversions.rs`, `cache.rs`,
`listen_notify.rs`, `dlock.rs`, `remote_only.rs`, `learner_only.rs`, `backup.rs`
and `backup_restore.rs`.

**Does not claim** `hiqlite/tests/cluster/self_heal.rs`, which `002` establishes
and which stays there, nor `hiqlite/tests/cluster/migrations/`, whose `.sql`
fixtures belong with the migration implementation under W-10.

**Extends**, without re-establishing: `005` on the findings register and the
adoption plan.

**Depends on** `002`, whose recorded non-completion this spec diagnoses, and
`003`, four of whose units it describes without claiming: `client/create.rs`,
`client/listen_notify.rs`, `client/mgmt.rs` and `client/stream.rs`. B-6 and KD-4
are about what those files do to a caller, so they cannot be written without
naming them; no line in any of them is modified, so no edge on them is taken
(the same reasoning as `011` D-3).

**No `spec-spine.toml` change is required.** `hiqlite/tests/cluster/**/*.rs` is
already in `index.extra_hashed_inputs`, declared by the pilot, and all fifteen
files are inside the `hiqlite` cargo package the coverage walk already counts.
This is the first adoption in the sequence that needs no inventory declaration
at all, and that is worth stating because the previous three each needed one.

## 3. Behavior

### B-1. The suite is one test, and the phase order is the contract

`hiqlite/tests/cluster/` builds **one** test binary with **two**
`#[tokio::test]` functions in it: `test_cluster` (`main.rs:39-71`) and
`learner_only_node_stays_non_voter_and_becomes_ready`
(`learner_only.rs:9-38`). An executed run prints `running 2 tests`.

`test_cluster` delegates everything to `exec_tests` (`main.rs:73-229`), which is
a flat sequence of `?`-propagating awaits. There are no sub-tests, no
`#[test]` per phase, and no way to select, skip or reorder one. The order is:

| # | phase | call site | the guarantee it establishes |
|---|---|---|---|
| 1 | cluster start | `main.rs:75` → `start.rs:8-18` | three nodes reach `Ok` from `start_node_with_cache` concurrently |
| 2 | health and membership | `main.rs:78` → `start.rs:92-125` | each node reports healthy db and cache, and both Raft groups agree on three members |
| 3 | migrations | `main.rs:85` → `migration.rs:24-68` | a syntactically bad migration applies nothing; a good set applies once, is visible on all three nodes, and is idempotent across repeated `migrate()` calls |
| 4 | execute and query | `main.rs:89` → `execute_query.rs:27-220` | writes through any node are readable from all three, the unique-constraint error surfaces as an error, and `query_as` / `query_map` / returning-execute agree |
| 5 | transactions | `main.rs:93` → `transaction.rs:9-105` | a multi-statement `txn` commits atomically and each per-statement result is returned in order |
| 6 | batch | `main.rs:97` → `batch.rs:9-76` | a batch runs statement by statement, comments are ignored, and a failing statement fails alone without aborting the ones before it |
| 7 | type conversions | `main.rs:101` → `type_conversions.rs:45-130` | the SQL type mapping round-trips integers, text, bool, the chrono family and JSON, including the `None` cases |
| 8 | cache | `main.rs:105` → `cache.rs:13-236` | put, get, delete, TTL expiry, variant separation, counters, `get_remove` and `replace` semantics including the two expiry rules `006` states |
| 9 | listen and notify | `main.rs:109` → `listen_notify.rs:14-44` | an event published on one node is delivered once to all three local subscribers, and no extra event follows |
| 10 | distributed locks | `main.rs:113` → `dlock.rs:6-60` | a held lock blocks every other client, and release hands the lock to exactly one waiter at a time in arrival order |
| 11 | remote-only clients | `main.rs:117` → `remote_only.rs:10-57` | a `Client::remote` against the member list works against a non-leader for db, cache, locks and listen/notify, and `get_remove` / `replace` stay atomic under concurrency |
| 12 | shutdown and restart | `main.rs:121-147` | three nodes shut down and restart into a healthy cluster, and the data written before the restart is still there |
| 13 | backup | `main.rs:149` → `backup.rs:9-40` | `backup()` produces a file that opens as a plain SQLite database |
| 14 | restore from S3, then from file | `main.rs:152-206` → `backup_restore.rs:11-45` | a node started with `HQL_BACKUP_RESTORE` set comes up on the backup image and the post-backup changes are gone |
| 15 | self-healing | `main.rs:214` → `self_heal.rs` (`002`) | the four damage classes `002` describes recover from peers |

**The order is load-bearing and undeclared.** Phase 4 inserts the rows phase 12
and phase 15 check for; phase 8 seeds the cache value phase 15 reads; phase 13
deletes `_metadata` from the copied backup so phase 14 exercises the validation
bypass. `check.rs:38-44` hard-codes the six row ids that phases 5 and 6 left
behind. None of this is written down anywhere, and section 4 states what it
costs.

### B-2. `learner_only` is a second, independent test in the same process

`learner_only_node_stays_non_voter_and_becomes_ready` (`learner_only.rs:9-38`)
builds its own three-node cluster on ports `35001-35003` and `36001-36003`, in
`tests/data_test_learner_only`, with `learner_only = true` on node 3
(`:60-65`). It asserts that the learner is a member but not a voter in **both**
Raft groups, that it reports `ServerState::Learner`, and that `/ready` answers
success on its API port.

It is the only test in the repository that reaches `NodeConfig::learner_only`,
and it is structured the opposite way to `test_cluster`: its waits are bounded
(`:67-80`, `:82-104`, thirty attempts at 500 ms), it cleans its own data
directory at both ends, and it shuts its nodes down through the public client.

It also runs **concurrently with `test_cluster`**, because libtest's default is
to run the two in parallel threads of one process. That was observed: in the run
section 5 records, both clusters were creating WAL metadata within the same
millisecond. KD-2 and KD-4 are consequences of that process sharing.

### B-3. Every cluster test runs with TLS off, deliberately

`start.rs:76-81` sets `config.tls_raft = None` and `config.tls_api = None` for
every node the suite starts, with an authored reason: TLS would route through
`axum_server`, which has no graceful shutdown, which the suite needs because it
runs three nodes in one process.

So no test in this repository has ever performed a hiqlite TLS handshake. That
is the reason `011` section 4 had to say that its entire wire behavior is read
from source, and it is recorded here as KD-6 rather than left as an inference a
reader has to make twice.

### B-4. The fixture configuration, and the two variables that change it

`build_config_with_nodes` (`start.rs:44-89`) starts from
`NodeConfig::from_toml("../hiqlite.toml", ...)` and then overrides: a per-node
data directory, `log_statements = true`, `wal_size = 8 * 1024` (deliberately
tiny, so log roll-over happens during the run), `default_raft_config(1000)`,
both TLS configurations to `None`, both secrets, a default `backup_config`, and
**`cache_storage_disk = false`** (`:86`).

That last line selects the in-memory cache log store for every cluster run,
which is the store `007` claims and the one F-023, F-029 and F-047 are about. It
is also why those three defects have no reachability evidence: the store is
exercised here constantly, and nothing in the suite asks it for a range it does
not hold.

Two environment variables change what the suite does, and neither is reported in
the result: `TEST_SKIP_S3_RESTORE` (`main.rs:171-175`) silently swaps the S3
restore for a file restore, and `HQL_BACKUP_RESTORE` / `HQL_BACKUP_SKIP_VALIDATION`
are set and unset around the restore phases (`backup_restore.rs:13-42`).

### B-5. Success ends the process; any panic ends the process

`test_cluster`'s success arm is `process::exit(0)` (`main.rs:65-68`), carrying an
authored `// TODO sometimes the test gets stuck here`. `set_panic_hook`
(`main.rs:231-254`) installs a global hook that prints the panic and calls
`process::exit(1)`.

Both are process-wide, and the process holds two tests. KD-2 is the consequence.

### B-6. A remote client's event subscription is asynchronous and unsignalled

`Client::remote` (`client/create.rs:127-215`) calls `RemoteListener::spawn`
(`:163`), which is `task::spawn(Self::handler(...))` followed by an immediate
return of the receiver (`client/listen_notify.rs:29-37`). The handler then
connects an SSE stream to `/listen` on the cached leader address (`:45-62`), and
the server registers the subscriber only when that connection is accepted.

`Client::remote` does not await any of this, returns no readiness signal, and
exposes none afterwards. `Client::listen` (`:104-110`) is
`recv_async().await` on an unbounded `flume` receiver with no timeout.

The composition of those two facts is the stall. Section 5 is the execution.

## 4. Evidence and its limits

**What this spec adds is not a test.** It adds ownership, the phase-to-guarantee
mapping in B-1, and one executed diagnosis. The acceptance block asserts the
structural claims above by pinning the exact text of each site, and asserts by
execution that the binary contains exactly the two tests B-1 and B-2 name.

**What the suite itself establishes** is column four of B-1's table, and only
under the configuration B-4 states: three nodes in one process, on loopback,
with TLS off, a 8 KiB WAL, an in-memory cache log store, and a 1000 ms Raft
election timeout.

**What it does not establish.**

- **Any phase independently.** Phase *n* passing means phases 1 to *n* passed in
  that order on that tree. A regression in phase 3 is reported as a failure of
  the whole suite and hides phases 4 to 15 entirely. There is no per-guarantee
  attribution, and KD-1 records the consequence.
- **Anything after the first stall or failure.** This is not hypothetical: it is
  what F-017 recorded and what section 5 reproduces.
- **Anything under TLS** (B-3), **any multi-process behavior**, **any real
  network**, and **any node that is not on loopback**. The suite cannot see
  F-005's exclusive-access class, F-044's scheme mismatch, or `010` B-4's
  advertised-versus-listen address distinction, because all three need two
  processes or two hosts.
- **The migration naming contract.** KD-5: the fixtures exist and the assertions
  are commented out.
- **The S3 path**, whenever `TEST_SKIP_S3_RESTORE` is set, which is what CI does
  (F-019). A passing CI run does not distinguish "S3 restore verified" from "S3
  restore not attempted", because nothing prints which branch ran.
- **That a green run means both tests passed.** KD-2.

## 5. The stall, diagnosed

F-017's second half records that the pilot's local run "stopped making progress
in the earlier remote-client phase", so `self_heal.rs` was never reached. That
was an honest report of a symptom. This section replaces it with a cause.

**The run.** `TEST_SKIP_S3_RESTORE=true cargo +1.95.0 test --features
cache,counters,dlock,listen_notify,macros,toml,external-state-machine --test
cluster -- --nocapture`, on 2026-09-21, on this tree. Phases 1 to 10 passed.
`learner_only_node_stays_non_voter_and_becomes_ready` passed, at 19:29:10.
`test_cluster` stopped inside phase 11 and never resumed.

**Where, to the microsecond.** The last progress line is
`>>> Test Listen / Notify with remote clients`, logged at `19:28:51.885931`
(`remote_only.rs:42`). The next statement is `client_1.notify(&msg)`
(`:48`), then `client_1.listen()` and `client_2.listen()` (`:50`, `:52`).
The relevant server-side and client-side lines, in timestamp order:

| time | line | meaning |
|---|---|---|
| `19:28:51.645559` | `Connecting to listen SSE stream: http://127.0.0.1:31001/listen` | client 1's `RemoteListener` task starts connecting, ~6 s after `Client::remote` returned |
| `19:28:51.769567` | the same line for client 2 | client 2's listener starts connecting |
| `19:28:51.885931` | `>>> Test Listen / Notify with remote clients` | the test reaches the phase and publishes |
| `19:28:52.060964` | `New notification listener subscribed` | the **server** registers client 1's subscription |
| `19:28:52.118574` | the same line again | the server registers client 2's subscription |

The publish at `~51.89` therefore reached a set of remote subscribers that was
still **empty**: the first registration lands 175 ms later. `Notify` is a
fan-out to currently-registered listeners with no buffering and no replay, so
the event is dropped. Both `listen()` calls are then `recv_async()` on a channel
that will never receive, with no timeout, and the process sits there.

**The cause is in the library, not in the test.** Per B-6, `Client::remote`
returns before its `/listen` subscription exists and offers the caller no way to
wait for it. Any consumer that constructs a remote client and publishes an event
soon afterwards has the same race; the test is merely a consumer that does it
fast enough to lose reliably on this machine. That is F-051, and it is the
finding that actually closes F-017's diagnostic half.

**Why it is not a flake.** The ordering in `remote_only.rs` is deterministic:
`notify` strictly precedes both `listen` calls. Whether the race is lost depends
only on whether the SSE connection completes inside the ~120 ms between
`Client::remote` returning and the publish. It is won on a machine where that
happens and lost on one where it does not, which is exactly the shape of a test
that passes in one environment and hangs in another.

**What the stall is not.** It is not a self-healing defect, and `002`'s
non-completion says nothing about `self_heal.rs`. Self-healing is phase 15 of
15; it was not reached because phase 11 does not return. KD-1 is the property
that turns one race into fifteen missing results.

**Not repaired here.** D-1.

## 6. Known defects

Recorded as found, none repaired here. Each is also filed in
`standards/spec/findings-register.md`.

**KD-1. One sequential test gives no per-guarantee attribution** (F-048). B-1.
Fifteen phases, one result. A failure or stall at phase *n* leaves phases *n+1*
to 15 unrun and unreported, and the suite reports a single failure that names
whichever assertion happened to be first. Consequence, already realised twice:
`002` had to record a non-completion for a guarantee whose test never ran, and
section 5's run leaves eleven of fifteen guarantees unestablished on this tree
for a reason unrelated to any of them. Not a fault in any phase; a property of
the composition. Observed by execution.

**KD-2. The success path exits the process while a second test may be running**
(F-049). `main.rs:65-68` calls `process::exit(0)` from inside `test_cluster`.
The binary contains two tests (B-2) and libtest runs them in parallel threads of
one process by default. If `test_cluster` finishes first, the process ends
immediately: the other test is terminated wherever it is, libtest never prints
its per-test result or the summary line, and cargo sees exit status 0 and
reports success. The reverse exposure is `set_panic_hook` (`main.rs:231-254`),
whose `process::exit(1)` turns any panic in either test into a whole-binary
failure attributed to neither.

In the observed run the hazard did not fire, because `learner_only` finished
first, at 19:29:10, and `test_cluster` never finished at all. Nothing enforces
that order. The concurrency is observed (`running 2 tests`, with both clusters
initialising in the same millisecond); the truncation is source-established.
The authored `// TODO sometimes the test gets stuck here` on the line above the
`exit` records that this exit path has been unreliable before.

**KD-3. The cluster health wait has no bound, and neither does the client call
underneath it** (F-050). `wait_for_healthy_cluster` (`start.rs:92-125`) is
`for i in 1..=3 { loop { sleep(1s); ... } }` with no iteration cap and no
deadline; it logs "Waiting for Node n to become healthy" forever. `check.rs:10-11`
reaches the same shape through the public API: `Client::wait_until_healthy_db`
and `wait_until_healthy_cache` (`client/mgmt.rs:139-153`) are unbounded `loop`s
over `is_healthy_*` with a 500 ms sleep.

Consequence: a regression that stops a cluster from forming is reported as an
unbounded hang, not as a failure, in a harness that imposes no timeout of its
own. CI then either sits until its job limit or is killed without a diagnosis.
The suite already contains the bounded form of exactly this wait
(`learner_only.rs:67-80`, thirty attempts then a real error), so the two idioms
sit side by side in one binary. The unbounded shape is source-established; that
the suite can hang indefinitely is observed.

**KD-4. `Client::remote` returns before its event subscription exists** (F-051).
B-6 and section 5. `RemoteListener::spawn` (`client/listen_notify.rs:29-37`)
starts a detached task and returns the receiver at once;
`Client::remote` (`client/create.rs:163`) does not await the connection and the
client exposes no readiness signal afterwards. An event published in the window
before the SSE stream is registered server-side is delivered to nobody, and
`Client::listen` (`client/listen_notify.rs:104-110`) then waits on it with no
timeout.

Consequence: any consumer that constructs a remote client and publishes soon
after can lose that event silently and block forever waiting for it. Observed by
execution, with the 175 ms losing window measured in section 5. The unit is
`003`'s, not this spec's; recorded here because this is where it was diagnosed.

**KD-5. Two of the three migration fixtures are on disk with their assertions
commented out** (F-052). `migration.rs:8-14` comments out the `rust_embed`
derives for `bad_1` and `bad_2`, and `:33-41` and `:121-126` comment out the
assertions that used them. The authored reason is at `:29-31`: `#[should_panic]`
does not work in an async helper called from another test. The fixtures
themselves are still in the tree
(`tests/cluster/migrations/bad_1/no_leading_index.sql`,
`bad_2/2_bad_start_index.sql`).

Consequence: the two migration-naming rules those fixtures exist to test, that a
file needs a leading integer index and that the sequence must start at 1, have
fixtures, a disabled test and no evidence. Only `bad_3`, the SQL-syntax case,
is exercised. This is W-10's to close. Source-established.

**KD-6. No cluster test has ever run with TLS enabled** (F-053). B-3,
`start.rs:76-81`, with the authored reason. Consequence, stated once here so
three other specs can cite it instead of re-deriving it: `011`'s entire wire
behavior, `010` B-5's TLS-dependent shutdown path, and F-044's scheme mismatch
are all unreachable by the only integration surface this repository has. Classed
as a limit, because the boundary is deliberate, authored and explained; what is
missing is any record of what it costs. Source-established.

**KD-7. A restart race in the WAL is papered over by a sleep in the test**
(F-054). `main.rs:140-142`: `// TODO if this next action comes too fast, there
will be a WAL log ID mismatch -> find out why and fix it`, followed by
`time::sleep(Duration::from_millis(1000))` before the first post-restart cache
write. A second 250 ms sleep at `:131-132` waits for the log sync task to notice
a closed channel.

Consequence: the restart guarantee of phase 12 holds only for a caller that
waits a second. A consumer that writes immediately after a restart meets the
mismatch the comment describes, and nothing in the library documents the wait.
Recorded as a defect in the code under test rather than as a test smell: the
sleep is the evidence, not the fault. **Confidence medium**, because the
mismatch itself was not reproduced here; the authored admission and the
workaround are what is established.

**KD-8. Two tests share process-wide environment mutation** (F-055).
`backup_restore.rs:13-42` sets and removes `HQL_BACKUP_RESTORE` and
`HQL_BACKUP_SKIP_VALIDATION` through `unsafe { env::set_var }`, and `main.rs:47`
removes the first at startup. These are process-wide, and per B-2 the process is
also running `learner_only`, which starts three nodes that read the environment
at startup. Nothing sequences the two tests.

Consequence: a learner-only node can be constructed while a restore variable
belonging to the other test is set, which would start it on a backup image. The
window is narrow in practice, because `learner_only` finishes early and the
restore phases are late, and it did not fire in the observed run.
**Confidence medium, not observed**; recorded because the ordering that makes it
safe is incidental and undeclared.

**Retained without change.** F-017, whose first half (fifteen unclaimed files)
is closed by this spec at M1 and whose second half (the recorded non-completion)
is diagnosed by section 5 rather than closed: the stall is still in the tree.
F-019 is cited by section 4 and unchanged.

## 7. Resolved decisions

**D-1 (2026-09-21, nothing here is repaired, and no test is modified).** KD-3 and
KD-4 have obvious shapes: a bounded wait, and a readiness signal from
`Client::remote`. Both are behavior changes. KD-3's is in this spec's own units
but changes what CI does on a real regression, which is W-22's startup-and-
background-task policy question in a different costume; KD-4's is in `003`'s
unit and changes a public constructor's contract. KD-2's fix is a three-line
change to a test, and it is still a change to what the suite reports, made
against a suite whose only current full-run outcome is a hang. An adoption spec
records; it does not take the first repair it sees.

**D-2 (2026-09-21, the diagnosis is delivered as a record, not as a regression
test).** A test that demonstrates KD-4 failing would have to construct a remote
client against a live cluster and publish inside the losing window, which is a
timing assertion against a race: it would pass on this machine and pass
vacuously on a faster one. The honest artifact is the measured timeline in
section 5, which names the mechanism and the window. When KD-4 is repaired, the
regression test is `Client::remote` returning only after its subscription is
live, which is an assertion about the repair and cannot be written before it.

**D-3 (2026-09-21, `learner_only.rs` is claimed although it is a separate
test).** It is in the directory, in the binary, and its interaction with
`test_cluster` is two of the eight findings. Claiming the directory and
excluding one of its two entry points would leave KD-2 and KD-8 describing an
unowned file.

**D-4 (2026-09-21, the phase table is part of the spec and not a comment in
`main.rs`).** B-1's fourth column is the mapping W-15 asks for, and writing it
into `exec_tests` as comments would be a change to a test file this adoption
deliberately does not modify, in a place where it would drift silently. It is
here, and the acceptance block pins each call site so it cannot stop matching.

## 8. Out of scope

- **Every repair.** KD-1 through KD-8 are recorded and left. In particular the
  restructuring KD-1 implies, splitting fifteen phases into fifteen tests with
  explicit fixtures, is a substantial change to the only evidence surface the
  project has and is not something an adoption performs.
- **`self_heal.rs`**, which stays `002`'s.
- **`tests/cluster/migrations/*.sql`**, which are W-10's along with
  `hiqlite/src/migration.rs`. KD-5 is recorded here because the disabled
  assertions are in a file this spec owns.
- **`hiqlite/src/backup.rs` and `s3.rs`**, which are W-09's. Phases 13 and 14
  exercise them; nothing here specifies them.
- **CI configuration.** F-019 is cited and unchanged, and no workflow is
  touched.
- **Ratification, enforcement, and any tool or pin change.**

## Verification

Run with `just spine-verify 012`.

```verify:cli
test -f hiqlite/tests/cluster/main.rs
test -f hiqlite/tests/cluster/learner_only.rs
sh -c 'spec-spine index owner hiqlite/tests/cluster/main.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine index owner hiqlite/tests/cluster/remote_only.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine index owner hiqlite/tests/cluster/learner_only.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine index owner hiqlite/tests/cluster/backup_restore.rs | grep -q 012-cluster-integration-evidence'
sh -c 'spec-spine index owner hiqlite/tests/cluster/self_heal.rs | grep -q 002-snapshot-publication-and-recovery'
sh -c 'spec-spine registry relationships 012-cluster-integration-evidence | grep -q 002-snapshot-publication-and-recovery'
sh -c 'cargo test -p hiqlite --features cache,counters,dlock,listen_notify,macros,toml,external-state-machine --test cluster -- --list | grep -q "^test_cluster: test$"'
sh -c 'cargo test -p hiqlite --features cache,counters,dlock,listen_notify,macros,toml,external-state-machine --test cluster -- --list | grep -q "^learner_only::learner_only_node_stays_non_voter_and_becomes_ready: test$"'
sh -c 'test "$(cargo test -p hiqlite --features cache,counters,dlock,listen_notify,macros,toml,external-state-machine --test cluster -- --list | grep -c ": test$")" = "2"'
grep -q 'process::exit(0);' hiqlite/tests/cluster/main.rs
grep -q 'TODO sometimes the test gets stuck here' hiqlite/tests/cluster/main.rs
grep -q 'process::exit(1);' hiqlite/tests/cluster/main.rs
grep -q 'TODO if this next action comes too fast, there will be a WAL log ID mismatch' hiqlite/tests/cluster/main.rs
grep -q 'TEST_SKIP_S3_RESTORE' hiqlite/tests/cluster/main.rs
grep -q 'config.tls_raft = None;' hiqlite/tests/cluster/start.rs
grep -q 'config.tls_api = None;' hiqlite/tests/cluster/start.rs
grep -q 'config.cache_storage_disk = false;' hiqlite/tests/cluster/start.rs
grep -q 'config.wal_size = 8 \* 1024;' hiqlite/tests/cluster/start.rs
sh -c 'grep -A2 "for i in 1..=3 {" hiqlite/tests/cluster/start.rs | grep -q "loop {"'
grep -q 'pub async fn wait_until_healthy_db' hiqlite/src/client/mgmt.rs
grep -q 'config.learner_only = node_id == 3;' hiqlite/tests/cluster/learner_only.rs
grep -q 'for _ in 0..30 {' hiqlite/tests/cluster/learner_only.rs
grep -q 'env::set_var("HQL_BACKUP_SKIP_VALIDATION", "true");' hiqlite/tests/cluster/backup_restore.rs
grep -q 'unsafe { env::remove_var("HQL_BACKUP_RESTORE") };' hiqlite/tests/cluster/main.rs
grep -q '// #\[derive(rust_embed::Embed)\]' hiqlite/tests/cluster/migration.rs
grep -q '// #\[folder = "tests/cluster/migrations/bad_1"\]' hiqlite/tests/cluster/migration.rs
grep -q '// #\[folder = "tests/cluster/migrations/bad_2"\]' hiqlite/tests/cluster/migration.rs
test -f hiqlite/tests/cluster/migrations/bad_1/no_leading_index.sql
test -f hiqlite/tests/cluster/migrations/bad_2/2_bad_start_index.sql
grep -q 'let rx_notify = Some(RemoteListener::spawn(' hiqlite/src/client/create.rs
sh -c 'grep -A2 "let (tx, rx) = flume::unbounded();" hiqlite/src/client/listen_notify.rs | grep -q "task::spawn(Self::handler"'
grep -q 'Connecting to listen SSE stream' hiqlite/src/client/listen_notify.rs
grep -q 'self.listen_rx().recv_async().await' hiqlite/src/client/listen_notify.rs
grep -q 'hiqlite/tests/cluster/\*\*/\*.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/012-cluster-integration-evidence'
```
