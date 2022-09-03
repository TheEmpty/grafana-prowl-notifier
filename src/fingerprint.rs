use crate::{grafana::Alert, Config};
use chrono::{serde::ts_seconds, DateTime, Utc};
use derive_getters::Getters;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub struct Fingerprints {
    data: HashMap<String, PreviousEvent>,
}

#[derive(Debug, Deserialize, Serialize, Getters)]
pub struct PreviousEvent {
    #[serde(with = "ts_seconds")]
    last_seen: DateTime<Utc>,
    last_status: String,
    fingerprint: String,
    name: Option<String>,
}

impl Fingerprints {
    pub fn load_or_default(config: &Config) -> Fingerprints {
        match std::fs::read_to_string(config.fingerprints_file()) {
            Ok(val) => match serde_json::from_str(&val) {
                Ok(v) => {
                    log::trace!("Loaded fingerprints: {:?}", v);
                    v
                }
                Err(e) => {
                    log::error!(
                        "Failed to load JSON from {}. Creating an empty HashMap. {:?}",
                        config.fingerprints_file(),
                        e
                    );
                    Fingerprints {
                        data: HashMap::new(),
                    }
                }
            },
            Err(e) => {
                log::warn!(
                    "Failed to load {}, creating empty HashMap. {:?}",
                    config.fingerprints_file(),
                    e
                );
                Fingerprints {
                    data: HashMap::new(),
                }
            }
        }
    }

    pub fn migrate_v1(config: &Config) -> Result<(), ()> {
        let val = std::fs::read_to_string(config.fingerprints_file()).map_err(|_| ())?;
        let data: HashMap<String, String> = serde_json::from_str(&val).map_err(|_| ())?;
        log::warn!("Migrating fingerprints before start");
        let mut new_data: HashMap<String, PreviousEvent> = HashMap::new();
        for (key, value) in data {
            let event = PreviousEvent {
                last_seen: Utc::now(),
                last_status: value,
                fingerprint: key.clone(),
                name: None,
            };
            new_data.insert(key, event);
        }
        let new = Fingerprints { data: new_data };
        match serde_json::to_string(&new) {
            Ok(serialized) => match std::fs::write(config.fingerprints_file(), serialized) {
                Ok(_) => {
                    log::debug!("Migration (migrate_v1) successful");
                    Ok(())
                }
                Err(e) => panic!("Failed to save fingerprints: {:?}", e),
            },
            Err(e) => panic!("Failed to serialize fingerprints: {:?}", e),
        }
    }

    pub fn insert(&mut self, alert: &Alert) {
        let event = PreviousEvent {
            last_seen: Utc::now(),
            last_status: alert.status().clone(),
            fingerprint: alert.fingerprint().clone(),
            name: Some(alert.labels().alertname().clone()),
        };
        self.data.insert(alert.fingerprint().clone(), event);
    }

    pub fn get(&self, alert: &Alert) -> Option<&PreviousEvent> {
        self.data.get(alert.fingerprint())
    }
}
