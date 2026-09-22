use crate::app_state::AppState;
use crate::network::raft_server;
use crate::network::{api, management};
use crate::{CacheVariants, Client, Error, NodeConfig, init, split_brain_check, store};
use axum::Router;
use axum::routing::{get, post};
use chrono::Utc;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task;
use tracing::{debug, info};

#[cfg(feature = "backup")]
use crate::backup;
#[cfg(feature = "dashboard")]
use crate::dashboard;


/// Whether this node keeps anything on disk that another process could corrupt.
///
/// The one configuration that does not is a cache-only node with
/// `cache_storage_disk = false` built with `in-memory-snapshots`: it never touches `data_dir`
/// at all, and taking a lock there would create a directory the node has deliberately been
/// configured not to need. Every other combination writes a database, a WAL, or a snapshot.
#[allow(unused_variables)]
fn storage_ownership_required(node_config: &NodeConfig) -> bool {
    #[cfg(feature = "sqlite")]
    {
        true
    }
    #[cfg(all(not(feature = "sqlite"), feature = "cache", feature = "in-memory-snapshots"))]
    {
        node_config.cache_storage_disk
    }
    #[cfg(all(
        not(feature = "sqlite"),
        feature = "cache",
        not(feature = "in-memory-snapshots")
    ))]
    {
        // Without `in-memory-snapshots` the cache state machine persists its snapshots, so it
        // needs `data_dir` whatever `cache_storage_disk` says.
        true
    }
    #[cfg(all(not(feature = "sqlite"), not(feature = "cache")))]
    {
        false
    }
}



/// Stop what a partial startup already started.
///
/// A constructor that returns `Err` must not leave threads running against the data directory
/// it was told to open: a WAL writer, a SQLite writer and a raft core outliving the failure
/// would keep the storage busy behind a node that reported it had not started.
#[allow(unused_variables)]
async fn teardown_partial_start(
    #[cfg(feature = "sqlite")] raft_db: crate::app_state::StateRaftDB,
    #[cfg(feature = "cache")] raft_cache: crate::app_state::StateRaftCache,
) {
    #[cfg(feature = "cache")]
    {
        let _ = raft_cache.raft.shutdown().await;
        if let Some(handle) = &raft_cache.shutdown_handle {
            let _ = handle.shutdown().await;
        }
    }
    #[cfg(feature = "sqlite")]
    teardown_raft_db(raft_db).await;
}

#[cfg(feature = "sqlite")]
async fn teardown_raft_db(raft_db: crate::app_state::StateRaftDB) {
    let _ = raft_db.raft.shutdown().await;
    let _ = raft_db.shutdown_handle.shutdown().await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    if raft_db
        .sql_writer
        .send_async(crate::store::state_machine::sqlite::writer::WriterRequest::Shutdown(tx))
        .await
        .is_ok()
    {
        let _ = rx.await;
    }
}

/// Bind a listener, or fail startup saying which endpoint and why.
pub(crate) async fn bind_listener(
    addr: &str,
    what: &'static str,
) -> Result<std::net::TcpListener, Error> {
    let parsed = SocketAddr::from_str(addr).map_err(|err| {
        Error::Startup(format!("{what} address `{addr}` is not a socket address: {err}").into())
    })?;

    let listener = TcpListener::bind(parsed).await.map_err(|err| {
        Error::Startup(format!("{what} could not bind to `{addr}`: {err}").into())
    })?;

    // Handed to the server as a std listener so the TLS and plaintext paths can both take an
    // already-bound socket. `set_nonblocking` is what `axum_server` expects of one.
    let listener = listener.into_std().map_err(|err| {
        Error::Startup(format!("{what} listener on `{addr}` is unusable: {err}").into())
    })?;
    listener.set_nonblocking(true).map_err(|err| {
        Error::Startup(format!("{what} listener on `{addr}` is unusable: {err}").into())
    })?;
    Ok(listener)
}

/// Serve a router on an already-bound listener, with graceful shutdown on both paths.
///
/// The TLS branches had no shutdown future at all (F-039), which the `TODO`s they carried
/// acknowledged: with TLS on both endpoints neither plaintext branch ran, both watch receivers
/// dropped when startup returned, and the shutdown sequence then `expect`ed a send on a channel
/// with no receivers. `axum_server::Handle` is the TLS equivalent of
/// `with_graceful_shutdown` and closes that.
///
/// Whichever branch runs, the server task's exit is observed rather than dropped: a listener
/// that stops serving takes the node out of service, unless it stopped because it was asked to.
#[allow(clippy::too_many_arguments)]
async fn serve_router(
    listener: std::net::TcpListener,
    addr: String,
    what: &'static str,
    router: Router,
    tls: Option<&crate::tls::ServerTlsConfig>,
    listen_addr: &str,
    mut rx_shutdown: tokio::sync::watch::Receiver<bool>,
    lifecycle: crate::lifecycle::NodeLifecycle,
) {
    match tls {
        Some(config) => {
            let config = config.server_config(listen_addr).await;
            let handle = axum_server::Handle::new();
            let shutdown_handle = handle.clone();
            task::spawn(async move {
                let _ = rx_shutdown.changed().await;
                shutdown_handle.graceful_shutdown(Some(Duration::from_secs(10)));
            });

            task::spawn(Box::pin(async move {
                let server = match axum_server::from_tcp_rustls(listener, config) {
                    Ok(server) => server,
                    Err(err) => {
                        lifecycle.fail(
                            crate::lifecycle::FailedComponent::Listener,
                            format!("{what} on {addr} could not start its TLS server: {err}"),
                        );
                        return;
                    }
                };
                let res = server
                    .handle(handle)
                    .serve(router.into_make_service())
                    .await;
                report_listener_exit(lifecycle, what, &addr, res);
            }));
        }
        None => {
            let listener = TcpListener::from_std(listener)
                .expect("a listener this process just bound to be convertible back");
            task::spawn(Box::pin(async move {
                let shutdown = shutdown_signal(rx_shutdown.clone());
                let res = axum::serve(listener, router.into_make_service())
                    .with_graceful_shutdown(shutdown)
                    .await;
                report_listener_exit(lifecycle, what, &addr, res);
            }));
        }
    }
}

/// A server task that returned. An error is a terminal node failure; a clean return is a
/// shutdown that has already been asked for.
fn report_listener_exit(
    lifecycle: crate::lifecycle::NodeLifecycle,
    what: &'static str,
    addr: &str,
    res: std::io::Result<()>,
) {
    match res {
        Ok(()) => debug!("{what} on {addr} stopped serving"),
        Err(err) => {
            lifecycle.fail(
                crate::lifecycle::FailedComponent::Listener,
                format!("{what} on {addr} stopped serving: {err}"),
            );
        }
    }
}

#[allow(clippy::extra_unused_type_parameters)]
pub async fn start_node_inner<C>(node_config: Box<NodeConfig>) -> Result<Client, Error>
where
    C: Debug + CacheVariants,
{
    node_config.is_valid()?;

    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        debug!("Error installing default rustls crypto provider, may have been installed already");
    }

    let tls_api_client_config = node_config.tls_api.clone().map(|c| c.client_config());
    let tls_raft = node_config.tls_raft.is_some();
    let tls_no_verify = node_config
        .tls_raft
        .as_ref()
        .map(|c| c.danger_tls_no_verify())
        .unwrap_or(false);

    let lifecycle = crate::lifecycle::NodeLifecycle::new();

    // Exclusive storage ownership, before anything touches the data directory. The restore
    // below deletes things, the state machines below that rebuild things, and a second process
    // doing either to the same directory is F-005. Held in `AppState` from here on.
    let storage_ownership = if storage_ownership_required(&node_config) {
        Some(crate::storage_lock::StorageOwnership::acquire(
            &node_config.data_dir,
        )?)
    } else {
        debug!("This node keeps no state on disk, so no storage ownership is taken");
        None
    };

    #[cfg(any(feature = "s3", feature = "dashboard"))]
    node_config.init_enc_keys();

    #[cfg(feature = "dashboard")]
    dashboard::init()?;

    #[cfg(all(feature = "backup", feature = "sqlite"))]
    let backup_applied = backup::restore_backup_start(&node_config).await?;

    let raft_config = Arc::new(node_config.raft_config.clone().validate().unwrap());

    let _do_reset_metadata = init::check_execute_reset(&node_config.data_dir).await?;
    #[cfg(feature = "sqlite")]
    let raft_db = store::start_raft_db(
        &node_config,
        raft_config.clone(),
        _do_reset_metadata,
        lifecycle.clone(),
    )
    .await?;

    #[cfg(feature = "cache")]
    let raft_cache = match store::start_raft_cache::<C>(
        &node_config,
        raft_config.clone(),
        lifecycle.clone(),
    )
    .await
    {
        Ok(cache) => cache,
        Err(err) => {
            // Partial startup: the sqlite group is already running, with a WAL writer thread
            // and a SQLite writer thread of its own. Returning without stopping them would
            // leave two threads holding this data directory open behind a constructor that
            // reported failure, and the storage ownership lock would only be released when the
            // local guard below happened to drop.
            #[cfg(feature = "sqlite")]
            teardown_raft_db(raft_db).await;
            return Err(err);
        }
    };

    let (api_addr, rpc_addr) = {
        let node = node_config
            .nodes
            .get(node_config.node_id as usize - 1)
            .expect("NodeConfig.node_id not found in NodeConfig.nodes");

        let api_addr = build_listen_addr(
            &node_config.listen_addr_api,
            &node.addr_api,
            tls_api_client_config.is_some(),
        );
        let addr_raft = build_listen_addr(&node_config.listen_addr_raft, &node.addr_raft, tls_raft);

        (api_addr, addr_raft)
    };

    // Both listeners are bound here, before the node is reported started and before anything
    // else is spawned, so an address that is already in use is a returned error and not a task
    // that panicked after the constructor said `Ok`.
    let rpc_listener = match bind_listener(&rpc_addr, "the internal RPC endpoint").await {
        Ok(listener) => listener,
        Err(err) => {
            teardown_partial_start(
                #[cfg(feature = "sqlite")]
                raft_db,
                #[cfg(feature = "cache")]
                raft_cache,
            )
            .await;
            return Err(err);
        }
    };
    let api_listener = match bind_listener(&api_addr, "the external API endpoint").await {
        Ok(listener) => listener,
        Err(err) => {
            teardown_partial_start(
                #[cfg(feature = "sqlite")]
                raft_db,
                #[cfg(feature = "cache")]
                raft_cache,
            )
            .await;
            return Err(err);
        }
    };

    #[cfg(feature = "sqlite")]
    let (tx_client_stream, rx_client_stream) = flume::bounded(1);

    let state = Arc::new(AppState {
        lifecycle: lifecycle.clone(),
        app_start: Utc::now(),
        storage_ownership: std::sync::Mutex::new(storage_ownership),
        is_shutting_down: AtomicBool::new(false),
        #[cfg(feature = "backup")]
        backups_dir: format!("{}/state_machine/backups", node_config.data_dir),
        id: node_config.node_id,
        #[cfg(feature = "cache")]
        nodes: node_config.nodes.clone(),
        addr_api: api_addr.clone(),
        #[cfg(feature = "sqlite")]
        raft_db,
        #[cfg(feature = "cache")]
        raft_cache,
        raft_lock: Arc::new(Mutex::new(())),
        secret_api: node_config.secret_api,
        secret_raft: node_config.secret_raft,
        #[cfg(feature = "dashboard")]
        dashboard: dashboard::DashboardState {
            password_dashboard: node_config.password_dashboard,
        },
        #[cfg(any(feature = "backup", feature = "dashboard"))]
        client_request_id: std::sync::atomic::AtomicUsize::new(0),
        #[cfg(any(feature = "backup", feature = "dashboard"))]
        tx_client_stream: tx_client_stream.clone(),
        health_check_delay_secs: node_config.health_check_delay_secs,
        learner_only: node_config.learner_only,
        #[cfg(feature = "s3")]
        s3_config: node_config.s3_config.clone(),
    });

    #[cfg(any(feature = "sqlite", feature = "cache"))]
    split_brain_check::spawn(
        state.clone(),
        node_config.nodes.clone(),
        node_config.tls_api.is_some(),
    )?;

    #[cfg(all(feature = "backup", feature = "sqlite"))]
    if backup_applied {
        backup::restore_backup_finish(&state).await;
    }

    let (tx_shutdown, rx_shutdown) = tokio::sync::watch::channel(false);

    let router_internal = Router::new()
        // .route("/stream", get(raft_server_split::stream))
        .route("/stream/sqlite", get(raft_server::stream_sqlite))
        .route("/stream/cache", get(raft_server::stream_cache))
        .route("/health", get(api::health))
        .route("/ping", get(api::ping))
        // .layer(compression_middleware.clone().into_inner())
        .with_state(state.clone());

    info!("rpc internal listening on {}", &rpc_addr);

    serve_router(
        rpc_listener,
        rpc_addr.clone(),
        "the internal RPC endpoint",
        router_internal,
        node_config.tls_raft.as_ref(),
        &node_config.listen_addr_raft,
        rx_shutdown.clone(),
        lifecycle.clone(),
    )
    .await;

    let default_routes = Router::new()
        .nest(
            "/cluster",
            Router::new()
                .route("/add_learner/{raft_type}", post(management::add_learner))
                .route(
                    "/become_member/{raft_type}",
                    post(management::become_member),
                )
                .route(
                    "/membership/{raft_type}",
                    get(management::get_membership)
                        .post(management::post_membership)
                        .delete(management::leave_cluster),
                )
                .route("/metrics/{raft_type}", get(management::metrics)),
        )
        .route("/listen", get(api::listen))
        .route("/stream/{raft_type}", get(api::stream))
        .route("/backup", post(api::post_create_backup))
        .route("/health", get(api::health))
        .route("/ready", get(api::ready))
        .route("/ping", get(api::ping));

    #[cfg(not(feature = "dashboard"))]
    let router_api = default_routes.with_state(state.clone());
    #[cfg(feature = "dashboard")]
    let router_api = if state.dashboard.password_dashboard.is_some() {
        default_routes
            .route("/", get(dashboard::handlers::redirect_to_index))
            .nest(
                "/dashboard",
                Router::new()
                    .route("/", get(dashboard::handlers::redirect_to_index))
                    .nest(
                        "/api",
                        Router::new()
                            .route("/metrics", get(dashboard::handlers::get_metrics))
                            .route("/pow", get(dashboard::handlers::get_pow))
                            .route("/query", post(dashboard::handlers::post_query))
                            .route(
                                "/session",
                                get(dashboard::handlers::get_session)
                                    .post(dashboard::handlers::post_session),
                            )
                            .route("/tables", get(dashboard::handlers::get_tables))
                            .route(
                                "/tables/{filter}",
                                get(dashboard::handlers::get_tables_filtered),
                            ),
                    )
                    .layer(dashboard::middleware::middleware())
                    .fallback(dashboard::static_files::handler),
            )
            .with_state(state.clone())
    } else {
        default_routes.with_state(state.clone())
    };

    #[cfg(feature = "dashboard")]
    dashboard::set_api_tls(node_config.tls_api.is_some());

    info!("api external listening on {api_addr}");

    serve_router(
        api_listener,
        api_addr.clone(),
        "the external API endpoint",
        router_api,
        node_config.tls_api.as_ref(),
        &node_config.listen_addr_api,
        rx_shutdown,
        lifecycle.clone(),
    )
    .await;

    #[cfg(feature = "sqlite")]
    let member_db = {
        let st = state.clone();
        let nodes = node_config.nodes.clone();
        let node_id = node_config.node_id;

        task::spawn(Box::pin(async move {
            init::become_cluster_member(
                st,
                &crate::app_state::RaftType::Sqlite,
                node_id,
                &nodes,
                tls_raft,
                tls_no_verify,
            )
            .await
        }))
    };

    #[cfg(feature = "cache")]
    let member_cache = {
        let st = state.clone();
        let nodes = node_config.nodes.clone();
        let node_id = node_config.node_id;

        task::spawn(Box::pin(async move {
            init::become_cluster_member(
                st,
                &crate::app_state::RaftType::Cache,
                node_id,
                &nodes,
                tls_raft,
                tls_no_verify,
            )
            .await
        }))
    };

    #[cfg(feature = "sqlite")]
    member_db.await??;
    #[cfg(feature = "cache")]
    member_cache.await??;

    let client = Client::new_local(
        state,
        tls_api_client_config,
        #[cfg(feature = "cache")]
        tls_no_verify,
        #[cfg(feature = "sqlite")]
        tx_client_stream,
        #[cfg(feature = "sqlite")]
        rx_client_stream,
        tx_shutdown,
        #[cfg(feature = "cache")]
        node_config.rate_limit_cache,
        #[cfg(feature = "sqlite")]
        node_config.rate_limit_db,
    )
    .await;

    // TODO fix that and also start backup cron jobs with no S3 config
    #[cfg(feature = "backup")]
    backup::start_cron(
        client.clone(),
        node_config.backup_config,
        #[cfg(feature = "s3")]
        node_config.s3_config,
    );

    Ok(client)
}

/// The port will be split off from the `node_addr`
fn build_listen_addr(listen_addr: &str, node_addr: &str, tls: bool) -> String {
    let port = if let Some((_, port)) = node_addr.split_once(':') {
        port
    } else if tls {
        "443"
    } else {
        "80"
    };
    format!("{listen_addr}:{port}")
}

async fn shutdown_signal(mut rx: tokio::sync::watch::Receiver<bool>) {
    let _ = rx.changed().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `build_listen_addr` takes the port from the advertised address and the
    /// host from the listen address. 010 B-2.
    #[test]
    fn listen_port_comes_from_the_advertised_address() {
        assert_eq!(
            build_listen_addr("0.0.0.0", "node1.cluster:8100", false),
            "0.0.0.0:8100"
        );
        assert_eq!(
            build_listen_addr("127.0.0.1", "node1.cluster:8200", true),
            "127.0.0.1:8200"
        );
    }

    /// With no port on the advertised address the scheme default is used, and
    /// it is the only place TLS changes the listen address. 010 B-2.
    #[test]
    fn missing_advertised_port_falls_back_to_the_scheme_default() {
        assert_eq!(build_listen_addr("0.0.0.0", "node1", true), "0.0.0.0:443");
        assert_eq!(build_listen_addr("0.0.0.0", "node1", false), "0.0.0.0:80");
    }

    /// 010 KD-1. `split_once(':')` splits a bracketed IPv6 advertised address at
    /// the first colon inside the brackets, so the "port" carries the rest of
    /// the address and the result is not a socket address at all. The caller
    /// then `expect`s it inside a detached task, so the node keeps running with
    /// no listener on that address.
    #[test]
    fn ipv6_advertised_address_produces_an_unparsable_listen_address() {
        let addr = build_listen_addr("::", "[fd00::1]:8100", false);
        assert_eq!(addr, "::::1]:8100");
        assert!(SocketAddr::from_str(&addr).is_err());

        let no_port = build_listen_addr("::", "[fd00::1]", false);
        assert_eq!(no_port, "::::1]");
        assert!(SocketAddr::from_str(&no_port).is_err());
    }

    /// F-040: the address parse and the bind were `expect`/`unwrap` inside tasks whose
    /// `JoinHandle`s were dropped, so a listener that could not start meant the node was
    /// reported started with an endpoint that did not exist, or the process ended, depending on
    /// a panic profile that for an embedded node belongs to the consumer.
    #[tokio::test]
    async fn a_listener_that_cannot_start_is_a_startup_error() {
        let err = bind_listener("this is not an address", "the test endpoint")
            .await
            .expect_err("an unparsable address is a startup error");
        let text = err.to_string();
        assert!(text.starts_with("Startup: "), "got: {text}");
        assert!(text.contains("the test endpoint"), "got: {text}");
        assert!(text.contains("not a socket address"), "got: {text}");

        // An ephemeral port this test takes first, so the second bind is guaranteed to lose.
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = held.local_addr().unwrap().to_string();

        let err = bind_listener(&addr, "the test endpoint")
            .await
            .expect_err("an address already in use is a startup error");
        let text = err.to_string();
        assert!(text.starts_with("Startup: "), "got: {text}");
        assert!(
            text.contains("could not bind"),
            "the operator is told what failed, got: {text}"
        );
        assert!(text.contains(&addr), "and on which address, got: {text}");

        drop(held);
        // And the same address binds once it is free, so the failure was the contention and not
        // the address.
        bind_listener(&addr, "the test endpoint")
            .await
            .expect("a free address binds");
    }
}
