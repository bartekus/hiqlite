---
id: "025-snapshot-crash-recovery-contract"
title: "Make internal snapshot publication, installation and restart selection one crash-recovery contract"
status: draft
created: "2026-09-21"
owner: "hiqlite maintainers"
risk: critical
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "002-snapshot-publication-and-recovery"
  - "024-exclusive-storage-ownership"
amends: ["024-exclusive-storage-ownership"]
# D-6: this spec's `## Verification` block IS 024's acceptance from now on, which is also 002's
# through 024. Whole-block replacement is the mechanism's unit.
amends_verification: ["024-exclusive-storage-ownership"]
amends_sections:
  - "5-known-defects"
extends:
  - spec: "002-snapshot-publication-and-recovery"
    unit: { kind: directory, path: "hiqlite/src/store/state_machine/sqlite/" }
    nature: superseding
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/adoption-plan.md" }
    nature: additive
summary: >
  Repairs F-003, F-004 and F-006 as one contract rather than three fixes.
  Publication is a synced same-directory rename instead of a byte copy into the
  final name; installation stages, validates and restores before publishing, so
  a failed install neither destroys the last recoverable state nor becomes a
  restart candidate; and restart selection validates candidates, falls back to
  an older snapshot only when the local WAL can still close the gap, and
  otherwise refuses with an actionable error instead of starting empty.
---

# 025: Make internal snapshot publication, installation and restart selection one crash-recovery contract

## 1. Purpose

`002` section 7 records four defects in the internal SQLite snapshot path.
`024` repaired the third. This spec repairs the other three, and it repairs them
together because they are not three defects: they are one contract, stated in
three places, that never said what a crash is allowed to leave behind.

- **F-003.** Publication wrote the final UUID name with `fs::copy`, byte by
  byte, and synced nothing. An interrupted copy leaves a short file under a name
  that startup selection accepts.
- **F-004.** Installation renamed the received file to its final published name
  and *then* restored from it. A failed restore left that file eligible for
  selection at the next start.
- **F-006.** Startup took the greatest UUID, then `assert_eq!`d on the snapshot
  id embedded in it. A corrupt newest file was a panic, never a fallback, and
  the build that produced it had already deleted every older copy.

Read together they compose: F-003 manufactures the corrupt newest file, F-004
manufactures a second kind of it, and F-006 is why either one is fatal.

One responsibility: **what a crash, at any point in producing, receiving or
selecting a snapshot, is allowed to leave on disk.**

## 2. Territory

**Extends** `002`'s `directory` unit `hiqlite/src/store/state_machine/sqlite/`
with nature `superseding`, which is the edge `024` used on `002`'s other unit.

**Amends** `024`'s known-defects section and carries its acceptance (D-6), which
is `002`'s through `024`.

**Ownership boundary.** OpenRaft decides when to build, send and install a
snapshot, and what it does with an error from any of them. This spec states only
what hiqlite writes, in what order, and what it will accept back.

## 3. Behavior

### B-1. One naming rule: a published snapshot is a bare UUID, everything else is staging

Three staging names exist and **none** of them is selectable, because none of
them parses as a UUID:

| name | written by | becomes |
|---|---|---|
| `{uuid}.temp~` | the writer's `VACUUM INTO` | `{uuid}.temp` |
| `{uuid}.temp` | the writer, complete and synced | `{uuid}` |
| `temp` | OpenRaft's receive stream | `{uuid}.incoming` |
| `{uuid}.incoming` | installation, staged and validated | `{uuid}` |

Every transition is a rename **within one directory**, so each either happened
or did not. The old path had one of these steps as a copy.

### B-2. Publication is a synced rename, not a copy

The writer syncs the vacuumed image before renaming it into the staging name and
syncs the directory after. The builder then renames the staging name to the
published UUID and syncs the directory again.

Both halves of that matter and only one of them was there before. The rename was
already atomic **with respect to a reader**; it orders nothing **with respect to
a crash**, because the directory entry can be present while the file's contents
have not been written back. A repair that only replaced `copy` with `rename`
would have fixed the torn-file case and left the empty-file case.

### B-3. Installation stages, validates, restores, and only then publishes

In that order, and the order is the contract:

1. rename the received `temp` to `{uuid}.incoming`, then sync the file and the
   directory;
2. **validate** it: SQLite's own `quick_check`, a `_metadata` row that exists and
   decodes, and an embedded snapshot id that matches the id it was sent as;
3. restore the live database **from the staging name**;
4. publish by renaming to `{uuid}`, and sync the directory.

What each failure leaves behind is therefore stated rather than discovered:

- **validation fails**: the staging file is removed and the live database has
  not been touched. Nothing is published. This is the case a stream that ended
  early produces, and it used to be the case that published a truncated file
  under its final name.
- **the restore fails**: nothing is published, so restart selection still sees
  the snapshot this node had *before* the install. The live database may be
  partially overwritten, which is why it is not the thing recovery relies on:
  the unclean-shutdown marker is still present, so the next start rebuilds from
  the last published snapshot and the retained log.
- **a crash between any two steps**: whatever is on disk is either a staging
  name, which nothing selects, or a published snapshot that was restored from
  successfully.

### B-4. Restart selection validates, falls back, and otherwise refuses

Selection walks the published snapshots newest first and **validates** each one
with the same three checks as B-2. A candidate that fails is skipped with a log
line naming why, where before it was an `expect` or an `assert_eq!` and so a
panic.

The newest usable snapshot is taken unconditionally: that is what this node was
last told to be at, and taking it is not a fallback.

**A fallback is conditional**, and this is the part that must not be got wrong.
Restoring an older snapshot rewinds this node's applied state, and the only
thing that can carry it forward again is the log. So an older candidate is
accepted only when the local WAL still holds every entry from that snapshot
onwards: the WAL's purge frontier must not have passed the snapshot's last
applied index. The frontier is read from the log store before the state machine
is built, which costs one read and no reordering, because the log store is
started first anyway.

**No peer is assumed.** At `N = 1` there is no peer, so assuming one would
simply be wrong. At `N > 1` a leader might be able to supply the missing history
and might equally have purged it or be unreachable. The answer is the same in
both cases and it is a refusal.

A refusal is an error with an actionable message: it names every candidate and
why it was rejected, and it names the two recoveries that exist, restoring a
backup or removing this node's data directory and re-joining as a learner on a
cluster whose other members are healthy.

**Refusing is the point.** Returning "no snapshot" here would have started a node
that had held state on an empty database, presenting itself as pristine.

### B-5. Two published snapshots are kept

The cleanup after a build kept exactly one. That is why F-006 had nothing to fall
back to even once selection could look: the build that produced the newest
snapshot had already deleted the only other copy. It now keeps the newest two.

Stale staging files are collected too, but conservatively: `temp` is never
touched, because an install may be streaming into it, and a `{uuid}.temp` or
`{uuid}.incoming` is removed only when its id sorts below the oldest snapshot
being kept, which makes it necessarily finished.

### B-6. Serving a peer is a different question from restarting

`get_current_snapshot` sends the newest **readable** snapshot, with no log-bound
check. A peer asking for a snapshot is not recovering this node, so the
constraint in B-4 does not apply to it; what does apply is that an unreadable
newest file must not be sent, which it now is not.

## 4. Evidence and its limits

Eight tests in `state_machine.rs`, six of them added here. **Five of the six fail
against the unrepaired behavior**, run and observed: 5 passed, 5 failed, where
the five that pass are the pre-existing ones plus the two staging-name tests,
which characterize behavior that was already correct.

Every injection is deterministic. Nothing is raced and nothing is timed: the
"interrupted copy" is a file written truncated on purpose, which is precisely
what an interrupted copy leaves, and the "stream that ended early" is a `temp`
file containing something that is not a database.

- a torn published snapshot is never selected, and the older valid one is;
- a corrupt newest snapshot falls back, for two corruptions that used to fail in
  two different ways;
- a fallback is **refused** when the WAL has purged past the older snapshot, and
  the message says both the purge frontier and what to do;
- no usable snapshot refuses startup instead of starting empty, while a
  directory with no snapshots at all is still a pristine node;
- an unusable received snapshot is discarded without publishing or restoring,
  asserted on a byte-identical live database and on the absence of both the
  published and the staging name, and the node can still select what it had;
- an `{uuid}.incoming` staging file is never a restart candidate;
- the previous published snapshot is kept, over three successive builds.

What the acceptance does **not** establish:

- **Nothing is crashed.** No process is killed and no power is lost. What is
  demonstrated is that a file in the state a crash would leave is not selected,
  and that the order of operations is the one B-3 describes. That the `fsync`
  calls make the rename durable on a given filesystem is **not** demonstrated by
  any test here and cannot be by a test in this process.
- **No Raft group runs.** `install_snapshot` is called directly, not by
  `RaftCore` streaming a real snapshot from a peer.
- **The restore-fails case is reasoned, not executed.** The validation-fails case
  is executed; a restore that fails *after* validation passed needs a writer
  fault injection this crate does not have for the SQLite writer.
- **`quick_check(1)` checks one page.** It catches a truncated or structurally
  broken image, which is what these defects produce. It is not `integrity_check`
  and does not walk the whole database.
- **The log-bound check uses the WAL's reported purge frontier.** Whether that
  frontier is itself accurate is `001`'s and `008`'s territory.

## 5. Known defects

**KD-1. A failed restore leaves the live database in an unknown state.** B-3
keeps the *last recoverable state* by not publishing, and recovery then depends
on the unclean-shutdown marker causing a rebuild at the next start. Nothing
rolls the live database back in place, and nothing marks the node unhealthy in
the meantime: it carries on serving from a database that a partial restore has
been written over. Repairing that means staging the live database too, which is
a larger change than this one.

**KD-2. The fallback is more conservative than a cluster needs.** At `N > 1` a
healthy leader may well be able to supply the history the local WAL has purged,
and B-4 refuses anyway. That is a deliberate trade, and the cost is a node that
could have recovered automatically needing an operator instead.

**KD-3. Two is a constant, not a policy.** `SNAPSHOTS_KEPT` is not configurable
and no analysis says two is the right number. It is one more than the number
that made F-006 unrecoverable.

**KD-4. Nothing verifies a snapshot against the log it claims to summarize.**
Validation establishes that a file is a readable snapshot carrying its own id. It
does not establish that its `last_applied_log_id` is consistent with what this
node's WAL holds, beyond the purge-frontier comparison B-4 makes when falling
back.

**KD-5. `002`'s fifth known defect is only half closed.** It asked for "a
deterministic interruption test for the mid-copy final name", which section 4
provides, "and a fail-closed startup policy", which B-4 provides. It also noted
that the cluster test never reached its self-healing section, which is still
true and is `012`'s.

## 6. Resolved decisions

**D-1 (2026-09-21, sync as well as rename).** Replacing `copy` with `rename`
alone would have closed the torn-file case and left the zero-length case, which
is the same defect with a different timing. Both syncs are named in B-2 for that
reason.

**D-2 (2026-09-21, validate before restoring, not after publishing).** The
alternative, publish-then-validate-then-restore, keeps F-004 exactly: a file that
fails validation is already selectable. Validation is the first thing that
touches the received bytes at all.

**D-3 (2026-09-21, restore from the staging name).** So that the published name
only ever appears for a snapshot that has already been restored from
successfully. This is what makes "a failed install is not a restart candidate"
true by construction rather than by cleanup.

**D-4 (2026-09-21, refuse rather than start empty, and refuse rather than assume
a peer).** Two refusals, one reason: a node that cannot establish what it was
must not present itself as something else. Owner direction was explicit that
`N = 1` must never assume another leader can supply missing history, and the
same rule is applied at `N > 1` because "might be able to" is not a recovery
plan. KD-2 records the cost.

**D-5 (2026-09-21, keep two).** The smallest number that makes a fallback
possible. KD-3 records that it is not a policy.

**D-6 (2026-09-21, this block is `024`'s acceptance).** Which is `002`'s through
`024`. All eighteen of `024`'s commands are carried forward unchanged.

## 7. Out of scope

- **Exclusive storage access.** `024`.
- **Rolling the live database back after a failed restore.** KD-1.
- **The cache state machine's snapshots.** `006`.
- **The external state-machine engine**, which `002` section 6 already describes
  as doing most of this correctly and which is unchanged here.
- **Backup and restore from an object store.** `013`, and the rest of F-056 to
  F-063.
- **Ratification, enforcement, publication and release.**

## Verification

Run with `just spine-verify 025`. **This block is `024`'s acceptance as well as
this spec's** (D-6), and `024`'s is `002`'s, so `spec-spine verify 002` and
`verify 024` both resolve here.

```verify:cli
# Package names, not library names: the downstream release renamed the three packages
# (`031` B-2), and `-p` takes a package name. `use hiqlite::..` is unaffected.
# --- 024's acceptance, which is also 002's, carried forward unchanged ---
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::restart_reconstructs_snapshot_then_replays_retained_wal -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::interrupted_staging_files_are_not_published_snapshots -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features external-state-machine external_state_machine::tests::snapshot_evidence_restore_receipts_and_staleness -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features external-state-machine external_state_machine::tests::online_backup_snapshot_preserves_implicit_rowids -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features external-state-machine external_state_machine::tests::durability_is_explicit_and_unclean_replayable_off_fails_closed -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite storage_lock::tests::a_second_process_is_refused_without_touching_the_data -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite storage_lock::tests::this_process_is_refused_while_another_process_holds_it -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite storage_lock::tests::an_orderly_shutdown_releases_ownership -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite storage_lock::tests::a_crash_releases_ownership -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite storage_lock::tests::a_contender_is_refused_while_ownership_is_held_even_to_restore -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite storage_lock::tests::a_second_node_in_the_same_process_is_refused -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite storage_lock::tests::an_aliased_path_to_the_same_directory_is_refused -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite storage_lock::tests::dropping_the_guard_releases_ownership -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite storage_lock::tests::the_owner_lock_file_is_recognisable -- --exact
sh -c 'grep -q "FileExt::try_lock" hiqlite/src/storage_lock.rs'
sh -c 'grep -q "StorageInUse" hiqlite/src/error.rs'
sh -c 'grep -q "release_storage_ownership" hiqlite/src/client/mgmt.rs'
sh -c '! grep -q "fs::remove_dir_all(node_config.data_dir.as_ref())" hiqlite/src/backup.rs'
# --- what this repair adds ---
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::a_torn_published_snapshot_is_never_selected -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::a_corrupt_newest_snapshot_falls_back_to_an_older_valid_one -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::a_fallback_is_refused_when_the_wal_has_purged_past_the_older_snapshot -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::no_usable_snapshot_refuses_startup_instead_of_starting_empty -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::an_unusable_received_snapshot_is_discarded_without_publishing_or_restoring -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::an_incoming_staging_file_is_never_a_restart_candidate -- --exact
cargo test -p hiqlite-patched --lib --no-default-features --features sqlite,auto-heal store::state_machine::sqlite::state_machine::tests::the_previous_published_snapshot_is_kept -- --exact
# publication must never be a byte copy into the final name again
sh -c '! grep -q "fs::copy(path_temp" hiqlite/src/store/state_machine/sqlite/snapshot_builder.rs'
sh -c 'grep -q "fs::rename(&path_temp, &path)" hiqlite/src/store/state_machine/sqlite/snapshot_builder.rs'
# the directory that holds a published name is synced
sh -c 'grep -q "fn sync_parent_dir_blocking" hiqlite/src/store/state_machine/sqlite/mod.rs'
# installation stages and validates before it restores, and publishes last
sh -c 'grep -q "fn validate_staged_snapshot" hiqlite/src/store/state_machine/sqlite/state_machine.rs'
sh -c 'grep -q "fn select_startup_snapshot" hiqlite/src/store/state_machine/sqlite/state_machine.rs'
sh -c 'grep -q "SNAPSHOTS_KEPT: usize = 2" hiqlite/src/store/state_machine/sqlite/snapshot_builder.rs'
```
