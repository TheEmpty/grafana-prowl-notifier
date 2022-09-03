mod config;
mod errors;
mod fingerprint;
mod grafana;

use crate::{
    config::Config,
    errors::{AddNotificationError, RequestError},
    grafana::{Alert, Message},
};
use fingerprint::Fingerprints;
use prowl::Notification;
use std::{
    io::{Read, Write},
    net::TcpListener,
};
use tokio::{
    sync::mpsc,
    time::{sleep, Duration},
};

#[tokio::main]
async fn main() {
    env_logger::init();

    // Migrate data if needed
    let config = Config::load();
    let _ = Fingerprints::migrate_v1(&config);

    // Build dependencies
    let (sender, reciever) = mpsc::unbounded_channel();
    let listener = TcpListener::bind(config.bind_host())
        .unwrap_or_else(|_| panic!("Faild to bind to {}", config.bind_host()));
    log::info!("Listening on {}", config.bind_host());

    // Run tasks
    tokio::spawn(process_notifications(
        *config.wait_secs_between_notifications(),
        *config.linear_retry_secs(),
        reciever,
    ));
    http_loop(listener, config, sender).await;
}

async fn process_notifications(
    wait_time: u64,
    retry_time: u64,
    mut reciever: mpsc::UnboundedReceiver<Notification>,
) {
    log::debug!("Notifications channel processor started.");
    while let Some(notification) = reciever.recv().await {
        'notification: loop {
            log::trace!("Processing {:?}", notification);
            match notification.add().await {
                // only move to next notification if we processed this one,
                Ok(_) => {
                    sleep(Duration::from_secs(wait_time)).await;
                    break 'notification;
                }
                Err(e) => {
                    log::error!("Failed to send notification due to {:?}.", e);
                    log::debug!("Waiting {retry_time}s to retry sending notifications");
                    sleep(Duration::from_secs(retry_time)).await;
                }
            }
        }
    }
    log::warn!("Notification channel has been closed.");
}

async fn http_loop(
    listener: TcpListener,
    config: Config,
    sender: mpsc::UnboundedSender<Notification>,
) {
    let mut fingerprints = Fingerprints::load_or_default(&config);

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                match process_request(&config, &mut stream, &sender, &mut fingerprints).await {
                    Ok(_) => {
                        let response = "HTTP/1.1 200 OK\r\n\r\nAccepted";
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(e) => {
                        log::error!("Error: {:?}", e);
                        let response = format!("HTTP/1.1 500 Internal Server Error\r\n\r\n{:?}", e);
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                }
            }
            Err(io_error) => {
                log::warn!("Could not open stream {}", io_error);
            }
        }
    }
}

async fn add_notification(
    alert: &Alert,
    config: &Config,
    sender: &mpsc::UnboundedSender<Notification>,
) -> Result<(), AddNotificationError> {
    let status = match alert.status().as_str() {
        "firing" => "🔥",
        "resolved" => "✅",
        _ => alert.status(),
    };
    let event = format!("[{status}] {}", &alert.labels().alertname());

    let description = format!("{}: {}", alert.status(), alert.annotations().summary());

    let notification = Notification::new(
        config.prowl_api_keys().to_owned(),
        Some(alert.get_priority()),
        Some(alert.generator_url().clone()),
        config.app_name().to_string(),
        event.clone(),
        description,
    )?;
    log::trace!("Built = {:?}", notification);
    sender.send(notification)?;
    log::debug!("Queued notification for {}", event);

    Ok(())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// TODO: some unit and integ tests around this funciton at least
async fn process_request(
    config: &Config,
    stream: &mut std::net::TcpStream,
    sender: &mpsc::UnboundedSender<Notification>,
    fingerprints: &mut Fingerprints,
) -> Result<(), RequestError> {
    let mut buffer = vec![];
    let bytes_read = stream
        .read_to_end(&mut buffer)
        .map_err(RequestError::StreamRead)?;
    let start_index = find_subsequence(&buffer, b"\r\n\r\n").ok_or(RequestError::NoMessageBody)?
        + "\r\n\r\n".len();

    let request_str =
        std::str::from_utf8(&buffer[start_index..bytes_read]).map_err(RequestError::BadMessage)?;
    log::trace!("Request =\n{}\nEOF", request_str);
    let request: Message = serde_json::from_str(request_str).map_err(RequestError::BadJson)?;
    let mut last_err = None;

    for event in request.alerts() {
        let status_changed = match fingerprints.get(event) {
            Some(v) => {
                log::trace!(
                    "Got previous value for {} = {} and is now {}",
                    event.fingerprint(),
                    v.last_status(),
                    event.status()
                );
                v.last_status() != event.status()
            }
            None => {
                log::trace!(
                    "Have not seen {} before. Current entries: {:?}.",
                    event.fingerprint(),
                    fingerprints
                );
                true
            }
        };

        log::debug!(
            "Looking at {}, status_changed = {status_changed}",
            event.labels().alertname()
        );

        fingerprints.insert(event);
        if status_changed {
            if let Err(err) = add_notification(event, config, sender).await {
                log::error!("Error queueing notification {:?}", err);
                last_err = Some(err);
            }
        }
    }

    // Even if an alert is resolved, Grafana may call again with the notification.
    // It may also call later or in a different batch. Should probably do a TTL
    // here in the future to prevent the fingerprints from growing to infinity.

    // Save latest fingerprint states to persistent storage
    match serde_json::to_string(&fingerprints) {
        Ok(serialized) => match std::fs::write(config.fingerprints_file(), serialized) {
            Ok(_) => {}
            Err(e) => log::error!("Failed to save fingerprints: {:?}", e),
        },
        Err(e) => log::error!("Failed to serialize fingerprints: {:?}", e),
    }

    match last_err {
        Some(err) => Err(RequestError::QueueError(err)),
        None => Ok(()),
    }
}
