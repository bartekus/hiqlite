//! One hiqlite node as its own process, for `035` section 5. Built twice from this file: against
//! hiqlite 0.14 (`old/`) and against the candidate (`new/`), so both run the same steps.
//!
//! `node <data_dir> <mode> [control_file]`. Ports come from `N1_PORT_API` and `N1_PORT_RAFT`.
//! Every line the harness reads starts with an upper-case tag.
//!
//! - `hold`: start, write `first`, print `HOLDING`, then obey the control file (`use` writes
//!   `after` and prints `USED`; `stop` stops), bounded at 120 s.
//! - `write-stop`: start, write `first`, stop.
//! - `read`: start, write `read`, count rows, stop.
//! - `killme`: start, write `first`, print `KILL_ME`, and wait to be killed, bounded at 120 s.
//! - `race`: start, hold for one second, stop.

use hiqlite::{Node, NodeConfig};
use std::borrow::Cow;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub enum Cache {
    Kv,
}

impl hiqlite::CacheVariants for Cache {
    fn hiqlite_cache_index(&self) -> usize {
        0
    }
    fn hiqlite_cache_variants() -> &'static [(usize, &'static str)] {
        &[(0, "Kv")]
    }
}

fn cfg(dir: &str) -> NodeConfig {
    let api = std::env::var("N1_PORT_API").expect("N1_PORT_API");
    let raft = std::env::var("N1_PORT_RAFT").expect("N1_PORT_RAFT");
    let mut c = NodeConfig {
        node_id: 1,
        nodes: vec![Node {
            id: 1,
            addr_raft: format!("127.0.0.1:{raft}"),
            addr_api: format!("127.0.0.1:{api}"),
        }],
        listen_addr_api: Cow::Borrowed("127.0.0.1"),
        listen_addr_raft: Cow::Borrowed("127.0.0.1"),
        data_dir: Cow::Owned(dir.to_string()),
        secret_raft: "0123456789abcdef0123".into(),
        secret_api: "0123456789abcdef0123".into(),
        cache_storage_disk: true,
        ..NodeConfig::default()
    };
    c.enc_keys.enc_key_active = "k1".into();
    c.enc_keys.enc_keys = vec![("k1".into(), vec![7u8; 32])];
    c
}

async fn use_node(client: &hiqlite::Client, tag: &str) -> bool {
    let w1 = client
        .execute(
            "CREATE TABLE IF NOT EXISTS t (k TEXT PRIMARY KEY, v TEXT)",
            vec![],
        )
        .await;
    let w2 = client
        .execute(
            "INSERT OR REPLACE INTO t VALUES ($1,'row')",
            vec![hiqlite::Param::Text(tag.to_string())],
        )
        .await;
    let w3 = client.put(Cache::Kv, tag.to_string(), &"yes".to_string(), None).await;
    let w4 = client.counter_add(Cache::Kv, "rate", 1).await;
    let rows: Vec<hiqlite::Row> = client
        .query_raw("SELECT k FROM t", vec![])
        .await
        .unwrap_or_default();
    let v: Option<String> = client.get(Cache::Kv, tag.to_string()).await.unwrap_or(None);
    let ok = w1.is_ok() && w2.is_ok() && w3.is_ok() && w4.is_ok() && v.is_some();
    println!(
        "USE tag={tag} ok={ok} create={} insert={} put={} counter_add={} sql_rows={} cache={:?}",
        w1.is_ok(),
        w2.is_ok(),
        w3.is_ok(),
        w4.is_ok(),
        rows.len(),
        v
    );
    ok
}

#[tokio::main]
async fn main() {
    let dir = std::env::args().nth(1).expect("data dir");
    let mode = std::env::args().nth(2).unwrap_or_default();
    let ctl = std::env::args().nth(3).unwrap_or_default();

    let t0 = Instant::now();
    let client = match hiqlite::start_node_with_cache::<Cache>(cfg(&dir)).await {
        Ok(c) => c,
        Err(e) => {
            println!("START_ERR ms={} err={e}", t0.elapsed().as_millis());
            std::process::exit(3);
        }
    };
    client.wait_until_healthy_db().await;
    client.wait_until_healthy_cache().await;
    println!("START_OK ms={}", t0.elapsed().as_millis());

    let mut failed = false;
    match mode.as_str() {
        "hold" => {
            failed |= !use_node(&client, "first").await;
            println!("HOLDING");
            let deadline = Instant::now() + Duration::from_secs(120);
            loop {
                if Instant::now() > deadline {
                    println!("HOLD_DEADLINE");
                    failed = true;
                    break;
                }
                if let Ok(cmd) = std::fs::read_to_string(&ctl) {
                    let _ = std::fs::remove_file(&ctl);
                    match cmd.trim() {
                        "use" => {
                            failed |= !use_node(&client, "after").await;
                            println!("USED");
                        }
                        "stop" => break,
                        _ => {}
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        "write-stop" => failed |= !use_node(&client, "first").await,
        "read" => failed |= !use_node(&client, "read").await,
        "killme" => {
            let ok = use_node(&client, "first").await;
            println!("KILL_ME ok={ok}");
            tokio::time::sleep(Duration::from_secs(120)).await;
            println!("KILL_DEADLINE");
            std::process::exit(4);
        }
        // Held for a second, so the other side of `035` X-4's race, launched within 50 ms,
        // meets a live node rather than one that already stopped.
        "race" => tokio::time::sleep(Duration::from_secs(1)).await,
        other => panic!("unknown mode {other}"),
    }

    let s = Instant::now();
    let r = client.shutdown().await;
    println!(
        "SHUTDOWN={} ms={}",
        match &r {
            Ok(()) => "Ok".to_string(),
            Err(e) => format!("Err({e})"),
        },
        s.elapsed().as_millis()
    );
    // The writer threads finish removing their lock files after `shutdown` returns in 0.14.
    tokio::time::sleep(Duration::from_millis(300)).await;
    println!("EXIT");
    if r.is_err() || failed {
        std::process::exit(5);
    }
}
