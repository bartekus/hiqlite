//! One hiqlite node, started by the N=3 harness as its own OS process.
//!
//! Usage: `n3-node --launch <path to launch.json>`.
//!
//! The node reports through files and a control socket (see `n3-proto`): a status file with
//! both raft groups' metrics, rewritten at a fixed interval; a shutdown record written after
//! `Client::shutdown()` returns; and line-delimited JSON requests on `127.0.0.1`. On `SIGTERM`
//! it calls `Client::shutdown()` once and exits with a code that names the result:
//!
//! | exit | meaning |
//! |---|---|
//! | 0 | `shutdown()` returned `Ok(())` |
//! | 3 | `shutdown()` returned `Err(Error::Timeout)` |
//! | 4 | `shutdown()` returned another error |
//! | 5 | `SIGTERM` arrived before `start_node` returned |
//! | 2 | the launch file or the configuration was refused |
//! | 1 | `start_node` returned an error |

#[cfg(all(feature = "rahi", feature = "rauthy"))]
compile_error!("enable exactly one of the `rahi` and `rauthy` features");
#[cfg(not(any(feature = "rahi", feature = "rauthy")))]
compile_error!("enable exactly one of the `rahi` and `rauthy` features");

use hiqlite::{CacheVariants, Client, Error, LogSync, Node, NodeConfig, Param};
use n3_proto::{
    CtlRequest, CtlResponse, Group, GroupStatus, KV_TABLE, Launch, LogIdView, NodeStatus, Phase,
    ShutdownRecord, ShutdownResult,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tracing::{error, info, warn};

const FEATURE_SET: &str = if cfg!(feature = "rahi") {
    "rahi"
} else {
    "rauthy"
};

/// Every control operation is bounded inside the node too, so a stuck write answers with an
/// error instead of holding the harness's connection until the harness's own bound.
const CTL_OP_BOUND: Duration = Duration::from_secs(20);

#[derive(Debug)]
enum Cache {
    Smoke,
}

impl CacheVariants for Cache {
    fn hiqlite_cache_index(&self) -> usize {
        match self {
            Cache::Smoke => 0,
        }
    }

    fn hiqlite_cache_variants() -> &'static [(usize, &'static str)] {
        &[(0, "smoke")]
    }
}

fn main() {
    let code = match real_main() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("N3_NODE_ERROR {err}");
            2
        }
    };
    std::process::exit(code);
}

fn real_main() -> Result<i32, String> {
    let mut args = std::env::args().skip(1);
    let launch_path = match (args.next().as_deref(), args.next()) {
        (Some("--launch"), Some(path)) => path,
        _ => return Err("usage: n3-node --launch <launch.json>".into()),
    };
    let launch: Launch = serde_json::from_slice(
        &std::fs::read(&launch_path).map_err(|e| format!("reading {launch_path}: {e}"))?,
    )
    .map_err(|e| format!("parsing {launch_path}: {e}"))?;

    if launch.feature_set != FEATURE_SET {
        return Err(format!(
            "this binary was built with the `{FEATURE_SET}` feature set, the launch file asks \
             for `{}`",
            launch.feature_set
        ));
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(false)
        .init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("building the runtime: {e}"))?;
    let code = rt.block_on(run(launch));
    // Exit here, without dropping the runtime: dropping it waits for every blocking task, and
    // after an unconfirmed (`Err(Timeout)`) shutdown the sequence may still hold one. The
    // process ending is what the exit code and the shutdown record then describe.
    std::process::exit(code);
}

fn node_config(launch: &Launch) -> Result<NodeConfig, String> {
    let wal_sync = LogSync::try_from(launch.log_sync.as_str())
        .map_err(|e| format!("log_sync `{}`: {e}", launch.log_sync))?;
    let key = decode_hex(&launch.enc_key_hex)?;
    if key.len() != 32 {
        return Err(format!("enc_key_hex must be 32 bytes, got {}", key.len()));
    }

    Ok(NodeConfig {
        node_id: launch.node_id,
        nodes: launch
            .nodes
            .iter()
            .map(|p| Node {
                id: p.id,
                addr_raft: p.addr_raft.clone(),
                addr_api: p.addr_api.clone(),
            })
            .collect(),
        listen_addr_api: launch.listen_addr.clone().into(),
        listen_addr_raft: launch.listen_addr.clone().into(),
        data_dir: launch.data_dir.clone().into(),
        wal_sync,
        cache_storage_disk: launch.cache_storage_disk,
        secret_raft: launch.secret_raft.clone(),
        secret_api: launch.secret_api.clone(),
        enc_keys: cryptr::EncKeys {
            enc_key_active: "n3".to_string(),
            enc_keys: vec![("n3".to_string(), key)],
        },
        ..Default::default()
    })
}

struct Shared {
    phase: AtomicU8,
    seq: AtomicU64,
}

fn phase_from(v: u8) -> Phase {
    match v {
        0 => Phase::Starting,
        1 => Phase::Running,
        _ => Phase::ShuttingDown,
    }
}

async fn run(launch: Launch) -> i32 {
    let config = match node_config(&launch) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("N3_NODE_ERROR {err}");
            return 2;
        }
    };
    let launch = Arc::new(launch);
    let shared = Arc::new(Shared {
        phase: AtomicU8::new(0),
        seq: AtomicU64::new(0),
    });

    // Installed before `start_node`, so a SIGTERM during startup is observed by this process
    // and not by the default disposition, and is reported as what it was.
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("N3_NODE_ERROR installing the SIGTERM handler: {err}");
            return 2;
        }
    };

    write_status(&launch, &shared, None).await;
    info!(
        "n3-node {} of cluster {} starting, feature set {FEATURE_SET}, log_sync {}",
        launch.node_id, launch.cluster, launch.log_sync
    );

    let started = tokio::select! {
        res = hiqlite::start_node_with_cache::<Cache>(config) => res,
        _ = sigterm.recv() => {
            let record = ShutdownRecord {
                result: ShutdownResult::Error,
                detail: Some("SIGTERM arrived before start_node returned; there was no \
                              client to shut down".into()),
                node_measured_ms: 0,
            };
            write_json_atomic(&launch.shutdown_path, &record);
            println!("N3_SHUTDOWN {}", serde_json::to_string(&record).unwrap_or_default());
            return 5;
        }
    };
    let client = match started {
        Ok(c) => c,
        Err(err) => {
            error!("start_node failed: {err}");
            eprintln!("N3_NODE_ERROR start_node: {err}");
            return 1;
        }
    };
    shared.phase.store(1, Ordering::SeqCst);
    info!("start_node returned");

    let status_task = {
        let (launch, shared, client) = (launch.clone(), shared.clone(), client.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(launch.status_interval_ms));
            loop {
                tick.tick().await;
                write_status(&launch, &shared, Some(&client)).await;
            }
        })
    };

    let ctl_task = match TcpListener::bind(&launch.ctl_addr).await {
        Ok(listener) => {
            let client = client.clone();
            tokio::spawn(async move { ctl_serve(listener, client).await })
        }
        Err(err) => {
            eprintln!(
                "N3_NODE_ERROR binding the control socket {}: {err}",
                launch.ctl_addr
            );
            return 2;
        }
    };
    write_status(&launch, &shared, Some(&client)).await;

    sigterm.recv().await;
    let t0 = Instant::now();
    shared.phase.store(2, Ordering::SeqCst);
    info!("SIGTERM received, calling Client::shutdown()");
    write_status(&launch, &shared, Some(&client)).await;
    ctl_task.abort();

    let res = client.shutdown().await;
    let elapsed = t0.elapsed();
    let (result, detail, code) = match res {
        Ok(()) => (ShutdownResult::Ok, None, 0),
        Err(Error::Timeout(msg)) => (ShutdownResult::Timeout, Some(msg), 3),
        Err(err) => (ShutdownResult::Error, Some(err.to_string()), 4),
    };
    status_task.abort();
    // The last status, so the file names the phase the node ended in.
    write_status(&launch, &shared, Some(&client)).await;
    let record = ShutdownRecord {
        result,
        detail,
        node_measured_ms: elapsed.as_millis() as u64,
    };
    write_json_atomic(&launch.shutdown_path, &record);
    println!(
        "N3_SHUTDOWN {}",
        serde_json::to_string(&record).unwrap_or_default()
    );
    info!("shutdown returned {result:?} after {elapsed:?}");
    code
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// openraft's metrics types are not re-exported by hiqlite, so they are read field by field
/// without being named.
macro_rules! group_status {
    ($m:expr) => {{
        let m = $m;
        let membership = m.membership_config.membership();
        GroupStatus {
            server_state: format!("{:?}", m.state),
            current_term: m.current_term,
            current_leader: m.current_leader,
            last_log_index: m.last_log_index,
            last_applied: m.last_applied.as_ref().map(|l| LogIdView {
                leader: l.leader_id.to_string(),
                index: l.index,
            }),
            membership_log_id: m.membership_config.log_id().as_ref().map(|l| LogIdView {
                leader: l.leader_id.to_string(),
                index: l.index,
            }),
            voters: membership.voter_ids().collect(),
            learners: membership.learner_ids().collect(),
        }
    }};
}

async fn write_status(launch: &Launch, shared: &Shared, client: Option<&Client>) {
    let (db, cache, node_failure) = match client {
        Some(c) => (
            c.metrics_db().await.ok().map(|m| group_status!(m)),
            c.metrics_cache().await.ok().map(|m| group_status!(m)),
            c.node_failure().map(|f| format!("{f:?}")),
        ),
        None => (None, None, None),
    };
    let status = NodeStatus {
        seq: shared.seq.fetch_add(1, Ordering::SeqCst),
        pid: std::process::id(),
        unix_ms: now_ms(),
        cluster: launch.cluster.clone(),
        node_id: launch.node_id,
        feature_set: FEATURE_SET.to_string(),
        phase: phase_from(shared.phase.load(Ordering::SeqCst)),
        db,
        cache,
        node_failure,
    };
    write_json_atomic(&launch.status_path, &status);
}

/// Write to a temporary name in the same directory, then rename, so a reader never sees a
/// partial file.
fn write_json_atomic<T: serde::Serialize>(path: &str, value: &T) {
    let tmp = format!("{path}.tmp");
    let bytes = match serde_json::to_vec_pretty(value) {
        Ok(b) => b,
        Err(err) => {
            warn!("serializing {path}: {err}");
            return;
        }
    };
    if let Err(err) = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)) {
        warn!("writing {path}: {err}");
    }
}

async fn ctl_serve(listener: TcpListener, client: Client) {
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(err) => {
                warn!("control accept: {err}");
                continue;
            }
        };
        let client = client.clone();
        tokio::spawn(async move {
            let (rd, mut wr) = stream.into_split();
            let mut lines = BufReader::new(rd).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let resp = match serde_json::from_str::<CtlRequest>(&line) {
                    Ok(req) => {
                        match tokio::time::timeout(CTL_OP_BOUND, ctl_handle(&client, req)).await {
                            Ok(resp) => resp,
                            Err(_) => CtlResponse::err(format!(
                                "the operation did not finish within {CTL_OP_BOUND:?}"
                            )),
                        }
                    }
                    Err(err) => CtlResponse::err(format!("bad request: {err}")),
                };
                let mut out = serde_json::to_string(&resp).unwrap_or_default();
                out.push('\n');
                if wr.write_all(out.as_bytes()).await.is_err() {
                    break;
                }
            }
        });
    }
}

async fn ctl_handle(client: &Client, req: CtlRequest) -> CtlResponse {
    match req {
        CtlRequest::Schema => {
            let sql = format!(
                "CREATE TABLE IF NOT EXISTS {KV_TABLE} (k TEXT PRIMARY KEY NOT NULL, v TEXT NOT NULL)"
            );
            match client.execute(sql, vec![]).await {
                Ok(_) => CtlResponse::ok(None),
                Err(err) => CtlResponse::err(err.to_string()),
            }
        }
        CtlRequest::Write {
            group: Group::Db,
            key,
            value,
        } => {
            let sql = format!(
                "INSERT INTO {KV_TABLE} (k, v) VALUES ($1, $2) \
                 ON CONFLICT(k) DO UPDATE SET v = excluded.v"
            );
            match client
                .execute(sql, vec![Param::from(key), Param::from(value)])
                .await
            {
                Ok(_) => CtlResponse::ok(None),
                Err(err) => CtlResponse::err(err.to_string()),
            }
        }
        CtlRequest::Write {
            group: Group::Cache,
            key,
            value,
        } => match client
            .put_bytes(Cache::Smoke, key, value.into_bytes(), None)
            .await
        {
            Ok(()) => CtlResponse::ok(None),
            Err(err) => CtlResponse::err(err.to_string()),
        },
        CtlRequest::Read {
            group: Group::Db,
            key,
        } => {
            let sql = format!("SELECT v FROM {KV_TABLE} WHERE k = $1");
            match client.query_consistent(sql, vec![Param::from(key)]).await {
                Ok(mut rows) => match rows.first_mut() {
                    Some(row) => CtlResponse::ok(Some(row.get::<String>("v"))),
                    None => CtlResponse::ok(None),
                },
                Err(err) => CtlResponse::err(err.to_string()),
            }
        }
        CtlRequest::Read {
            group: Group::Cache,
            key,
        } => match client.get_bytes(Cache::Smoke, key).await {
            Ok(v) => CtlResponse::ok(v.map(|b| String::from_utf8_lossy(&b).into_owned())),
            Err(err) => CtlResponse::err(err.to_string()),
        },
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd-length hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("hex: {e}")))
        .collect()
}
