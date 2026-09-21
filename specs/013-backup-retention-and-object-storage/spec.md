---
id: "013-backup-retention-and-object-storage"
title: "Adopt backup, retention and object storage"
status: draft
kind: "adoption"
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "002-snapshot-publication-and-recovery"
  - "009-configuration-contract"
  - "012-cluster-integration-evidence"
origin:
  retroactive: true
  paths:
    - "hiqlite/src/backup.rs"
    - "hiqlite/src/s3.rs"
establishes:
  - "hiqlite/src/backup.rs"
  - "hiqlite/src/s3.rs"
extends:
  - spec: "000-hiqlite-ownership-bootstrap"
    unit: { kind: file, path: "spec-spine.toml" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Adopts the backup schedule, the two retention sweeps, the restore sequence and
  the S3 client as one contract, because the file naming convention is the only
  thing that ties them together and three different functions disagree about
  what it is. Eight defects and contradictions recorded, three of them executed,
  including a retention filter whose boolean guard is inverted and therefore
  deletes files that are not backups. Repairs no runtime behavior.
---

# 013: Adopt backup, retention and object storage

## 1. Purpose

A hiqlite backup is a file name. `backup_node_{node_id}_{ts}.sqlite` is what
`create_backup` writes, what the local retention sweep parses, what the remote
retention sweep parses, what the dashboard listing filters on, and what
`HQL_BACKUP_RESTORE` names. Five pieces of code, one convention, and no single
place that states it.

This spec states it, and then states where the five disagree. Two of the eight
findings below are consequences of that convention being reimplemented rather
than shared: F-056, a retention sweep that deletes files it should not, and
F-062, a failure message that counts retries that were never attempted.

**It is an adoption.** Every behavior is described as found; nothing is
repaired. The only change to a shipped file is a `#[cfg(test)]` module appended
to `backup.rs`.

## 2. Territory

**Establishes** `hiqlite/src/backup.rs` (the schedule, both retention sweeps,
the restore sequence, and the post-restore snapshot and purge) and
`hiqlite/src/s3.rs` (the bucket configuration and the four transfer helpers).
Neither was claimed.

**Extends**, without re-establishing: `000` on `spec-spine.toml` for the
freshness declarations of section 7; `005` on the findings register and the
adoption plan.

**Depends on** `002`, which owns `store/state_machine/sqlite/`, where the backup
is actually produced; `009` for the eight environment variables; and `012`,
which owns the only tests that exercise any of this.

**Describes without claiming**: `writer.rs:620-700` and `:779-843`
(`create_backup` and the `WriterRequest::Backup` arm), `start.rs:52`, `:122` and
`:310-315`, and `client/backup.rs:60-230`. B-1 and B-6 are about how those sites
use this spec's units; no line in any of them is modified, so no edge is taken
(the reasoning `011` D-3 records).

## 3. Behavior

### B-1. One backup, produced by the leader's writer thread, on two triggers

A backup is produced in `002`'s unit, not this one. `create_backup`
(`writer.rs:779-843`) runs on the SQLite writer thread: it `VACUUM main INTO`s a
`{path}~` temporary, renames it into `backup_node_{node_id}_{ts}.sqlite`, reopens
that file and overwrites its `_metadata` row with `StateMachineData::default()`,
so a restored image carries no stale Raft state. The temp-then-rename is
deliberate and commented: a crash mid-vacuum must not leave a partial file under
a name the restore path would accept.

Two things trigger it. `Client::backup()` is the explicit one. The scheduled one
is `start_cron` (`backup.rs:93-158`), spawned from `start.rs:310-315`, which
sleeps until the next `cron_schedule` occurrence and then calls
`backup_cron_job` (`:160-193`), which is `client.backup()` plus the remote
retention sweep.

The acknowledgement is sent when the local file exists. The S3 upload is a
detached task started inside `create_backup` (`writer.rs:817-842`) with ten
attempts and a linear backoff; the local retention sweep is a second detached
task (`writer.rs:663-673`). Both report only through logs.

### B-2. Two retention sweeps, two different ideas of a backup file name

**Remote**, `backup_cron_job` (`backup.rs:167-190`): list the bucket, skip
buckets whose name is not the configured one, and for each object whose key
parses through `dt_from_backup_name` to a timestamp older than
`now - keep_days`, delete it. `dt_from_backup_name` (`:248-278`) is strict: the
key must start with `backup_node_`, contain a further `_`, end in `.sqlite`, and
have a parsable `i64` between them. Every rejection logs and returns `None`.

**Local**, `backup_local_cleanup` (`:195-246`): walk the backups directory, skip
directories, and apply

```rust
if !s.starts_with("backup_node_") && !s.ends_with(".sqlite") {
    continue;
}
```

then strip `.sqlite`, take the text after the last `_`, parse it as `i64`, and
delete the file if that value is both greater than `ts_min` and older than the
threshold. The guard is `!A && !B` where the intent is `!(A && B)`: a file is
skipped only when it matches **neither** half. KD-1 is the consequence, and it
is executed.

**A third filter**, `Client::backup_list_local` (`client/backup.rs:148-151`),
filters the same directory on `starts_with("backup_node_")` alone, and
`backup_list_s3` (`:189-192`) does the same for the bucket. So the directory
that the listing shows and the directory that the sweep prunes are defined by
three predicates, of which no two are the same.

### B-3. `ts_min` is a floor against parse accidents, and it is off by an hour

`backup_local_cleanup:196-197` sets `ts_min = 1704063600` with the comment
`// 2024/01/01 00:00:00`. A file is deleted only when its parsed timestamp is
strictly greater than that, which stops a file whose trailing token happens to
parse as a small integer from being treated as an ancient backup. The mechanism
is sound. The value is `2023-12-31T23:00:00Z`, which is midnight on 2024-01-01 in
CET and not in UTC, while every timestamp it is compared against comes from
`Utc::now()`. KD-8.

### B-4. The restore sequence destroys before it verifies the replacement landed

`restore_backup_start` (`:283-297`) runs from `start.rs:52`, before the Raft is
up. If `HQL_BACKUP_RESTORE` is set:

- on **node 1** it calls `restore_backup` and returns `Ok(true)`;
- on **every other node** it logs a warning and runs
  `let _ = fs::remove_dir_all(node_config.data_dir)`, discarding the result, then
  returns `Ok(false)` so the node proceeds into a normal cluster join.

`restore_backup` (`:305-377`) then, in order: rejects an `S3` source with no
`s3_config`; builds the folder set; pulls from S3 or copies from the local path
into `backups/`; validates with `is_metadata_ok`; **removes** `path_db`,
`path_snapshots`, `path_lock_file` and `path_logs`; recreates `path_db`; and
copies the staged backup into place.

The validation is before the destruction, which is the right order. The copy is
after it, which is not: a failure at `:368` leaves the node with neither its old
database nor a new one, and there is no rollback and no fsync. KD-2 and KD-3.

### B-5. Validation is one query, bypassable by an environment variable

`is_metadata_ok` (`:379-398`) returns `Ok(())` immediately when
`HQL_BACKUP_SKIP_VALIDATION` is exactly `"true"`; any other value, including
`"TRUE"`, validates. Otherwise it opens the candidate with `rusqlite`, selects
`data FROM _metadata WHERE key = 'meta'`, and deserializes the blob with
`deserialize(&bytes).unwrap()`.

So the contract is: a restorable backup is a readable SQLite file with a
`_metadata` row whose blob is a `StateMachineData`. Nothing checks the schema,
the row counts, or which cluster the image came from; the authored `TODO` at
`:393` says as much. And the `unwrap` makes a corrupt blob a panic rather than a
validation error. KD-4.

### B-6. After a restore, node 1 forces a snapshot and purges the log

`restore_backup_finish` (`:406-482`), called from `start.rs:122` when
`restore_backup_start` returned `true`, waits for Raft init, waits for a leader,
`debug_assert!`s that the leader is this node, issues ten `QueryWrite::RTT`
writes so the log has a committed index to purge up to, waits for
`last_applied.index >= 10`, triggers a snapshot, waits for it to appear in the
metrics, and purges the log up to that index.

Four of its five loops sleep. The purge loop (`:477-479`) does not: it is
`while let Err(err) = ... { error!(...) }` with no sleep, no bound and no exit.
KD-5.

### B-7. The S3 client is configuration plus four transfers, and validates nothing

`S3Config` (`s3.rs:10-13`) is one `Bucket`. `new` (`:16-43`) maps every failure
to `Error::S3`, and carries a `TODO` at `:40`: the credentials are never
exercised, so a wrong key is first discovered by the detached upload task in
`create_backup`, as a log line, after the backup has already been acknowledged.

`try_from_env` (`:45-76`) is the opposite in style. Its authored comment is "we
assume that all values exist when we can read the url successfully", and it acts
on that: once `HQL_S3_URL` parses, `HQL_S3_BUCKET`, `HQL_S3_REGION`,
`HQL_S3_PATH_STYLE`, `HQL_S3_KEY` and `HQL_S3_SECRET` are each `expect`ed, the
path-style flag is `parse().expect(...)`, and `Bucket::new` is `unwrap`ed where
the constructor four lines up returns a mapped error. KD-6.

The transfers are `push` (file to bucket, encrypted through `cryptr`), `pull`
(bucket to file, `overwrite_target: true`), and `pull_channel`, which spawns a
task and reports a stream failure by sending it into the channel writer. All
three inherit `cryptr`'s encryption; this spec does not specify it.

## 4. Evidence and its limits

Three characterization tests, appended to `backup.rs` as this spec's only change
to a shipped file. Each asserts current behavior and would fail if that behavior
changed; none is a regression test for a repair.

| test | what it establishes |
|---|---|
| `backup::tests::backup_config_validates_only_the_cron_expression` | the cron string is the only validated input, and `keep_days` is accepted unchecked, including zero |
| `backup::tests::the_remote_retention_filter_accepts_only_the_documented_name_shape` | all four rejection paths of `dt_from_backup_name`, and that each is silent (B-2) |
| `backup::tests::local_cleanup_deletes_files_that_are_not_backups` | the local sweep's guard, by executing it: an expired backup is deleted, a fresh one survives, a file matching neither half is skipped, and **a non-backup file matching only the suffix half is deleted** (KD-1) |

**What the tests do not establish.**

- **Anything about S3.** No bucket was contacted, no object was pushed, pulled,
  listed or deleted, and no credential was exercised. `s3.rs` has no test in this
  repository at all, and the one integration path that would reach it is skipped
  whenever `TEST_SKIP_S3_RESTORE` is set, which is what CI does (F-019).
- **The restore sequence.** B-4 and B-5 are read from source. The cluster suite
  does exercise a file restore (`012` B-1 phase 14), but only the success path,
  and only after `012` KD-1's earlier phases have all passed.
- **`create_backup` itself,** which is `002`'s unit and is described here only
  as the producer of the name the rest of this spec parses.
- **The cron schedule.** No test advances a clock through `start_cron`. B-1's
  timing, the `next <= now` clamp and the retry loop are source-established.
- **Anything about `keep_days = 0`,** which the first test shows is accepted: what
  a zero-day retention does to a backup taken seconds earlier was not executed.

## 5. Known defects

Recorded as found, none repaired. Each is also filed in
`standards/spec/findings-register.md`.

**KD-1. The local retention guard is inverted and deletes files that are not
backups** (F-056). `backup.rs:221-223`. The guard skips a file only when it
matches neither `backup_node_` nor `.sqlite`, where the intent is to skip unless
it matches both. Consequence: any file in the backups directory whose name ends
in `.sqlite` and whose text after the last `_` parses as a plausible Unix
timestamp is deleted by the sweep, and so is any file starting with
`backup_node_` regardless of suffix. `Client::backup_list_local`
(`client/backup.rs:148-151`) filters the same directory on the prefix alone, so
the listing and the sweep disagree about what a backup is.

**Observed by execution**: `local_cleanup_deletes_files_that_are_not_backups`
writes `someone_elses_1704153600.sqlite` into a temporary directory and the
sweep deletes it. The directory is hiqlite's own and normally holds nothing else,
which is why this is a latent defect rather than an incident; nothing prevents an
operator from putting a file there, and `HQL_BACKUP_RESTORE=file:` invites
exactly that by naming a path the restore then copies into `backups/`.

**KD-2. The restore removes the live database before the replacement is in
place** (F-057). `backup.rs:354-368`. `remove_dir_all` on the database,
snapshots, lock file and log directories happens first; `fs::copy` of the
validated backup happens after. A failure of the copy, or of the `create_dir_all`
and `set_path_access` between them, leaves the node with neither its previous
state nor the backup, and there is no rollback. The copied file is also never
`sync_data`ed before the node starts on it, so a crash between the copy and the
writeback can leave a short image that the next start accepts. Same family as
F-003 and F-004, in a different file. Source-established.

**KD-3. Every node that is not node 1 deletes its data directory before any
restore has succeeded** (F-058). `backup.rs:287-293`. Whenever
`HQL_BACKUP_RESTORE` parses to a known prefix, nodes 2 and 3 run
`let _ = fs::remove_dir_all(data_dir)` and carry on, with the result discarded.
This happens before node 1 has pulled, validated or copied anything, and there is
no coordination between them. Consequence: a restore that fails on node 1, for
instance because the `file:` path does not exist or the object is not in the
bucket, has already destroyed the other two nodes' state, so the cluster cannot
fall back to what it had. The discarded `Result` means a failed deletion is also
invisible, and the node then joins with a half-removed directory.
Source-established.

**KD-4. Backup validation is one query, and a corrupt payload panics rather than
failing validation** (F-059). `backup.rs:379-398`. `deserialize(&bytes).unwrap()`
at `:391` runs inside `spawn_blocking`, so a `_metadata` row that exists but does
not decode as `StateMachineData` panics the blocking task; the caller sees a join
error rather than "this backup is not valid". The validation itself checks only
that the row exists and decodes: not the schema, not the row counts, and not
which cluster produced the image, which the authored `TODO` at `:393`
acknowledges. `HQL_BACKUP_SKIP_VALIDATION` disables even that, and the
comparison is against the exact string `"true"`, so `"TRUE"` validates; that
direction is fail-closed. Source-established.

**KD-5. The post-restore log purge retries forever with no delay** (F-060).
`backup.rs:477-479`: `while let Err(err) = state.raft_db.raft.trigger().purge_log(last_log).await { error!(...) }`.
No sleep, no attempt bound, no exit. Consequence: a purge that keeps failing, for
example because the Raft is shutting down, spins this task at full CPU while
emitting an error line per iteration. Every other loop in the same function
sleeps between 50 ms and 100 ms, and the snapshot trigger twelve lines above was
deliberately changed to bail out rather than loop. Source-established.

**KD-6. The S3 configuration panics on a missing variable and validates no
credential** (F-061). `s3.rs:45-76`. Reading `HQL_S3_URL` successfully commits
`try_from_env` to five further `expect`s and a `parse().expect` and a
`Bucket::new(...).unwrap()`, on the authored assumption at `:47` that all values
exist together. A single missing or misspelled variable therefore ends the
process at configuration time, while the same `Bucket::new` failure in `new`
(`:37-38`) is a returned `Error::S3`. Separately, the `TODO` at `:40` records
that no code path checks the credentials: a wrong key is discovered by the
detached upload task in `create_backup` (`writer.rs:817-842`), after the backup
has been acknowledged, as an error log. Same class as F-009 and F-042.
Source-established.

**KD-7. The cron failure message counts retries that were not attempted**
(F-062). `backup.rs:121-155`. The loop is `for _ in 0..5`, but only a
forward-to-leader error sleeps and retries; every other error logs and `break`s
on the first attempt, leaving `success` false. The message that then runs is
`"Backup task failed after {retries} retries"` with `retries = 5`. Consequence:
an operator reading the log believes five backup attempts were made and all
failed, when one was made. Classed as a contradiction between authored text and
behavior, not a defect, by the register's class test. Source-established.

**KD-8. The retention floor constant is an hour earlier than its comment**
(F-063). `backup.rs:196-197`: `1704063600` is annotated `// 2024/01/01 00:00:00`
but is `2023-12-31T23:00:00Z`, which is that midnight in CET. Every value it is
compared against comes from `Utc::now()`. The consequence is one hour of
difference on a guard whose purpose is to be far in the past, so nothing
observable follows from it; it is recorded because the constant guards a
deletion and its stated meaning is what a reader would check it against.
Source-established.

**Retained without change.** F-019, which records that the S3 path is never
exercised in CI, and which section 4 cites rather than restates.

## 6. Resolved decisions

**D-1 (2026-09-21, nothing here is repaired, including KD-1).** KD-1 is a
one-character fix and it is tempting. It is also a change to what a scheduled
task deletes, on a tree where no test covered that task until this change added
one, and the correct predicate is a choice between the two that already exist in
`client/backup.rs` and `dt_from_backup_name`. Picking one of them is the repair,
and the repair belongs with the reconciliation of all three filters, not with the
adoption that found them disagreeing. KD-2 and KD-3 are the restore-atomicity
decision W-20 already holds for snapshots, in a second file.

**D-2 (2026-09-21, the tests characterize the defect rather than assert the
fix).** `local_cleanup_deletes_files_that_are_not_backups` asserts that the
non-backup file **is** deleted, with the finding id in the assertion message.
That is the shape constitution VI requires of an adoption: the test pins what the
code does so a later repair has a fixed baseline to be reviewed against, and the
test is expected to be inverted by that repair.

**D-3 (2026-09-21, `s3.rs` is claimed although nothing here executes it).**
Claiming a file whose evidence is entirely source-read is what M1 is for, and
leaving it unclaimed would leave KD-6 filed against no owner. Section 4 states
that no bucket was contacted, in those words, so the claim cannot be mistaken for
coverage.

## 7. The inventory declarations this spec requires

Neither `hiqlite/src/backup.rs` nor `hiqlite/src/s3.rs` is in any content hash at
the pinned revision, for the reason `010` section 8 and `011` section 8 give: the
existing globs reach the query, client, network and sqlite state-machine trees
but not `hiqlite/src/*.rs`. Without declarations, `spec-spine lint
--fail-on-warn` exits `1` with `L-008` warnings.

`spec-spine.toml` gains two `extra_hashed_inputs` entries. No
`[coverage] governed_scope` entry is needed; both files are inside the `hiqlite`
cargo package the walk already counts.

**It is a freshness declaration, not an enforcement setting.**

## 8. Out of scope

- **Every repair.** KD-1 to KD-8 are recorded and left.
- **`create_backup` and the writer thread**, which are `002`'s.
- **`Client::backup`, `backup_list_local`, `backup_list_s3`,
  `backup_file_local` and `backup_s3_stream`**, which are `003`'s units; B-2
  names two of them to state that three filters disagree.
- **`cryptr`'s encryption**, named as pinned dependency behavior.
- **The dashboard's backup views**, which are W-12's.
- **The startup-error policy.** W-22, which KD-6 joins.
- **Ratification, enforcement, and any tool or pin change.**

## Verification

Run with `just spine-verify 013`.

```verify:cli
test -f hiqlite/src/backup.rs
test -f hiqlite/src/s3.rs
sh -c 'spec-spine index owner hiqlite/src/backup.rs | grep -q 013-backup-retention-and-object-storage'
sh -c 'spec-spine index owner hiqlite/src/s3.rs | grep -q 013-backup-retention-and-object-storage'
sh -c 'spec-spine registry relationships 013-backup-retention-and-object-storage | grep -q 002-snapshot-publication-and-recovery'
cargo test -p hiqlite --lib backup::tests::backup_config_validates_only_the_cron_expression -- --exact
cargo test -p hiqlite --lib backup::tests::the_remote_retention_filter_accepts_only_the_documented_name_shape -- --exact
cargo test -p hiqlite --lib backup::tests::local_cleanup_deletes_files_that_are_not_backups -- --exact
grep -q 'backup_node_{node_id}_{ts}.sqlite' hiqlite/src/store/state_machine/sqlite/writer.rs
sh -c 'grep -q "if !s.starts_with(\"backup_node_\") && !s.ends_with(\".sqlite\")" hiqlite/src/backup.rs'
grep -q 'let ts_min = 1704063600;' hiqlite/src/backup.rs
grep -q '// 2024/01/01 00:00:00' hiqlite/src/backup.rs
sh -c 'grep -q "if !name.starts_with(\"backup_node_\")" hiqlite/src/client/backup.rs'
grep -q 'fn dt_from_backup_name' hiqlite/src/backup.rs
grep -q 'let _ = fs::remove_dir_all(node_config.data_dir.as_ref()).await;' hiqlite/src/backup.rs
grep -q 'let _ = fs::remove_dir_all(&path_db).await;' hiqlite/src/backup.rs
grep -q 'fs::copy(&path_backup, &path_db_full).await?;' hiqlite/src/backup.rs
sh -c '! grep -q "sync_data" hiqlite/src/backup.rs'
sh -c 'grep -q "env::var(\"HQL_BACKUP_SKIP_VALIDATION\") == Ok(\"true\".to_string())" hiqlite/src/backup.rs'
grep -q 'let _meta: StateMachineData = deserialize(&bytes).unwrap();' hiqlite/src/backup.rs
sh -c 'grep -A1 "while let Err(err) = state.raft_db.raft.trigger().purge_log(last_log).await" hiqlite/src/backup.rs | grep -q "error!"'
sh -c '! grep -A2 "while let Err(err) = state.raft_db.raft.trigger().purge_log(last_log).await" hiqlite/src/backup.rs | grep -q "sleep"'
grep -q 'Backup task failed after {} retries' hiqlite/src/backup.rs
grep -q 'let retries = 5;' hiqlite/src/backup.rs
grep -q 'we assume that all values exist when we can read the url successfully' hiqlite/src/s3.rs
grep -q 'HQL_S3_BUCKET not found' hiqlite/src/s3.rs
grep -q 'let bucket = Bucket::new(url, bucket_name, region, credentials, options).unwrap();' hiqlite/src/s3.rs
grep -q 'TODO try to list bucket and make sure access creds work fine' hiqlite/src/s3.rs
grep -q 'hiqlite/src/backup.rs' spec-spine.toml
grep -q 'hiqlite/src/s3.rs' spec-spine.toml
spec-spine check --fail-on-unresolved --fail-on-warn
spec-spine lint --fail-on-warn
spec-spine index coverage
sh -c '! grep -rl "$(printf "\342\200\224")" specs/013-backup-retention-and-object-storage'
```
