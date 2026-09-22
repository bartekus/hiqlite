---
id: "026-backup-and-restore-integrity"
title: "Repair backup retention, restore ordering, validation and configuration failure"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "013-backup-retention-and-object-storage"
  - "024-exclusive-storage-ownership"
  - "025-snapshot-crash-recovery-contract"
amends: ["013-backup-retention-and-object-storage"]
# D-7: this spec's `## Verification` block IS 013's acceptance from now on. It had to be: 024
# repaired a unit 013 owns and did not carry 013's block, which left that block asserting code
# that no longer exists. Section 5 records that rather than tidying it away.
amends_verification: ["013-backup-retention-and-object-storage"]
amends_sections:
  - "3-behavior"
  - "5-known-defects"
extends:
  - spec: "013-backup-retention-and-object-storage"
    unit: { kind: file, path: "hiqlite/src/backup.rs" }
    nature: superseding
  - spec: "013-backup-retention-and-object-storage"
    unit: { kind: file, path: "hiqlite/src/s3.rs" }
    nature: superseding
  - spec: "003-client-consistency-and-retry-outcomes"
    unit: { kind: directory, path: "hiqlite/src/client/" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Repairs F-056 through F-063. One predicate decides what a backup file is, a
  restore stages the replacement before destroying anything, a follower moves
  its state aside instead of deleting it on an environment variable alone,
  validation checks integrity and returns errors instead of panicking, the
  post-restore purge is bounded, the S3 configuration names the variable that
  is wrong and proves its credentials before a restore, and two reporting
  contradictions are corrected. States N=1 as the supported restore topology
  and says why N=3 is not.
---

# 026: Repair backup retention, restore ordering, validation and configuration failure

## 1. Purpose

`013` adopted `backup.rs` and `s3.rs` as found and recorded eight defects, one
of which it demonstrated deleting a file that was not a backup. This spec
repairs all eight.

They are not one defect with eight faces, but they do share a shape: **every one
of them is a place where the code's stated intent and its behavior disagree, and
where the disagreement only shows up when something goes wrong.** A retention
guard that reads as "skip unless it is a backup" and means "skip only if it is
neither"; a restore that reads as "check, then replace" and means "check,
destroy, then try to replace"; a validation that reads as a check and is a
panic; a retry count that is printed and not counted.

One responsibility: **what backup and restore do when the input, the transfer,
or the environment is not what was expected.**

## 2. Territory

**Extends** `013`'s two `file` units with nature `superseding`, and `003`'s
`hiqlite/src/client/` additively, because the backup listing is the third place
that decided what a backup file is.

**Amends** `013` sections 3 and 5, and carries its acceptance (D-7).

**Ownership boundary.** The object store is not hiqlite's: a bucket's
availability, its retention policy and its access control belong to whoever
operates it. This spec states what hiqlite does with what the bucket gives back.
Nothing here claims a backup is durable in the bucket.

## 3. Behavior

### B-1. One predicate decides what a backup file is

`dt_from_backup_name` is the single definition of
`backup_node_{node_id}_{unix_seconds}.sqlite`, and it now also checks that the
node id parses. Three callers use it: the local retention sweep, the S3
retention filter, and `Client::backup_list_local`.

There were three different predicates before, no two the same. The sweep's was
`!starts_with(prefix) && !ends_with(suffix)`, which skips a file only when it
matches **neither** half where the intent is to skip unless it matches **both**,
so any `.sqlite` file whose trailing token parsed as a plausible timestamp
reached the deletion branch. `013` demonstrated that deleting
`someone_elses_1704153600.sqlite`. The listing checked the prefix alone, so it
could show a file the sweep would never delete and hide one it would.

### B-2. The retention floor is stated in the zone it is compared in

`TS_MIN` is `1_704_067_200`, which is `2024-01-01T00:00:00Z`. It was
`1704063600`, annotated `2024/01/01 00:00:00`, which is that midnight in CET,
while every value it guards comes from `Utc::now()`. Nothing observable followed
from the hour. It is corrected because the constant guards a deletion and its
stated meaning is what a reader checks it against.

### B-3. A restore stages the replacement before destroying anything

The order was: validate, remove the database, the snapshots, the lock marker and
the logs, recreate the database directory, copy the backup into it. A failure of
the copy, or of the `create_dir_all` or the access-rights call between them, left
the node with neither its previous state nor the backup, and there was no
rollback.

It is now: validate, copy the backup to a staging name **beside** the database,
sync it, remove the snapshots, the lock marker and the logs, then **rename** the
staging file onto the database and sync the directory.

The database is the last thing touched and it is replaced by a rename, so a
crash anywhere in the sequence leaves either the old database or the new one at
the final name. The three removals now **report** their failures instead of
discarding them: a restore that could not remove the old snapshots or logs has
left a new database beside stale state, which is not a state to start on.

### B-4. A follower moves its state aside rather than deleting it

Every node that is not node 1 used to run `remove_dir_all(data_dir)` whenever
`HQL_BACKUP_RESTORE` parsed, discarding the result, and return into a normal
cluster join. That happens **before** node 1 has pulled, validated or copied
anything, and the two are not coordinated, so a restore that failed on node 1
had already destroyed every other node's state.

The deletion is now a move into `{data_dir}/pre-restore-{unix_seconds}/`, which
no hiqlite path reads, and its failures are returned. The node still starts as
though its directory were empty; the difference is that the previous state is
recoverable byte for byte if the restore this was preparing for never happens.
The storage owner lock is left where it is (`024` B-2), and an earlier
quarantine is not nested inside a new one.

**This is a mitigation, not the coordination the defect asks for.** See B-7.

### B-5. Validation checks the image and returns errors

Three checks where there was one:

- SQLite's own `quick_check`, which catches a truncated or structurally broken
  image. Nothing checked the image at all before.
- a `_metadata` row that exists, with a readable table.
- a metadata blob that **decodes**, returned as a named error rather than
  `unwrap`ed. The `unwrap` was inside `spawn_blocking`, so a corrupt blob panicked
  the task and the caller saw a join failure instead of "this backup is not
  valid".

`HQL_BACKUP_SKIP_VALIDATION` is parsed case-insensitively and **logs a warning**
when it takes effect. The old comparison was against the exact string `"true"`,
so `TRUE` silently validated; that direction is fail-closed, which is why this
is a correction rather than a repair, but an operator who meant to skip
validation and did not is worth telling either way.

### B-6. The purge is bounded, the retry count is counted, and the S3 configuration names what is wrong

- The post-restore log purge was `while let Err(..)` with no sleep, no bound and
  no exit, so a purge that kept failing spun the task at full CPU emitting an
  error line per iteration. It is now ten attempts with a 100 ms sleep, and it
  bails out for the same reason the snapshot trigger twelve lines above it does.
- The backup cron loop printed `"Backup task failed after {} retries"` with the
  literal bound, while only a forward-to-leader error retried at all. It now
  counts attempts and reports the last error.
- `S3Config::try_from_env` committed to five `expect`s, a `parse().expect` and an
  `unwrap` as soon as `HQL_S3_URL` read successfully. Each failure is now a named
  `Error::Config` saying which variable is missing or unparsable.
  `HQL_S3_PATH_STYLE` is optional and defaults to `true`, being the one variable
  with an obvious default. The reading is split into `from_lookup`, which takes
  the lookup as an argument, so it is testable without process-wide environment
  mutation, which `009` D-3 records as the reason no environment route in this
  corpus has a test.
- `S3Config::verify_access` proves the credentials with one list call, and the
  restore path calls it before pulling. A wrong key used to surface as a failed
  transfer partway through a restore that had already been announced, or as an
  error log from the detached upload task **after** a backup had been
  acknowledged.

### B-7. The supported restore topology is N=1, and this says why

A single-node restore is repaired and tested here: stage before destroy,
validated input, bounded purge.

A **multi-node restore is not supported by this release.** B-4 makes the
follower path non-destructive, which removes the worst outcome, and it does not
make the topology safe. Nothing coordinates the followers with node 1: they act
on an environment variable, at their own start time, with no signal that node 1
has validated or applied anything, and no way to tell a restore that succeeded
from one that never happened. Making that safe means a coordination point, which
is a design this spec does not attempt.

Stated here so a release note can repeat it without inventing it.

## 4. Evidence and its limits

Seven tests in `backup.rs`, four of them added or replaced here. **Four fail
against the unrepaired code**, run and observed: 3 passed, 4 failed.

- the local sweep only deletes files that are actually backups, asserted against
  the F-056 shape and two more lookalikes, one of which the old prefix-only
  listing predicate also disagreed about. This **replaces**
  `local_cleanup_deletes_files_that_are_not_backups`, which pinned the defect as
  expected behavior;
- the retention floor is UTC midnight on 2024-01-01;
- an invalid backup fails validation instead of panicking, for a file that is not
  a database and for a real database whose metadata does not decode;
- a follower moves its state aside instead of deleting it, asserted on the
  recovered bytes, on the owner lock surviving, and on a second run not nesting;
- missing S3 variables are named rather than panicking, through `from_lookup`.

What the acceptance does **not** establish:

- **No S3 transfer happens.** `verify_access` is not called in any test, and
  neither is `push` or `pull`. F-019's skipped S3 tests are unchanged, and the
  credential probe is a source change.
- **`restore_backup` is not executed end to end.** B-3's ordering is a source
  change. What is tested is the validation it depends on and the follower path
  beside it. A test of the full restore needs a real backup file and a real data
  directory layout, which is `012`'s surface.
- **The S3 test cannot exercise the environment route.** It drives `from_lookup`,
  so it proves the named errors and not that `try_from_env` reads the right
  variable names. That gap is `009` D-3's and is unchanged.
- **The cron loop is not executed.** B-6's attempt counting is a source change;
  `013` section 4 already stated that no test drives `start_cron`.
- **Nothing is crashed.** B-3's claim that a crash leaves either the old database
  or the new one rests on the rename being atomic, which is not demonstrated by a
  test here.

## 5. Known defects

**KD-1. `024` broke `013`'s acceptance and did not carry it.** `024` repaired the
follower wipe as a consequence of where it put the storage owner lock, which
changed a line `013`'s acceptance block asserted verbatim. `013`'s block has
been failing on the integration branch since that merge, and this spec is where
it is repaired, because this is the spec that takes `013`'s acceptance.

Recorded rather than tidied away. The rule it breaks is real: a repair to a unit
another spec owns has to carry that spec's acceptance, and `024` extended the
unit without doing so. Recorded as F-096.

**KD-2. A multi-node restore is still uncoordinated.** B-7. The followers act on
an environment variable with no signal from node 1.

**KD-3. The quarantine is never cleaned up.** `pre-restore-*` directories
accumulate, one per restore-join, each holding a full copy of the previous data
directory. That is deliberate, because deleting them is the behavior this spec
removed, but it is an operator's disk to manage and nothing says so except a
warning log.

**KD-4. `quick_check(1)` checks one page.** It catches a truncated or
structurally broken image, which is what a failed transfer produces. It is not
`integrity_check` and does not walk the whole database.

**KD-5. Validation still cannot tell whose backup this is.** The `TODO` `013`
quotes is unchanged: nothing compares a cluster or backup identity against what
the operator asked for, so restoring the wrong cluster's backup is accepted as
long as it is a structurally valid hiqlite database.

**KD-6. The credential probe is one list call at restore time.** It is not run at
startup, so a node configured for S3 backups with bad credentials still starts
and still discovers it at the first backup. Moving it to startup is startup-error
work.

**KD-7. `keep_days` is still never range-checked**, including against zero, which
`013` recorded and which this spec does not change.

## 6. Resolved decisions

**D-1 (2026-09-21, one predicate, not three consistent ones).** The alternative
was to make all three agree by copying the corrected guard. Declined: three
copies that agree today are three copies that can disagree tomorrow, and this
defect is what that looks like.

**D-2 (2026-09-21, stage beside the database, not in a sibling directory).** A
rename is only atomic within a filesystem, and a staging file beside its
destination is guaranteed to be on the same one. The cost is a `.restoring` file
in the database directory if the process dies mid-copy, which is inert and is
removed by the next restore.

**D-3 (2026-09-21, the follower quarantines rather than coordinates).** The
coordination B-7 describes is the right repair and is not attempted here. What is
done instead is the change that makes the failure survivable, and the topology
boundary is stated rather than implied. Shipping the deletion unchanged while
calling N=3 restore supported was the alternative, and it is the one this
decision exists to refuse.

**D-4 (2026-09-21, `verify_access` at restore, not at startup).** Startup is
where it belongs and startup is not this spec's. Calling it at restore closes the
case where a wrong key destroys a node's state in exchange for nothing, which is
the one that costs something. KD-6 records the rest.

**D-5 (2026-09-21, `from_lookup` rather than an environment-mutating test).**
`009` D-3 established that the environment route cannot be tested without
process-wide mutation. Extracting the pure function is the response that does not
require the mutation and does not leave the logic untested.

**D-6 (2026-09-21, the skip flag stays opt-in and gets louder).** Making
validation unskippable was considered and declined: an operator restoring a
backup taken by a different hiqlite version may legitimately need it. A warning
is the cost of keeping the escape hatch honest.

**D-7 (2026-09-21, this block is `013`'s acceptance).** Twelve of `013`'s
thirty-four commands asserted the defective expressions this spec removed, and
one named a test it replaces. Each is replaced below and marked with what it was.
The rest are carried forward unchanged.

## 7. Out of scope

- **Coordinating a multi-node restore.** B-7, KD-2.
- **Startup configuration failure in general.** The S3 half is repaired here
  because it is `013`'s unit; `NodeConfig::from_env`'s own panics are not.
- **S3 transfer evidence.** F-019, unchanged.
- **Snapshot publication and installation.** `025`.
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 026`. **This block is `013`'s acceptance as well as
this spec's** (D-7). `013`'s own file is not edited.

```verify:cli
# --- 013's acceptance, carried forward, with the defect-pinning commands replaced ---
test -f hiqlite/src/backup.rs
test -f hiqlite/src/s3.rs
sh -c 'spec-spine index owner hiqlite/src/backup.rs | grep -q 013-backup-retention-and-object-storage'
sh -c 'spec-spine index owner hiqlite/src/s3.rs | grep -q 013-backup-retention-and-object-storage'
sh -c 'spec-spine registry relationships 013-backup-retention-and-object-storage | grep -q 002-snapshot-publication-and-recovery'
cargo test -p hiqlite --lib backup::tests::backup_config_validates_only_the_cron_expression -- --exact
cargo test -p hiqlite --lib backup::tests::the_remote_retention_filter_accepts_only_the_documented_name_shape -- --exact
# was local_cleanup_deletes_files_that_are_not_backups, which pinned F-056
cargo test -p hiqlite --lib backup::tests::local_cleanup_only_deletes_files_that_are_actually_backups -- --exact
grep -q 'backup_node_{node_id}_{ts}.sqlite' hiqlite/src/store/state_machine/sqlite/writer.rs
# was a grep for the inverted `!starts_with && !ends_with` guard
sh -c 'grep -q "let Some(dt) = dt_from_backup_name(s) else" hiqlite/src/backup.rs'
# was `let ts_min = 1704063600;` and its CET comment
sh -c 'grep -q "const TS_MIN: i64 = 1_704_067_200;" hiqlite/src/backup.rs'
# was a grep for the listing's prefix-only predicate
sh -c 'grep -q "crate::backup::dt_from_backup_name(&name).is_none()" hiqlite/src/client/backup.rs'
grep -q 'fn dt_from_backup_name' hiqlite/src/backup.rs
# was `let _ = fs::remove_dir_all(node_config.data_dir.as_ref()).await;`, which 024 removed
sh -c 'grep -q "quarantine_data_dir_contents(node_config.data_dir.as_ref())" hiqlite/src/backup.rs'
# was `let _ = fs::remove_dir_all(&path_db).await;`, the destroy-before-replace step
sh -c '! grep -q "let _ = fs::remove_dir_all(&path_db).await;" hiqlite/src/backup.rs'
# was `fs::copy(&path_backup, &path_db_full).await?;`, the non-atomic replacement
sh -c 'grep -q "fs::rename(&path_db_staged, &path_db_full).await?;" hiqlite/src/backup.rs'
# was `! grep sync_data`: the restored image is now synced before it is published
sh -c 'grep -q "sync_file(&path_db_staged).await?;" hiqlite/src/backup.rs'
# was the exact-string comparison for HQL_BACKUP_SKIP_VALIDATION
sh -c 'grep -q "eq_ignore_ascii_case(\"true\")" hiqlite/src/backup.rs'
# was `let _meta: StateMachineData = deserialize(&bytes).unwrap();`
sh -c '! grep -q "deserialize(&bytes).unwrap();" hiqlite/src/backup.rs'
sh -c 'grep -q "PRAGMA quick_check(1)" hiqlite/src/backup.rs'
# was the unbounded `while let Err(..)` purge with no sleep
sh -c '! grep -q "while let Err(err) = state.raft_db.raft.trigger().purge_log(last_log).await" hiqlite/src/backup.rs'
sh -c 'grep -q "for attempt in 1..=10u32 {" hiqlite/src/backup.rs'
# was `Backup task failed after {} retries` with `let retries = 5;`
sh -c 'grep -q "Backup task failed after {attempts} attempt(s)" hiqlite/src/backup.rs'
sh -c 'grep -q "let max_attempts = 5;" hiqlite/src/backup.rs'
# was the four S3 greps pinning the assumption comment, the expects, the unwrap and the TODO
sh -c '! grep -q "we assume that all values exist when we can read the url successfully" hiqlite/src/s3.rs'
sh -c '! grep -q "HQL_S3_BUCKET not found" hiqlite/src/s3.rs'
sh -c '! grep -q "Bucket::new(url, bucket_name, region, credentials, options).unwrap()" hiqlite/src/s3.rs'
sh -c 'grep -q "pub async fn verify_access" hiqlite/src/s3.rs'
grep -q 'hiqlite/src/backup.rs' spec-spine.toml
grep -q 'hiqlite/src/s3.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/013-backup-retention-and-object-storage'
sh -c '! grep -rl "$(printf "\342\200\224")" specs/026-backup-and-restore-integrity'
# --- what this repair adds ---
cargo test -p hiqlite --lib --features sqlite,backup backup::tests::the_retention_floor_is_utc_midnight_on_2024_01_01 -- --exact
cargo test -p hiqlite --lib --features sqlite,backup backup::tests::an_invalid_backup_fails_validation_instead_of_panicking -- --exact
cargo test -p hiqlite --lib --features sqlite,backup backup::tests::a_follower_moves_its_state_aside_instead_of_deleting_it -- --exact
cargo test -p hiqlite --lib --features sqlite,backup,s3 backup::tests::missing_s3_variables_are_named_rather_than_panicking -- --exact
```
