mod errors;
mod models;
mod subsystems;
#[cfg(test)]
mod test;

use models::{config::Config, fingerprint::Fingerprints};
use prowl_queue::{LinearRetry, ProwlQueue, ProwlQueueOptions, RetryMethod};
use std::net::TcpListener;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

#[tokio::main]
async fn main() {
    env_logger::init();

    // Migrate data if needed
    let config = Config::load(std::env::args().nth(1));
    let _ = Fingerprints::migrate_v1(&config);

    // Build dependencies
    let listener = TcpListener::bind(config.bind_host())
        .unwrap_or_else(|_| panic!("Faild to bind to {}", config.bind_host()));
    log::info!("Listening on {}", config.bind_host());
    let fingerprints = Fingerprints::load_or_default(&config);
    let fingerprints = Arc::new(Mutex::new(fingerprints));

    let retry_secs = config.linear_retry_secs();
    let retry_secs = Duration::from_secs(*retry_secs);
    let retry_method = LinearRetry::new(retry_secs, None);
    let retry_method = RetryMethod::Linear(retry_method);
    let options = ProwlQueueOptions::new(retry_method);
    let (sender, reciever) = ProwlQueue::new(options).into_parts();

    // Run tasks
    if !*config.test_mode() {
        tokio::spawn(reciever.async_loop());
    }
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
