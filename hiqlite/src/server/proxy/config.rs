use crate::{Error, Node, tls::ServerTlsConfig};
use cryptr::EncKeys;
use spow::pow::Pow;
use std::env;
use tracing::debug;

#[derive(Debug)]
pub struct Config {
    pub listen_port: u16,
    pub nodes: Vec<String>,
    pub tls_config: Option<ServerTlsConfig>,
    pub secret_api: String,
}

impl Config {
    pub fn parse(filename: String) -> Self {
        if dotenvy::from_filename("config").is_err() {
            debug!("config file './config' not found");
        }
        if dotenvy::from_filename_override(&filename).is_err() {
            debug!("config file '{}' not found", filename);
        }
        dotenvy::dotenv_override().ok();

        let listen_port = env::var("LISTEN_PORT")
            .unwrap_or_else(|_| "8200".to_string())
            .parse::<u16>()
            .expect("Cannot parse LISTEN_PORT to u16");

        EncKeys::from_env()
            .expect("ENC_KEYS not configured correctly")
            .init()
            .unwrap();

        let enc_key_active = EncKeys::get_key_active().unwrap();
        Pow::init_bytes(enc_key_active);

        Self {
            listen_port,
            nodes: Node::parse_from_env("HQL_NODES")
                .into_iter()
                .map(|n| n.addr_api)
                .collect::<Vec<_>>(),
            tls_config: ServerTlsConfig::from_env("API")
                .unwrap_or_else(|err| panic!("Invalid API TLS configuration: {err}")),
            // F-009's shape, in the proxy. Left as a panic here and recorded as such: the
            // proxy's configuration constructor is infallible and making it fallible is a
            // change to `015`'s surface, not this spec's.
            secret_api: env::var("HQL_SECRET_API").expect("HQL_SECRET_API not found"),
            // password_dashboard,
        }
    }

    pub fn is_valid(&self) -> Result<(), Error> {
        if self.nodes.is_empty() {
            return Err(Error::Config("'nodes' must not be empty".into()));
        }

        if self.secret_api.len() < 16 {
            return Err(Error::Config(
                // F-074: this named `secret_raft`, which the proxy has no concept of.
                "'secret_api' should be at least 16 characters long".into(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(nodes: Vec<String>, secret_api: &str) -> Config {
        Config {
            listen_port: 8200,
            nodes,
            tls_config: None,
            secret_api: secret_api.to_string(),
        }
    }

    /// Replaces `proxy_validation_covers_two_fields_and_names_a_third`, which pinned F-074:
    /// the message for a short secret named `secret_raft`, which the proxy has no concept of.
    ///
    /// Still characterizes what the validation does **not** cover, which is unchanged: nothing
    /// validates the port, the TLS material or node reachability.
    #[test]
    fn proxy_validation_covers_two_fields_and_names_the_right_one() {
        let ok = cfg(vec!["127.0.0.1:8200".to_string()], "0123456789abcdef");
        assert!(ok.is_valid().is_ok());

        let no_nodes = cfg(Vec::new(), "0123456789abcdef");
        let err = no_nodes.is_valid().unwrap_err().to_string();
        assert!(err.contains("'nodes' must not be empty"), "{err}");

        // exactly 16 is accepted, 15 is not
        assert!(
            cfg(vec!["127.0.0.1:8200".to_string()], "0123456789abcde")
                .is_valid()
                .is_err()
        );

        let short = cfg(vec!["127.0.0.1:8200".to_string()], "short");
        let err = short.is_valid().unwrap_err().to_string();
        assert!(
            err.contains("'secret_api'"),
            "the message must name the field the proxy actually has: {err}"
        );
        assert!(
            !err.contains("'secret_raft'"),
            "and not one it does not: {err}"
        );
    }
}
