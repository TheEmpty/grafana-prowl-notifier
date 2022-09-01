mod config;
mod errors;
mod grafana;

use crate::{
    config::Config,
    errors::{AddNotificationError, RequestError},
    grafana::{Alert, Message},
};
use prowl::Notification;
use std::{
    collections::{hash_map::Entry, HashMap},
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
    let config = Config::load();
    let (sender, reciever) = mpsc::unbounded_channel();
    let listener = TcpListener::bind(config.bind_host())
        .unwrap_or_else(|_| panic!("Faild to bind to {}", config.bind_host()));
    log::info!("Listening on {}", config.bind_host());

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
    let mut fingerprint_to_last_status = HashMap::new();

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                match process_request(
                    &config,
                    &mut stream,
                    &sender,
                    &mut fingerprint_to_last_status,
                )
                .await
                {
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
    let event = if alert.status() == "firing" {
        format!("[🔥] {}", &alert.labels().alertname())
    } else if alert.status() == "resolved" {
        format!("[✅] {}", &alert.labels().alertname())
    } else {
        format!("[{}] {}", &alert.status(), &alert.labels().alertname())
    };

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
    fingerprint_to_last_status: &mut HashMap<String, String>,
) -> Result<(), RequestError> {
    let mut buffer = vec![];
    let bytes_read = stream.read_to_end(&mut buffer).map_err(RequestError::Io)?;
    let start_index = find_subsequence(&buffer, b"\r\n\r\n").ok_or(RequestError::NoMessageBody)?
        + "\r\n\r\n".len();

    let request_str =
        std::str::from_utf8(&buffer[start_index..bytes_read]).map_err(RequestError::BadMessage)?;
    log::trace!("Request =\n{}\nEOF", request_str);
    let request: Message = serde_json::from_str(request_str).map_err(RequestError::BadJson)?;
    let mut last_err = None;
    for event in request.alerts() {
        let previous_status = fingerprint_to_last_status.entry(event.fingerprint().clone());
        let status_changed = match previous_status {
            Entry::Occupied(ref v) => v.get() != event.status(),
            Entry::Vacant(_) => true,
        };

        log::debug!(
            "Looking at {}, status_changed = {status_changed}",
            event.labels().alertname()
        );

        if status_changed {
            // blocked by: https://github.com/rust-lang/rust/issues/65225
            // previous_status.insert_entry(event.status.clone());
            fingerprint_to_last_status.insert(event.fingerprint().clone(), event.status().clone());
            if let Err(err) = add_notification(event, config, sender).await {
                log::error!("Error queueing notification {:?}", err);
                last_err = Some(err);
            }
        }

        // Note: even if resolved, Grafana may call again with the same fingerprint and status.
    }

    match last_err {
        Some(err) => Err(RequestError::QueueError(err)),
        None => Ok(()),
    }
}
