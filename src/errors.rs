use std::{net::AddrParseError, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DevhostError {
    #[error("failed to read {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse TOML in {path}: {source}")]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid config: {0}")]
    InvalidConfig(String),

    #[error("invalid listen address: {0}")]
    AddrParse(#[from] AddrParseError),

    #[error("invalid target URI: {0}")]
    Uri(#[from] hyper::http::uri::InvalidUri),

    #[error("server error: {0}")]
    Hyper(#[from] hyper::Error),

    #[error("proxy client error: {0}")]
    Client(#[from] hyper_util::client::legacy::Error),

    #[error("watcher error: {0}")]
    Notify(#[from] notify::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("command `{command}` failed with {status}: {stderr}")]
    CommandFailed {
        command: String,
        status: String,
        stderr: String,
    },
}

pub type Result<T> = std::result::Result<T, DevhostError>;
