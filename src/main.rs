use derive_getters::Getters;
use prowl::Notification;
use serde::Deserialize;
use std::{
    collections::{hash_map::Entry, HashMap},
    fs::File,
    io::{BufReader, Read, Write},
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

    tokio::spawn(process_notifications(*config.linear_retry_secs(), reciever));
    http_loop(config, sender).await;
}

async fn process_notifications(
    retry_time: u64,
    mut reciever: mpsc::UnboundedReceiver<Notification>,
) {
    log::debug!("Notifications channel processor started.");
    while let Some(notification) = reciever.recv().await {
        'notification: loop {
            log::trace!("Processing {:?}", notification);
            match notification.add().await {
                // only move to next notification if we processed this one,
                Ok(_) => break 'notification,
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

async fn http_loop(config: Config, sender: mpsc::UnboundedSender<Notification>) {
    let listener = TcpListener::bind(config.bind_host())
        .unwrap_or_else(|_| panic!("Faild to bind to {}", config.bind_host()));
    log::info!("Listening on {}", config.bind_host());

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

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn get_priority(alert: &Alert) -> prowl::Priority {
    if alert.status == "firing" {
        let alertname = &alert.labels.alertname;
        if alertname.starts_with("[critical]") || alertname.starts_with("[CRIT]") {
            prowl::Priority::Emergency
        } else if alertname.starts_with("[high]") || alertname.starts_with("[HIGH]") {
            prowl::Priority::High
        } else {
            prowl::Priority::Normal
        }
    } else {
        prowl::Priority::VeryLow
    }
}

async fn add_notification(
    alert: &Alert,
    config: &Config,
    sender: &mpsc::UnboundedSender<Notification>,
) -> Result<(), AddNotificationError> {
    let event = if alert.status == "firing" {
        format!("[🔥] {}", &alert.labels.alertname)
    } else if alert.status == "resolved" {
        format!("[✅] {}", &alert.labels.alertname)
    } else {
        format!("[{}] {}", &alert.status, &alert.labels.alertname)
    };

    let description = format!("{}: {}", &alert.status, &alert.annotations.summary);

    let notification = Notification::new(
        config.prowl_api_keys.to_owned(),
        Some(get_priority(alert)),
        Some(alert.generatorURL.clone()),
        config.app_name().to_string(),
        event,
        description,
    )?;
    log::debug!("Built = {:?}", notification);
    sender.send(notification)?;
    log::trace!("Sent notification");

    Ok(())
}

async fn process_request(
    config: &Config,
    stream: &mut std::net::TcpStream,
    sender: &mpsc::UnboundedSender<Notification>,
    fingerprint_to_last_status: &mut HashMap<String, String>,
) -> Result<(), RequestError> {
    // TODO: move to a non-static buffer size.
    // read into vec
    let mut buffer = [0; 8192];
    let bytes_read = stream.read(&mut buffer).map_err(RequestError::Io)?;
    let start_index = find_subsequence(&buffer, b"\r\n\r\n").ok_or(RequestError::NoMessageBody)?
        + "\r\n\r\n".len();

    let request_str =
        std::str::from_utf8(&buffer[start_index..bytes_read]).map_err(RequestError::BadMessage)?;
    log::trace!("Request =\n{}\nEOF", request_str);
    let request: Message = serde_json::from_str(request_str).map_err(RequestError::BadJson)?;
    let mut last_err = None;
    for event in request.alerts {
        let previous_status = fingerprint_to_last_status.entry(event.fingerprint.clone());
        let status_changed = match previous_status {
            Entry::Occupied(ref v) => v.get() != &event.status,
            Entry::Vacant(_) => true,
        };

        log::trace!(
            "Looking at {}, status_changed = {status_changed}",
            event.labels.alertname
        );

        if status_changed {
            // blocked by: https://github.com/rust-lang/rust/issues/65225
            // previous_status.insert_entry(event.status.clone());
            fingerprint_to_last_status.insert(event.fingerprint.clone(), event.status.clone());
            if let Err(err) = add_notification(&event, config, sender).await {
                log::error!("Error queueing notification {:?}", err);
                last_err = Some(err);
            }
        }

        if event.status == "resolved" {
            // No more reason to hold it in memory.
            // Fingerprint should never be the same again.
            fingerprint_to_last_status.remove(&event.fingerprint);
        }
    }

    match last_err {
        Some(err) => Err(RequestError::QueueError(err)),
        None => Ok(()),
    }
}

#[derive(Deserialize, Getters)]
struct Config {
    #[serde(default = "default_retry_secs")]
    linear_retry_secs: u64,
    #[serde(default = "default_app_name")]
    app_name: String,
    #[serde(default = "default_bind_host")]
    bind_host: String,
    prowl_api_keys: Vec<String>,
}

fn default_retry_secs() -> u64 {
    60
}

fn default_app_name() -> String {
    "Grafana".to_string()
}

fn default_bind_host() -> String {
    "0.0.0.0:3333".to_string()
}

impl Config {
    pub fn load() -> Self {
        let filename = match std::env::args().nth(1) {
            Some(x) => {
                log::debug!("Using argument for config file: '{x}'.");
                x
            }
            None => {
                log::debug!("Using default config file path, ./config.json");
                "config.json".to_string()
            }
        };

        let config_file =
            File::open(&filename).unwrap_or_else(|_| panic!("Faild to find config {filename}"));
        let config_reader = BufReader::new(config_file);
        serde_json::from_reader(config_reader).expect("Error reading configuration.")
    }
}

#[derive(Deserialize)]
struct Message {
    alerts: Vec<Alert>,
}

#[allow(non_snake_case)]
#[derive(Deserialize)]
struct Alert {
    status: String,
    labels: Label,
    annotations: Annotation,
    generatorURL: String,
    fingerprint: String,
}

#[derive(Deserialize)]
struct Label {
    alertname: String,
}

#[derive(Deserialize)]
struct Annotation {
    summary: String,
}

#[derive(Debug)]
enum RequestError {
    Io(std::io::Error),
    NoMessageBody,
    BadMessage(std::str::Utf8Error),
    BadJson(serde_json::Error),
    QueueError(AddNotificationError),
}

#[derive(Debug)]
enum AddNotificationError {
    Add(prowl::AddError),
    Creation(prowl::CreationError),
    Channel(mpsc::error::SendError<Notification>),
}

impl From<prowl::AddError> for AddNotificationError {
    fn from(error: prowl::AddError) -> Self {
        Self::Add(error)
    }
}

impl From<prowl::CreationError> for AddNotificationError {
    fn from(error: prowl::CreationError) -> Self {
        Self::Creation(error)
    }
}

impl From<mpsc::error::SendError<Notification>> for AddNotificationError {
    fn from(error: mpsc::error::SendError<Notification>) -> Self {
        Self::Channel(error)
    }
}
