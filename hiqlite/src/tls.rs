use crate::Error;
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertificateParams, DnType, ExtendedKeyUsagePurpose, Issuer};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use std::borrow::Cow;
use std::env;
use std::ops::{Add, Sub};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use time::OffsetDateTime;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tracing::info;

static KEY_PAIR: OnceLock<rcgen::KeyPair> = OnceLock::new();

/// The API endpoint's trust anchor for this process, set once at startup.
///
/// A node-level fact that every REST client in this process has to agree about. See
/// `http_client::build_http_client`.
static API_TRUST_ANCHOR: OnceLock<Option<String>> = OnceLock::new();

/// Whether this node's API clients skip certificate verification.
static API_NO_VERIFY: OnceLock<bool> = OnceLock::new();

/// Record the API endpoint's trust anchor and verification setting.
///
/// Idempotent; the first value wins. Both are node-level facts and are set together so no
/// caller can take one endpoint's answer to the other endpoint's question, which is F-044.
pub(crate) fn set_api_tls_facts(ca_path: Option<String>, no_verify: bool) {
    let _ = API_TRUST_ANCHOR.set(ca_path);
    let _ = API_NO_VERIFY.set(no_verify);
}

/// Whether this node's API clients skip certificate verification.
pub(crate) fn api_no_verify() -> bool {
    API_NO_VERIFY.get().copied().unwrap_or(false)
}

/// The API endpoint's trust anchor, if this process is a node and one was configured.
pub(crate) fn api_trust_anchor() -> Option<String> {
    API_TRUST_ANCHOR.get().cloned().flatten()
}

/// `TlsAutoCertificates` generates self-signed TLS certificates. Clients do not validate them,
/// because for the **raft WebSocket channel** they do not have to: that channel performs a
/// three-way handshake which authenticates both ends without the secret ever being sent over
/// the network.
///
/// **That justification does not cover the REST surface, and F-045 is that mismatch.** Every
/// `/cluster/*`, `/listen` and `/backup` call sends `secret_api` in an `X-API-SECRET` header,
/// in cleartext inside the TLS session. With a self-signed certificate nobody verifies, or with
/// any `danger_tls_no_verify`, an on-path attacker can terminate that session and read the
/// secret. The handshake protects the raft channel; it protects nothing that sends a header.
///
/// So: use `TlsAutoCertificates` for encryption between nodes you already trust the network
/// between, and `Specific` with a real certificate and a trust anchor
/// ([`ServerTlsConfigCerts::ca`]) anywhere the REST surface is reachable by anyone else.
///
/// If you want to handle certificates yourself, use the `Specific` variant.
#[derive(Debug, Clone)]
pub enum ServerTlsConfig {
    TlsAutoCertificates,
    Specific(ServerTlsConfigCerts),
}

#[derive(Debug, Clone)]
pub struct ServerTlsConfigCerts {
    pub key: Cow<'static, str>,
    pub cert: Cow<'static, str>,
    /// A PEM file of trust anchors this node's **clients** verify peers against.
    ///
    /// F-043: without this there was no way to make verification work. The client trust store
    /// is built empty unless the `webpki-roots` feature is on, and that feature carries the
    /// public web roots, which do not sign an internal cluster's certificates. So
    /// `danger_tls_no_verify = false` with specific certificates verified against **nothing**
    /// and every connection failed: the safe setting was unreachable, which is a good reason
    /// for an operator to reach for the unsafe one.
    pub ca: Option<Cow<'static, str>>,
    pub danger_tls_no_verify: bool,
}

impl ServerTlsConfigCerts {
    pub fn new<S: Into<Cow<'static, str>>>(key: S, cert: S) -> Self {
        Self {
            key: key.into(),
            cert: cert.into(),
            ca: None,
            danger_tls_no_verify: false,
        }
    }
}

impl ServerTlsConfig {
    pub fn danger_tls_no_verify(&self) -> bool {
        match self {
            ServerTlsConfig::TlsAutoCertificates => true,
            ServerTlsConfig::Specific(s) => s.danger_tls_no_verify,
        }
    }

    /// Read this endpoint's TLS material from the environment, or say what is wrong.
    ///
    /// Three defects lived in the old version of this function and they are repaired together
    /// because they are one question, "what does a half-answer mean":
    ///
    /// - **F-042.** `HQL_TLS_AUTO_CERTS` was `parse().unwrap_or(false)`, so a typo meant "off",
    ///   while `HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY` four lines below was `.expect(..)`, so
    ///   the same typo ended the process. Two booleans, four lines apart, opposite answers to
    ///   a malformed value. Both are configuration errors now, which is the policy `027` B-1
    ///   states once for the whole crate.
    /// - **F-041.** `if key.is_some() && cert.is_some()` fell straight through to `None` when
    ///   exactly one was set, and logged nothing: an operator who set the certificate and
    ///   misspelled the key variable got a **plaintext** endpoint and no indication of it. That
    ///   is now an error naming both variables.
    /// - The `.unwrap()`s that followed the `is_some()` checks are gone with the shape.
    pub fn from_env(variant: &str) -> Result<Option<Self>, crate::Error> {
        let parse_bool = |name: String, raw: String| -> Result<bool, crate::Error> {
            raw.trim().parse::<bool>().map_err(|err| {
                crate::Error::Config(
                    format!("{name} must be `true` or `false`: {err}").into(),
                )
            })
        };

        let tls_auto_certificates = match env::var("HQL_TLS_AUTO_CERTS") {
            Ok(v) => parse_bool("HQL_TLS_AUTO_CERTS".to_string(), v)?,
            Err(_) => false,
        };

        let key_var = format!("HQL_TLS_{variant}_KEY");
        let cert_var = format!("HQL_TLS_{variant}_CERT");
        let ca_var = format!("HQL_TLS_{variant}_CA");
        let no_verify_var = format!("HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY");

        let key = env::var(&key_var).ok();
        let cert = env::var(&cert_var).ok();
        let ca = env::var(&ca_var).ok();
        let no_verify = match env::var(&no_verify_var) {
            Ok(v) => parse_bool(no_verify_var, v)?,
            Err(_) => false,
        };

        match (key, cert) {
            (Some(key), Some(cert)) => Ok(Some(Self::Specific(ServerTlsConfigCerts {
                key: key.into(),
                cert: cert.into(),
                ca: ca.map(Cow::from),
                danger_tls_no_verify: no_verify,
            }))),
            // Exactly one of the pair. Refusing beats silently serving plaintext.
            (Some(_), None) => Err(crate::Error::Config(
                format!("{key_var} is set but {cert_var} is not; TLS needs both").into(),
            )),
            (None, Some(_)) => Err(crate::Error::Config(
                format!("{cert_var} is set but {key_var} is not; TLS needs both").into(),
            )),
            (None, None) => {
                if tls_auto_certificates {
                    Ok(Some(Self::TlsAutoCertificates))
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub async fn server_config(&self, url: &str) -> axum_server::tls_rustls::RustlsConfig {
        match self {
            ServerTlsConfig::TlsAutoCertificates => Self::server_config_self_signed(url).await,
            ServerTlsConfig::Specific(s) => RustlsConfig::from_pem_file(
                PathBuf::from(s.cert.as_ref()),
                PathBuf::from(s.key.as_ref()),
            )
            .await
            .expect("valid TLS certificate"),
        }
    }

    pub async fn server_config_self_signed(url: &str) -> axum_server::tls_rustls::RustlsConfig {
        let key_pair = if let Some(kp) = KEY_PAIR.get() {
            kp
        } else {
            info!("Generating new self-signed TLS certificates");
            let key_pair = tokio::task::spawn_blocking(|| rcgen::KeyPair::generate().unwrap())
                .await
                .unwrap();
            KEY_PAIR.set(key_pair).unwrap();
            KEY_PAIR.get().unwrap()
        };

        let name = if let Some((name, _)) = url.rsplit_once(":") {
            name
        } else {
            url
        };

        let mut params = CertificateParams::new(vec![name.to_string()]).unwrap();
        params.distinguished_name.push(DnType::CommonName, name);
        // params.use_authority_key_identifier_extension = true;
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ClientAuth);
        let now = OffsetDateTime::now_utc();
        params.not_before = now.sub(Duration::from_secs(60));
        // The certificate will be valid for 3 years. We don't really need to care here. It will
        // not be verified anyway. The 3-way handshake with the secrets will validate client and
        // server once the connection is established. We only want to take advantage of the
        // encryption at this point.
        let exp = now.add(Duration::from_secs(3600 * 365 * 3));
        params.not_after = exp;

        let iss = Issuer::from_params(&params, &key_pair);
        let cert = params.signed_by(&key_pair, &iss).unwrap();

        let pem_key = key_pair.serialize_pem();
        let pem_cert = cert.pem();

        RustlsConfig::from_pem(pem_cert.as_bytes().to_vec(), pem_key.as_bytes().to_vec())
            .await
            .expect("Cannot build self-signed TLS certificates")
    }

    pub fn client_config(&self) -> Arc<ClientConfig> {
        match self {
            ServerTlsConfig::TlsAutoCertificates => build_tls_config(true, None),
            ServerTlsConfig::Specific(s) => {
                build_tls_config(s.danger_tls_no_verify, s.ca.as_deref())
            }
        }
    }

    /// The trust anchors this configuration's clients verify against, if any.
    pub fn ca_path(&self) -> Option<&str> {
        match self {
            ServerTlsConfig::TlsAutoCertificates => None,
            ServerTlsConfig::Specific(s) => s.ca.as_deref(),
        }
    }
}


/// Read a PEM file of certificates.
pub(crate) fn load_pem_certs(path: &str) -> Result<Vec<CertificateDer<'static>>, Error> {
    let pem = std::fs::read(path)
        .map_err(|err| Error::Config(format!("cannot read {path}: {err}").into()))?;
    let mut reader = std::io::BufReader::new(pem.as_slice());
    rustls_pemfile_certs(&mut reader)
        .map_err(|err| Error::Config(format!("{path} is not a PEM certificate file: {err}").into()))
}

/// Minimal PEM certificate reader, so this does not add a dependency for one file format.
fn rustls_pemfile_certs(
    reader: &mut std::io::BufReader<&[u8]>,
) -> Result<Vec<CertificateDer<'static>>, String> {
    use std::io::BufRead;

    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";

    let mut out = Vec::new();
    let mut b64 = String::new();
    let mut inside = false;
    for line in reader.lines() {
        let line = line.map_err(|err| err.to_string())?;
        let line = line.trim();
        if line == BEGIN {
            inside = true;
            b64.clear();
            continue;
        }
        if line == END {
            inside = false;
            let der = base64_decode(&b64)?;
            out.push(CertificateDer::from(der));
            continue;
        }
        if inside {
            b64.push_str(line);
        }
    }
    if out.is_empty() {
        return Err("no CERTIFICATE block found".to_string());
    }
    Ok(out)
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, c) in T.iter().enumerate() {
        rev[*c as usize] = i as u8;
    }

    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = rev[c as usize];
        if v == 255 {
            return Err(format!("invalid base64 character `{}`", c as char));
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

pub fn build_tls_config(tls_no_verify: bool, ca_path: Option<&str>) -> Arc<ClientConfig> {
    #[allow(unused_mut)]
    let mut root_store = tokio_rustls::rustls::RootCertStore::empty();
    #[cfg(feature = "webpki-roots")]
    root_store.add_parsable_certificates(webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().cloned());

    // F-043: the store above is empty without `webpki-roots`, and the public web roots that
    // feature carries do not sign an internal cluster's certificates either way. This is the
    // trust anchor that makes verification possible at all.
    if let Some(path) = ca_path {
        match load_pem_certs(path) {
            Ok(certs) => {
                let (added, ignored) = root_store.add_parsable_certificates(certs);
                info!("Loaded {added} trust anchor(s) from {path}, {ignored} ignored");
            }
            Err(err) => {
                // Not a panic, and not silent. A client that cannot read its trust anchors
                // will fail every handshake, which is the fail-closed outcome; saying so here
                // is what turns that into a diagnosable one.
                tracing::error!("Cannot read TLS trust anchors from {path}: {err}");
            }
        }
    }

    let config = if tls_no_verify {
        tokio_rustls::rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoTlsVerifier {}))
            .with_no_client_auth()
    } else {
        tokio_rustls::rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    };

    Arc::new(config)
}

pub async fn into_tls_stream(
    host: &str,
    stream: TcpStream,
    config: Arc<ClientConfig>,
) -> Result<TlsStream<TcpStream>, Error> {
    let dnsname = ServerName::try_from(host.to_string()).expect("invalid host address");
    let connector = tokio_rustls::TlsConnector::from(config);
    let tls_stream = connector.connect(dnsname, stream).await?;
    Ok(tls_stream)
}

#[derive(Debug)]
struct NoTlsVerifier {}

impl ServerCertVerifier for NoTlsVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 011 B-1. `TlsAutoCertificates` disables certificate verification for
    /// whichever endpoint selects it, and `ServerTlsConfigCerts::new` starts
    /// from verification enabled.
    #[test]
    fn auto_certificates_always_disable_verification() {
        assert!(ServerTlsConfig::TlsAutoCertificates.danger_tls_no_verify());

        let specific = ServerTlsConfig::Specific(ServerTlsConfigCerts::new(
            "tls/key.pem",
            "tls/cert-chain.pem",
        ));
        assert!(!specific.danger_tls_no_verify());
    }

    /// Replaces `the_tls_material_type_has_no_field_for_a_trust_anchor`, which pinned F-043:
    /// the type carried a key, a certificate and one boolean, so the only trust roots a
    /// verifying client could ever have were the webpki bundle, added only under a feature
    /// neither consumer of this release enables, and which does not sign an internal cluster's
    /// certificates anyway. `danger_tls_no_verify = false` therefore verified against an empty
    /// store and failed every handshake: the safe setting was unreachable.
    #[test]
    fn the_tls_material_type_carries_a_trust_anchor() {
        let certs = ServerTlsConfigCerts::new("tls/key.pem", "tls/cert-chain.pem");

        assert_eq!(certs.key.as_ref(), "tls/key.pem");
        assert_eq!(certs.cert.as_ref(), "tls/cert-chain.pem");
        assert!(!certs.danger_tls_no_verify);
        assert!(
            certs.ca.is_none(),
            "the constructor does not invent a trust anchor"
        );

        let with_ca = ServerTlsConfigCerts {
            ca: Some("tls/ca-chain.pem".into()),
            ..certs.clone()
        };
        let cfg = ServerTlsConfig::Specific(with_ca);
        assert_eq!(cfg.ca_path(), Some("tls/ca-chain.pem"));
        assert!(
            !cfg.danger_tls_no_verify(),
            "supplying a trust anchor is the alternative to turning verification off"
        );

        // Auto certificates have no trust anchor to offer, and say so.
        assert_eq!(ServerTlsConfig::TlsAutoCertificates.ca_path(), None);
    }

    /// The PEM reader accepts a real chain and rejects what is not one.
    #[test]
    fn the_trust_anchor_reader_accepts_a_chain_and_rejects_anything_else() {
        let dir = std::env::temp_dir().join(format!("hiqlite-ca-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Two minimal CERTIFICATE blocks. The bodies are not valid X.509 and do not need to
        // be: this asserts the framing, and `add_parsable_certificates` is what judges the DER.
        let chain = dir.join("chain.pem");
        std::fs::write(
            &chain,
            "-----BEGIN CERTIFICATE-----\nQUJD\n-----END CERTIFICATE-----\n\
             -----BEGIN CERTIFICATE-----\nREVG\n-----END CERTIFICATE-----\n",
        )
        .unwrap();
        let certs = load_pem_certs(chain.to_str().unwrap()).expect("two blocks are two certs");
        assert_eq!(certs.len(), 2);
        assert_eq!(certs[0].as_ref(), b"ABC");
        assert_eq!(certs[1].as_ref(), b"DEF");

        let not_pem = dir.join("not.pem");
        std::fs::write(&not_pem, "this is not a certificate").unwrap();
        assert!(load_pem_certs(not_pem.to_str().unwrap()).is_err());

        assert!(
            load_pem_certs(dir.join("absent.pem").to_str().unwrap()).is_err(),
            "a missing trust anchor file is a configuration error, not an empty store"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
