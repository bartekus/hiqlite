//! `037`: a node is not healthy, and its client does not serve reads, until startup recovery has
//! applied everything its local log held when it started.
//!
//! Only the public start and client API is used, so the same file runs against the published
//! build and shows the defect there (F-134): after an unclean stop the SQLite state machine is
//! rebuilt by replaying the Raft log, and the node reported healthy as soon as it was leader,
//! before the replay had applied anything. A consumer that waited for health and then read found
//! its database empty.
//!
//! The unclean stop is stood in for by a clean stop followed by recreating the unclean-stop
//! marker (`state_machine/lock`), which is exactly what a crash leaves behind for this check.
//! The replay only has to be slower than the election, which a few thousand log entries make
//! certain on every host this ran on. The cache case needs no crash at all: a disk-backed cache
//! keeps its state machine in memory and replays its log on every start.

#![cfg(all(feature = "sqlite", feature = "cache"))]

use hiqlite::{CacheVariants, Node, NodeConfig, Param};
use std::time::{Duration, Instant};

/// Enough single-row writes that replaying them takes far longer than an N=1 election.
const ROWS: i64 = 3000;
/// How long a correct start may take to become healthy.
const HEALTHY_WITHIN: Duration = Duration::from_secs(120);

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
    let dir = format!("../target/test_data/recovery_readiness/{case}");
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
        // the health endpoint's grace period answers `true` unconditionally; it is not what
        // this file is about, and it would hide the answer the node gives on its own
        health_check_delay_secs: 0,
        ..Default::default()
    };
    c.enc_keys.enc_key_active = "k1".into();
    c.enc_keys.enc_keys = vec![("k1".into(), vec![7u8; 32])];
    c
}

async fn start(dir: &str, port_api: u16) -> hiqlite::Client {
    hiqlite::start_node_with_cache::<Cache>(config(dir, port_api))
        .await
        .expect("the node starts")
}

/// `GET /health` on the node's API, answered with the status line's code.
async fn http_health(port_api: u16) -> Option<u16> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port_api))
        .await
        .ok()?;
    s.write_all(b"GET /health HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\n\r\n")
        .await
        .ok()?;
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(10), s.read_to_end(&mut buf)).await;
    let head = String::from_utf8_lossy(&buf);
    head.split_whitespace().nth(1)?.parse().ok()
}

async fn count(client: &hiqlite::Client) -> Result<i64, hiqlite::Error> {
    let mut row = client
        .query_raw_one("SELECT COUNT(*) AS c FROM rows", Vec::new())
        .await?;
    Ok(row.get::<i64>("c"))
}

/// A node restarted over the unclean-stop marker rebuilds its SQLite state machine from the log.
/// From the first moment it reports healthy, through the client and through `/health`, a read
/// must see every row it acknowledged before the stop.
#[cfg(feature = "auto-heal")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f134_unclean_stop_rebuild_is_not_healthy_before_the_replay() {
    let port = 38711;
    let dir = fresh("unclean_stop");

    let client = start(&dir, port).await;
    client.wait_until_healthy_db().await;
    client
        .execute("CREATE TABLE rows (id INTEGER PRIMARY KEY)", Vec::new())
        .await
        .unwrap();
    for i in 0..ROWS {
        client
            .execute("INSERT INTO rows (id) VALUES ($1)", vec![Param::from(i)])
            .await
            .unwrap();
    }
    assert_eq!(count(&client).await.unwrap(), ROWS);
    client.shutdown().await.unwrap();
    drop(client);

    // what a crash leaves behind for the state machine's check
    std::fs::write(format!("{dir}/state_machine/lock"), b"").unwrap();

    let client = start(&dir, port).await;

    // Poll as tightly as the API allows. The first time either the client or the endpoint
    // says healthy, the data must already be there.
    let deadline = Instant::now() + HEALTHY_WITHIN;
    loop {
        assert!(Instant::now() < deadline, "the node never became healthy");

        let client_says = client.is_healthy_db().await.is_ok();
        let endpoint_says = http_health(port).await == Some(200);
        if client_says || endpoint_says {
            let seen = count(&client).await;
            assert!(
                matches!(seen, Ok(n) if n == ROWS),
                "healthy (client: {client_says}, /health: {endpoint_says}) while the rebuilt \
                 state machine held {seen:?} of {ROWS} acknowledged rows"
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    client.shutdown().await.unwrap();
}

/// The disk-backed cache keeps its state machine in memory, so every start replays its log.
/// From the first moment the cache reports healthy, every acknowledged key must be readable.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f134_cache_replay_is_not_healthy_before_the_replay() {
    let port = 38721;
    let dir = fresh("cache_replay");

    let client = start(&dir, port).await;
    client.wait_until_healthy_cache().await;
    for i in 0..ROWS {
        client
            .put(Cache::One, format!("k{i}"), &i, None)
            .await
            .unwrap();
    }
    client.shutdown().await.unwrap();
    drop(client);

    // A clean stop: the in-memory cache state machine is rebuilt from its log on every start,
    // so no crash is needed for the replay to matter.

    let client = start(&dir, port).await;

    let deadline = Instant::now() + HEALTHY_WITHIN;
    loop {
        assert!(Instant::now() < deadline, "the cache never became healthy");

        if client.is_healthy_cache().await.is_ok() {
            let last: Result<Option<i64>, _> = client.get(Cache::One, format!("k{}", ROWS - 1)).await;
            assert!(
                matches!(last, Ok(Some(n)) if n == ROWS - 1),
                "the cache was healthy while the last acknowledged key read as {last:?}"
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    client.shutdown().await.unwrap();
}
