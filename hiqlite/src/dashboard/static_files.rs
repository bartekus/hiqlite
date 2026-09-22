use axum::body::Body;
use axum::extract::Request;
use axum::http::Uri;
use axum::{
    http::{header, Response, StatusCode},
    response,
};
use rust_embed::RustEmbed;
use std::borrow::Cow;
use tracing::debug;

// cache lifetime in seconds -> 6 months
static CACHE_CTRL_VAL: &str = "max-age=15552000, public";

#[derive(RustEmbed)]
#[folder = "static"]
pub struct DashboardHtml;

pub async fn handler(uri: Uri, req: Request) -> response::Response {
    let (_, path) = uri.path().split_at(1); // split off the first `/`
    let mime = mime_guess::from_path(path);

    // if path.len() < 4 {
    //     warn!("path: {}", path);
    // }

    // skip encoding on already compressed data types
    //
    // F-085: this was `&path[path.len().saturating_sub(4)..]`, a byte slice four bytes from
    // the end, which panics when that offset is not a UTF-8 character boundary. The handler is
    // the `/dashboard` fallback and sits **outside** the `Session` extractor, so any
    // unauthenticated request for a path ending in a multi-byte character reached it. Under a
    // consumer's unwinding profile that is a dropped connection; under `panic = "abort"` it is
    // the node.
    //
    // The extension is what the check is about, so ask for the extension.
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (path, encoding) = if matches!(ext.as_str(), "png" | "ico" | "jpg" | "svg" | "jpeg") {
        (Cow::from(path), "none")
    } else {
        let accept_encoding = req
            .headers()
            .get("accept-encoding")
            .map(|h| h.to_str().unwrap_or("none"))
            .unwrap_or("none");
        if accept_encoding.contains("br") {
            (Cow::from(format!("{path}.br")), "br")
        } else if accept_encoding.contains("gzip") {
            (Cow::from(format!("{path}.gz")), "gzip")
        } else {
            (Cow::from(path), "none")
        }
    };

    let cache_ctrl = if path.starts_with("_app/") {
        CACHE_CTRL_VAL
    } else {
        "max-age=3600, public"
    };

    match DashboardHtml::get(path.as_ref()) {
        Some(content) => Response::builder()
            .header(header::CACHE_CONTROL, cache_ctrl)
            .header(header::CONTENT_TYPE, mime.first_or_octet_stream().as_ref())
            .header(header::CONTENT_ENCODING, encoding)
            .body(Body::from(content.data))
            .unwrap(),

        None => {
            debug!("Asset {path} not found");
            // for a in DashboardHtml::iter() {
            //     warn!("Available asset: {}", a);
            // }
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("not found"))
                .unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Request;

    fn request_for(path: &str) -> (Uri, Request) {
        let uri: Uri = path.parse().unwrap();
        let req = Request::builder()
            .uri(uri.clone())
            .body(Body::empty())
            .unwrap();
        (uri, req)
    }

    #[tokio::test]
    async fn a_known_asset_is_served_with_its_cache_headers() {
        let (uri, req) = request_for("/index.html");
        let resp = handler(uri, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "max-age=3600, public"
        );
    }

    #[tokio::test]
    async fn an_unknown_asset_is_a_plain_404() {
        let (uri, req) = request_for("/does-not-exist.html");
        let resp = handler(uri, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// Replaces `a_multibyte_path_panics_the_fallback`, which pinned F-085: the compression
    /// suffix check sliced the path at `len().saturating_sub(4)` without asking whether that
    /// byte index is a character boundary. `http::Uri` accepts raw UTF-8 in a path, so a
    /// request whose last four bytes split a multi-byte character panicked this handler, which
    /// is the **unauthenticated** `/dashboard` fallback: no credential is needed to reach it.
    #[tokio::test]
    async fn a_multibyte_path_is_served_or_missing_but_never_a_panic() {
        for path in [
            "/\u{20ac}abc",
            "/\u{20ac}",
            "/caf\u{e9}.png",
            "/\u{1f600}\u{1f600}",
            "/a",
            "/",
        ] {
            let (uri, req) = request_for(path);
            let resp = handler(uri, req).await;
            assert!(
                resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::OK,
                "{path} must be answered, not panicked: {}",
                resp.status()
            );
        }
    }

    /// The extension check still does what it is for: an already-compressed type is served
    /// without a content encoding.
    #[tokio::test]
    async fn an_already_compressed_type_is_not_encoded_again() {
        let (uri, req) = request_for("/favicon.ico");
        let resp = handler(uri, req).await;
        // Present or absent, what matters is that it was not served with an encoding.
        assert!(
            resp.headers().get(header::CONTENT_ENCODING).is_none(),
            "an .ico must not be content-encoded"
        );
    }
}
