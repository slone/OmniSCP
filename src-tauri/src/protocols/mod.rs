pub mod sftp;

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{OmniError, Result};

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
    pub owner: Option<String>,
    pub group: Option<String>,
}

/// Renders Unix mode bits as `drwxr-xr-x`.
pub fn format_permissions(mode: u32, is_dir: bool) -> String {
    const BITS: [(u32, char); 9] = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    let mut s = String::with_capacity(10);
    s.push(if is_dir { 'd' } else { '-' });
    for (bit, c) in BITS {
        s.push(if mode & bit != 0 { c } else { '-' });
    }
    s
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
