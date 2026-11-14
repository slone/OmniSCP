use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

/// What went wrong, as a code the interface translates (`src/i18n`), with the
/// values it mentions. Text coming from the system, the server or a library
/// cannot be translated and travels apart, as `detail`.
#[derive(Debug, Clone, Serialize)]
pub struct Msg {
    pub code: &'static str,
    pub params: BTreeMap<&'static str, String>,
    pub detail: Option<String>,
    /// The error this one wraps, translated on its own.
    pub cause: Option<Box<OmniError>>,
}

impl Msg {
    pub fn new(code: &'static str) -> Self {
        Self { code, params: BTreeMap::new(), detail: None, cause: None }
    }

    pub fn with(mut self, key: &'static str, value: impl fmt::Display) -> Self {
        self.params.insert(key, value.to_string());
        self
    }

    pub fn detail(mut self, detail: impl fmt::Display) -> Self {
        self.detail = Some(detail.to_string());
        self
    }

    pub fn cause(mut self, cause: OmniError) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }

    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
    }
}

/// For logs and tests only; users see the translation.
impl fmt::Display for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        for (key, value) in &self.params {
            write!(f, " {key}={value:?}")?;
        }
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        if let Some(cause) = &self.cause {
            write!(f, ": {cause}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum OmniError {
    Network(Msg),
    Auth(Msg),
    Permission(Msg),
    Protocol(Msg),
    Transfer(Msg),
    Config(Msg),
    /// Server key not in known_hosts; carries the SHA256 fingerprint for the user to confirm.
    HostKeyUnknown(String),
    /// Server key differs from the one recorded in known_hosts; carries the received fingerprint.
    HostKeyMismatch(String),
    /// The target changed since the user last saw it; the operation was not done.
    Conflict(Msg),
    /// Operation stopped at the user's request.
    Cancelled,
    /// The session died; the user has to connect again.
    ConnectionLost,
    /// A local file operation failed for another reason than permissions.
    Io(Msg),
}

impl OmniError {
    pub fn kind(&self) -> &'static str {
        match self {
            OmniError::Network(_) => "Network",
            OmniError::Auth(_) => "Auth",
            OmniError::Permission(_) => "Permission",
            OmniError::Protocol(_) => "Protocol",
            OmniError::Transfer(_) => "Transfer",
            OmniError::Config(_) => "Config",
            OmniError::HostKeyUnknown(_) => "HostKeyUnknown",
            OmniError::HostKeyMismatch(_) => "HostKeyMismatch",
            OmniError::Conflict(_) => "Conflict",
            OmniError::Cancelled => "Cancelled",
            OmniError::ConnectionLost => "ConnectionLost",
            OmniError::Io(_) => "Io",
        }
    }

    pub fn msg(&self) -> Option<&Msg> {
        match self {
            OmniError::Network(m)
            | OmniError::Auth(m)
            | OmniError::Permission(m)
            | OmniError::Protocol(m)
            | OmniError::Transfer(m)
            | OmniError::Config(m)
            | OmniError::Conflict(m)
            | OmniError::Io(m) => Some(m),
            _ => None,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            OmniError::HostKeyUnknown(_) => "hostKey.unknown",
            OmniError::HostKeyMismatch(_) => "hostKey.mismatch",
            OmniError::Cancelled => "cancelled",
            OmniError::ConnectionLost => "connectionLost",
            other => other.msg().map_or("unknown", |m| m.code),
        }
    }

    /// Wraps an I/O error with the path it concerns.
    pub fn io_context(path: impl fmt::Display, err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let code = match err.kind() {
            ErrorKind::PermissionDenied => "io.permission",
            ErrorKind::NotFound => "io.notFound",
            ErrorKind::AlreadyExists => "io.exists",
            ErrorKind::DirectoryNotEmpty => "io.notEmpty",
            ErrorKind::IsADirectory => "io.isDirectory",
            ErrorKind::StorageFull => "io.full",
            _ => "io.failed",
        };
        let msg = Msg::new(code).with("path", path).detail(err);
        if code == "io.permission" { OmniError::Permission(msg) } else { OmniError::Io(msg) }
    }
}

impl Serialize for OmniError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("OmniError", 7)?;
        let fingerprint = match self {
            OmniError::HostKeyUnknown(fp) | OmniError::HostKeyMismatch(fp) => Some(fp),
            _ => None,
        };
        let msg = self.msg();
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("params", &msg.map(|m| &m.params))?;
        state.serialize_field("detail", &msg.and_then(|m| m.detail.as_ref()))?;
        state.serialize_field("cause", &msg.and_then(|m| m.cause.as_ref()))?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field("fingerprint", &fingerprint)?;
        state.end()
    }
}

impl fmt::Display for OmniError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OmniError::HostKeyUnknown(fp) => write!(f, "hostKey.unknown {fp}"),
            OmniError::HostKeyMismatch(fp) => write!(f, "hostKey.mismatch {fp}"),
            OmniError::Cancelled => write!(f, "cancelled"),
            OmniError::ConnectionLost => write!(f, "connectionLost"),
            other => match other.msg() {
                Some(msg) => write!(f, "{}: {msg}", other.kind()),
                None => write!(f, "{}", other.kind()),
            },
        }
    }
}

impl std::error::Error for OmniError {}

impl From<std::io::Error> for OmniError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::PermissionDenied => OmniError::Permission(Msg::new("io.permissionNoPath").detail(err)),
            _ => OmniError::Io(Msg::new("io.failedNoPath").detail(err)),
        }
    }
}

pub type Result<T> = std::result::Result<T, OmniError>;

#[cfg(test)]
mod tests {
    /// Every code built in the backend has a translation in each language.
    #[test]
    fn every_error_code_is_translated() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut codes = BTreeSet::new();
        collect_codes(&root.join("src"), &mut codes);
        for fixed in ["hostKey.unknown", "hostKey.mismatch", "cancelled", "connectionLost"] {
            codes.insert(fixed.to_string());
        }
        assert!(codes.len() > 50, "found only {codes:?}");

        for lang in ["fr", "en"] {
            let path = root.join(format!("../src/i18n/{lang}.ts"));
            let source = std::fs::read_to_string(&path).unwrap();
            let errors = &source[source.find("errors: {").expect("an errors section")..];
            let missing: Vec<_> = codes
                .iter()
                .filter(|code| !errors.contains(&format!("'{code}':")))
                .collect();
            assert!(missing.is_empty(), "{lang}.ts misses {missing:?}");
        }
    }

    use std::collections::BTreeSet;
    use std::path::Path;

    fn collect_codes(dir: &Path, codes: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_codes(&path, codes);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                for (pattern, prefix) in [("Msg::new(\"", ""), ("=> \"io.", "io.")] {
                    for part in source.split(pattern).skip(1) {
                        let code = &part[..part.find('"').unwrap()];
                        codes.insert(format!("{prefix}{code}"));
                    }
                }
            }
        }
    }
}
