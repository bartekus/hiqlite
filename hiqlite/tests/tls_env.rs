//! Environment-route evidence for `011-transport-security-material`.
//!
//! These assertions live in their own integration target for two reasons. The
//! library crate is `#![forbid(unsafe_code)]`, which edition 2024 makes fatal
//! for `env::set_var`; and a process-wide environment variable set inside the
//! library's unit-test binary would race every other test in it, which is the
//! limit `009` D-3 recorded for the configuration contract. A separate binary
//! whose only tests are these two removes both problems.

#![cfg(any(feature = "sqlite", feature = "cache"))]

use hiqlite::tls::ServerTlsConfig;
use std::env;

/// Clears only this variant's own three variables. `HQL_TLS_AUTO_CERTS` is
/// shared by every variant, so only the test that actually exercises it touches
/// it; otherwise the two tests in this binary would race each other over it.
fn clear(variant: &str) {
    unsafe {
        env::remove_var(format!("HQL_TLS_{variant}_KEY"));
        env::remove_var(format!("HQL_TLS_{variant}_CERT"));
        env::remove_var(format!("HQL_TLS_{variant}_DANGER_TLS_NO_VERIFY"));
    }
}

fn clear_auto_certs() {
    unsafe { env::remove_var("HQL_TLS_AUTO_CERTS") };
}

/// 011 B-3 and KD-1: every branch of `ServerTlsConfig::from_env`, including the
/// two that downgrade a configured endpoint without reporting anything.
#[test]
fn from_env_branches_including_the_silent_downgrade() {
    // Nothing set at all: no TLS for this variant.
    clear("TEST");
    clear_auto_certs();
    assert!(ServerTlsConfig::from_env("TEST").is_none());

    // Key and cert together: specific certificates, verification enabled.
    unsafe {
        env::set_var("HQL_TLS_TEST_KEY", "tls/key.pem");
        env::set_var("HQL_TLS_TEST_CERT", "tls/cert-chain.pem");
    }
    let cfg = ServerTlsConfig::from_env("TEST").expect("key and cert are both set");
    assert!(matches!(cfg, ServerTlsConfig::Specific(_)));
    assert!(!cfg.danger_tls_no_verify());

    // The override is read from this variant's own variable.
    unsafe { env::set_var("HQL_TLS_TEST_DANGER_TLS_NO_VERIFY", "true") };
    let cfg = ServerTlsConfig::from_env("TEST").expect("key and cert are both set");
    assert!(cfg.danger_tls_no_verify());

    // KD-1, first half: the certificate variable is misspelled or missing and
    // auto-certificates are off. The key is ignored, nothing is reported, and
    // the endpoint runs in plaintext.
    clear("TEST");
    clear_auto_certs();
    unsafe { env::set_var("HQL_TLS_TEST_KEY", "tls/key.pem") };
    assert!(ServerTlsConfig::from_env("TEST").is_none());

    // KD-1, second half: the same mistake with auto-certificates on downgrades
    // to a self-signed certificate that no client verifies, also silently.
    unsafe { env::set_var("HQL_TLS_AUTO_CERTS", "true") };
    let cfg = ServerTlsConfig::from_env("TEST").expect("auto certificates are enabled");
    assert!(matches!(cfg, ServerTlsConfig::TlsAutoCertificates));
    assert!(cfg.danger_tls_no_verify());

    // KD-2, the contrast: an unparsable HQL_TLS_AUTO_CERTS is silently false.
    clear("TEST");
    unsafe { env::set_var("HQL_TLS_AUTO_CERTS", "not-a-bool") };
    assert!(ServerTlsConfig::from_env("TEST").is_none());

    clear("TEST");
    clear_auto_certs();
}

/// 011 KD-2: the per-variant override panics on a value that does not parse as
/// a bool, where `HQL_TLS_AUTO_CERTS` read four lines earlier does not.
#[test]
#[should_panic(expected = "Cannot parse HQL_TLS_*_DANGER_TLS_NO_VERIFY to bool")]
fn a_malformed_no_verify_override_panics() {
    clear("PANIC");
    unsafe {
        env::set_var("HQL_TLS_PANIC_KEY", "tls/key.pem");
        env::set_var("HQL_TLS_PANIC_CERT", "tls/cert-chain.pem");
        env::set_var("HQL_TLS_PANIC_DANGER_TLS_NO_VERIFY", "yes");
    }
    let _ = ServerTlsConfig::from_env("PANIC");
}
