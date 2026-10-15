use std::fmt;

#[derive(Debug)]
pub enum OmniError {
    Network(String),
    Auth(String),
    Permission(String),
    Protocol(String),
    Transfer(String),
    Config(String),
    Io(std::io::Error),
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
        OmniError::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, OmniError>;
