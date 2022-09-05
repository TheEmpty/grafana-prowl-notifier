use crate::models::config::Config;
use prowl::Notification;
use tokio::{
    sync::mpsc,
    time::{sleep, Duration},
};

pub(crate) async fn main_loop(config: Config, mut reciever: mpsc::UnboundedReceiver<Notification>) {
    log::debug!("Notifications channel processor started.");
    while let Some(notification) = reciever.recv().await {
        'notification: loop {
            log::trace!("Processing {:?}", notification);
            if *config.test_mode() {
                break 'notification;
            }
            match notification.add().await {
                // only move to next notification if we processed this one,
                Ok(_) => {
                    sleep(Duration::from_secs(
                        *config.wait_secs_between_notifications(),
                    ))
                    .await;
                    break 'notification;
                }
                Err(e) => {
                    log::error!("Failed to send notification due to {:?}.", e);
                    log::debug!(
                        "Waiting {}s to retry sending notifications",
                        config.linear_retry_secs()
                    );
                    sleep(Duration::from_secs(*config.linear_retry_secs())).await;
                }
            }
        }
    }
    log::warn!("Notification channel has been closed.");
}
