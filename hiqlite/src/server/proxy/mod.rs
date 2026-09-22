use crate::server::proxy::state::AppStateProxy;
use crate::{Client, Error};
use axum::Router;
use axum::routing::get;
use config::Config;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;

pub mod config;
mod handlers;
mod notify;
mod state;
mod stream;

/// The proxy's route table, separated from its state so it can be constructed by a test.
///
/// F-067 was a route literal that panics at construction, and nothing in this repository ever
/// constructed this router: `start_proxy` needs a live upstream client, and CI never enables
/// the `server` feature (F-019). Splitting the table out is what makes the defect reachable by
/// a test that costs nothing.
fn routes() -> Router<Arc<AppStateProxy>> {
    Router::new()
        .nest(
            "/cluster",
            Router::new()
                // .route("/add_learner/{raft_type}", post(management::add_learner))
                // .route("/become_member/{raft_type}", post(management::become_member))
                // .route(
                //     "/membership/{raft_type}",
                //     get(management::get_membership).post(management::post_membership),
                // )
                // F-067: this was `"/metrics/:raft_type"`, axum 0.7's path-parameter spelling.
                // axum 0.8 panics at router construction on a `:` segment, so `hiqlite proxy`
                // has not started at all since that upgrade.
                .route("/metrics/{raft_type}", get(handlers::metrics)),
        )
        .route("/listen", get(handlers::listen))
        .route("/stream", get(handlers::stream))
        // .route("/health", get(api::health))
        .route("/ping", get(handlers::ping))
}

pub async fn start_proxy(config: Config) -> Result<(), Error> {
    if config.tls_config.is_some() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("default CryptoProvider installation to succeed");
    }

    let tls_client_config = config.tls_config.as_ref().map(|c| c.client_config());

    let client = Client::remote(
        config.nodes,
        tls_client_config.is_some(),
        config
            .tls_config
            .as_ref()
            .map(|c| c.danger_tls_no_verify())
            .unwrap_or(false),
        config.secret_api.clone(),
        false,
        None,
        None,
    )
    .await?;

    let tx_notify = notify::spawn_listener(client.clone());

    let state = Arc::new(AppStateProxy {
        client,
        secret_api: config.secret_api,
        tx_notify,
        // dashboard_password: config.password_dashboard,
    });

    let router = routes().with_state(state.clone());

    let addr_str = format!("0.0.0.0:{}", config.listen_port);
    info!("listening on {}", addr_str);
    let addr = SocketAddr::from_str(&addr_str).expect("valid socket address");

    if let Some(config) = &config.tls_config {
        let tls_config = config.server_config(&addr_str).await;

        axum_server::bind_rustls(addr, tls_config)
            .serve(router.into_make_service())
            .await
            .unwrap();
    } else {
        axum_server::bind(addr)
            .serve(router.into_make_service())
            .await
            .unwrap();
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;

    /// Replaces `the_proxy_metrics_route_is_rejected_by_the_pinned_axum`, which pinned F-067
    /// by asserting that the literal the proxy used panics. It does; the proxy no longer uses
    /// it.
    ///
    /// This constructs the **real** route table instead. Under the defect this line panics, so
    /// it is the test the proxy never had: `start_proxy` needs a live upstream client, and CI
    /// never enables the `server` feature (F-019), which is why a router that could not be
    /// built at all went unnoticed through a whole axum major version.
    #[test]
    fn the_proxy_route_table_can_be_constructed() {
        let _routes = routes();
    }

    /// The 0.7 spelling is still rejected by the pinned axum, which is what made F-067 a
    /// divergence rather than a version gap. Kept so the reason the repair was needed stays
    /// asserted rather than remembered.
    #[test]
    #[should_panic(expected = "Path segments must not start with `:`")]
    fn the_zero_seven_spelling_is_still_rejected() {
        let _router: Router<Arc<AppStateProxy>> = Router::new().nest(
            "/cluster",
            Router::new().route("/metrics/:raft_type", get(handlers::metrics)),
        );
    }

    /// The node registers the same capture with 0.8 syntax and is accepted,
    /// which is what makes F-067 a divergence rather than a version gap.
    #[test]
    fn the_same_capture_in_zero_eight_syntax_is_accepted() {
        let _router: Router<Arc<AppStateProxy>> = Router::new().nest(
            "/cluster",
            Router::new().route("/metrics/{raft_type}", get(handlers::metrics)),
        );
    }
}
