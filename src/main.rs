use chrono::{DateTime, Utc};
use derive_getters::Getters;
use log::{debug, error, info, warn};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;
use std::io::{Read, Write};
use std::net::TcpListener;

const MAX_CACHE_MISS_TIME: i64 = 10;
const MAX_FINGER_PRINTS_DEFAULT: usize = 25;
const DEFAULT_BIND: &str = "0.0.0.0:3333";

#[tokio::main]
async fn main() {
    env_logger::init();
    let config = Config::load();
    let bind_host = match config.bind_host() {
        Some(v) => v.clone(),
        None => DEFAULT_BIND.to_string(),
    };
    let listener =
        TcpListener::bind(&bind_host).unwrap_or_else(|_| panic!("Faild to bind to {bind_host}"));
    info!("Listening on {bind_host}");
    let mut fingerprints = HashSet::new();
    let max_finger_prints = config
        .notification_finger_print_cache_size()
        .unwrap_or(MAX_FINGER_PRINTS_DEFAULT);

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                match process_request(
                    &config,
                    &mut stream,
                    &mut fingerprints,
                    config.prowl_api_keys(),
                )
                .await
                {
                    Ok(_) => {
                        let response = "HTTP/1.1 200 OK\r\n\r\nAccepted";
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(e) => {
                        error!("Error: {:?}", e);
                        let response = format!("HTTP/1.1 500 Internal Server Error\r\n\r\n{:?}", e);
                        let _ = stream.write_all(response.as_bytes());
                        let _ = stream.flush();
                    }
                }
            }
            Err(io_error) => {
                warn!("Could not open stream {}", io_error);
            }
        }
    }

    fingerprints.shrink_to(max_finger_prints);
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

async fn send_notification(alert: &Alert, api_key: Vec<String>) -> Result<(), ProwlError> {
    let event = if alert.status == "firing" {
        format!("[🔥] {}", &alert.labels.alertname)
    } else if alert.status == "resolved" {
        format!("[✅] {}", &alert.labels.alertname)
    } else {
        format!("[{}] {}", &alert.status, &alert.labels.alertname)
    };

    let description = format!("{}: {}", &alert.status, &alert.annotations.summary);

    let notification = prowl::Notification::new(
        api_key,
        Some(get_priority(alert)),
        Some(alert.generatorURL.clone()),
        "Grafana".to_string(),
        event,
        description,
    )?;
    debug!("Built = {:?}", notification);
    notification.add().await?;

    Ok(())
}

fn recent(config: &Config, event: &Alert) -> bool {
    let now = Utc::now();
    let recent_minutes = config.max_cache_miss_time().unwrap_or(MAX_CACHE_MISS_TIME);
    match event.startsAt.parse::<DateTime<Utc>>() {
        Ok(x) => now.signed_duration_since(x).num_minutes() <= recent_minutes,
        Err(e) => {
            log::error!(
                "Failed to parse DateTime string. Due to {:?}. Was: {}",
                e,
                event.startsAt
            );
            true
        }
    }
}

async fn process_request(
    config: &Config,
    stream: &mut std::net::TcpStream,
    fingerprints: &mut HashSet<String>,
    api_keys: &[String],
) -> Result<(), RequestError> {
    let mut buffer = [0; 20480];
    let bytes_read = stream.read(&mut buffer).map_err(RequestError::Io)?;
    let start_index = find_subsequence(&buffer, b"\r\n\r\n").ok_or(RequestError::NoMessageBody)?
        + "\r\n\r\n".len();

    let request_str =
        std::str::from_utf8(&buffer[start_index..bytes_read]).map_err(RequestError::BadMessage)?;
    log::trace!("Request =\n{}\nEOF", request_str);
    let request: Message = serde_json::from_str(request_str).map_err(RequestError::BadJson)?;
    let mut last_err = None;
    for event in request.alerts {
        let fingerprinted = fingerprints.contains(&event.fingerprint);
        let resolved = event.status == "resolved";
        let recent = recent(config, &event);

        log::trace!("Looking at {}, fingerprinted = {fingerprinted}, recent = {recent}, resolved = {resolved}", event.labels.alertname);
        if (recent && !fingerprinted) || resolved {
            fingerprints.insert(event.fingerprint.clone());

            if let Err(err) = send_notification(&event, api_keys.to_owned()).await {
                error!("Error sending notification {:?}", err);
                last_err = Some(err);
            }
        } else if !recent {
            log::info!(
                "Skipping {} because it was not recent. Was not fingerprinted.",
                event.labels.alertname
            );
            let max_finger_prints = config
                .notification_finger_print_cache_size()
                .unwrap_or(MAX_FINGER_PRINTS_DEFAULT);
            if fingerprints.len() < max_finger_prints {
                fingerprints.insert(event.fingerprint.clone());
                log::info!("Added {} to fingerprints.", event.labels.alertname);
            } else {
                log::info!(
                    "Not adding {} to fingerprints since it was full.",
                    event.labels.alertname
                );
            }
        }
    }

    match last_err {
        Some(err) => Err(RequestError::Prowl(err)),
        None => Ok(()),
    }
}

#[derive(Deserialize, Getters)]
struct Config {
    prowl_api_keys: Vec<String>,
    bind_host: Option<String>,
    max_cache_miss_time: Option<i64>,
    notification_finger_print_cache_size: Option<usize>,
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

        let config_file = File::open(&filename).expect("Faild to find config {filename}");
        let config_reader = BufReader::new(config_file);
        serde_json::from_reader(config_reader).expect("Error reading configuration.")
    }
}

impl From<prowl::AddError> for ProwlError {
    fn from(error: prowl::AddError) -> Self {
        Self::Add(error)
    }
}

impl From<prowl::CreationError> for ProwlError {
    fn from(error: prowl::CreationError) -> Self {
        Self::Creation(error)
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
    startsAt: String,
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
    Prowl(ProwlError),
}

#[derive(Debug)]
enum ProwlError {
    Add(prowl::AddError),
    Creation(prowl::CreationError),
}
