use crate::knobs::ConfigError;
use laser_sdk::prelude::{Capabilities, Laser, LaserError};
use secrecy::{ExposeSecret, SecretString};
use std::fmt;

const LOCAL_CONNECTION_STRING: &str = "iggy://iggy:laser@127.0.0.1:8090?nodelay=true";
const DEFAULT_TCP_PORT: u16 = 8090;

/// Opens connections from one resolved connection string. The string stays secret.
#[derive(Clone)]
pub struct LaserFactory {
    connection: SecretString,
}

impl LaserFactory {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            connection: SecretString::from(resolve_connection_string()?),
        })
    }

    pub fn from_connection_string(connection_string: impl Into<String>) -> Self {
        Self {
            connection: SecretString::from(connection_string.into()),
        }
    }

    pub fn target(&self) -> ConnectionTarget {
        connection_target(self.connection.expose_secret())
    }

    pub async fn connect(&self, stream: &str) -> Result<Laser, LaserError> {
        Laser::builder()
            .connection_string(self.connection.expose_secret())
            .stream(stream)
            .capabilities(Capabilities::OPEN)
            .build()
            .await
    }
}

pub struct ConnectionTarget {
    pub host: String,
    pub port: u16,
    pub tls: bool,
}

impl fmt::Display for ConnectionTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = if self.tls { "tls" } else { "tcp" };
        write!(f, "{}:{} ({scheme})", self.host, self.port)
    }
}

fn connection_target(connection_string: &str) -> ConnectionTarget {
    let host = host_of(connection_string).to_owned();
    let port = port_of(connection_string).unwrap_or(DEFAULT_TCP_PORT);
    let tls = connection_string.contains("tls=true")
        || connection_string.contains("tls_ca_file=")
        || (is_laserdata_host(&host) && std::env::var("LASER_NO_TLS").is_err());
    ConnectionTarget { host, port, tls }
}

fn resolve_connection_string() -> Result<String, ConfigError> {
    if let Ok(provided) = std::env::var("LASER_CONNECTION_STRING")
        && !provided.trim().is_empty()
    {
        return Ok(ensure_default_port(provided.trim().to_owned()));
    }
    let server = std::env::var("LASER_SERVER").unwrap_or_default();
    let server = server.trim();
    if server.is_empty() {
        return Ok(LOCAL_CONNECTION_STRING.to_owned());
    }
    let credentials = resolve_credentials()?;
    Ok(ensure_default_port(format!(
        "iggy+tcp://{credentials}{server}"
    )))
}

fn resolve_credentials() -> Result<String, ConfigError> {
    if let Ok(token) = std::env::var("LASER_TOKEN")
        && !token.is_empty()
    {
        return Ok(format!("{token}@"));
    }
    match (
        std::env::var("LASER_USERNAME"),
        std::env::var("LASER_PASSWORD"),
    ) {
        (Ok(username), Ok(password)) if !username.is_empty() && !password.is_empty() => {
            Ok(format!("{username}:{password}@"))
        }
        _ => Err(ConfigError::MissingCredentials),
    }
}

fn ensure_default_port(connection_string: String) -> String {
    let (scheme, remainder) = match connection_string.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, connection_string.as_str()),
    };
    let (authority, path_and_query) = match remainder.find(['/', '?']) {
        Some(index) => (&remainder[..index], &remainder[index..]),
        None => (remainder, ""),
    };
    let (user_info, host_and_port) = match authority.rsplit_once('@') {
        Some((user_info, host)) => (format!("{user_info}@"), host),
        None => (String::new(), authority),
    };
    if split_host_port(host_and_port).1.is_some() {
        return connection_string;
    }
    match scheme {
        Some(scheme) => {
            format!("{scheme}://{user_info}{host_and_port}:{DEFAULT_TCP_PORT}{path_and_query}")
        }
        None => format!("{user_info}{host_and_port}:{DEFAULT_TCP_PORT}{path_and_query}"),
    }
}

fn host_of(connection_string: &str) -> &str {
    split_host_port(authority_of(connection_string)).0
}

fn port_of(connection_string: &str) -> Option<u16> {
    split_host_port(authority_of(connection_string))
        .1
        .and_then(|port| port.parse().ok())
}

fn authority_of(connection_string: &str) -> &str {
    let after_scheme = connection_string
        .split_once("://")
        .map_or(connection_string, |(_, rest)| rest);
    let authority = after_scheme
        .split(['/', '?'])
        .next()
        .unwrap_or(after_scheme);
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
}

fn split_host_port(authority: &str) -> (&str, Option<&str>) {
    if let Some(bracketed) = authority.strip_prefix('[')
        && let Some(closing) = bracketed.find(']')
    {
        let host = &bracketed[..closing];
        let suffix = &bracketed[closing + 1..];
        return (host, suffix.strip_prefix(':'));
    }
    authority
        .rsplit_once(':')
        .map_or((authority, None), |(host, port)| (host, Some(port)))
}

// Mirrors `Laser::connect`'s own auto-TLS host check so the startup narration
// names the scheme the SDK is actually about to pick, without this factory
// touching the CA or the connection string itself. Trailing-dot match rejects
// look-alikes like laserdata.cloud.attacker.com.
fn is_laserdata_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "laserdata.cloud"
        || host.ends_with(".laserdata.cloud")
        || host == "laserdata.com"
        || host.ends_with(".laserdata.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_hosts_when_checked_then_should_reject_laserdata_look_alikes() {
        assert!(is_laserdata_host(
            "starter-123.us-west-1.aws.laserdata.cloud"
        ));
        assert!(!is_laserdata_host("127.0.0.1"));
        assert!(!is_laserdata_host("laserdata.cloud.attacker.com"));
    }

    #[test]
    fn given_a_laserdata_host_when_targeted_then_should_narrate_tls() {
        let target = connection_target("iggy+tcp://token@starter.laserdata.cloud:8090");
        assert!(target.tls);
        assert_eq!(target.to_string(), "starter.laserdata.cloud:8090 (tls)");
    }

    #[test]
    fn given_a_credentialed_string_when_described_then_should_expose_only_host_and_port() {
        let target = connection_target("iggy://iggy:iggy@127.0.0.1:8090");
        assert_eq!(target.to_string(), "127.0.0.1:8090 (tcp)");
    }

    // The shared connection-string vectors: the same grammar the Laser SDK
    // examples resolve, so the two helpers cannot drift apart silently.
    #[test]
    fn given_a_portless_authority_when_normalized_then_should_inject_the_default_port() {
        assert_eq!(
            ensure_default_port("iggy+tcp://u:p@host.laserdata.cloud".to_owned()),
            "iggy+tcp://u:p@host.laserdata.cloud:8090"
        );
        assert_eq!(
            ensure_default_port("iggy+tcp://u:p@host:9010".to_owned()),
            "iggy+tcp://u:p@host:9010"
        );
    }

    #[test]
    fn given_a_schemeless_authority_when_normalized_then_should_inject_the_default_port() {
        assert_eq!(
            ensure_default_port("u:p@host.laserdata.cloud".to_owned()),
            "u:p@host.laserdata.cloud:8090"
        );
        assert_eq!(
            ensure_default_port("u:p@host:9010".to_owned()),
            "u:p@host:9010"
        );
    }

    #[test]
    fn given_paths_and_queries_when_parsing_then_should_extract_host_and_port() {
        assert_eq!(host_of("iggy://u:p@host:9010?tls=true"), "host");
        assert_eq!(host_of("iggy://u:p@host/path"), "host");
        assert_eq!(port_of("iggy://u:p@host:9010?tls=true"), Some(9010));
        assert_eq!(port_of("iggy://u:p@host"), None);
    }

    #[test]
    fn given_a_bracketed_ipv6_authority_when_parsing_then_should_preserve_the_host() {
        assert_eq!(host_of("iggy://u:p@[::1]:9010"), "::1");
        assert_eq!(port_of("iggy://u:p@[::1]:9010"), Some(9010));
        assert_eq!(
            ensure_default_port("iggy://u:p@[::1]".to_owned()),
            "iggy://u:p@[::1]:8090"
        );
    }
}
