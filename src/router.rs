use std::collections::HashMap;

use hyper::Uri;

use crate::config::RouteConfig;

#[derive(Debug, Clone)]
pub struct RouteTable {
    exact: HashMap<String, Uri>,
    wildcards: Vec<WildcardRoute>,
}

#[derive(Debug, Clone)]
struct WildcardRoute {
    suffix: String,
    target: Uri,
}

impl RouteTable {
    pub fn new(routes: &[RouteConfig]) -> Self {
        let mut exact = HashMap::new();
        let mut wildcards = Vec::new();

        for route in routes {
            if let Some(suffix) = route.host.strip_prefix("*.") {
                wildcards.push(WildcardRoute {
                    suffix: suffix.to_string(),
                    target: route.target.clone(),
                });
            } else {
                exact.insert(route.host.clone(), route.target.clone());
            }
        }

        wildcards.sort_by(|left, right| right.suffix.len().cmp(&left.suffix.len()));

        Self { exact, wildcards }
    }

    pub fn resolve(&self, host: &str) -> Option<Uri> {
        let host = normalize_request_host(host)?;

        if let Some(target) = self.exact.get(&host) {
            return Some(target.clone());
        }

        for wildcard in &self.wildcards {
            if host != wildcard.suffix && host.ends_with(&format!(".{}", wildcard.suffix)) {
                return Some(wildcard.target.clone());
            }
        }

        None
    }

    pub fn entries(&self) -> Vec<(String, Uri)> {
        let mut entries = self
            .exact
            .iter()
            .map(|(host, target)| (host.clone(), target.clone()))
            .collect::<Vec<_>>();

        entries.extend(
            self.wildcards
                .iter()
                .map(|route| (format!("*.{}", route.suffix), route.target.clone())),
        );

        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries
    }
}

fn normalize_request_host(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }

    if let Some((without_port, port)) = host.rsplit_once(':') {
        if port.chars().all(|ch| ch.is_ascii_digit()) {
            return Some(without_port.to_string());
        }
    }

    Some(host)
}

#[cfg(test)]
mod tests {
    use hyper::Uri;

    use super::*;

    fn route(host: &str, target: &str) -> RouteConfig {
        RouteConfig {
            host: host.to_string(),
            target: target.parse::<Uri>().unwrap(),
        }
    }

    #[test]
    fn exact_match_wins_over_wildcard() {
        let table = RouteTable::new(&[
            route("app.test", "http://127.0.0.1:2000"),
            route("*.test", "http://127.0.0.1:3000"),
        ]);

        assert_eq!(
            table.resolve("app.test").unwrap().to_string(),
            "http://127.0.0.1:2000/"
        );
    }

    #[test]
    fn wildcard_matches_subdomains() {
        let table = RouteTable::new(&[route("*.app.test", "http://127.0.0.1:2000")]);

        assert_eq!(
            table.resolve("one-line.app.test").unwrap().to_string(),
            "http://127.0.0.1:2000/"
        );
        assert!(table.resolve("app.test").is_none());
    }

    #[test]
    fn longest_wildcard_suffix_wins() {
        let table = RouteTable::new(&[
            route("*.test", "http://127.0.0.1:1000"),
            route("*.app.test", "http://127.0.0.1:2000"),
        ]);

        assert_eq!(
            table.resolve("api.app.test").unwrap().to_string(),
            "http://127.0.0.1:2000/"
        );
    }

    #[test]
    fn unmatched_host_returns_none() {
        let table = RouteTable::new(&[route("app.test", "http://127.0.0.1:2000")]);

        assert!(table.resolve("api.test").is_none());
    }

    #[test]
    fn host_matching_is_case_insensitive() {
        let table = RouteTable::new(&[route("app.test", "http://127.0.0.1:2000")]);

        assert_eq!(
            table.resolve("APP.TEST:8080").unwrap().to_string(),
            "http://127.0.0.1:2000/"
        );
    }
}
