use prowl::Notification;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub(crate) enum RequestError {
    #[error("Failed to read http input stream. {0}")]
    StreamRead(std::io::Error),
    #[error("The HTML request did not have an HTML body or was improperly formatted.")]
    NoMessageBody,
    #[error("HTML message body could not be converted to Utf8. {0}")]
    BadMessage(std::str::Utf8Error),
    #[error("HTTP Request did not have the content-length header")]
    NoContentLength,
    #[error("Sender said they had {0} bytes, but only sent {1} bytes.")]
    BadContentLength(usize, usize),
    #[error("The HTTP request did not have a request line.")]
    NoRequestLine,
    #[error("The HTTP request-line was not properly formatted.")]
    RequestLineParse,
}

#[derive(Debug, Error)]
pub(crate) enum GrafanaWebhookError {
    #[error("Failed to queue notification. {0}")]
    QueueError(AddNotificationError),
    #[error("JSON from Grafana could not be parsed. {0}")]
    BadJson(serde_json::Error),
    #[error("Wrong method, expected POST but got {0}")]
    WrongMethod(String),
}

#[derive(Debug, Error)]
pub(crate) enum AddNotificationError {
    #[error("Failed to create prowl notification. {0}")]
    Creation(prowl::CreationError),
    #[error("Failed to queue notification to be sent. {0}")]
    Channel(mpsc::error::SendError<Notification>),
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
