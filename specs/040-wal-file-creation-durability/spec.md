---
id: "040-wal-file-creation-durability"
title: "Make a new WAL file's header, length and name durable before anything is appended to it"
status: approved
created: "2026-09-25"
owner: "hiqlite maintainers"
risk: high
implementation: complete
depends_on:
  - "000-hiqlite-ownership-bootstrap"
  - "001-wal-durability-and-completion"
  - "008-wal-append-completion-notification"
amends: ["001-wal-durability-and-completion"]
amends_sections:
  - "3-synchronization-modes"
extends:
  # B-1 and B-2: `WalFile::create_file` and its directory sync, in `hiqlite-wal/src/wal.rs`.
  - spec: "001-wal-durability-and-completion"
    unit: { kind: directory, path: "hiqlite-wal/src/" }
    nature: additive
  - spec: "005-adoption-assessment-and-plan"
    unit: { kind: file, path: "standards/spec/findings-register.md" }
    nature: additive
summary: >
  Repairs F-135. Creating a WAL file, the first one in an empty directory or a
  new one at rollover, flushed its header asynchronously and never synced the
  file or the directory that lists it, so under `LogSync::Immediate` an append
  acknowledged just after a rollover could be in a file a power cut unlists.
  Creation now flushes the header, syncs the file and syncs the directory, and
  fails if any of them fails. Amends 001 section 3.
---

# 040: Make a new WAL file's header, length and name durable before anything is appended to it

## 1. Purpose

F-135 (recorded by `038` D-5 from the patched.3 release review):
`WalFile::create_file` (`hiqlite-wal/src/wal.rs`) created the file with
`File::create_new`, set its length, wrote the header through a mapping and
called only `flush_async`. There was no `sync_all` of the file and no sync of
its directory. It runs for the first WAL file of an empty directory
(`WalFileSet::read`) and for every rollover (`WalFileSet::add_file`).

Under `LogSync::Immediate` the writer flushes the appended bytes before it
acknowledges, but a data flush does not make the file's name durable. After a
power cut, an append acknowledged just after a rollover could be in a file the
directory no longer lists; `WalFileSet::check_integrity` then refuses the start
rather than serving a shorter log. On an N=1 node that is an acknowledged write
that cannot be recovered.

## 2. Ownership

This spec extends `001`'s `hiqlite-wal/src/` directory for one function,
`WalFile::create_file`, and the directory sync it calls. The append path, the
synchronization modes and the metadata write stay `001`'s and `008`'s. OpenRaft
owns what an acknowledged entry means for consensus; hiqlite owns whether the
bytes and the file that holds them are on stable storage when it acknowledges.

## 3. Behavior

### B-1. A new WAL file is durable before it is used

Creating a WAL file MUST, before returning success: write the header, flush the
header synchronously, `sync_all` the file (its length and metadata), and sync
the directory that holds it. This applies to the first file of an empty
directory and to every file created at rollover. It holds in every `LogSync`
mode, because it runs at creation and not per append.

### B-2. A failed sync fails the creation

A failure of any of those steps MUST be returned as an error from the creation,
never ignored. The directory sync runs on Unix, where opening a directory and
syncing it is the platform's mechanism; elsewhere no directory sync is
performed. This differs deliberately from the metadata write's best-effort
directory sync (`001` section 4), which stays as it is.

### Amendment to `001` section 3

`001` section 3's `Immediate` row describes only the per-append flush. Read with
this spec: under `Immediate`, an acknowledged append is also in a file whose
header, length and directory entry were made durable when the file was created
(B-1), so a rollover no longer opens a window in which the acknowledgement
outlives the file's name. The async modes' statements are unchanged. `001`'s
text is not edited (`amends` never edits the amended spec).

## 4. Evidence and its limits

`hiqlite-wal/src/wal.rs`, `wal::tests::creating_a_wal_file_syncs_its_directory`:
it creates the first WAL file of an empty directory through
`WalFileSet::read`, then a second through `WalFileSet::add_file`, and asserts
after each that the directory was synced, through a test-only record of
directory syncs (`synced_dirs`). Observed failing first on `3d6f582` (the test
alone, before the fix): "the first WAL file was created without syncing its
directory". Passing after the fix, with the whole `hiqlite-wal` library suite
(40 tests), on macOS arm64; CI runs it on Linux amd64 (`Check`,
`just test-no-s3`).

Limits. The test proves the sync is **called** for the right directory at both
creation paths; it does not prove durability across a real power cut, which no
test here can produce. The record is written after the sync returns, so a
creation whose sync failed records nothing and returns the error (B-2), but no
test injects that failure. `file.sync_all()` and the synchronous header flush
are read from the source, not observed by the test.

## 5. Known defects

None new. The metadata write's directory sync stays best-effort (`001` section
4, its stated limit).

## 6. Out of scope

The metadata write's directory sync, the per-append flush of each `LogSync`
mode, and removing WAL files at purge (a removed name that reappears after a
power cut is read and skipped or checked at startup, `001` section 6).

## 7. Resolved decisions

**D-1 (2026-09-25, the header flush becomes synchronous).** F-135 names the
directory entry. Syncing the directory alone would leave a durable name for a
file whose header was only queued for writeback, which `WalFile::read_from_file`
refuses as corrupted. The header flush is therefore synchronous and the file is
`sync_all`ed before the directory sync. The cost is paid once per WAL file, not
per append.

## Verification

Run with `just spine-verify 040`.

```verify:cli
# B-1: both creation paths sync the directory (fails on 3d6f582's parent, section 4)
cargo test -p hiqlite-wal-patched --lib wal::tests::creating_a_wal_file_syncs_its_directory -- --exact
# B-1: the header flush is synchronous and the file is synced before the directory
sh -c 'grep -A14 "pub fn create_file" hiqlite-wal/src/wal.rs | grep -q "mmap.flush()?;"'
sh -c 'grep -A14 "pub fn create_file" hiqlite-wal/src/wal.rs | grep -q "file.sync_all()?;"'
sh -c 'grep -A20 "pub fn create_file" hiqlite-wal/src/wal.rs | grep -q "sync_dir(dir)?;"'
# B-2: the directory sync propagates its error on Unix
sh -c 'grep -A3 "^fn sync_dir" hiqlite-wal/src/wal.rs | grep -q "File::open(dir)?.sync_all()?;"'
sh -c 'grep -q "^### F-135 " standards/spec/findings-register.md'
```
