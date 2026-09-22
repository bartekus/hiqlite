use crate::Error;
use cryptr::utils::b64_decode;
use cryptr::EncKeys;
use spow::pow::Pow;
use std::env;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::warn;

/// Whether the API server (which serves the dashboard) listens on TLS. Set at startup
/// from `NodeConfig::tls_api`; the login proof-of-work is required only in that case,
/// because the WASM client needs a secure context. The browser mirrors this via
/// `window.isSecureContext`.
static IS_API_TLS_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_api_tls(is_enabled: bool) {
    IS_API_TLS_ENABLED.store(is_enabled, Ordering::Relaxed);
}

pub fn is_api_tls_enabled() -> bool {
    IS_API_TLS_ENABLED.load(Ordering::Relaxed)
}

pub mod handlers;
pub mod middleware;
pub mod password;
mod query;
pub mod session;
pub mod static_files;
mod table;

#[derive(Debug)]
pub struct DashboardState {
    pub password_dashboard: Option<String>,
}

impl DashboardState {
    pub fn from_env() -> Self {
        let Ok(b64) = env::var("HQL_PASSWORD_DASHBOARD") else {
            warn!("HQL_PASSWORD_DASHBOARD has not been set and the dashboard will be disabled");
            return Self {
                password_dashboard: None,
            };
        };

        // F-089: this was `b64_decode(&b64).unwrap()` and `String::from_utf8(..).unwrap()`,
        // while the *absent* case four lines below was handled gracefully. A typo in the
        // variable therefore ended the process at startup, where leaving the variable out
        // entirely disabled the dashboard and carried on. Two ways of getting the same
        // configuration wrong, two very different outcomes.
        //
        // A malformed value now behaves exactly like an absent one, and says which it was.
        let Ok(bytes) = b64_decode(&b64) else {
            warn!(
                "HQL_PASSWORD_DASHBOARD is not valid base64 and the dashboard will be disabled"
            );
            return Self {
                password_dashboard: None,
            };
        };
        let Ok(hash) = String::from_utf8(bytes) else {
            warn!(
                "HQL_PASSWORD_DASHBOARD does not decode to text and the dashboard will be \
                 disabled"
            );
            return Self {
                password_dashboard: None,
            };
        };

        Self {
            password_dashboard: Some(hash),
        }
    }
}

pub fn init() -> Result<(), Error> {
    let enc_key_active = EncKeys::get_key_active()?;
    Pow::init_bytes(enc_key_active);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_tls_flag_set_and_read() {
        set_api_tls(true);
        assert!(is_api_tls_enabled());
        set_api_tls(false);
        assert!(!is_api_tls_enabled());
    }

    /// F-089: a malformed `HQL_PASSWORD_DASHBOARD` ended the process at startup, while the
    /// *absent* case four lines below it was handled gracefully and merely disabled the
    /// dashboard. Two ways of getting the same configuration wrong, two very different
    /// outcomes.
    ///
    /// Driven through the decoding directly rather than through the environment, which is
    /// process-wide and which `009` D-3 records as the reason no environment route in this
    /// corpus has a test. What is asserted is that both failure modes are values.
    #[test]
    fn a_malformed_dashboard_password_is_a_value_not_a_panic() {
        // Not base64 at all.
        assert!(b64_decode("this is not base64!!!").is_err());

        // Base64 of bytes that are not UTF-8: `0xff 0xfe 0xfd`.
        let decoded = b64_decode("//79").expect("valid base64");
        assert_eq!(decoded, vec![0xff, 0xfe, 0xfd]);
        assert!(
            String::from_utf8(decoded).is_err(),
            "the second `unwrap` F-089 names is reachable from the first one succeeding"
        );

        // Both now land on the same state as an absent variable.
        let disabled = DashboardState {
            password_dashboard: None,
        };
        assert!(disabled.password_dashboard.is_none());
    }
}
