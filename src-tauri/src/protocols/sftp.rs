use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use ssh2::{ErrorCode, Session, Sftp};

use crate::error::{OmniError, Result};
use crate::protocols::ssh::open_session;
use crate::protocols::{
    AccountNames, ConnectParams, CopyError, FileEntry, Progress, Protocol, copy_with_progress, format_permissions,
    remote_join, sort_entries, temp_path_beside, write_local_atomically,
};

// SFTP status codes (draft-ietf-secsh-filexfer-02, section 7).
const SFTP_NO_SUCH_FILE: i32 = 2;
const SFTP_PERMISSION_DENIED: i32 = 3;

/// Account files larger than this are not read for names.
const MAX_ACCOUNT_FILE: u64 = 4 * 1024 * 1024;

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
    params: ConnectParams,
    sftp: Option<Sftp>,
    session: Option<Session>,
}

impl SftpConnection {
    pub fn new(params: &ConnectParams) -> Self {
        Self { params: params.clone(), sftp: None, session: None }
    }

    fn sftp(&self) -> Result<&Sftp> {
        self.sftp
            .as_ref()
            .ok_or_else(|| OmniError::Protocol("Not connected".into()))
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

    let mode = stat.perm.map(|p| p & 0o7777);
    let permissions = mode.map(|m| format_permissions(m, is_dir));

    FileEntry {
        name,
        path,
        is_dir,
        is_symlink,
        size: stat.size.unwrap_or(0),
        modified: stat.mtime.map(|t| t as i64),
        permissions,
        mode,
        uid: stat.uid,
        gid: stat.gid,
        owner: None,
        group: None,
    }
}

impl Protocol for SftpConnection {
    fn connect(&mut self) -> Result<()> {
        let session = open_session(&self.params)?;
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

    fn keepalive(&self) {
        if let Some(session) = &self.session {
            let _ = session.keepalive_send();
        }
    }

    fn is_alive(&self) -> bool {
        self.realpath(".").is_ok()
    }

    fn realpath(&self, path: &str) -> Result<String> {
        let resolved = self
            .sftp()?
            .realpath(Path::new(path))
            .map_err(|e| sftp_err(format!("realpath failed for {path}"), e))?;
        Ok(resolved.to_string_lossy().into_owned())
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

    fn set_permissions(&self, path: &str, mode: u32) -> Result<()> {
        let stat = ssh2::FileStat { size: None, uid: None, gid: None, perm: Some(mode & 0o7777), atime: None, mtime: None };
        self.sftp()?
            .setstat(Path::new(path), stat)
            .map_err(|e| sftp_err(format!("Failed to change the permissions of {path}"), e))
    }

    fn read_link(&self, path: &str) -> Result<String> {
        let target = self
            .sftp()?
            .readlink(Path::new(path))
            .map_err(|e| sftp_err(format!("Failed to read the link {path}"), e))?;
        Ok(target.to_string_lossy().into_owned())
    }

    fn account_names(&self) -> AccountNames {
        let read = |path: &str| -> Option<String> {
            let sftp = self.sftp().ok()?;
            let mut file = sftp.open(Path::new(path)).ok()?;
            let mut data = Vec::new();
            use std::io::Read;
            (&mut file).take(MAX_ACCOUNT_FILE).read_to_end(&mut data).ok()?;
            Some(String::from_utf8_lossy(&data).into_owned())
        };
        AccountNames::parse(&read("/etc/passwd").unwrap_or_default(), &read("/etc/group").unwrap_or_default())
    }

    fn download(&self, remote: &str, local: &Path, progress: Progress) -> Result<()> {
        let mut remote_file = self
            .sftp()?
            .open(Path::new(remote))
            .map_err(|e| sftp_err(format!("Failed to open remote file {remote}"), e))?;
        let mtime = remote_file.stat().ok().and_then(|s| s.mtime);

        write_local_atomically(local, mtime, |local_file| {
            copy_with_progress(&mut remote_file, local_file, progress).map_err(|e| match e {
                CopyError::Cancelled => OmniError::Cancelled,
                CopyError::Read(e) => OmniError::Transfer(format!("Failed to read {remote}: {e}")),
                CopyError::Write(e) => OmniError::io_context(format!("Failed to write {}", local.display()), e),
            })
        })
    }
}
