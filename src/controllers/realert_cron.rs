use crate::models::{config::Config, fingerprint::Fingerprints};
use chrono::Utc;
use prowl::Notification;
use std::sync::Arc;
use tokio::{
    sync::{mpsc, Mutex},
    time::sleep,
};

// TODO: tests
pub(crate) async fn main_loop(
    config: Config,
    sender: mpsc::UnboundedSender<Notification>,
    fingerprints: Arc<Mutex<Fingerprints>>,
) {
    let cron_string = match config.realert_cron() {
        Some(x) => x,
        None => {
            log::trace!("Cron re-alert not configured. Exiting cron loop.");
            return;
        }
    };
    loop {
        let now = Utc::now();
        match cron_parser::parse(cron_string, &now) {
            Ok(next_time) => {
                let again_time = match next_time.signed_duration_since(now).to_std() {
                    Ok(x) => x,
                    Err(e) => {
                        log::error!("Failed to convert chrono duration to std, {e}. Exiting loop because wtf.");
                        return;
                    }
                };
                log::trace!("{:?} until next cron re-alert", again_time);
                sleep(again_time).await;
            }
            Err(e) => {
                log::error!("Cron string could not be parsed, {e}");
                break;
            }
        };

        let mut finger_guard = fingerprints.lock().await;
        let mut updated: Vec<crate::models::fingerprint::PreviousEvent> = vec![];
        {
            for (_, fingerprint) in finger_guard.iter() {
                let resolved = fingerprint.last_status() == "resolved";
                if !resolved {
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
        // wait a minute to not match an infinite number of times during that one minute.
        sleep(std::time::Duration::from_secs(60)).await;
    }
}
