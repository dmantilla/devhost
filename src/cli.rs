use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "devhost",
    version,
    about = "Route local development hostnames to local HTTP ports"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the HTTP reverse proxy.
    Serve {
        /// Path to a devhost TOML config file.
        #[arg(short, long, default_value = "devhost.toml")]
        config: PathBuf,

        /// Install/update dnsmasq DNS config and restart dnsmasq before serving.
        #[arg(long)]
        setup_dns: bool,
    },

    /// Print the configured routes.
    Routes {
        /// Path to a devhost TOML config file.
        #[arg(short, long, default_value = "devhost.toml")]
        config: PathBuf,
    },

    /// Validate the config file.
    Validate {
        /// Path to a devhost TOML config file.
        #[arg(short, long, default_value = "devhost.toml")]
        config: PathBuf,
    },

    /// Install dnsmasq and macOS resolver config for wildcard local DNS.
    InstallDns {
        /// Path to a devhost TOML config file.
        #[arg(short, long, default_value = "devhost.toml")]
        config: PathBuf,

        /// Print the files that would be written without changing the system.
        #[arg(long)]
        dry_run: bool,
    },

    /// Remove Devhost's dnsmasq and macOS resolver config.
    UninstallDns {
        /// Path to a devhost TOML config file.
        #[arg(short, long, default_value = "devhost.toml")]
        config: PathBuf,

        /// Print the files that would be removed without changing the system.
        #[arg(long)]
        dry_run: bool,
    },

    /// Check proxy, dnsmasq, and macOS resolver setup.
    Doctor {
        /// Path to a devhost TOML config file.
        #[arg(short, long, default_value = "devhost.toml")]
        config: PathBuf,
    },
}
