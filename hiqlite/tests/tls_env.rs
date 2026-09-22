//! Environment-route evidence for `011-transport-security-material`, as repaired by `030`.
//!
//! These assertions live in their own integration target for two reasons. The library crate is
//! `#![forbid(unsafe_code)]`, which edition 2024 makes fatal for `env::set_var`; and a
//! process-wide environment variable set inside the library's unit-test binary would race every
//! other test in it, which is the limit `009` D-3 recorded for the configuration contract. A
//! separate binary whose only tests are these removes both problems.

#![cfg(any(feature = "sqlite", feature = "cache"))]

use hiqlite::tls::ServerTlsConfig;
use std::env;

/// Clears only this variant's own three variables. `HQL_TLS_AUTO_CERTS` is shared by every
/// variant, so only the test that actually exercises it touches it; otherwise the tests in this
/// binary would race each other over it.
fn clear(variant: &str) {
    unsafe {
        env::remove_var(format!("HQL_TLS_{variant}_KEY"));
        env::remove_var(format!("HQL_TLS_{variant}_CERT"));
        env::remove_var(format!("HQL_TLS_{variant}_CA"));
        env::remove_var(format!("HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY"));
    }
}

fn clear_auto_certs() {
    unsafe { env::remove_var("HQL_TLS_AUTO_CERTS") };
}

/// Every branch of `ServerTlsConfig::from_env`, in one test.
///
/// One test on purpose. `HQL_TLS_AUTO_CERTS` is shared by every variant and the environment is
/// process-wide, so two tests that both set it race each other; the original file kept that in
/// one place for the same reason and this repair has more branches to cover, not fewer.
#[test]
fn from_env_branches() {
    // ---- nothing set at all: no TLS for this variant ----
    clear("TEST");
    clear_auto_certs();
    assert!(ServerTlsConfig::from_env("TEST").unwrap().is_none());

    // ---- key and cert together: specific certificates, verification enabled ----
    unsafe {
        env::set_var("HQL_TLS_TEST_KEY", "tls/key.pem");
        env::set_var("HQL_TLS_TEST_CERT", "tls/cert-chain.pem");
    }
    let cfg = ServerTlsConfig::from_env("TEST")
        .unwrap()
        .expect("key and cert are both set");
    assert!(matches!(cfg, ServerTlsConfig::Specific(_)));
    assert!(!cfg.danger_tls_no_verify());
    assert_eq!(cfg.ca_path(), None, "no trust anchor was given");

    // ---- the trust anchor F-043 added ----
    unsafe { env::set_var("HQL_TLS_TEST_CA", "tls/ca-chain.pem") };
    let cfg = ServerTlsConfig::from_env("TEST").unwrap().unwrap();
    assert_eq!(
        cfg.ca_path(),
        Some("tls/ca-chain.pem"),
        "verification has to be able to verify against something"
    );
    unsafe { env::remove_var("HQL_TLS_TEST_CA") };

    // ---- the override is read from this variant's own variable ----
    unsafe { env::set_var("HQL_TLS_TEST_DANGER_TLS_NO_VERIFY", "true") };
    let cfg = ServerTlsConfig::from_env("TEST").unwrap().unwrap();
    assert!(cfg.danger_tls_no_verify());

    // ---- F-042: a malformed per-variant override was a panic ----
    unsafe { env::set_var("HQL_TLS_TEST_DANGER_TLS_NO_VERIFY", "yes") };
    let err = ServerTlsConfig::from_env("TEST")
        .expect_err("a malformed override is a configuration error, not a panic");
    assert!(
        err.to_string().contains("HQL_TLS_TEST_DANGER_TLS_NO_VERIFY"),
        "got: {err}"
    );

    // ---- F-041: exactly one of the key/cert pair was a silent plaintext downgrade ----
    clear("TEST");
    unsafe { env::set_var("HQL_TLS_TEST_KEY", "tls/key.pem") };
    let err = ServerTlsConfig::from_env("TEST")
        .expect_err("a key with no certificate cannot serve TLS");
    let text = err.to_string();
    assert!(text.contains("HQL_TLS_TEST_KEY"), "got: {text}");
    assert!(text.contains("HQL_TLS_TEST_CERT"), "got: {text}");

    clear("TEST");
    unsafe { env::set_var("HQL_TLS_TEST_CERT", "tls/cert-chain.pem") };
    let err = ServerTlsConfig::from_env("TEST")
        .expect_err("a certificate with no key cannot serve TLS");
    assert!(err.to_string().contains("TLS needs both"), "got: {err}");

    // ---- auto certificates, with nothing else set ----
    clear("TEST");
    unsafe { env::set_var("HQL_TLS_AUTO_CERTS", "true") };
    let cfg = ServerTlsConfig::from_env("TEST")
        .unwrap()
        .expect("auto certificates are enabled");
    assert!(matches!(cfg, ServerTlsConfig::TlsAutoCertificates));
    assert!(
        cfg.danger_tls_no_verify(),
        "a self-signed certificate is verified by nobody, and this type says so"
    );
    assert_eq!(cfg.ca_path(), None);

    // ---- half a pair is still an error with auto-certificates on, where it used to
    // downgrade to a certificate nobody verifies ----
    unsafe { env::set_var("HQL_TLS_TEST_KEY", "tls/key.pem") };
    assert!(
        ServerTlsConfig::from_env("TEST").is_err(),
        "half a pair is a mistake whatever the auto-certificate setting says"
    );

    // ---- F-042's other half: `HQL_TLS_AUTO_CERTS` was `unwrap_or(false)`, so the same typo
    // that ended the process four lines below meant "off" here ----
    clear("TEST");
    unsafe { env::set_var("HQL_TLS_AUTO_CERTS", "not-a-bool") };
    let err = ServerTlsConfig::from_env("TEST")
        .expect_err("both booleans answer a malformed value the same way now");
    assert!(err.to_string().contains("HQL_TLS_AUTO_CERTS"), "got: {err}");

    clear("TEST");
    clear_auto_certs();
}
