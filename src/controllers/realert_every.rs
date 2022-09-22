use crate::models::{config::Config, fingerprint::Fingerprints};
use chrono::Utc;
use prowl::Notification;
use std::sync::Arc;
use tokio::{
    sync::{mpsc, Mutex},
    time::{sleep, Duration},
};

// TODO: tests
pub(crate) async fn main_loop(
    config: Config,
    sender: mpsc::UnboundedSender<Notification>,
    fingerprints: Arc<Mutex<Fingerprints>>,
) {
    let ttl = match config.alert_every_minutes() {
        Some(x) => chrono::Duration::minutes(*x),
        None => {
            log::trace!("Alert-every-minutes re-alert not configured. Exiting cron loop.");
            return;
        }
    };
    loop {
        let mut finger_guard = fingerprints.lock().await;
        let alert_again_time = Utc::now()
            .checked_sub_signed(ttl)
            .expect("The alert_every_minutes is before epoch");
        let mut updated: Vec<crate::models::fingerprint::PreviousEvent> = vec![];
        {
            for (_, fingerprint) in finger_guard.iter() {
                let past_time = fingerprint.last_alerted() <= &alert_again_time;
                let resolved = fingerprint.last_status() == "resolved";
                if past_time && !resolved {
                    let name = match fingerprint.name() {
                        Some(name) => name.clone(),
                        None => "Unknown".to_string(),
                    };
                    let event = format!("[🕓] {}", name);
                    let description = format!("{name} is still firing.");
                    let notification = Notification::new(
                        config.prowl_api_keys().to_owned(),
                        fingerprint.priority().clone(),
                        None,
                        config.app_name().to_string(),
                        event,
                        description,
                    );
                    log::trace!("Queued {:?}", notification);
                    updated.push(fingerprint.clone());
                    match notification {
                        Ok(notification) => match sender.send(notification) {
                            Ok(_) => {}
                            Err(e) => {
                                log::error!("Failed to add notification, {e}");
                            }
                        },
                        Err(e) => {
                            log::error!("Failed to add re-alert notification due to {e}");
                        }
                    }
                }
            }
        }
        for fingerprint in updated {
            finger_guard.update_last_alerted_from_previous_event(&fingerprint);
        }
        finger_guard.save(&config);
        drop(finger_guard);
        sleep(Duration::from_secs(60)).await;
    }
}
