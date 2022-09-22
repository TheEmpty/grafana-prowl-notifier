mod errors;
mod models;
mod subsystems;
#[cfg(test)]
mod test;

use models::{config::Config, fingerprint::Fingerprints};
use std::net::TcpListener;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

#[tokio::main]
async fn main() {
    env_logger::init();

    // Migrate data if needed
    let config = Config::load(std::env::args().nth(1));
    let _ = Fingerprints::migrate_v1(&config);

    // Build dependencies
    let (sender, reciever) = mpsc::unbounded_channel();
    let listener = TcpListener::bind(config.bind_host())
        .unwrap_or_else(|_| panic!("Faild to bind to {}", config.bind_host()));
    log::info!("Listening on {}", config.bind_host());
    let fingerprints = Fingerprints::load_or_default(&config);
    let fingerprints = Arc::new(Mutex::new(fingerprints));

    // Run tasks
    tokio::spawn(subsystems::notifications::main_loop(
        config.clone(),
        reciever,
    ));
    tokio::spawn(subsystems::realert_every::main_loop(
        config.clone(),
        sender.clone(),
        fingerprints.clone(),
    ));
    tokio::spawn(subsystems::realert_cron::main_loop(
        config.clone(),
        sender.clone(),
        fingerprints.clone(),
    ));
    subsystems::server::main_loop(listener, config, sender, fingerprints).await;
}
