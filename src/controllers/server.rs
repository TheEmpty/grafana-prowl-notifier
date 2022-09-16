use crate::{
    errors::{AddNotificationError, GrafanaWebhookError, RequestError},
    models::{
        config::Config,
        fingerprint::Fingerprints,
        grafana::{Alert, Message},
        http,
    },
};
use prowl::Notification;
use std::{net::TcpListener, sync::Arc};
use tokio::{
    sync::{mpsc, Mutex},
    time::Duration,
};

pub(crate) async fn main_loop(
    listener: TcpListener,
    config: Config,
    sender: mpsc::UnboundedSender<Notification>,
    mut fingerprints: Arc<Mutex<Fingerprints>>,
) {
    log::trace!("Listening for incoming connections");
    for stream in listener.incoming() {
        log::trace!("Connection incoming");
        match stream {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .expect("Failed to set read timeout");
                match http::Request::from_stream(&mut stream) {
                    Ok(request) => match request.request_line().path().as_str() {
                        "/webhooks/grafana" => {
                            let response =
                                grafana_webook(&config, request, &sender, &mut fingerprints).await;
                            let _ = response.send(&mut stream);
                        }
                        _ => {
                            let body = "Not found".to_string();
                            let headers = vec![
                                "HTTP/1.1 404 Not Found".to_string(),
                                "Content-Type: text/plain".to_string(),
                            ];
                            let _ = http::Response::new(headers, Some(body)).send(&mut stream);
                        }
                    },
                    Err(RequestError::NoContentLength) => {
                        let headers = vec!["HTTP/1.1 411 Length Required".to_string()];
                        let _ = http::Response::new(headers, None).send(&mut stream);
                    }
                    Err(e) => {
                        log::error!("Failed to process request due to {}", e);
                        let body = format!("{}", e);
                        let headers = vec![
                            "HTTP/1.1 500 Internal Server Error".to_string(),
                            "Content-Type: text/plain".to_string(),
                        ];
                        let _ = http::Response::new(headers, Some(body)).send(&mut stream);
                    }
                }
                fingerprints.lock().await.save(&config);
            }
            Err(io_error) => {
                log::warn!("Could not open stream {}", io_error);
            }
        }
    }
}

fn create_grafana_failure_response(error: GrafanaWebhookError) -> http::Response {
    log::error!("Grafana failed to process request due to {}", error);
    let body = format!("{}", error);
    let headers = vec![
        "HTTP/1.1 500 Internal Server Error".to_string(),
        "Content-Type: text/plain".to_string(),
    ];
    http::Response::new(headers, Some(body))
}

async fn grafana_webook(
    config: &Config,
    request: http::Request,
    sender: &mpsc::UnboundedSender<Notification>,
    fingerprints: &mut Arc<Mutex<Fingerprints>>,
) -> http::Response {
    log::trace!("Processing request");

    if request.request_line().method() != "POST" {
        return create_grafana_failure_response(GrafanaWebhookError::WrongMethod(
            request.request_line().method().clone(),
        ));
    }

    let request: Result<Message, GrafanaWebhookError> =
        serde_json::from_str(request.body()).map_err(GrafanaWebhookError::BadJson);
    let request = match request {
        Ok(r) => r,
        Err(e) => return create_grafana_failure_response(e),
    };
    let mut last_err = None;

    let mut fingerprints = fingerprints.lock().await;
    for event in request.alerts() {
        // Even if an alert is resolved, Grafana may call again with the notification.
        match fingerprints.changed(event) {
            false => fingerprints.update_last_seen(event),
            true => {
                fingerprints.update_last_alerted(event);
                if let Err(err) = add_notification(event, config, sender).await {
                    log::error!("Error queueing notification {:?}", err);
                    last_err = Some(err);
                }
            }
        };
    }

    if let Some(e) = last_err {
        create_grafana_failure_response(GrafanaWebhookError::QueueError(e))
    } else {
        let body = "Accepted";
        let headers = vec![
            "HTTP/1.1 200 OK".to_string(),
            "Content-Type: text/plain".to_string(),
        ];
        http::Response::new(headers, Some(body.to_string()))
    }
}

async fn add_notification(
    alert: &Alert,
    config: &Config,
    sender: &mpsc::UnboundedSender<Notification>,
) -> Result<(), AddNotificationError> {
    let status = match alert.status().as_str() {
        "firing" => "🔥",
        "resolved" => "✅",
        _ => alert.status(),
    };
    let event = format!("[{status}] {}", &alert.labels().alertname());

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

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_const;

    #[tokio::test]
    async fn test_add_notification() {
        let config = Config::load(Some("src/resources/test-dev-null.json".to_string()));
        let alert: Alert = serde_json::from_str(&test_const::create_firing_alert())
            .expect("Failed to load default, firing alert");
        let (sender, mut reciever) = mpsc::unbounded_channel();

        add_notification(&alert, &config, &sender)
            .await
            .expect("Failed to add notification");
        drop(sender);
        let notification = reciever.recv().await.expect("Failed to get first result");
        assert!(reciever.recv().await.is_none());

        assert_eq!(notification.priority(), &Some(prowl::Priority::Normal));
        assert_eq!(
            notification.url(),
            &Some("http://something/this".to_string())
        );
        assert_eq!(notification.application(), "Grafana");
        assert_eq!(notification.event(), "[🔥] Alert Name");
        assert_eq!(notification.description(), "firing: Annotation Summary");
    }

    #[tokio::test]
    async fn test_high_alert() {
        let config = Config::load(Some("src/resources/test-dev-null.json".to_string()));
        let json = test_const::create_firing_alert_with_prefix("[high] ");
        let firing_alert: Alert = serde_json::from_str(&json).expect("Failed to load alert");
        let json = test_const::create_resolved_alert_with_prefix("[high] ");
        let resolved_alert: Alert = serde_json::from_str(&json).expect("Failed to load alert");
        let (sender, mut reciever) = mpsc::unbounded_channel();

        add_notification(&firing_alert, &config, &sender)
            .await
            .expect("Failed to add notification");
        add_notification(&resolved_alert, &config, &sender)
            .await
            .expect("Failed to add notification");
        drop(sender);
        let firing_notification = reciever.recv().await.expect("Failed to get first result");
        let resolved_notification = reciever.recv().await.expect("Failed to get first result");
        assert!(reciever.recv().await.is_none());

        assert_eq!(firing_notification.event(), "[🔥] [high] Alert Name");
        assert_eq!(firing_notification.priority(), &Some(prowl::Priority::High));
        assert_eq!(
            firing_notification.url(),
            &Some("http://something/this".to_string())
        );
        assert_eq!(firing_notification.application(), "Grafana");
        assert_eq!(
            firing_notification.description(),
            "firing: Annotation Summary"
        );

        assert_eq!(resolved_notification.event(), "[✅] [high] Alert Name");
        assert_eq!(
            resolved_notification.priority(),
            &Some(prowl::Priority::VeryLow)
        );
        assert_eq!(
            resolved_notification.url(),
            &Some("http://something/this".to_string())
        );
        assert_eq!(resolved_notification.application(), "Grafana");
        assert_eq!(
            resolved_notification.description(),
            "resolved: Annotation Summary"
        );
    }

    #[tokio::test]
    async fn test_crit_alert() {
        let config = Config::load(Some("src/resources/test-dev-null.json".to_string()));
        let json = test_const::create_firing_alert_with_prefix("[critical] ");
        let firing_alert: Alert = serde_json::from_str(&json).expect("Failed to load alert");
        let json = test_const::create_resolved_alert_with_prefix("[critical] ");
        let resolved_alert: Alert = serde_json::from_str(&json).expect("Failed to load alert");
        let (sender, mut reciever) = mpsc::unbounded_channel();

        add_notification(&firing_alert, &config, &sender)
            .await
            .expect("Failed to add notification");
        add_notification(&resolved_alert, &config, &sender)
            .await
            .expect("Failed to add notification");
        drop(sender);
        let firing_notification = reciever.recv().await.expect("Failed to get first result");
        let resolved_notification = reciever.recv().await.expect("Failed to get first result");
        assert!(reciever.recv().await.is_none());

        assert_eq!(
            firing_notification.priority(),
            &Some(prowl::Priority::Emergency)
        );
        assert_eq!(
            firing_notification.url(),
            &Some("http://something/this".to_string())
        );
        assert_eq!(firing_notification.application(), "Grafana");
        assert_eq!(firing_notification.event(), "[🔥] [critical] Alert Name");
        assert_eq!(
            firing_notification.description(),
            "firing: Annotation Summary"
        );

        assert_eq!(
            resolved_notification.priority(),
            &Some(prowl::Priority::VeryLow)
        );
        assert_eq!(
            resolved_notification.url(),
            &Some("http://something/this".to_string())
        );
        assert_eq!(resolved_notification.application(), "Grafana");
        assert_eq!(resolved_notification.event(), "[✅] [critical] Alert Name");
        assert_eq!(
            resolved_notification.description(),
            "resolved: Annotation Summary"
        );
    }

    #[tokio::test]
    async fn test_grafana_webook() {
        // firing
        let body = format!("{{\"alerts\": [{}]}}", test_const::create_firing_alert());
        let headers = vec![
            "POST / HTTP/1.1".to_string(),
            "Host: 127.0.0.1:3000".to_string(),
            "Accept: */*".to_string(),
            "User-Agent: UnitTest/1.0".to_string(),
            format!("Content-Length: {}", body.len()),
        ]
        .join("\r\n");
        let request = format!("{headers}\r\n\r\n{body}");
        let mut firing_stream = std::io::BufReader::new(request.as_bytes());
        let firing_request =
            http::Request::from_stream(&mut firing_stream).expect("Failed to build request");
        let mut firing_stream2 = std::io::BufReader::new(request.as_bytes());
        let firing_request2 =
            http::Request::from_stream(&mut firing_stream2).expect("Failed to build request");

        // resolved
        let body = format!("{{\"alerts\": [{}]}}", test_const::create_resolved_alert());
        let headers = vec![
            "POST / HTTP/1.1".to_string(),
            "Host: 127.0.0.1:3000".to_string(),
            "Accept: */*".to_string(),
            "User-Agent: UnitTest/1.0".to_string(),
            format!("Content-Length: {}", body.len()),
        ]
        .join("\r\n");
        let request = format!("{headers}\r\n\r\n{body}");
        let mut resolved_stream = std::io::BufReader::new(request.as_bytes());
        let resolved_request =
            http::Request::from_stream(&mut resolved_stream).expect("Failed to build request");

        // others
        let config = Config::load(Some("src/resources/test-dev-null.json".to_string()));
        let fingerprints = Fingerprints::load_or_default(&config);
        let mut fingerprints = Arc::new(Mutex::new(fingerprints));
        let (sender, mut reciever) = mpsc::unbounded_channel();

        let response = grafana_webook(&config, firing_request, &sender, &mut fingerprints).await;
        assert_eq!(response.headers().get(0).expect("No headers"), "HTTP/1.1 200 OK");

        let response = grafana_webook(&config, firing_request2, &sender, &mut fingerprints).await;
        assert_eq!(response.headers().get(0).expect("No headers"), "HTTP/1.1 200 OK");

        let response = grafana_webook(&config, resolved_request, &sender, &mut fingerprints).await;
        assert_eq!(response.headers().get(0).expect("No headers"), "HTTP/1.1 200 OK");

        drop(sender);
        let firing_notification = reciever.recv().await.expect("Failed to get first result");
        let resolved_notification = reciever.recv().await.expect("Failed to get second result");
        assert!(reciever.recv().await.is_none());

        assert_eq!(
            firing_notification.priority(),
            &Some(prowl::Priority::Normal)
        );
        assert_eq!(
            firing_notification.url(),
            &Some("http://something/this".to_string())
        );
        assert_eq!(firing_notification.application(), "Grafana");
        assert_eq!(firing_notification.event(), "[🔥] Alert Name");
        assert_eq!(
            firing_notification.description(),
            "firing: Annotation Summary"
        );

        assert_eq!(
            resolved_notification.priority(),
            &Some(prowl::Priority::VeryLow)
        );
        assert_eq!(
            resolved_notification.url(),
            &Some("http://something/this".to_string())
        );
        assert_eq!(resolved_notification.application(), "Grafana");
        assert_eq!(resolved_notification.event(), "[✅] Alert Name");
        assert_eq!(
            resolved_notification.description(),
            "resolved: Annotation Summary"
        );
    }
}
