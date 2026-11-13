pub mod scp;
pub mod sftp;
pub mod ssh;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::config::Profile;
use crate::error::{OmniError, Result};

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

/// The implementation for a profile's protocol, not connected yet.
pub fn for_profile(params: &ConnectParams) -> Result<Box<dyn Protocol>> {
    match params.profile.protocol {
        ProtocolType::Sftp => Ok(Box::new(sftp::SftpConnection::new(params))),
        ProtocolType::Scp => Ok(Box::new(scp::ScpConnection::new(params))),
        other => Err(OmniError::Protocol(format!("Protocol {other:?} is not supported yet"))),
    }
}

/// What a connection test found, the session being closed again.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeResult {
    /// Where browsing would start.
    pub initial_path: String,
    /// The profile's starting folder could not be listed; the login itself worked.
    pub directory_error: Option<String>,
}

/// Connects with `params`, checks the starting folder and disconnects.
pub fn probe(params: &ConnectParams) -> Result<ProbeResult> {
    let mut conn = for_profile(params)?;
    conn.connect()?;
    let result = match params.profile.remote_directory.as_deref().filter(|d| !d.is_empty()) {
        Some(dir) => ProbeResult {
            initial_path: dir.to_string(),
            directory_error: conn.list_dir(dir).err().map(|e| e.to_string()),
        },
        None => ProbeResult {
            initial_path: conn.realpath(".").unwrap_or_else(|_| "/".to_string()),
            directory_error: None,
        },
    };
    let _ = conn.disconnect();
    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolType {
    Sftp,
    Scp,
    Ftp,
    Ftps,
    WebDav,
    WebDavs,
}

impl ProtocolType {
    pub fn default_port(self) -> u16 {
        match self {
            ProtocolType::Sftp | ProtocolType::Scp => 22,
            ProtocolType::Ftp => 21,
            ProtocolType::Ftps => 990,
            ProtocolType::WebDav => 80,
            ProtocolType::WebDavs => 443,
        }
    }

    pub fn is_encrypted(self) -> bool {
        matches!(
            self,
            ProtocolType::Sftp
                | ProtocolType::Scp
                | ProtocolType::Ftps
                | ProtocolType::WebDavs
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    /// For a symlink, describes the target: a link to a directory can be browsed.
    pub is_dir: bool,
    /// Transfers never follow links to directories (they can loop) and deletes remove the link.
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<i64>,
    pub permissions: Option<String>,
    /// Permission bits, special ones included (0o7777 at most).
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    /// Account names, filled in by `AccountNames::name_entries` when they are known.
    pub owner: Option<String>,
    pub group: Option<String>,
}

/// User and group names by number, read once per connection from the account files.
#[derive(Debug, Default, Clone)]
pub struct AccountNames {
    users: HashMap<u32, String>,
    groups: HashMap<u32, String>,
}

impl AccountNames {
    /// Reads `/etc/passwd` and `/etc/group` formats (`name:x:id:...`); odd lines are ignored.
    pub fn parse(passwd: &str, group: &str) -> Self {
        fn ids(text: &str) -> HashMap<u32, String> {
            let mut map = HashMap::new();
            for line in text.lines().filter(|l| !l.starts_with('#')) {
                let mut fields = line.split(':');
                let (Some(name), Some(_), Some(id)) = (fields.next(), fields.next(), fields.next()) else {
                    continue;
                };
                if let Ok(id) = id.parse() {
                    // The first entry wins, as for the system's own lookups.
                    map.entry(id).or_insert_with(|| name.to_string());
                }
            }
            map
        }
        Self { users: ids(passwd), groups: ids(group) }
    }

    pub fn name_entries(&self, entries: &mut [FileEntry]) {
        for entry in entries {
            if let Some(name) = entry.uid.and_then(|id| self.users.get(&id)) {
                entry.owner = Some(name.clone());
            }
            if let Some(name) = entry.gid.and_then(|id| self.groups.get(&id)) {
                entry.group = Some(name.clone());
            }
        }
    }
}

/// Execute bits and the special bit sharing their column: setuid, setgid, sticky.
const EXEC_COLUMNS: [(u32, u32, char); 3] = [(0o100, 0o4000, 's'), (0o010, 0o2000, 's'), (0o001, 0o1000, 't')];

/// Renders Unix mode bits as `drwxr-xr-x`, with `s`/`S`/`t`/`T` for the special bits as `ls` does.
pub fn format_permissions(mode: u32, is_dir: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_dir { 'd' } else { '-' });
    for (shift, (exec, special, letter)) in [6, 3, 0].into_iter().zip(EXEC_COLUMNS) {
        s.push(if mode & (0o4 << shift) != 0 { 'r' } else { '-' });
        s.push(if mode & (0o2 << shift) != 0 { 'w' } else { '-' });
        s.push(match (mode & exec != 0, mode & special != 0) {
            (true, true) => letter,
            (false, true) => letter.to_ascii_uppercase(),
            (true, false) => 'x',
            (false, false) => '-',
        });
    }
    s
}

/// Reads back what `format_permissions` (or `ls -l`) writes; anything after the 10th character is ignored.
pub fn parse_permissions(text: &str) -> u32 {
    let chars: Vec<char> = text.chars().skip(1).take(9).collect();
    let mut mode = 0;
    for (column, (exec, special, _)) in EXEC_COLUMNS.into_iter().enumerate() {
        let shift = 6 - 3 * column as u32;
        let at = |i: usize| chars.get(column * 3 + i).copied().unwrap_or('-');
        if at(0) == 'r' { mode |= 0o4 << shift; }
        if at(1) == 'w' { mode |= 0o2 << shift; }
        match at(2) {
            'x' => mode |= exec,
            's' | 't' => mode |= exec | special,
            'S' | 'T' => mode |= special,
            _ => {}
        }
    }
    mode
}

/// Sort: directories first, then case-insensitive alphabetical.
pub fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Receives the number of bytes copied since the previous call; returning `false` cancels.
pub type Progress<'a> = &'a mut dyn FnMut(u64) -> bool;

const COPY_BUFFER_SIZE: usize = 256 * 1024;

/// Copies `reader` into `writer`, reporting progress after each chunk.
pub fn copy_with_progress(
    reader: &mut impl Read,
    writer: &mut impl Write,
    progress: Progress,
) -> std::result::Result<(), CopyError> {
    let mut buf = vec![0u8; COPY_BUFFER_SIZE];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(CopyError::Read(e)),
        };
        writer.write_all(&buf[..n]).map_err(CopyError::Write)?;
        if !progress(n as u64) {
            return Err(CopyError::Cancelled);
        }
    }
    writer.flush().map_err(CopyError::Write)
}

#[derive(Debug)]
pub enum CopyError {
    Read(std::io::Error),
    Write(std::io::Error),
    Cancelled,
}

/// Joins a remote (always `/`-separated) directory and a file name.
pub fn remote_join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Parent of a remote path, `None` at the root.
pub fn remote_parent(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rfind('/') {
        Some(0) => Some("/".to_string()),
        Some(i) => Some(trimmed[..i].to_string()),
        None => None,
    }
}

/// Last segment of a remote path.
pub fn remote_file_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// Short hidden name for data being written, renamed over the destination once complete.
pub fn temp_file_name() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(".omniscp-{millis:x}-{count:x}.part")
}

/// Where to write data destined for `remote`, in the same folder so it can be renamed over it.
pub fn temp_path_beside(remote: &str) -> String {
    match remote_parent(remote) {
        Some(parent) => remote_join(&parent, &temp_file_name()),
        None => temp_file_name(),
    }
}

/// Writes through a temporary file and only then replaces `local`, keeping the
/// permissions of the file it replaces. `mtime` is the source's date, which sync compares.
pub fn write_local_atomically(
    local: &Path,
    mtime: Option<u64>,
    copy: impl FnOnce(&mut std::fs::File) -> Result<()>,
) -> Result<()> {
    let temp = local.with_file_name(temp_file_name());
    let mut file = std::fs::File::create(&temp)
        .map_err(|e| OmniError::io_context(format!("Failed to create {}", temp.display()), e))?;
    let write_err = |e| OmniError::io_context(format!("Failed to write {}", local.display()), e);

    let result = copy(&mut file).and_then(|()| {
        // Not worth losing the download over.
        if let Some(mtime) = mtime {
            let _ = file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(mtime));
        }
        file.sync_all().map_err(write_err)
    });
    // Windows cannot rename or remove a file that is still open.
    drop(file);

    let result = result.and_then(|()| {
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

/// Rejects names that would escape the target directory or are not valid file names.
pub fn validate_file_name(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(OmniError::Config(format!("Invalid file name: \"{name}\"")));
    }
    Ok(())
}

/// First free variant of `name` ("report (1).pdf", "report (2).pdf", ...).
pub fn unique_name(name: &str, exists: impl Fn(&str) -> bool) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| !exists(candidate))
        .expect("unbounded range always yields a free name")
}

pub trait Protocol: Send + Sync {
    fn connect(&mut self) -> Result<()>;
    fn disconnect(&mut self) -> Result<()>;
    /// Asks the server, so this can block; a session can die without any local sign.
    fn is_alive(&self) -> bool;
    /// The absolute form of a folder's path; `.` gives the initial folder.
    fn realpath(&self, path: &str) -> Result<String>;
    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>>;
    /// The destination only changes once the data is complete: a failed or
    /// cancelled copy leaves any existing file untouched.
    fn upload(&self, local: &Path, remote: &str, progress: Progress) -> Result<()>;
    /// Same guarantee as `upload`.
    fn download(&self, remote: &str, local: &Path, progress: Progress) -> Result<()>;
    fn mkdir(&self, path: &str) -> Result<()>;
    fn rename(&self, from: &str, to: &str) -> Result<()>;
    /// Removes a directory only if it is empty.
    fn remove_empty_dir(&self, path: &str) -> Result<()>;
    /// Deletes a file, or a directory and everything it contains.
    fn delete(&self, path: &str) -> Result<()>;
    fn stat(&self, path: &str) -> Result<FileEntry>;
    fn exists(&self, path: &str) -> bool {
        self.stat(path).is_ok()
    }
    /// Sets the permission bits (at most 0o7777) of one file or folder, never recursively.
    fn set_permissions(&self, path: &str, mode: u32) -> Result<()>;
    /// Where a symlink points, as stored in the link.
    fn read_link(&self, path: &str) -> Result<String>;
    /// The server's account names; empty when they cannot be read (a chroot, for instance).
    fn account_names(&self) -> AccountNames {
        AccountNames::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_round_trip_with_special_bits() {
        for (mode, text) in [
            (0o755, "-rwxr-xr-x"),
            (0o644, "-rw-r--r--"),
            (0o4755, "-rwsr-xr-x"),
            (0o2750, "-rwxr-s---"),
            (0o1777, "-rwxrwxrwt"),
            (0o4644, "-rwSr--r--"),
            (0o1640, "-rw-r----T"),
            (0, "----------"),
        ] {
            assert_eq!(format_permissions(mode, false), text);
            assert_eq!(parse_permissions(text), mode, "{text}");
        }
        assert_eq!(parse_permissions("-rw-r--r--+"), 0o644);
        assert_eq!(parse_permissions("-rw-r--r--@"), 0o644);
    }

    #[test]
    fn account_names_from_system_files() {
        let names = AccountNames::parse(
            "# comment\nroot:x:0:0:root:/root:/bin/sh\ndeploy:x:1000:1000::/home/deploy:/bin/bash\nbroken\ndup:x:1000:1:::\n",
            "root:x:0:\nwww-data:x:33:deploy\n",
        );
        let entry = |uid, gid| FileEntry {
            name: "f".into(),
            path: "/f".into(),
            is_dir: false,
            is_symlink: false,
            size: 0,
            modified: None,
            permissions: None,
            mode: None,
            uid: Some(uid),
            gid: Some(gid),
            owner: None,
            group: None,
        };
        let mut entries = [entry(1000, 33), entry(4242, 4242)];
        names.name_entries(&mut entries);
        assert_eq!((entries[0].owner.as_deref(), entries[0].group.as_deref()), (Some("deploy"), Some("www-data")));
        assert_eq!((entries[1].owner.as_deref(), entries[1].group.as_deref()), (None, None));
    }

    #[test]
    fn remote_paths() {
        assert_eq!(remote_join("/", "a"), "/a");
        assert_eq!(remote_join("/home/u", "a"), "/home/u/a");
        assert_eq!(remote_parent("/home/u/"), Some("/home".into()));
        assert_eq!(remote_parent("/home"), Some("/".into()));
        assert_eq!(remote_parent("/"), None);
        assert_eq!(remote_file_name("/home/u/file.txt"), "file.txt");
    }

    #[test]
    fn unique_name_keeps_extension() {
        let taken = ["a.txt", "a (1).txt"];
        assert_eq!(unique_name("a.txt", |n| taken.contains(&n)), "a (2).txt");
        assert_eq!(unique_name(".bashrc", |_| false), ".bashrc (1)");
        assert_eq!(unique_name("dir", |_| false), "dir (1)");
    }

    #[test]
    fn file_name_validation() {
        assert!(validate_file_name("ok.txt").is_ok());
        for bad in ["", " ", ".", "..", "a/b", "a\\b"] {
            assert!(validate_file_name(bad).is_err(), "{bad:?} should be rejected");
        }
    }
}
