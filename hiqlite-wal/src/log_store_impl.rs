use crate::{LogStore, LogStoreReader, reader, writer};
use bincode::config::{Configuration, Fixint, LittleEndian};
use bincode::error::{DecodeError, EncodeError};
use openraft::storage::{LogFlushed, RaftLogStorage};
use openraft::{
    AnyError, ErrorSubject, ErrorVerb, LogId, OptionalSend, RaftLogId, RaftLogReader,
    RaftTypeConfig, StorageError, StorageIOError, Vote,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::Bound;
use std::fmt::Debug;
use std::ops::RangeBounds;
use tokio::sync::oneshot;
use tracing::debug;

const BINCODE_CONFIG: Configuration<LittleEndian, Fixint> = bincode::config::legacy();

#[inline(always)]
pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, EncodeError> {
    // We are using the legacy config on purpose here. It uses fixed-width integer fields, which
    // uses a bit more space, but is faster.
    bincode::serde::encode_to_vec(value, BINCODE_CONFIG)
}

#[inline(always)]
pub fn deserialize<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, DecodeError> {
    bincode::serde::decode_from_slice::<T, _>(bytes, BINCODE_CONFIG).map(|(res, _)| res)
}

/// The writer or reader thread has ended, so the channel it owned is gone.
///
/// Every call below used to `expect` or `unwrap` here. That turns a terminal storage thread
/// into a panic on whichever task made the call, which for the raft paths is the `RaftCore`
/// task: under `panic = "abort"` it ends the process and under unwinding it kills that task
/// silently. A queued or in-flight operation whose thread has gone away is a storage error,
/// and openraft has a channel for exactly that.
#[inline]
/// The writer's own verdict on an append whose entry stream it stopped reading.
///
/// The writer stops reading only after it has decided the batch's outcome, so that decision is
/// already on its way down `ack`. This waits for it and reports it, so the caller is told why
/// the append failed rather than that a channel was closed.
async fn writer_verdict<T: RaftTypeConfig>(
    ack_rx: oneshot::Receiver<Result<(), crate::error::Error>>,
) -> StorageError<T::NodeId> {
    match ack_rx.await {
        // The expected case: the writer refused the batch and said why.
        Ok(Err(err)) => StorageIOError::write_logs(&err).into(),
        // The writer reported success for a batch it never finished reading. That is a broken
        // contract rather than a storage failure, and it is named as one instead of being
        // returned to openraft as a successful append.
        Ok(Ok(())) => StorageError::IO {
            source: StorageIOError::new(
                ErrorSubject::Logs,
                ErrorVerb::Write,
                AnyError::error(
                    "the WAL writer reported this append as complete while it was still being \
                     sent, so the batch it persisted is not the batch that was submitted",
                ),
            ),
        },
        // The writer is gone without a verdict, which is what a panic in it looks like here.
        Err(_) => thread_gone::<T>(ErrorSubject::Logs, ErrorVerb::Write, "writer"),
    }
}

fn thread_gone<T: RaftTypeConfig>(
    subject: ErrorSubject<T::NodeId>,
    verb: ErrorVerb,
    what: &'static str,
) -> StorageError<T::NodeId> {
    StorageError::IO {
        source: StorageIOError::new(
            subject,
            verb,
            AnyError::error(format!(
                "the WAL {what} thread is no longer running, so this operation cannot complete"
            )),
        ),
    }
}

impl<T> RaftLogReader<T> for LogStore<T>
where
    T: RaftTypeConfig,
{
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<T::Entry>, StorageError<T::NodeId>> {
        try_get_log_entries::<T, _>(&self.reader, range).await
    }
}

impl<T> RaftLogReader<T> for LogStoreReader<T>
where
    T: RaftTypeConfig,
{
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> Result<Vec<T::Entry>, StorageError<T::NodeId>> {
        try_get_log_entries::<T, _>(&self.tx, range).await
    }
}

#[tracing::instrument(skip_all)]
#[inline(always)]
// The error type is huge, but it's given by the openraft trait definition.
#[allow(clippy::result_large_err)]
async fn try_get_log_entries<
    T: RaftTypeConfig,
    RB: RangeBounds<u64> + Clone + Debug + OptionalSend,
>(
    tx: &flume::Sender<reader::Action>,
    range: RB,
) -> Result<Vec<T::Entry>, StorageError<T::NodeId>> {
    let from = match range.start_bound() {
        Bound::Included(i) => *i,
        // `Excluded(u64::MAX)` means "nothing after this" - treat it as an empty range below
        Bound::Excluded(i) => i.checked_add(1).unwrap_or(u64::MAX),
        Bound::Unbounded => 0,
    };
    let until = match range.end_bound() {
        Bound::Included(i) => *i,
        // `Excluded(0)` means "up to but not including log 0" - an empty range
        Bound::Excluded(i) => i.saturating_sub(1),
        Bound::Unbounded => unreachable!(),
    };
    debug!("Entering try_get_log_entries() from {from} until {until}");

    if from > until {
        // empty range - nothing to read (also covers `Excluded(u64::MAX)` / `Excluded(0)`)
        return Ok(Vec::new());
    }
    let mut res: Vec<T::Entry> = Vec::with_capacity((until - from + 1) as usize);

    let (ack, rx) = flume::bounded(1);
    tx.send_async(reader::Action::Logs { from, until, ack })
        .await
        .map_err(|_| thread_gone::<T>(ErrorSubject::Logs, ErrorVerb::Read, "reader"))?;

    while let Some(data_res) = rx
        .recv_async()
        .await
        .map_err(|_| thread_gone::<T>(ErrorSubject::Logs, ErrorVerb::Read, "reader"))?
    {
        let data = data_res.map_err(|err| StorageError::IO {
            source: StorageIOError::read_logs(&err),
        })?;
        let entry = deserialize::<T::Entry>(&data).map_err(|err| StorageError::IO {
            source: StorageIOError::<T::NodeId>::read_logs(&err),
        })?;
        res.push(entry);
    }

    Ok(res)
}

impl<T> RaftLogStorage<T> for LogStore<T>
where
    T: RaftTypeConfig,
{
    type LogReader = LogStoreReader<T>;

    #[tracing::instrument(skip_all)]
    async fn get_log_state(&mut self) -> Result<openraft::LogState<T>, StorageError<T::NodeId>> {
        debug!("Entering get_log_state()");

        let (ack, rx) = oneshot::channel();
        self.reader
            .send_async(reader::Action::LogState(ack))
            .await
            .map_err(|err| {
                StorageIOError::new(ErrorSubject::Logs, ErrorVerb::Read, AnyError::new(&err))
            })?;

        let log_state = rx
            .await
            .map_err(|_| thread_gone::<T>(ErrorSubject::Logs, ErrorVerb::Read, "reader"))?
            .map_err(|err| {
                StorageIOError::new(ErrorSubject::Logs, ErrorVerb::Read, AnyError::new(&err))
            })?;

        let last_purged_log_id = if let Some(bytes) = log_state.last_purged_log_id {
            Some(deserialize(&bytes).map_err(|err| {
                StorageIOError::new(ErrorSubject::Logs, ErrorVerb::Read, AnyError::new(&err))
            })?)
        } else {
            None
        };
        let last_log_id = if let Some(bytes) = log_state.last_log {
            Some(deserialize(&bytes).map_err(|err| {
                StorageIOError::new(ErrorSubject::Logs, ErrorVerb::Read, AnyError::new(&err))
            })?)
        } else {
            None
        };

        Ok(openraft::LogState {
            last_purged_log_id,
            last_log_id,
        })
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn get_log_reader(&mut self) -> Self::LogReader {
        debug!("Entering get_log_reader()");

        self.spawn_reader()
            .expect("Error spawning additional LogStoreReader")
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn save_vote(&mut self, vote: &Vote<T::NodeId>) -> Result<(), StorageError<T::NodeId>> {
        debug!("Entering save_vote(): {:?}", vote);

        let value = serialize(vote).map_err(|err| StorageError::IO {
            source: StorageIOError::write_vote(&err),
        })?;
        let (ack, rx) = oneshot::channel();
        self.writer
            .send_async(writer::Action::Vote { value, ack })
            .await
            .map_err(|_| thread_gone::<T>(ErrorSubject::Vote, ErrorVerb::Write, "writer"))?;

        rx.await
            .map_err(|_| thread_gone::<T>(ErrorSubject::Vote, ErrorVerb::Write, "writer"))?
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write_vote(&err),
            })?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn read_vote(&mut self) -> Result<Option<Vote<T::NodeId>>, StorageError<T::NodeId>> {
        debug!("Entering read_vote()");

        let (ack, rx) = oneshot::channel();

        self.reader
            .send_async(reader::Action::Vote(ack))
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::read_vote(&err),
            })?;

        let vote = match rx
            .await
            .map_err(|_| thread_gone::<T>(ErrorSubject::Vote, ErrorVerb::Read, "reader"))?
            .map_err(|err| StorageError::IO {
                source: StorageIOError::read_vote(&err),
            })? {
            Some(b) => Some(deserialize(&b).map_err(|err| StorageError::IO {
                source: StorageIOError::read_vote(&err),
            })?),
            None => None,
        };

        Ok(vote)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn append<I>(
        &mut self,
        entries: I,
        callback: LogFlushed<T>,
    ) -> Result<(), StorageError<T::NodeId>>
    where
        I: IntoIterator<Item = T::Entry> + Send,
        I::IntoIter: Send,
    {
        debug!("Entering append()");

        let (tx, rx) = flume::bounded(1);
        let (ack, ack_rx) = oneshot::channel();

        // Forward the writer's result verbatim. This closure is the OpenRaft error boundary: it
        // previously hardcoded `Ok(())`, so no failure the writer knew about could reach
        // `RaftCore`. It must stay a pure forward.
        let callback: writer::AppendCompletion =
            Box::new(move |res| callback.log_io_completed(res));
        self.writer
            .send_async(writer::Action::Append { rx, callback, ack })
            .await
            .map_err(|err| StorageIOError::write_logs(&err))?;

        // Every early return between here and the end-of-stream marker drops `tx`, which is
        // the truncated stream the writer now names rather than mistaking for a clean end. The
        // writer reports that failure on both channels, so returning here does not leave the
        // completion callback unfired.
        //
        // A **failed send** is different from a failed serialization, and is not reported as
        // one. The writer only stops reading a batch once it has already decided that batch's
        // outcome, so a closed receiver means the verdict is waiting on `ack`. Returning the
        // `SendError` here would replace "this entry is larger than the WAL file" with
        // "sending on a closed channel", which names the symptom and discards the cause. Which
        // of the two the caller saw depended purely on whether the writer got to `drop(rx)`
        // before this loop got to its next send.
        for entry in entries {
            let data = serialize(&entry).map_err(|err| StorageIOError::write_logs(&err))?;
            if tx
                .send_async(Some((entry.get_log_id().index, data)))
                .await
                .is_err()
            {
                return Err(writer_verdict::<T>(ack_rx).await);
            }
        }
        if tx.send_async(None).await.is_err() {
            return Err(writer_verdict::<T>(ack_rx).await);
        }

        ack_rx
            .await
            .map_err(|_| thread_gone::<T>(ErrorSubject::Logs, ErrorVerb::Write, "writer"))?
            .map_err(|err| StorageIOError::write_logs(&err))?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn truncate(&mut self, log_id: LogId<T::NodeId>) -> Result<(), StorageError<T::NodeId>> {
        debug!("truncate(): [{:?}, +oo)", log_id);

        let (ack, rx) = oneshot::channel();
        self.writer
            .send_async(writer::Action::Remove {
                from: log_id.index,
                until: u64::MAX,
                last_log: None,
                ack,
            })
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write_logs(&err),
            })?;

        rx.await
            .map_err(|_| thread_gone::<T>(ErrorSubject::Logs, ErrorVerb::Write, "writer"))?
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write_logs(&err),
            })
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn purge(&mut self, log_id: LogId<T::NodeId>) -> Result<(), StorageError<T::NodeId>> {
        debug!("purge(): [0, {:?}]", log_id);

        let last_log = Some(serialize(&log_id).map_err(|err| StorageError::IO {
            source: StorageIOError::write_logs(&err),
        })?);
        let (ack, rx) = oneshot::channel();
        self.writer
            .send_async(writer::Action::Remove {
                from: 0,
                until: log_id.index,
                last_log,
                ack,
            })
            .await
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write_logs(&err),
            })?;

        rx.await
            .map_err(|_| thread_gone::<T>(ErrorSubject::Logs, ErrorVerb::Write, "writer"))?
            .map_err(|err| StorageError::IO {
                source: StorageIOError::write_logs(&err),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::fault;
    use crate::{LogStore, LogSync};
    use openraft::storage::RaftLogStorageExt;
    use openraft::testing::blank_ent;

    openraft::declare_raft_types!(
        pub TestTypeConfig:
            D = String,
            R = String,
            Node = openraft::BasicNode,
            SnapshotData = tokio::fs::File,
    );

    async fn start(base: &str) -> LogStore<TestTypeConfig> {
        let _ = std::fs::remove_dir_all(base);
        LogStore::<TestTypeConfig>::start(base.to_string(), LogSync::Immediate, 64 * 1024)
            .await
            .unwrap()
    }

    /// W-07's recovery half, through the adapter rather than through the WAL internals.
    ///
    /// A truncated append is **not** a torn record: every entry in the prefix was written with
    /// a valid CRC, a valid id and a header update, so nothing in the integrity check will ever
    /// notice or roll back the missing suffix. That is why "recovery tolerates torn trailing
    /// records" is not an answer here, and why this test reopens the store and reads what is
    /// actually there through `get_log_state` and `try_get_log_entries`.
    ///
    /// The batch is large enough to cross at least one WAL file boundary before it is cut off,
    /// so the prefix spans a rollover. Every supported `LogSync` mode is swept, because what
    /// each mode does to the bytes before the process ends differs and W-07's bar names all
    /// three.
    ///
    /// What this does **not** establish: that the bytes survive a power loss. The process is
    /// not killed here, so what is demonstrated is that the persisted prefix is consecutive,
    /// self-consistent and readable after a fresh open, not that any particular mode made it
    /// durable. `ImmediateAsync` and `IntervalMillis` start a writeback and do not wait for it;
    /// naming that as durability is the mistake `001` exists to prevent.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_truncated_append_leaves_a_recoverable_prefix_in_every_log_sync_mode() {
        // Small enough that a batch of this size crosses a file boundary mid-collection.
        const WAL_SIZE: u32 = 8 * 1024;
        const BATCH: u64 = 200;

        for (mode, label) in [
            (LogSync::Immediate, "immediate"),
            (LogSync::ImmediateAsync, "immediate_async"),
            (LogSync::IntervalMillis(50), "interval"),
        ] {
            let base = format!("test_data/adapter_truncated_recovery_{label}");
            let _ = std::fs::remove_dir_all(&base);
            let mut store = bounded(
                label,
                "start",
                LogStore::<TestTypeConfig>::start(base.clone(), mode.clone(), WAL_SIZE),
            )
            .await
            .unwrap();

            // A healthy append first, so the truncated one lands on top of existing state.
            bounded(
                label,
                "healthy append",
                store.blocking_append(vec![blank_ent::<TestTypeConfig>(1, 1, 1)]),
            )
            .await
            .unwrap_or_else(|err| panic!("{label}: the healthy append must succeed: {err}"));

            // A truncated batch, dispatched on the writer channel the adapter uses. The entry
            // sender is dropped without the end-of-stream marker, which is what the adapter
            // itself does when a send fails partway through a batch.
            let (ack_tx, ack_rx) = oneshot::channel();
            let (entry_tx, entry_rx) = flume::bounded(1);
            let (note_tx, note_rx) = std::sync::mpsc::channel();
            bounded(
                label,
                "dispatch the truncated append",
                store.writer.send_async(writer::Action::Append {
                    rx: entry_rx,
                    callback: Box::new(move |res| {
                        let _ = note_tx.send(res);
                    }),
                    ack: ack_tx,
                }),
            )
            .await
            .unwrap();
            for index in 2..=BATCH {
                let entry = blank_ent::<TestTypeConfig>(1, 1, index);
                let bytes = serialize(&entry).unwrap();
                bounded(
                    label,
                    "send an entry",
                    entry_tx.send_async(Some((index, bytes))),
                )
                .await
                .unwrap();
            }
            drop(entry_tx);

            let err = bounded(label, "the truncated append's acknowledgement", ack_rx)
                .await
                .unwrap_or_else(|_| panic!("{label}: the truncated append must be acknowledged"))
                .expect_err(&format!("{label}: a truncated append is never a success"));
            assert!(
                matches!(err, crate::error::Error::IncompleteAppend(_)),
                "{label}: got {err}"
            );
            let notified = note_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap_or_else(|_| panic!("{label}: exactly one completion must arrive"))
                .expect_err(&format!("{label}: the completion is a failure"));
            assert!(notified.to_string().contains("IncompleteAppend"));

            // The writer is terminal. Queued work must come back as a storage error rather than
            // panic the calling task or leave it waiting forever, which is what the `unwrap`s
            // on these channels used to do.
            let queued = bounded(
                label,
                "an append to the terminal writer",
                store.blocking_append(vec![blank_ent::<TestTypeConfig>(1, 1, BATCH + 1)]),
            )
            .await;
            assert!(
                queued.is_err(),
                "{label}: an append dispatched to a terminal writer must fail, not succeed"
            );
            let vote = bounded(
                label,
                "a vote to the terminal writer",
                store.save_vote(&Vote::new(1, 1)),
            )
            .await;
            assert!(
                vote.is_err(),
                "{label}: a vote write to a terminal writer must fail, not panic"
            );

            // The prefix actually crossed a WAL file boundary: this is the rollover half of
            // W-07's bar, pinned rather than assumed, because the boundary is crossed inside
            // the collection loop and a smaller batch would not reach it.
            let wal_files = std::fs::read_dir(&base)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().ends_with(".wal"))
                .count();
            assert!(
                wal_files > 1,
                "{label}: the truncated batch must have rolled over at least once, found \
                 {wal_files} WAL file(s)"
            );

            drop(store);

            // Explicit synchronization, not a sleep: the terminal writer releases its advisory
            // lock when `run` returns, which is *after* it has already acknowledged and
            // notified, so the acknowledgement is not a signal that the storage is free. Wait
            // for the lock itself, bounded, and fail loudly if it never clears.
            let mut released = false;
            for _ in 0..200 {
                if !crate::lockfile::LockFile::is_locked(&base).unwrap() {
                    released = true;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            assert!(
                released,
                "{label}: a terminal writer must release its advisory lock"
            );

            // Reopen and read through the adapter. The lock file is left behind by a terminal
            // writer, so this is the not-a-clean-start path, which runs the deep integrity
            // check.
            let mut reopened = bounded(
                label,
                "reopen",
                LogStore::<TestTypeConfig>::start(base, mode, WAL_SIZE),
            )
            .await
            .unwrap_or_else(|err| panic!("{label}: the store must reopen: {err}"));

            let state = bounded(label, "log state", reopened.get_log_state())
                .await
                .unwrap();
            let last = state
                .last_log_id
                .unwrap_or_else(|| panic!("{label}: the healthy entry at least must survive"))
                .index;
            assert!(
                last >= 1,
                "{label}: the prefix that was persisted must still be there"
            );
            assert!(
                last <= BATCH,
                "{label}: the store must not report entries beyond the batch it was sent, got {last}"
            );

            let entries = bounded(
                label,
                "read the prefix",
                reopened.try_get_log_entries(1..=last),
            )
            .await
            .unwrap();
            assert_eq!(
                entries.len() as u64,
                last,
                "{label}: every index up to the reported last must be readable"
            );
            for (offset, entry) in entries.iter().enumerate() {
                assert_eq!(
                    entry.get_log_id().index,
                    offset as u64 + 1,
                    "{label}: the recovered prefix must be consecutive with no hole"
                );
            }

            // Reading past the end is answered, not fatal.
            let beyond = bounded(
                label,
                "read past the end",
                reopened.try_get_log_entries(last + 1..last + 10),
            )
            .await
            .unwrap();
            assert!(beyond.is_empty(), "{label}: nothing exists past the prefix");

            bounded(label, "stop", reopened.stop()).await.unwrap();
        }

        /// Every wait in this test is bounded and names itself, so a stall is a failure that
        /// says where it stalled. The test used to hang CI for as long as the job allowed.
        async fn bounded<T>(
            label: &str,
            step: &str,
            fut: impl std::future::Future<Output = T>,
        ) -> T {
            tokio::time::timeout(std::time::Duration::from_secs(20), fut)
                .await
                .unwrap_or_else(|_| panic!("{label}: stalled at: {step}"))
        }
    }

    /// The adapter boundary, exercised through the real trait method and the real completion
    /// channel.
    ///
    /// `LogFlushed` has no public constructor in the pinned openraft, so the callback cannot be
    /// built by a test directly. `RaftLogStorageExt::blocking_append` is openraft's own public
    /// wrapper: it constructs a real `LogFlushed`, calls `RaftLogStorage::append`, and awaits the
    /// completion oneshot, mapping both a dropped sender and a notified `Err` into a
    /// `StorageError`. That makes it the smallest boundary at which hiqlite's adapter can be
    /// observed end to end without standing up a Raft node.
    ///
    /// This is the test that catches an adapter hardcoding `Ok(())`: with the writer reporting
    /// the injected failure correctly, a hardcoded success would make this append return `Ok`
    /// even though the flush failed.
    #[tokio::test(flavor = "multi_thread")]
    async fn append_adapter_forwards_a_persistence_failure_to_openraft() {
        let base = "test_data/adapter_persistence_failure";
        let mut store = start(base).await;

        // The healthy path first: the same channel must report success when nothing fails.
        store
            .blocking_append(vec![blank_ent::<TestTypeConfig>(1, 1, 1)])
            .await
            .expect("a healthy append must complete successfully");

        let _armed = fault::arm_persistence_failure(base);
        let err = store
            .blocking_append(vec![blank_ent::<TestTypeConfig>(1, 1, 2)])
            .await
            .expect_err("a persistence failure must reach openraft as an error");

        let reported = format!("{err}");
        assert!(
            reported.contains("injected persistence failure"),
            "openraft must receive the underlying cause, not a closed-channel error; got: \
            {reported}"
        );
    }

    /// The three verdicts `writer_verdict` can reach, driven directly.
    ///
    /// This is the path CI exercised and local runs did not. When the writer stops reading a
    /// batch before the adapter has finished sending it, the send fails and the real outcome is
    /// on the acknowledgement channel. Whether that happens is a race between the writer's
    /// `drop(rx)` and the adapter's next send, so it is tested here rather than left to whether
    /// a given machine loses it: `append_adapter_reports_a_rejected_append_as_an_error` passed
    /// forty times locally and failed on the first CI run.
    #[tokio::test]
    async fn a_writer_that_stopped_reading_is_reported_by_its_verdict_not_by_the_channel() {
        // The expected case: the writer refused the batch and said why.
        let (ack, ack_rx) = oneshot::channel();
        ack.send(Err(crate::error::Error::WalSizeExceeded(
            "entry is larger than the WAL file".into(),
        )))
        .unwrap();
        let err = writer_verdict::<TestTypeConfig>(ack_rx).await;
        assert!(
            format!("{err}").contains("WalSizeExceeded"),
            "the caller must be told why the writer refused the batch, got: {err}"
        );
        assert!(
            !format!("{err}").contains("closed channel"),
            "and must not be told about the channel instead, got: {err}"
        );

        // A success reported for a batch that was never finished is a broken contract, and is
        // named as one rather than returned to openraft as a successful append.
        let (ack, ack_rx) = oneshot::channel();
        ack.send(Ok(())).unwrap();
        let err = writer_verdict::<TestTypeConfig>(ack_rx).await;
        assert!(
            format!("{err}").contains("not the batch that was submitted"),
            "got: {err}"
        );

        // No verdict at all, which is what a panic in the writer looks like from here.
        let (ack, ack_rx) = oneshot::channel::<Result<(), crate::error::Error>>();
        drop(ack);
        let err = writer_verdict::<TestTypeConfig>(ack_rx).await;
        assert!(format!("{err}").contains("no longer running"), "got: {err}");
    }

    /// An append the writer rejects must not reach openraft as a successful storage call. The
    /// rejection surfaces on the acknowledgement path, which is what `append` returns.
    /// No longer feature-gated: the rejection is the default behavior now.
    #[tokio::test(flavor = "multi_thread")]
    async fn append_adapter_reports_a_rejected_append_as_an_error() {
        let base = "test_data/adapter_append_rejection";
        let mut store = start(base).await;

        let mut oversized = blank_ent::<TestTypeConfig>(1, 1, 1);
        oversized.payload = openraft::EntryPayload::Normal("x".repeat(128 * 1024));

        let err = store
            .blocking_append(vec![oversized])
            .await
            .expect_err("a rejected append must never report success");
        assert!(
            format!("{err}").contains("WalSizeExceeded"),
            "the rejection cause must survive: {err}"
        );
    }
}
