use std::time::Duration;

// not really dead code
// It will be used in any (real) scenario. This is only to get rid of a warning during some
// `clippy` checks.
#[allow(dead_code)]
pub fn build_http_client(tls_no_verify: bool) -> reqwest::Client {
    // The API endpoint's trust anchor is a node-level fact, set once at startup. Consulting it
    // here rather than threading it through seven signatures keeps the four call sites, all of
    // which target `addr_api`, from being able to disagree about it. A remote client has no
    // node configuration, so this is `None` there, which `030` KD-2 records.
    let ca_path = crate::tls::api_trust_anchor();
    let ca_path = ca_path.as_deref();
    #[allow(unused_mut)]
    let mut builder = reqwest::Client::builder()
        .http2_prior_knowledge()
        .tls_danger_accept_invalid_certs(tls_no_verify)
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(30));

    #[cfg(feature = "webpki-roots")]
    {
        builder = builder.tls_certs_merge(
            webpki_root_certs::TLS_SERVER_ROOT_CERTS
                .iter()
                .map(|c| reqwest::Certificate::from_der(c).unwrap()),
        );
    }

    // F-043: the REST clients had the same empty trust store as the raft ones, so verification
    // against an internally issued certificate was not expressible here either.
    if let Some(path) = ca_path {
        match crate::tls::load_pem_certs(path) {
            Ok(certs) => {
                for der in certs {
                    match reqwest::Certificate::from_der(der.as_ref()) {
                        Ok(cert) => builder = builder.add_root_certificate(cert),
                        Err(err) => {
                            tracing::error!("Ignoring a trust anchor in {path}: {err}")
                        }
                    }
                }
            }
            Err(err) => tracing::error!("Cannot read TLS trust anchors from {path}: {err}"),
        }
    }

    builder.build().unwrap()
}
