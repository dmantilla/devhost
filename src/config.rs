use std::{
    collections::HashSet,
    fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
};

use hyper::Uri;
use serde::Deserialize;

use crate::errors::{DevhostError, Result};

const DEFAULT_LISTEN: &str = "127.0.0.1:80";
const DEFAULT_DNS_TLD: &str = "test";
const DEFAULT_DNS_LOOPBACK_IP: &str = "127.0.0.1";
const DEFAULT_DNSMASQ_CONFIG_PATH: &str = "/opt/homebrew/etc/dnsmasq.d/devhost.conf";
const DEFAULT_DNSMASQ_CONF_PATH: &str = "/opt/homebrew/etc/dnsmasq.conf";

#[derive(Debug, Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub routes: Vec<RouteConfig>,
    pub dns: DnsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteConfig {
    pub host: String,
    pub target: Uri,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsConfig {
    pub tld: String,
    pub loopback_ip: IpAddr,
    pub dnsmasq_config_path: PathBuf,
    pub dnsmasq_conf_path: PathBuf,
    pub resolver_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    listen: Option<String>,
    dns: Option<RawDns>,
    #[serde(default)]
    routes: Vec<RawRoute>,
}

#[derive(Debug, Deserialize)]
struct RawRoute {
    host: String,
    port: u16,
}

#[derive(Debug, Deserialize)]
struct RawDns {
    tld: Option<String>,
    loopback_ip: Option<String>,
    dnsmasq_config_path: Option<PathBuf>,
    dnsmasq_conf_path: Option<PathBuf>,
    resolver_path: Option<PathBuf>,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|source| DevhostError::ReadConfig {
            path: path.to_path_buf(),
            source,
        })?;

        Self::from_toml_str(&source).map_err(|err| match err {
            DevhostError::ParseConfig { source, .. } => DevhostError::ParseConfig {
                path: path.to_path_buf(),
                source,
            },
            other => other,
        })
    }

    pub fn from_toml_str(source: &str) -> Result<Self> {
        let raw: RawConfig =
            toml::from_str(source).map_err(|source| DevhostError::ParseConfig {
                path: "<memory>".into(),
                source,
            })?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self> {
        let listen = raw.listen.unwrap_or_else(|| DEFAULT_LISTEN.to_string());
        let listen = listen.parse::<SocketAddr>()?;
        let dns = DnsConfig::from_raw(raw.dns)?;

        let mut exact_hosts = HashSet::new();
        let mut routes = Vec::with_capacity(raw.routes.len());

        for route in raw.routes {
            let host = normalize_host(&route.host)?;
            let target = target_from_port(route.port)?;

            if !host.starts_with("*.") && !exact_hosts.insert(host.clone()) {
                return Err(DevhostError::InvalidConfig(format!(
                    "duplicate exact host `{host}`"
                )));
            }

            routes.push(RouteConfig { host, target });
        }

        Ok(Self {
            listen,
            routes,
            dns,
        })
    }
}

impl DnsConfig {
    fn from_raw(raw: Option<RawDns>) -> Result<Self> {
        let raw = raw.unwrap_or(RawDns {
            tld: None,
            loopback_ip: None,
            dnsmasq_config_path: None,
            dnsmasq_conf_path: None,
            resolver_path: None,
        });

        let tld = validate_tld(raw.tld.as_deref().unwrap_or(DEFAULT_DNS_TLD))?;
        let loopback_ip = raw
            .loopback_ip
            .unwrap_or_else(|| DEFAULT_DNS_LOOPBACK_IP.to_string())
            .parse::<IpAddr>()
            .map_err(|err| {
                DevhostError::InvalidConfig(format!("invalid dns loopback_ip: {err}"))
            })?;
        let dnsmasq_config_path = raw
            .dnsmasq_config_path
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DNSMASQ_CONFIG_PATH));
        let dnsmasq_conf_path = raw
            .dnsmasq_conf_path
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DNSMASQ_CONF_PATH));
        let resolver_path = raw
            .resolver_path
            .unwrap_or_else(|| PathBuf::from(format!("/etc/resolver/{tld}")));

        Ok(Self {
            tld,
            loopback_ip,
            dnsmasq_config_path,
            dnsmasq_conf_path,
            resolver_path,
        })
    }
}

fn normalize_host(host: &str) -> Result<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();

    if host.is_empty() {
        return Err(DevhostError::InvalidConfig(
            "route host cannot be empty".into(),
        ));
    }

    if host.contains("://") {
        return Err(DevhostError::InvalidConfig(format!(
            "route host `{host}` must not include a scheme"
        )));
    }

    if host.contains('/') || host.contains(':') || host.chars().any(char::is_whitespace) {
        return Err(DevhostError::InvalidConfig(format!(
            "route host `{host}` must be a hostname, not a URL or host:port"
        )));
    }

    if host.contains('*') {
        let wildcard_count = host.matches('*').count();
        if wildcard_count != 1 || !host.starts_with("*.") || host.len() <= 2 {
            return Err(DevhostError::InvalidConfig(format!(
                "wildcard host `{host}` must use a left-most wildcard like `*.app.test`"
            )));
        }
    }

    Ok(host)
}

fn target_from_port(port: u16) -> Result<Uri> {
    if port == 0 {
        return Err(DevhostError::InvalidConfig(format!(
            "route port `{port}` must be between 1 and 65535"
        )));
    }

    Ok(format!("http://127.0.0.1:{port}").parse::<Uri>()?)
}

fn validate_tld(tld: &str) -> Result<String> {
    let tld = tld.trim().trim_matches('.').to_ascii_lowercase();

    if tld.is_empty()
        || tld.contains('.')
        || tld.contains('*')
        || tld.contains(':')
        || tld.contains('/')
        || tld.chars().any(char::is_whitespace)
    {
        return Err(DevhostError::InvalidConfig(format!(
            "dns tld `{tld}` must be a single hostname label like `test`"
        )));
    }

    Ok(tld)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_exact_route() {
        let config = Config::from_toml_str(
            r#"
            [[routes]]
            host = "app.test"
            port = 2000
            "#,
        )
        .unwrap();

        assert_eq!(config.listen.to_string(), DEFAULT_LISTEN);
        assert_eq!(config.routes[0].host, "app.test");
        assert_eq!(
            config.routes[0].target.to_string(),
            "http://127.0.0.1:2000/"
        );
        assert_eq!(config.dns.tld, "test");
        assert_eq!(config.dns.loopback_ip.to_string(), "127.0.0.1");
    }

    #[test]
    fn accepts_valid_wildcard_route() {
        let config = Config::from_toml_str(
            r#"
            [[routes]]
            host = "*.app.test"
            port = 2000
            "#,
        )
        .unwrap();

        assert_eq!(config.routes[0].host, "*.app.test");
    }

    #[test]
    fn rejects_duplicate_exact_hosts() {
        let err = Config::from_toml_str(
            r#"
            [[routes]]
            host = "app.test"
            port = 2000

            [[routes]]
            host = "APP.test"
            port = 3000
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("duplicate exact host"));
    }

    #[test]
    fn rejects_non_leftmost_wildcard() {
        let err = Config::from_toml_str(
            r#"
            [[routes]]
            host = "api.*.test"
            port = 2000
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("left-most wildcard"));
    }

    #[test]
    fn rejects_missing_port() {
        let err = Config::from_toml_str(
            r#"
            [[routes]]
            host = "app.test"
            target = "http://127.0.0.1:2000"
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("missing field `port`"));
    }

    #[test]
    fn rejects_zero_port() {
        let err = Config::from_toml_str(
            r#"
            [[routes]]
            host = "app.test"
            port = 0
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("between 1 and 65535"));
    }

    #[test]
    fn rejects_port_over_range() {
        assert!(Config::from_toml_str(
            r#"
            [[routes]]
            host = "app.test"
            port = 70000
            "#,
        )
        .is_err());
    }

    #[test]
    fn accepts_custom_dns_config() {
        let config = Config::from_toml_str(
            r#"
            listen = "127.0.0.1:8080"

            [dns]
            tld = "localhost"
            loopback_ip = "127.0.0.1"
            dnsmasq_config_path = "/tmp/devhost.conf"
            dnsmasq_conf_path = "/tmp/dnsmasq.conf"
            resolver_path = "/tmp/resolver/localhost"
            "#,
        )
        .unwrap();

        assert_eq!(config.listen.to_string(), "127.0.0.1:8080");
        assert_eq!(config.dns.tld, "localhost");
        assert_eq!(
            config.dns.dnsmasq_config_path,
            PathBuf::from("/tmp/devhost.conf")
        );
    }

    #[test]
    fn rejects_multi_label_dns_tld() {
        let err = Config::from_toml_str(
            r#"
            [dns]
            tld = "app.test"
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("single hostname label"));
    }
}
