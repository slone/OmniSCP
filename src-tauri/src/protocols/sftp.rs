use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use ssh2::{CheckResult, ErrorCode, HashType, KnownHostFileKind, Session, Sftp};

use crate::config::{AuthMethod, Profile};
use crate::error::{OmniError, Result};
use crate::protocols::{
    CopyError, FileEntry, Progress, Protocol, copy_with_progress, format_permissions, remote_join,
    remote_parent, sort_entries, temp_file_name,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Applies to every blocking libssh2 call (handshake, auth, SFTP operations).
const SESSION_TIMEOUT_MS: u32 = 30_000;

// SFTP status codes (draft-ietf-secsh-filexfer-02, section 7).
const SFTP_NO_SUCH_FILE: i32 = 2;
const SFTP_PERMISSION_DENIED: i32 = 3;

/// Everything needed to open a connection; cloned to open extra sessions (transfers).
#[derive(Clone)]
pub struct ConnectParams {
    pub profile: Profile,
    pub password: Option<String>,
    /// App-managed known_hosts file, where host keys accepted by the user are recorded.
    pub known_hosts_path: PathBuf,
    /// Fingerprint the user explicitly accepted for an unknown host.
    pub trusted_fingerprint: Option<String>,
}

/// Maps an SFTP failure to a permission error when the server says so.
fn sftp_err(context: String, err: ssh2::Error) -> OmniError {
    match err.code() {
        ErrorCode::SFTP(SFTP_PERMISSION_DENIED) => OmniError::Permission(format!("{context}: {err}")),
        ErrorCode::SFTP(SFTP_NO_SUCH_FILE) => {
            OmniError::Protocol(format!("{context}: no such file or directory"))
        }
        _ => OmniError::Protocol(format!("{context}: {err}")),
    }
}

pub struct SftpConnection {
    host: String,
    port: u16,
    username: String,
    auth_method: AuthMethod,
    password: Option<String>,
    known_hosts_path: PathBuf,
    trusted_fingerprint: Option<String>,
    sftp: Option<Sftp>,
    session: Option<Session>,
}

impl SftpConnection {
    pub fn new(params: &ConnectParams) -> Self {
        Self {
            host: params.profile.host.clone(),
            port: params.profile.port,
            username: params.profile.username.clone(),
            auth_method: params.profile.auth_method.clone(),
            password: params.password.clone(),
            known_hosts_path: params.known_hosts_path.clone(),
            trusted_fingerprint: params.trusted_fingerprint.clone(),
            sftp: None,
            session: None,
        }
    }

    fn sftp(&self) -> Result<&Sftp> {
        self.sftp
            .as_ref()
            .ok_or_else(|| OmniError::Protocol("Not connected".into()))
    }

    pub fn realpath(&self, path: &str) -> Result<String> {
        let resolved = self
            .sftp()?
            .realpath(Path::new(path))
            .map_err(|e| sftp_err(format!("realpath failed for {path}"), e))?;
        Ok(resolved.to_string_lossy().into_owned())
    }

    /// Builds an entry from `lstat` data, describing the target of a symlink.
    fn to_entry(&self, path: PathBuf, lstat: &ssh2::FileStat) -> Result<FileEntry> {
        if !lstat.file_type().is_symlink() {
            return Ok(file_stat_to_entry(path, lstat, false));
        }
        // A dangling link has no target to describe.
        let target = self.sftp()?.stat(&path).ok();
        Ok(file_stat_to_entry(path, target.as_ref().unwrap_or(lstat), true))
    }

    /// Moves the finished temporary file onto `target`. SFTP v3 servers refuse to
    /// rename over an existing file, so the old one is set aside until the new one is in place.
    fn install_upload(&self, temp: &str, target: &str, target_exists: bool) -> Result<()> {
        if !target_exists {
            return self.rename(temp, target);
        }
        let backup = temp_path_beside(target);
        self.rename(target, &backup)?;
        if let Err(e) = self.rename(temp, target) {
            let _ = self.rename(&backup, target);
            return Err(e);
        }
        // The new file is in place; a leftover backup is not worth failing the transfer.
        let _ = self.sftp()?.unlink(Path::new(&backup));
        Ok(())
    }

    fn open_tcp(&self) -> Result<TcpStream> {
        let addr = format!("{}:{}", self.host, self.port);
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
    fn verify_host_key(&self, session: &Session) -> Result<()> {
        let (key, key_type) = session
            .host_key()
            .ok_or_else(|| OmniError::Protocol("Server did not provide a host key".into()))?;
        let fingerprint = session
            .host_key_hash(HashType::Sha256)
            .map(|hash| format!("SHA256:{}", base64_no_pad(hash)))
            .ok_or_else(|| OmniError::Protocol("Failed to hash host key".into()))?;

        let known_hosts_err =
            |e: ssh2::Error| OmniError::Protocol(format!("known_hosts error: {e}"));

        let mut found = false;
        for file in [home_dir().map(|h| h.join(".ssh/known_hosts")), Some(self.known_hosts_path.clone())]
            .into_iter()
            .flatten()
            .filter(|f| f.exists())
        {
            let mut known_hosts = session.known_hosts().map_err(known_hosts_err)?;
            // A partially unparsable file still yields the entries read before the error.
            let _ = known_hosts.read_file(&file, KnownHostFileKind::OpenSSH);
            match known_hosts.check_port(&self.host, self.port, key) {
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

        if self.trusted_fingerprint.as_deref() != Some(fingerprint.as_str()) {
            return Err(OmniError::HostKeyUnknown(fingerprint));
        }

        let mut known_hosts = session.known_hosts().map_err(known_hosts_err)?;
        if self.known_hosts_path.exists() {
            known_hosts
                .read_file(&self.known_hosts_path, KnownHostFileKind::OpenSSH)
                .map_err(known_hosts_err)?;
        }
        let host_entry = if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        };
        known_hosts
            .add(&host_entry, key, "added by OmniSCP", key_type.into())
            .map_err(known_hosts_err)?;
        if let Some(parent) = self.known_hosts_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        known_hosts
            .write_file(&self.known_hosts_path, KnownHostFileKind::OpenSSH)
            .map_err(known_hosts_err)?;
        Ok(())
    }
}

fn temp_path_beside(remote: &str) -> String {
    match remote_parent(remote) {
        Some(parent) => remote_join(&parent, &temp_file_name()),
        None => temp_file_name(),
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

fn file_stat_to_entry(path: PathBuf, stat: &ssh2::FileStat, is_symlink: bool) -> FileEntry {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    let is_dir = stat
        .perm
        .map(|p| p & 0o170000 == 0o040000)
        .unwrap_or(false);

    let permissions = stat.perm.map(|p| format_permissions(p & 0o7777, is_dir));

    FileEntry {
        name,
        path,
        is_dir,
        is_symlink,
        size: stat.size.unwrap_or(0),
        modified: stat.mtime.map(|t| t as i64),
        permissions,
        owner: stat.uid.map(|u| u.to_string()),
        group: stat.gid.map(|g| g.to_string()),
    }
}

impl Protocol for SftpConnection {
    fn connect(&mut self) -> Result<()> {
        let tcp = self.open_tcp()?;

        let mut session = Session::new()
            .map_err(|e| OmniError::Protocol(format!("Failed to create SSH session: {e}")))?;
        session.set_tcp_stream(tcp);
        session.set_timeout(SESSION_TIMEOUT_MS);
        session
            .handshake()
            .map_err(|e| OmniError::Protocol(format!("SSH handshake failed: {e}")))?;

        self.verify_host_key(&session)?;

        match &self.auth_method {
            AuthMethod::Password => {
                let password = self
                    .password
                    .as_deref()
                    .ok_or_else(|| OmniError::Auth("Password required but not provided".into()))?;
                session
                    .userauth_password(&self.username, password)
                    .map_err(|e| OmniError::Auth(format!(
                        "Password authentication failed for user \"{}\": {e}",
                        self.username
                    )))?;
            }
            AuthMethod::KeyFile { path } => {
                let key = key_file_path(path)?;
                session
                    .userauth_pubkey_file(&self.username, None, &key, None)
                    .map_err(|e| {
                        OmniError::Auth(format!(
                            "Key file authentication failed for user \"{}\": {e}",
                            self.username
                        ))
                    })?;
            }
            AuthMethod::KeyWithPassphrase { path } => {
                let key = key_file_path(path)?;
                let passphrase = self.password.as_deref().ok_or_else(|| {
                    OmniError::Auth("Passphrase required but not provided".into())
                })?;
                session
                    .userauth_pubkey_file(&self.username, None, &key, Some(passphrase))
                    .map_err(|e| {
                        OmniError::Auth(format!(
                            "Key file authentication with passphrase failed for user \"{}\": {e}",
                            self.username
                        ))
                    })?;
            }
            AuthMethod::Agent => {
                session
                    .userauth_agent(&self.username)
                    .map_err(|e| OmniError::Auth(format!(
                        "SSH agent authentication failed for user \"{}\": {e}",
                        self.username
                    )))?;
            }
        }

        if !session.authenticated() {
            return Err(OmniError::Auth("Authentication failed".into()));
        }

        let sftp = session
            .sftp()
            .map_err(|e| OmniError::Protocol(format!("SFTP subsystem error: {e}")))?;

        self.sftp = Some(sftp);
        self.session = Some(session);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<()> {
        self.sftp = None;
        if let Some(session) = self.session.take() {
            let _ = session.disconnect(None, "bye", None);
        }
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.realpath(".").is_ok()
    }

    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        let entries = self
            .sftp()?
            .readdir(Path::new(path))
            .map_err(|e| sftp_err(format!("Failed to list directory {path}"), e))?;

        let mut files = Vec::with_capacity(entries.len());
        for (joined, stat) in entries {
            // ssh2 joins with the local separator, which would be `\` on Windows.
            let name = joined.file_name().unwrap_or_default().to_string_lossy();
            let entry_path = PathBuf::from(remote_join(path, &name));
            files.push(self.to_entry(entry_path, &stat)?);
        }
        sort_entries(&mut files);

        Ok(files)
    }

    fn stat(&self, path: &str) -> Result<FileEntry> {
        let stat = self
            .sftp()?
            .lstat(Path::new(path))
            .map_err(|e| sftp_err(format!("Failed to stat {path}"), e))?;
        self.to_entry(path.into(), &stat)
    }

    fn mkdir(&self, path: &str) -> Result<()> {
        self.sftp()?
            .mkdir(Path::new(path), 0o755)
            .map_err(|e| sftp_err(format!("Failed to create directory {path}"), e))
    }

    fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.sftp()?
            .rename(Path::new(from), Path::new(to), None)
            .map_err(|e| sftp_err(format!("Failed to rename {from} -> {to}"), e))
    }

    fn remove_empty_dir(&self, path: &str) -> Result<()> {
        self.sftp()?
            .rmdir(Path::new(path))
            .map_err(|e| sftp_err(format!("Failed to remove directory {path}"), e))
    }

    fn delete(&self, path: &str) -> Result<()> {
        let sftp = self.sftp()?;
        // lstat: a symlink to a directory is removed as a link, never followed.
        let stat = sftp
            .lstat(Path::new(path))
            .map_err(|e| sftp_err(format!("Failed to stat {path}"), e))?;

        if stat.is_dir() {
            for entry in self.list_dir(path)? {
                self.delete(&entry.path.to_string_lossy())?;
            }
            sftp.rmdir(Path::new(path))
                .map_err(|e| sftp_err(format!("Failed to remove directory {path}"), e))
        } else {
            sftp.unlink(Path::new(path))
                .map_err(|e| sftp_err(format!("Failed to delete {path}"), e))
        }
    }

    fn upload(&self, local: &Path, remote: &str, progress: Progress) -> Result<()> {
        let sftp = self.sftp()?;
        let existing = sftp.lstat(Path::new(remote)).ok();
        if existing.as_ref().is_some_and(|s| s.is_dir()) {
            return Err(OmniError::Transfer(format!("{remote} is a directory")));
        }

        let mut local_file = std::fs::File::open(local)
            .map_err(|e| OmniError::io_context(format!("Failed to open {}", local.display()), e))?;
        let temp = temp_path_beside(remote);
        let mut remote_file = sftp
            .create(Path::new(&temp))
            .map_err(|e| sftp_err(format!("Failed to create a temporary file for {remote}"), e))?;

        let result = copy_with_progress(&mut local_file, &mut remote_file, progress)
            .map_err(|e| match e {
                CopyError::Cancelled => OmniError::Cancelled,
                CopyError::Read(e) => OmniError::io_context(format!("Failed to read {}", local.display()), e),
                CopyError::Write(e) => OmniError::Transfer(format!("Failed to write {remote}: {e}")),
            })
            // Dropping the handle would swallow a failure to commit the last writes.
            .and_then(|()| {
                remote_file
                    .close()
                    .map_err(|e| OmniError::Transfer(format!("Failed to write {remote}: {e}")))
            })
            .and_then(|()| {
                // Keep the source's date (sync compares them) and, for a replaced file, its
                // permissions. Failing to set them is not worth losing the upload.
                let mtime = local_file
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                let perm = sftp.stat(Path::new(remote)).ok().and_then(|s| s.perm).map(|p| p & 0o7777);
                let stat = ssh2::FileStat { size: None, uid: None, gid: None, perm, atime: mtime, mtime };
                let _ = sftp.setstat(Path::new(&temp), stat);
                self.install_upload(&temp, remote, existing.is_some())
            });

        if result.is_err() {
            let _ = sftp.unlink(Path::new(&temp));
        }
        result
    }

    fn download(&self, remote: &str, local: &Path, progress: Progress) -> Result<()> {
        let mut remote_file = self
            .sftp()?
            .open(Path::new(remote))
            .map_err(|e| sftp_err(format!("Failed to open remote file {remote}"), e))?;
        let temp = local.with_file_name(temp_file_name());
        let mut local_file = std::fs::File::create(&temp)
            .map_err(|e| OmniError::io_context(format!("Failed to create {}", temp.display()), e))?;
        let write_err = |e| OmniError::io_context(format!("Failed to write {}", local.display()), e);

        let result = copy_with_progress(&mut remote_file, &mut local_file, progress)
            .map_err(|e| match e {
                CopyError::Cancelled => OmniError::Cancelled,
                CopyError::Read(e) => OmniError::Transfer(format!("Failed to read {remote}: {e}")),
                CopyError::Write(e) => write_err(e),
            })
            .and_then(|()| {
                // Keep the source's date, which sync compares; not worth losing the download over.
                if let Some(mtime) = remote_file.stat().ok().and_then(|s| s.mtime) {
                    let _ = local_file.set_modified(UNIX_EPOCH + Duration::from_secs(mtime));
                }
                local_file.sync_all().map_err(write_err)
            });
        // Windows cannot rename or remove a file that is still open.
        drop(local_file);

        let result = result.and_then(|()| {
            // A replaced file keeps its permissions; failing to copy them is not worth losing the download.
            if let Ok(meta) = std::fs::metadata(local) {
                let _ = std::fs::set_permissions(&temp, meta.permissions());
            }
            std::fs::rename(&temp, local).map_err(write_err)
        });

        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
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
