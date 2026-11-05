use serde::Serialize;
use std::fmt;

#[derive(Debug)]
pub enum OmniError {
    Network(String),
    Auth(String),
    Permission(String),
    Protocol(String),
    Transfer(String),
    Config(String),
    /// Server key not in known_hosts; carries the SHA256 fingerprint for the user to confirm.
    HostKeyUnknown(String),
    /// Server key differs from the one recorded in known_hosts; carries the received fingerprint.
    HostKeyMismatch(String),
    /// Operation stopped at the user's request.
    Cancelled,
    /// The session died; the user has to connect again.
    ConnectionLost,
    Io(std::io::Error),
}

impl Serialize for OmniError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("OmniError", 3)?;
        let kind = match self {
            OmniError::Network(_) => "Network",
            OmniError::Auth(_) => "Auth",
            OmniError::Permission(_) => "Permission",
            OmniError::Protocol(_) => "Protocol",
            OmniError::Transfer(_) => "Transfer",
            OmniError::Config(_) => "Config",
            OmniError::HostKeyUnknown(_) => "HostKeyUnknown",
            OmniError::HostKeyMismatch(_) => "HostKeyMismatch",
            OmniError::Cancelled => "Cancelled",
            OmniError::ConnectionLost => "ConnectionLost",
            OmniError::Io(_) => "Io",
        };
        let fingerprint = match self {
            OmniError::HostKeyUnknown(fp) | OmniError::HostKeyMismatch(fp) => Some(fp),
            _ => None,
        };
        state.serialize_field("kind", kind)?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field("fingerprint", &fingerprint)?;
        state.end()
    }
}

impl fmt::Display for OmniError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OmniError::Network(msg) => write!(f, "Network error: {msg}"),
            OmniError::Auth(msg) => write!(f, "Authentication error: {msg}"),
            OmniError::Permission(msg) => write!(f, "Permission denied: {msg}"),
            OmniError::Protocol(msg) => write!(f, "Protocol error: {msg}"),
            OmniError::Transfer(msg) => write!(f, "Transfer error: {msg}"),
            OmniError::Config(msg) => write!(f, "Configuration error: {msg}"),
            OmniError::HostKeyUnknown(fp) => write!(f, "Unknown host key: {fp}"),
            OmniError::HostKeyMismatch(fp) => write!(
                f,
                "Host key mismatch: the server presented {fp}, which differs from the recorded key"
            ),
            OmniError::Cancelled => write!(f, "Cancelled"),
            OmniError::ConnectionLost => write!(f, "Connection to the server was lost"),
            OmniError::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for OmniError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OmniError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for OmniError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::PermissionDenied => OmniError::Permission(err.to_string()),
            _ => OmniError::Io(err),
        }
    }
}

impl OmniError {
    /// Wraps an I/O error with the path or operation it concerns.
    pub fn io_context(context: impl fmt::Display, err: std::io::Error) -> Self {
        let message = format!("{context}: {err}");
        match err.kind() {
            std::io::ErrorKind::PermissionDenied => OmniError::Permission(message),
            kind => OmniError::Io(std::io::Error::new(kind, message)),
        }
    }
}

pub type Result<T> = std::result::Result<T, OmniError>;
