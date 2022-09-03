use derive_getters::Getters;
use serde::Deserialize;
use std::{fs::File, io::BufReader};

#[derive(Deserialize, Getters)]
pub struct Config {
    #[serde(default = "default_retry_secs")]
    linear_retry_secs: u64,
    #[serde(default = "default_wait_time")]
    wait_secs_between_notifications: u64,
    #[serde(default = "default_app_name")]
    app_name: String,
    #[serde(default = "default_bind_host")]
    bind_host: String,
    prowl_api_keys: Vec<String>,
    fingerprints_file: String,
}

fn default_retry_secs() -> u64 {
    60
}

fn default_wait_time() -> u64 {
    0
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
