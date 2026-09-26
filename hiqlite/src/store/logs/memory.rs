use crate::NodeId;
use crate::store::StorageResult;
use crate::store::state_machine::memory::TypeConfigKV;
use openraft::OptionalSend;
use openraft::RaftLogReader;
use openraft::StorageError;
use openraft::StorageIOError;
use openraft::Vote;
use openraft::storage::LogFlushed;
use openraft::storage::LogState;
use openraft::storage::RaftLogStorage;
use openraft::{CommittedLeaderId, Entry};
use openraft::{LeaderId, LogId};
use std::collections::{BTreeMap, Bound, VecDeque};
use std::fmt::Debug;
use std::ops::{Deref, RangeBounds};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{Mutex, RwLock, oneshot};
use tokio::time::Instant;
use tokio::{fs, task};
use tracing::info;

type Logs = Arc<RwLock<VecDeque<Entry<TypeConfigKV>>>>;

#[derive(Debug, Clone)]
struct LogData {
    last_purged: Option<LogId<u64>>,
    vote: Option<Vote<NodeId>>,
}

#[derive(Debug, Clone)]
pub struct LogStoreMemory {
    logs: Logs,
    data: Arc<Mutex<LogData>>,
}

impl LogStoreMemory {
    pub fn new() -> Self {
        // TODO we could initialize with the correct amount of when to take snapshots and purge logs
        let logs = Arc::new(RwLock::new(VecDeque::with_capacity(1000)));
        let data = LogData {
            last_purged: None,
            vote: None,
        };

        Self {
            logs,
            data: Arc::new(Mutex::new(data)),
        }
    }
}

impl RaftLogReader<TypeConfigKV> for LogStoreMemory {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> StorageResult<Vec<Entry<TypeConfigKV>>> {
        let start = match range.start_bound() {
            Bound::Included(i) => *i,
            // A start bound one past `u64::MAX` selects nothing rather than wrapping.
            Bound::Excluded(i) => match i.checked_add(1) {
                Some(s) => s,
                None => return Ok(Vec::default()),
            },
            Bound::Unbounded => 0,
        };
        // An exclusive end bound of zero names the empty range. It used to compute `*i - 1`,
        // which underflowed: a debug build panicked and a release build wrapped to `u64::MAX`
        // and then panicked below. `[0, 0)` is a legal request and its answer is no entries.
        let end = match range.end_bound() {
            Bound::Included(i) => *i,
            Bound::Excluded(i) => match i.checked_sub(1) {
                Some(e) => e,
                None => return Ok(Vec::default()),
            },
            Bound::Unbounded => panic!("open end log entries get"),
        };
        if end < start {
            return Ok(Vec::default());
        }

        let logs = self.logs.read().await;

        // "Entry that is not found is allowed" (openraft 0.9.24, storage/mod.rs:162-167). An
        // empty store therefore answers every range with no entries; it used to `expect` on
        // the front and panic.
        let (Some(front), Some(back)) = (logs.front(), logs.back()) else {
            return Ok(Vec::default());
        };
        let first_log_id = front.log_id.index;
        let last_log_id = back.log_id.index;

        // Clamp to what the deque actually holds. A request that reaches below the purge
        // frontier or above the last append is answered with the intersection, not a panic.
        let start = start.max(first_log_id);
        let end = end.min(last_log_id);
        if end < start {
            return Ok(Vec::default());
        }

        let range_start = (start - first_log_id) as usize;
        let range_end = (end - first_log_id) as usize;

        let mut res = Vec::with_capacity((end - start + 1) as usize);
        for entry in logs.range(range_start..=range_end) {
            res.push((*entry).clone());
        }

        debug_assert!(
            res.first().unwrap().log_id.index == start && res.last().unwrap().log_id.index == end,
            "the clamped range must select exactly [{start}, {end}]"
        );

        Ok(res)
    }
}

impl RaftLogStorage<TypeConfigKV> for LogStoreMemory {
    type LogReader = Self;

    async fn get_log_state(&mut self) -> StorageResult<LogState<TypeConfigKV>> {
        // Lock order is `data` then `logs`, everywhere in this file that needs both.
        let lock = self.data.lock().await;
        let last_purged_log_id = lock.last_purged;

        // `back()`, not `get(len())`: the latter is always one past the end, so this reported
        // `None` for every non-empty deque. When there are no entries the trait requires the
        // purge frontier rather than `None` (openraft 0.9.24, storage/mod.rs:146-148), which
        // is why this and the `last_purged` bookkeeping in `purge` had to be repaired
        // together.
        let last_log_id = {
            let logs = self.logs.read().await;
            logs.back().map(|entry| entry.log_id)
        }
        .or(last_purged_log_id);

        Ok(LogState {
            last_purged_log_id,
            last_log_id,
        })
    }

    // async fn save_committed(
    //     &mut self,
    //     committed: Option<LogId<NodeId>>,
    // ) -> Result<(), StorageError<NodeId>> {
    //     let mut lock = self.data.lock().await;
    //     lock.commited = committed;
    //     Ok(())
    // }
    //
    // async fn read_committed(&mut self) -> Result<Option<LogId<NodeId>>, StorageError<NodeId>> {
    //     Ok(self.data.lock().await.commited)
    // }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        let mut lock = self.data.lock().await;
        lock.vote = Some(*vote);
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        Ok(self.data.lock().await.vote)
    }

    #[tracing::instrument(level = "trace", skip_all)]
    async fn append<I>(
        &mut self,
        entries: I,
        callback: LogFlushed<TypeConfigKV>,
    ) -> StorageResult<()>
    where
        I: IntoIterator<Item = Entry<TypeConfigKV>> + Send,
        I::IntoIter: Send,
    {
        {
            let mut logs = self.logs.write().await;
            for entry in entries {
                logs.push_back(entry);
            }
        }

        callback.log_io_completed(Ok(()));

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn truncate(&mut self, log_id: LogId<NodeId>) -> StorageResult<()> {
        let mut logs = self.logs.write().await;

        if logs.is_empty() {
            info!("Logs are empty - nothing to truncate");
            return Ok(());
        }

        let first_offset = logs.front().unwrap().log_id.index;
        if log_id.index <= first_offset {
            // Everything this store holds is at or above the truncation point. Guarded rather
            // than asserted, because the subtraction below underflows in a release build,
            // where the old `debug_assert!` did nothing.
            logs.clear();
            return Ok(());
        }
        let truncate_from = (log_id.index - first_offset) as usize;
        // Compares like with like. The old assertion compared the deque offset to an absolute
        // log index, which are equal only while the front sits at index 0, and it `unwrap`ed a
        // `get` that is legitimately `None` for a truncate at one past the last entry.
        debug_assert!(
            logs.get(truncate_from)
                .is_none_or(|e| e.log_id.index == log_id.index),
            "offset {truncate_from} must hold log index {}",
            log_id.index
        );

        // `VecDeque::truncate(n)` keeps `n` elements, so the entry at `log_id.index` is
        // removed: that is the inclusive semantics `truncate` requires. Do not copy this
        // shape into `purge`, where `drain(..n)` removes `n` and means the opposite.
        logs.truncate(truncate_from);

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn purge(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        // Both locks, in the file's one order: `data` then `logs`. They are held together for
        // the whole update so no reader can observe a purged deque beside a stale frontier, or
        // an advanced frontier beside entries that are still there.
        let mut data = self.data.lock().await;
        let mut logs = self.logs.write().await;

        // The frontier advances even when there is nothing to remove: a purge naming an index
        // this store never held is still a statement about what has been purged. It only ever
        // moves forward.
        let advance_frontier = |data: &mut LogData| {
            if data
                .last_purged
                .is_none_or(|current| current.index < log_id.index)
            {
                data.last_purged = Some(log_id);
            }
        };

        if logs.is_empty() {
            info!("Logs are empty - nothing to purge");
            advance_frontier(&mut data);
            return Ok(());
        }

        let first_offset = logs.front().unwrap().log_id.index;
        if log_id.index < first_offset {
            // Already purged past this point. Nothing to remove and nothing to advance.
            return Ok(());
        }

        // `drain(..=n)` removes the element at offset `n`, which is the entry at
        // `log_id.index`. The old `drain(..n)` was exclusive, so the entry the purge named
        // survived it: openraft 0.9.24 requires "Purge logs upto `log_id`, inclusive"
        // (storage/v2.rs:138). Clamped, because a purge naming an index beyond the last entry
        // must empty the deque rather than panic on an out-of-range bound in a release build,
        // where the old `debug_assert!` did nothing.
        let purge_until = ((log_id.index - first_offset) as usize).min(logs.len() - 1);
        logs.drain(..=purge_until);

        advance_frontier(&mut data);

        Ok(())
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openraft::EntryPayload;
    use openraft::storage::RaftLogStorage;

    fn entry(index: u64) -> Entry<TypeConfigKV> {
        Entry {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), index),
            payload: EntryPayload::Blank,
        }
    }

    fn log_id(index: u64) -> LogId<NodeId> {
        LogId::new(CommittedLeaderId::new(1, 1), index)
    }

    async fn store_with(indexes: impl IntoIterator<Item = u64>) -> LogStoreMemory {
        let store = LogStoreMemory::new();
        {
            let mut logs = store.logs.write().await;
            for i in indexes {
                logs.push_back(entry(i));
            }
        }
        store
    }

    async fn indexes(store: &LogStoreMemory) -> Vec<u64> {
        store
            .logs
            .read()
            .await
            .iter()
            .map(|e| e.log_id.index)
            .collect()
    }

    /// Replaces `get_log_state_reports_no_last_log_id_even_after_append`, which pinned the
    /// off-by-one at `logs.get(logs.len())` as expected behavior (`007` KD-1, F-021). The
    /// expectation is replaced rather than extended, so the corpus does not assert both.
    #[tokio::test]
    async fn get_log_state_reports_the_last_stored_entry() {
        let mut store = store_with([1, 2]).await;

        let state = store.get_log_state().await.unwrap();
        assert_eq!(
            state.last_log_id.map(|id| id.index),
            Some(2),
            "the last present entry, not the slot one past the end"
        );
        assert!(state.last_purged_log_id.is_none());
    }

    /// The trait requires `last_log_id` to equal `last_purged_log_id` when the store holds no
    /// entries, rather than `None` (openraft 0.9.24, `storage/mod.rs:146-148`). This is the
    /// divergence `007` records without an identifier of its own: it only becomes observable
    /// once `purge` assigns the frontier, which is why the two repairs had to land together.
    #[tokio::test]
    async fn get_log_state_falls_back_to_the_purge_frontier_when_empty() {
        let mut store = LogStoreMemory::new();

        let state = store.get_log_state().await.unwrap();
        assert!(state.last_log_id.is_none(), "a fresh store has no frontier");

        let mut store = store_with([1, 2, 3]).await;
        store.purge(log_id(3)).await.unwrap();

        assert!(indexes(&store).await.is_empty(), "purge emptied the deque");
        let state = store.get_log_state().await.unwrap();
        assert_eq!(state.last_purged_log_id.map(|id| id.index), Some(3));
        assert_eq!(
            state.last_log_id.map(|id| id.index),
            Some(3),
            "with no entries, last_log_id is the purge frontier"
        );
    }

    /// Replaces `purge_removes_entries_below_the_given_index`, whose doc comment stated the
    /// exclusive rule as if it were the contract and whose assertion pinned it (`007` KD-5,
    /// F-029). openraft 0.9.24 requires "Purge logs upto `log_id`, inclusive"
    /// (`storage/v2.rs:138`), so the entry the purge names must not survive it.
    #[tokio::test]
    async fn purge_removes_the_entry_it_names_and_records_the_frontier() {
        let mut store = store_with(1..=5).await;

        store.purge(log_id(3)).await.unwrap();

        assert_eq!(
            indexes(&store).await,
            vec![4, 5],
            "index 3 was named by the purge and must be gone"
        );
        let state = store.get_log_state().await.unwrap();
        assert_eq!(
            state.last_purged_log_id.map(|id| id.index),
            Some(3),
            "`007` KD-4: the frontier was never assigned before this repair"
        );
        assert_eq!(state.last_log_id.map(|id| id.index), Some(5));
    }

    /// The frontier only moves forward, and a purge that names nothing the store holds still
    /// states what has been purged.
    #[tokio::test]
    async fn purge_advances_the_frontier_monotonically_and_on_an_empty_store() {
        let mut store = LogStoreMemory::new();
        store.purge(log_id(7)).await.unwrap();
        assert_eq!(
            store
                .get_log_state()
                .await
                .unwrap()
                .last_purged_log_id
                .map(|id| id.index),
            Some(7),
            "an empty store still records the frontier it was told about"
        );

        store.purge(log_id(3)).await.unwrap();
        assert_eq!(
            store
                .get_log_state()
                .await
                .unwrap()
                .last_purged_log_id
                .map(|id| id.index),
            Some(7),
            "a lower purge index never walks the frontier backwards"
        );
    }

    /// In a release build the old `debug_assert!(logs.len() >= purge_until)` did nothing, and
    /// `drain(..purge_until)` then panicked on an out-of-range bound.
    #[tokio::test]
    async fn purge_beyond_the_last_entry_empties_the_store_without_panicking() {
        let mut store = store_with(1..=3).await;

        store.purge(log_id(9)).await.unwrap();

        assert!(indexes(&store).await.is_empty());
        assert_eq!(
            store
                .get_log_state()
                .await
                .unwrap()
                .last_purged_log_id
                .map(|id| id.index),
            Some(9)
        );
    }

    /// `007` KD-2: the assertion compared a deque offset to an absolute log index, which are
    /// equal only while the front sits at index 0. A purge advances the front, so this
    /// sequence fired the assertion in a debug build. Test binaries are debug builds, so this
    /// test is the assertion's witness.
    #[tokio::test]
    async fn truncate_after_a_purge_advanced_the_front_does_not_fire_the_assertion() {
        let mut store = store_with(1..=5).await;
        store.purge(log_id(2)).await.unwrap();
        assert_eq!(indexes(&store).await, vec![3, 4, 5]);

        store.truncate(log_id(4)).await.unwrap();

        assert_eq!(
            indexes(&store).await,
            vec![3],
            "truncate is inclusive: index 4 and everything above it is removed"
        );
    }

    /// The second half of KD-2: `logs.get(truncate_from).unwrap()` panicked for a truncate at
    /// exactly one past the last entry, which is a legal no-op call.
    #[tokio::test]
    async fn truncate_one_past_the_end_is_a_no_op() {
        let mut store = store_with(1..=3).await;

        store.truncate(log_id(4)).await.unwrap();

        assert_eq!(indexes(&store).await, vec![1, 2, 3]);
    }

    /// `007` KD-3 / F-023: `Bound::Excluded(0)` computed `0 - 1`. A debug build panicked; a
    /// release build wrapped to `u64::MAX` and then panicked below. `[0, 0)` is legal and
    /// selects nothing.
    #[tokio::test]
    async fn an_exclusive_end_bound_of_zero_returns_no_entries() {
        let mut store = store_with(1..=3).await;

        let res = store.try_get_log_entries(0..0).await.unwrap();

        assert!(res.is_empty());
    }

    /// `007` KD-6 / F-047: the `expect` on `logs.front()` panicked for any non-zero end bound
    /// against an empty deque. openraft 0.9.24 states that "Entry that is not found is
    /// allowed" (`storage/mod.rs:162-167`).
    #[tokio::test]
    async fn reading_an_empty_store_returns_no_entries() {
        let mut store = LogStoreMemory::new();

        let res = store.try_get_log_entries(1..5).await.unwrap();

        assert!(res.is_empty());
    }

    /// A range that only partly overlaps what the deque holds is answered with the
    /// intersection. The old code asserted the range was fully held and, in a release build,
    /// indexed past the end.
    #[tokio::test]
    async fn a_range_is_clamped_to_what_the_store_actually_holds() {
        let mut store = store_with(3..=5).await;

        let below: Vec<u64> = store
            .try_get_log_entries(1..5)
            .await
            .unwrap()
            .iter()
            .map(|e| e.log_id.index)
            .collect();
        assert_eq!(below, vec![3, 4], "the start is clamped up to the front");

        let above: Vec<u64> = store
            .try_get_log_entries(4..99)
            .await
            .unwrap()
            .iter()
            .map(|e| e.log_id.index)
            .collect();
        assert_eq!(above, vec![4, 5], "the end is clamped down to the back");

        assert!(
            store.try_get_log_entries(10..20).await.unwrap().is_empty(),
            "a range entirely above what is held selects nothing"
        );
        assert!(
            store.try_get_log_entries(0..3).await.unwrap().is_empty(),
            "a range entirely below the front selects nothing"
        );
    }

    /// The deque and the purge frontier are two pieces of one answer. `purge` holds both locks
    /// together so no concurrent `get_log_state` can see a purged deque beside a stale
    /// frontier, which would report a `last_log_id` lower than what has already been removed.
    ///
    /// This is an interleaving regression, not a timing test: the readers are spawned against
    /// a shared store and every observation they make is asserted, so a torn intermediate
    /// state fails the test whenever it is observed rather than depending on catching it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_concurrent_reader_never_sees_a_purged_deque_beside_a_stale_frontier() {
        let store = store_with(1..=200).await;

        let mut readers = Vec::new();
        for _ in 0..4 {
            let mut reader = store.clone();
            readers.push(task::spawn(async move {
                for _ in 0..500 {
                    let state = reader.get_log_state().await.unwrap();
                    let front = reader.logs.read().await.front().map(|e| e.log_id.index);

                    if let (Some(purged), Some(front)) =
                        (state.last_purged_log_id.map(|id| id.index), front)
                    {
                        assert!(
                            front > purged,
                            "a reader saw front {front} at or below the purge frontier {purged}"
                        );
                    }
                    assert!(
                        state.last_log_id.is_some(),
                        "once anything has been purged or stored, there is always a frontier"
                    );
                    task::yield_now().await;
                }
            }));
        }

        let mut purger = store.clone();
        for i in 1..=200 {
            purger.purge(log_id(i)).await.unwrap();
            task::yield_now().await;
        }

        for reader in readers {
            reader.await.unwrap();
        }

        let mut store = store;
        let state = store.get_log_state().await.unwrap();
        assert!(indexes(&store).await.is_empty());
        assert_eq!(state.last_purged_log_id.map(|id| id.index), Some(200));
        assert_eq!(state.last_log_id.map(|id| id.index), Some(200));
    }
}
