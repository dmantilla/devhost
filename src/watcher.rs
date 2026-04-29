use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{sync::RwLock, time::sleep};
use tracing::{error, info, warn};

use crate::{config::Config, errors::Result, router::RouteTable};

pub fn spawn_config_watcher(
    config_path: PathBuf,
    listen_addr: std::net::SocketAddr,
    routes: Arc<RwLock<RouteTable>>,
) -> Result<RecommendedWatcher> {
    let watch_dir = watch_dir_for_config(&config_path);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    })?;

    watcher.watch(&watch_dir, RecursiveMode::NonRecursive)?;

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Err(err) = event {
                error!(error = %err, "config watcher event failed");
                continue;
            }

            sleep(Duration::from_millis(350)).await;
            while rx.try_recv().is_ok() {}

            match Config::load(&config_path) {
                Ok(config) => {
                    if config.listen != listen_addr {
                        warn!(
                            configured = %config.listen,
                            active = %listen_addr,
                            "listen changes require restarting devhost"
                        );
                    }

                    let next_routes = RouteTable::new(&config.routes);
                    *routes.write().await = next_routes;
                    info!("reloaded devhost routes");
                }
                Err(err) => {
                    error!(error = %err, "config reload failed; keeping previous routes");
                }
            }
        }
    });

    Ok(watcher)
}

fn watch_dir_for_config(config_path: &Path) -> PathBuf {
    match config_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::watch_dir_for_config;

    #[test]
    fn relative_config_in_current_dir_watches_dot() {
        assert_eq!(
            watch_dir_for_config(Path::new("devhost.toml")),
            Path::new(".")
        );
    }

    #[test]
    fn config_with_parent_watches_parent_dir() {
        assert_eq!(
            watch_dir_for_config(Path::new("configs/devhost.toml")),
            Path::new("configs")
        );
    }
}
