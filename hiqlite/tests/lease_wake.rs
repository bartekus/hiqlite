//! `039`: a distributed lock left held by a restart does not strand the caller who asks for it
//! next.
//!
//! Only the public start and client API is used, so the same file runs against the published
//! build and shows the defect there (F-138, reported by rahi as its `045` D-22): the lock state is
//! rebuilt from the cache log on every start, so a lock whose holder never released it, or whose
//! release had not been committed when the node stopped, reads as held after the restart. A
//! caller that asks for it before that lease runs out is queued, and it must then be granted
//! within one lease window plus a bounded margin, not left waiting until its request times out.
//!
//! A clean stop stands in for the crash: what matters is that the log holds the `Lock` entry and
//! no `LockRelease` for it, which a holder that is never dropped guarantees.

#![cfg(all(feature = "cache", feature = "dlock"))]

use hiqlite::{CacheVariants, Node, NodeConfig};
use std::time::{Duration, Instant};

/// The lease `hiqlite` grants, in seconds (`LOCK_VALID_SECONDS`, not configurable).
const LEASE: Duration = Duration::from_secs(10);
/// How long past one lease a queued caller may reasonably wait to be granted.
const MARGIN: Duration = Duration::from_secs(5);

#[derive(Debug)]
enum Cache {
    One,
}

impl CacheVariants for Cache {
    fn hiqlite_cache_index(&self) -> usize {
        match self {
            Cache::One => 0,
        }
    }

    fn hiqlite_cache_variants() -> &'static [(usize, &'static str)] {
        &[(0, "One")]
    }
}

fn fresh(case: &str) -> String {
    let dir = format!("../target/test_data/lease_wake/{case}");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(dir: &str, port_api: u16) -> NodeConfig {
    let mut c = NodeConfig {
        node_id: 1,
        nodes: vec![Node {
            id: 1,
            addr_raft: format!("127.0.0.1:{}", port_api + 1),
            addr_api: format!("127.0.0.1:{port_api}"),
        }],
        data_dir: dir.to_string().into(),
        secret_raft: "SuperSecureSecret1337".to_string(),
        secret_api: "SuperSecureSecret1337".to_string(),
        cache_storage_disk: true,
        health_check_delay_secs: 0,
        ..Default::default()
    };
    c.enc_keys.enc_key_active = "k1".into();
    c.enc_keys.enc_keys = vec![("k1".into(), vec![7u8; 32])];
    c
}

async fn start(dir: &str, port_api: u16) -> hiqlite::Client {
    let client = hiqlite::start_node_with_cache::<Cache>(config(dir, port_api))
        .await
        .expect("the node starts");
    client.wait_until_healthy_cache().await;
    client
}

/// Ask for `key` and report how long the grant took, failing the test past the bound.
async fn lock_within(client: &hiqlite::Client, key: &'static str) -> (hiqlite::Lock, Duration) {
    let bound = LEASE + MARGIN;
    let asked = Instant::now();
    match tokio::time::timeout(bound, client.lock(key)).await {
        Ok(Ok(lock)) => (lock, asked.elapsed()),
        Ok(Err(err)) => panic!("lock({key}) failed after {:?}: {err}", asked.elapsed()),
        Err(_) => panic!(
            "lock({key}) was not granted within one lease plus {MARGIN:?} ({bound:?}) after a \
             restart that left it held"
        ),
    }
}

/// The holder dies with the node: its lock is replayed as held, and the first caller after the
/// restart asks well inside that replayed lease.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f138_a_lock_held_at_the_stop_is_granted_to_the_next_caller_within_one_lease() {
    let port = 38741;
    let dir = fresh("held_at_stop");

    let client = start(&dir, port).await;
    let held = client.lock("k").await.unwrap();
    // Never released: the log keeps the `Lock` entry and no `LockRelease`.
    std::mem::forget(held);
    client.shutdown().await.unwrap();
    drop(client);

    let client = start(&dir, port).await;
    let (lock, took) = lock_within(&client, "k").await;
    eprintln!("granted {took:?} after asking");
    drop(lock);

    client.shutdown().await.unwrap();
}

/// Several callers queue behind the replayed holder; every one of them is served, in turn,
/// without any of them sitting out a request timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f138_callers_queued_behind_a_replayed_lock_are_each_served() {
    let port = 38751;
    let dir = fresh("queued_behind_replay");

    let client = start(&dir, port).await;
    std::mem::forget(client.lock("k").await.unwrap());
    client.shutdown().await.unwrap();
    drop(client);

    let client = start(&dir, port).await;
    let asked = Instant::now();
    let mut waiters = Vec::new();
    for _ in 0..3 {
        let c = client.clone();
        waiters.push(tokio::spawn(async move {
            let (lock, _) = lock_within(&c, "k").await;
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop(lock);
        }));
    }
    for w in waiters {
        w.await.unwrap();
    }
    eprintln!("all three served {:?} after asking", asked.elapsed());

    client.shutdown().await.unwrap();
}
