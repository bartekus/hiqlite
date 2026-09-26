use crate::Error;
use crate::app_state::RaftType;
use crate::helpers::serialize;
use crate::server::proxy::state::AppStateProxy;
use crate::server::proxy::stream;
use crate::store::state_machine::memory::notify_handler::NotifyRequest;
use axum::Json;
use axum::extract::Path;
use axum::http::header::ACCEPT;
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response, sse};
use fastwebsockets::upgrade;
use futures_util::Stream;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::Arc;
use tracing::error;

pub type AppStateExt = axum::extract::State<Arc<AppStateProxy>>;

static HEADER_NAME_SECRET: &str = "X-API-SECRET";

pub async fn ping() {}

pub async fn listen(
    state: AppStateExt,
    headers: HeaderMap,
) -> Result<sse::Sse<impl Stream<Item = Result<sse::Event, Error>>>, Error> {
    validate_secret(&state, &headers)?;

    let (tx, rx) = flume::unbounded();
    let (ack, ack_rx) = tokio::sync::oneshot::channel();
    state
        .tx_notify
        .send_async(NotifyRequest::Listen((tx, ack)))
        .await?;

    // F-051, on the proxy's copy of this endpoint. The response is not sent until the
    // subscription exists, for the same reason and with the same consequence as
    // `network::api::listen`: a client that sees the stream open is a client the handler will
    // send to. The proxy forwards the same contract it is proxying.
    ack_rx
        .await
        .map_err(|_| Error::Error("the notification handler is not running".into()))?;

    Ok(sse::Sse::new(rx.into_stream()).keep_alive(sse::KeepAlive::default()))
}

pub async fn stream(
    state: AppStateExt,
    ws: upgrade::IncomingUpgrade,
) -> Result<impl IntoResponse, Error> {
    let (response, socket) = ws.upgrade()?;

    tokio::task::spawn(async move {
        if let Err(err) = stream::handle_socket(state, socket).await {
            // if let Err(err) = handle_socket_sequential(state, socket).await {
            error!("Error in websocket connection: {}", err);
        }
    });

    Ok(response)
}

pub(crate) async fn metrics(
    state: AppStateExt,
    headers: HeaderMap,
    Path(raft_type): Path<RaftType>,
) -> Result<Response, Error> {
    validate_secret(&state, &headers)?;

    let metrics = match raft_type {
        #[cfg(feature = "sqlite")]
        RaftType::Sqlite => state.client.metrics_db().await?,
        #[cfg(feature = "cache")]
        RaftType::Cache => state.client.metrics_cache().await?,
        RaftType::Unknown => panic!("neither `sqlite` nor `cache` feature enabled"),
    };

    fmt_ok(headers, &metrics)
}

#[inline(always)]
fn fmt_ok<S: Debug + Serialize>(headers: HeaderMap, payload: S) -> Result<Response, Error> {
    if let Some(accept) = headers.get(ACCEPT)
        && accept == HeaderValue::from_static("application/json")
    {
        return Ok(Json(payload).into_response());
    }
    Ok(serialize(&payload)?.into_response())
}

#[inline(always)]
fn validate_secret(state: &AppStateExt, headers: &HeaderMap) -> Result<(), Error> {
    match headers.get(HEADER_NAME_SECRET) {
        None => Err(Error::Token("API Secret missing".into())),
        Some(secret) => {
            // F-068: this was a plain `!=` over the bytes, which returns as soon as they
            // differ. The node's own `validate_secret` already uses a constant-time compare
            // (`network/mod.rs`); the proxy validates the same secret and did not.
            if !constant_time_eq::constant_time_eq(state.secret_api.as_bytes(), secret.as_bytes()) {
                Err(Error::Token("Invalid API Secret".into()))
            } else {
                Ok(())
            }
        }
    }
}
