use crate::models::{config::Config, grafana::Alert};
use chrono::{serde::ts_seconds, DateTime, Utc};
use derive_getters::Getters;
use prowl::Priority;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Fingerprints {
    data: HashMap<String, PreviousEvent>,
}

#[derive(Debug, Deserialize, Clone, Serialize, Getters)]
pub(crate) struct PreviousEvent {
    #[serde(with = "ts_seconds")]
    last_seen: DateTime<Utc>,
    first_alerted: Option<DateTime<Utc>>,
    last_alerted: DateTime<Utc>,
    last_status: String,
    fingerprint: String,
    priority: Option<Priority>,
    name: Option<String>,
    summary: Option<String>,
}

impl Fingerprints {
    pub(crate) fn load_or_default(config: &Config) -> Fingerprints {
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

    pub(crate) fn migrate_v1(config: &Config) -> Result<(), ()> {
        let val = std::fs::read_to_string(config.fingerprints_file()).map_err(|_| ())?;
        let data: HashMap<String, String> = serde_json::from_str(&val).map_err(|_| ())?;
        log::warn!("Migrating fingerprints before start");
        let mut new_data: HashMap<String, PreviousEvent> = HashMap::new();
        for (key, value) in data {
            let event = PreviousEvent {
                last_seen: Utc::now(),
                first_alerted: None,
                last_alerted: Utc::now(),
                last_status: value,
                fingerprint: key.clone(),
                name: None,
                priority: None,
                summary: None,
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

    pub(crate) fn iter(&self) -> std::collections::hash_map::Iter<String, PreviousEvent> {
        self.data.iter()
    }

    pub(crate) fn changed(&self, alert: &Alert) -> bool {
        match self.data.get(alert.fingerprint()) {
            None => {
                log::trace!(
                    "Have not seen {} before. Current entries: {:?}.",
                    alert.fingerprint(),
                    self.data,
                );
                true
            }
            Some(prev) => {
                log::trace!(
                    "Got previous value for {} = {} and is now {}",
                    alert.fingerprint(),
                    prev.last_status(),
                    alert.status()
                );
                prev.last_status() != alert.status()
            }
        }
    }

    pub(crate) fn update_last_seen(&mut self, alert: &Alert) {
        let last_alerted = match self.data.get(alert.fingerprint()) {
            None => Utc::now(),
            Some(prev) => *prev.last_alerted(),
        };

        let first_alerted = if alert.status() == "resolved" {
            None
        } else {
            match self.data.get(alert.fingerprint()) {
                None => None,
                Some(x) => *x.first_alerted(),
            }
        };

        let event = PreviousEvent {
            last_seen: Utc::now(),
            last_status: alert.status().clone(),
            first_alerted,
            last_alerted,
            fingerprint: alert.fingerprint().clone(),
            name: Some(alert.labels().alertname().clone()),
            priority: Some(alert.get_priority()),
            summary: Some(alert.annotations().summary().clone()),
        };

        self.data.insert(alert.fingerprint().clone(), event);
    }

    pub(crate) fn update_last_alerted(&mut self, alert: &Alert) {
        let first_alerted = match self.data.get(alert.fingerprint()) {
            None => Some(Utc::now()),
            Some(prev) => *prev.first_alerted(),
        };
        let event = PreviousEvent {
            last_seen: Utc::now(),
            last_status: alert.status().clone(),
            first_alerted,
            last_alerted: Utc::now(),
            fingerprint: alert.fingerprint().clone(),
            name: Some(alert.labels().alertname().clone()),
            priority: Some(alert.get_priority()),
            summary: Some(alert.annotations().summary().clone()),
        };
        self.data.insert(alert.fingerprint().clone(), event);
    }

    pub(crate) fn update_last_alerted_from_previous_event(
        &mut self,
        previous_event: &PreviousEvent,
    ) {
        let new_event = PreviousEvent {
            last_seen: *previous_event.last_seen(),
            last_status: previous_event.last_status().clone(),
            first_alerted: *previous_event.first_alerted(),
            last_alerted: Utc::now(),
            fingerprint: previous_event.fingerprint.clone(),
            name: previous_event.name().clone(),
            priority: previous_event.priority().clone(),
            summary: previous_event.summary().clone(),
        };
        self.data
            .insert(previous_event.fingerprint.clone(), new_event);
    }

    pub(crate) fn save(&self, config: &Config) {
        match serde_json::to_string(self) {
            Ok(serialized) => match std::fs::write(config.fingerprints_file(), serialized) {
                Ok(_) => {}
                Err(e) => log::error!("Failed to save fingerprints: {:?}", e),
            },
            Err(e) => log::error!("Failed to serialize fingerprints: {:?}", e),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::models::grafana::Alert;

    #[test]
    fn test_changed() {
        let config = Config::load(Some("src/resources/test-dev-null.json".to_string()));
        let mut fingerprints = Fingerprints::load_or_default(&config);
        let alert: Alert = serde_json::from_str(&crate::test::consts::create_firing_alert())
            .expect("Failed to load default, firing alert");
        let resolved: Alert = serde_json::from_str(&crate::test::consts::create_resolved_alert())
            .expect("Failed to load default, resolved alert");

        fingerprints.update_last_alerted(&alert);
        assert_eq!(false, fingerprints.changed(&alert));
        assert_eq!(true, fingerprints.changed(&resolved));

        fingerprints.update_last_alerted(&resolved);
        assert_eq!(true, fingerprints.changed(&alert));
        assert_eq!(false, fingerprints.changed(&resolved));
    }

    #[test]
    fn test_resolved_first() {
        let config = Config::load(Some("src/resources/test-dev-null.json".to_string()));
        let mut fingerprints = Fingerprints::load_or_default(&config);
        let resolved: Alert = serde_json::from_str(&crate::test::consts::create_resolved_alert())
            .expect("Failed to load default, resolved alert");

        fingerprints.update_last_seen(&resolved);
        fingerprints.update_last_seen(&resolved);
        fingerprints.update_last_alerted(&resolved);
        fingerprints.update_last_seen(&resolved);
        // TODO: asserts?
    }

    // TODO: test around first_alerted

    #[test]
    fn load_fingerprints() {
        let config = Config::load(Some(
            "src/resources/test-fingerprints-v3-config.json".to_string(),
        ));
        let fingerprints = Fingerprints::load_or_default(&config);
        assert_eq!(fingerprints.data.len(), 2);
    }

    // TODO: test alert is > realert time
}
