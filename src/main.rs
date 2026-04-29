use std::sync::Arc;

use clap::Parser;
use devhost::{
    cli::{Cli, Command},
    config::Config,
    dnsmasq,
    errors::Result,
    proxy,
    router::RouteTable,
    watcher,
};
use tokio::sync::RwLock;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Command::Serve { config, setup_dns } => {
            if setup_dns {
                println!("setting up dnsmasq DNS before starting devhost...");
                dnsmasq::setup_for_serve(&config)?;
                println!("dnsmasq DNS setup complete");
            }

            let loaded = Config::load(&config)?;
            let listen = loaded.listen;
            let routes = Arc::new(RwLock::new(RouteTable::new(&loaded.routes)));
            info!(
                config = %config.display(),
                routes = loaded.routes.len(),
                "loaded devhost config"
            );
            for route in &loaded.routes {
                info!(host = %route.host, target = %route.target, "configured route");
            }
            let _watcher = watcher::spawn_config_watcher(config, listen, routes.clone())?;

            proxy::serve(listen, routes).await
        }
        Command::Routes { config } => {
            let loaded = Config::load(config)?;
            let table = RouteTable::new(&loaded.routes);

            for (host, target) in table.entries() {
                println!("{host} -> {target}");
            }

            Ok(())
        }
        Command::Validate { config } => {
            let loaded = Config::load(config)?;
            println!(
                "config valid: listening on {}, {} route(s)",
                loaded.listen,
                loaded.routes.len()
            );

            Ok(())
        }
        Command::InstallDns { config, dry_run } => {
            let loaded = Config::load(config)?;
            let plan = dnsmasq::install(&loaded, dry_run)?;

            if dry_run {
                println!("would write {}", plan.dnsmasq_config_path.display());
                print!("{}", plan.dnsmasq_config);
                println!(
                    "would ensure include in {}",
                    plan.dnsmasq_conf_path.display()
                );
                print!("{}", plan.dnsmasq_conf_include);
                println!("would write {}", plan.resolver_path.display());
                print!("{}", plan.resolver_config);
            } else {
                println!(
                    "installed dnsmasq config: {}",
                    plan.dnsmasq_config_path.display()
                );
                println!(
                    "updated dnsmasq main config: {}",
                    plan.dnsmasq_conf_path.display()
                );
                println!(
                    "installed resolver config: {}",
                    plan.resolver_path.display()
                );
            }

            if plan.clean_url_ready {
                println!("clean URLs are ready for http://<name>.{}", loaded.dns.tld);
            } else {
                println!(
                    "DNS is ready, but clean URLs need Devhost on port 80; with the current config use http://<name>.{}:{}",
                    loaded.dns.tld,
                    loaded.listen.port()
                );
            }
            println!(
                "restart dnsmasq after install, for example: sudo brew services restart dnsmasq"
            );

            Ok(())
        }
        Command::UninstallDns { config, dry_run } => {
            let loaded = Config::load(config)?;
            let touched = dnsmasq::uninstall(&loaded, dry_run)?;

            for path in touched {
                if dry_run {
                    println!("would remove Devhost DNS state from {}", path.display());
                } else {
                    println!("removed Devhost DNS state from {}", path.display());
                }
            }
            println!(
                "restart dnsmasq after uninstall, for example: sudo brew services restart dnsmasq"
            );

            Ok(())
        }
        Command::Doctor { config } => {
            let loaded = Config::load(config)?;
            let report = dnsmasq::doctor(&loaded);

            for check in report.checks {
                let status = if check.ok { "ok" } else { "fail" };
                println!("[{status}] {}: {}", check.name, check.detail);
            }

            Ok(())
        }
    }
}
