use derive_getters::Getters;
use serde::Deserialize;
use std::{fs::File, io::BufReader};

#[derive(Clone, Deserialize, Getters)]
pub(crate) struct Config {
    #[serde(default = "default_retry_secs")]
    linear_retry_secs: u64,
    #[serde(default = "default_app_name")]
    app_name: String,
    #[serde(default = "default_bind_host")]
    bind_host: String,
    alert_every_minutes: Option<i64>,
    realert_cron: Option<String>,
    prowl_api_keys: Vec<String>,
    fingerprints_file: String,
    #[serde(default = "bool::default")]
    test_mode: bool,
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
    pub(crate) fn load(filename: Option<String>) -> Self {
        let filename = match filename {
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_default() {
        let config = Config::load(Some("src/resources/test-min-config.json".to_string()));
        assert_eq!(config.linear_retry_secs(), &60);
        assert_eq!(config.app_name(), "Grafana");
        assert_eq!(config.bind_host(), "0.0.0.0:3333");
        assert_eq!(config.alert_every_minutes(), &None);
        assert_eq!(config.realert_cron(), &None);
        assert_eq!(config.test_mode(), &false);
    }

    #[test]
    fn test_full_config() {
        let config = Config::load(Some("src/resources/test-max-config.json".to_string()));
        assert_eq!(config.app_name(), "Home Lab");
        assert_eq!(config.bind_host(), "127.0.0.1:1234");
        assert_eq!(config.prowl_api_keys(), &vec!["api_key1", "api_key2"]);
        assert_eq!(config.fingerprints_file(), "/var/fingerprints.json");
        assert_eq!(config.linear_retry_secs(), &11);
        assert_eq!(config.alert_every_minutes(), &Some(33));
        assert_eq!(config.realert_cron(), &Some("0 9 * * MON-FRI".to_string()));
        assert_eq!(config.test_mode(), &true);
    }
}
