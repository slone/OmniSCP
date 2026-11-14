// What SFTP and SCP share: opening an SSH session, checking the server's
// identity and authenticating.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

use ssh2::{CheckResult, HashType, KnownHostFileKind, Session};

use crate::config::AuthMethod;
use crate::error::{OmniError, Result};
use crate::protocols::ConnectParams;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Applies to every blocking libssh2 call (handshake, auth, SFTP operations).
const SESSION_TIMEOUT_MS: u32 = 30_000;
/// Lets `Protocol::keepalive` send a message when this long has passed without traffic.
const KEEPALIVE_SECS: u32 = 15;

/// Returns an authenticated session on a server whose identity was checked.
pub fn open_session(params: &ConnectParams) -> Result<Session> {
    let tcp = open_tcp(params)?;

    let mut session = Session::new()
        .map_err(|e| OmniError::Protocol(format!("Failed to create SSH session: {e}")))?;
    session.set_tcp_stream(tcp);
    session.set_timeout(SESSION_TIMEOUT_MS);
    session
        .handshake()
        .map_err(|e| OmniError::Protocol(format!("SSH handshake failed: {e}")))?;

    verify_host_key(&session, params)?;
    authenticate(&session, params)?;
    // Only sets the interval: messages go out when a paused transfer calls keepalive_send.
    // Set before the handshake, it breaks the key exchange.
    session.set_keepalive(false, KEEPALIVE_SECS);
    Ok(session)
}

fn open_tcp(params: &ConnectParams) -> Result<TcpStream> {
    let addr = format!("{}:{}", params.profile.host, params.profile.port);
    let candidates = addr
        .to_socket_addrs()
        .map_err(|e| OmniError::Network(format!("Failed to resolve {addr}: {e}")))?;

    let mut last_err = None;
    for candidate in candidates {
        match TcpStream::connect_timeout(&candidate, CONNECT_TIMEOUT) {
            Ok(tcp) => return Ok(tcp),
            Err(e) => last_err = Some(e),
        }
    }
    Err(OmniError::Network(match last_err {
        Some(e) => format!("Failed to connect to {addr}: {e}"),
        None => format!("No address found for {addr}"),
    }))
}

/// Checks the server key against ~/.ssh/known_hosts and the app's own known_hosts.
/// An unknown key is only accepted when the user confirmed its fingerprint beforehand.
fn verify_host_key(session: &Session, params: &ConnectParams) -> Result<()> {
    let host = &params.profile.host;
    let port = params.profile.port;
    let (key, key_type) = session
        .host_key()
        .ok_or_else(|| OmniError::Protocol("Server did not provide a host key".into()))?;
    let fingerprint = session
        .host_key_hash(HashType::Sha256)
        .map(|hash| format!("SHA256:{}", base64_no_pad(hash)))
        .ok_or_else(|| OmniError::Protocol("Failed to hash host key".into()))?;

    let known_hosts_err = |e: ssh2::Error| OmniError::Protocol(format!("known_hosts error: {e}"));

    let mut found = false;
    for file in [home_dir().map(|h| h.join(".ssh/known_hosts")), Some(params.known_hosts_path.clone())]
        .into_iter()
        .flatten()
        .filter(|f| f.exists())
    {
        let mut known_hosts = session.known_hosts().map_err(known_hosts_err)?;
        // A partially unparsable file still yields the entries read before the error.
        let _ = known_hosts.read_file(&file, KnownHostFileKind::OpenSSH);
        match known_hosts.check_port(host, port, key) {
            CheckResult::Match => found = true,
            CheckResult::Mismatch => return Err(OmniError::HostKeyMismatch(fingerprint)),
            CheckResult::NotFound => {}
            CheckResult::Failure => {
                return Err(OmniError::Protocol(format!(
                    "Failed to check host key against {}",
                    file.display()
                )));
            }
        }
    }
    if found {
        return Ok(());
    }

    if params.trusted_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(OmniError::HostKeyUnknown(fingerprint));
    }

    let mut known_hosts = session.known_hosts().map_err(known_hosts_err)?;
    if params.known_hosts_path.exists() {
        known_hosts
            .read_file(&params.known_hosts_path, KnownHostFileKind::OpenSSH)
            .map_err(known_hosts_err)?;
    }
    let host_entry = if port == 22 { host.clone() } else { format!("[{host}]:{port}") };
    known_hosts
        .add(&host_entry, key, "added by OmniSCP", key_type.into())
        .map_err(known_hosts_err)?;
    if let Some(parent) = params.known_hosts_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    known_hosts
        .write_file(&params.known_hosts_path, KnownHostFileKind::OpenSSH)
        .map_err(known_hosts_err)?;
    Ok(())
}

fn authenticate(session: &Session, params: &ConnectParams) -> Result<()> {
    let username = &params.profile.username;
    match &params.profile.auth_method {
        AuthMethod::Password => {
            let password = params
                .password
                .as_deref()
                .ok_or_else(|| OmniError::Auth("Password required but not provided".into()))?;
            session.userauth_password(username, password).map_err(|e| {
                OmniError::Auth(format!("Password authentication failed for user \"{username}\": {e}"))
            })?;
        }
        AuthMethod::KeyFile { path } => {
            let key = key_file_path(path)?;
            session.userauth_pubkey_file(username, None, &key, None).map_err(|e| {
                OmniError::Auth(format!("Key file authentication failed for user \"{username}\": {e}"))
            })?;
        }
        AuthMethod::KeyWithPassphrase { path } => {
            let key = key_file_path(path)?;
            let passphrase = params
                .password
                .as_deref()
                .ok_or_else(|| OmniError::Auth("Passphrase required but not provided".into()))?;
            session.userauth_pubkey_file(username, None, &key, Some(passphrase)).map_err(|e| {
                OmniError::Auth(format!(
                    "Key file authentication with passphrase failed for user \"{username}\": {e}"
                ))
            })?;
        }
        AuthMethod::Agent => {
            session.userauth_agent(username).map_err(|e| {
                OmniError::Auth(format!("SSH agent authentication failed for user \"{username}\": {e}"))
            })?;
        }
    }

    if session.authenticated() {
        Ok(())
    } else {
        Err(OmniError::Auth("Authentication failed".into()))
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Expands a leading `~` to the user's home directory.
fn expand_tilde(path: &str) -> PathBuf {
    let rest = if path == "~" {
        Some("")
    } else {
        path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"))
    };
    match (rest, home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

fn key_file_path(path: &str) -> Result<PathBuf> {
    if path.trim().is_empty() {
        return Err(OmniError::Config("No SSH key file configured for this profile".into()));
    }
    let expanded = expand_tilde(path.trim());
    if !expanded.is_file() {
        return Err(OmniError::Config(format!(
            "SSH key file not found: {}",
            expanded.display()
        )));
    }
    Ok(expanded)
}

/// Standard base64 without padding, as used by OpenSSH fingerprints.
fn base64_no_pad(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, &b)| acc | (b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 0x3f) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_openssh_encoding() {
        assert_eq!(base64_no_pad(b""), "");
        assert_eq!(base64_no_pad(b"f"), "Zg");
        assert_eq!(base64_no_pad(b"fo"), "Zm8");
        assert_eq!(base64_no_pad(b"foo"), "Zm9v");
        assert_eq!(base64_no_pad(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn expand_tilde_only_touches_leading_tilde() {
        let home = home_dir().unwrap();
        assert_eq!(expand_tilde("~/.ssh/id_rsa"), home.join(".ssh/id_rsa"));
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(expand_tilde("/etc/~x"), PathBuf::from("/etc/~x"));
        assert_eq!(expand_tilde("~other/key"), PathBuf::from("~other/key"));
    }
}
