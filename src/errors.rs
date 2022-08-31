use prowl::Notification;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum RequestError {
    Io(std::io::Error),
    NoMessageBody,
    BadMessage(std::str::Utf8Error),
    BadJson(serde_json::Error),
    QueueError(AddNotificationError),
}

#[derive(Debug)]
pub enum AddNotificationError {
    Add(prowl::AddError),
    Creation(prowl::CreationError),
    Channel(mpsc::error::SendError<Notification>),
}

impl From<prowl::AddError> for AddNotificationError {
    fn from(error: prowl::AddError) -> Self {
        Self::Add(error)
    }
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
