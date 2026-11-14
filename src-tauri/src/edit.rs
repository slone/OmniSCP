//! Editing a remote file with a local application: the file is downloaded to a
//! folder of its own, the user's application opens it, and each save is sent back,
//! unless the server's copy changed meanwhile.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::error::{OmniError, Result};
use crate::protocols::{FileEntry, Protocol, remote_file_name};

pub const CHANGED_EVENT: &str = "remote-edit-changed";
/// How often local copies are checked; a save is sent once it stayed the same for one interval.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Editing is for documents and scripts; bigger files go through the transfer queue.
pub const MAX_EDIT_SIZE: u64 = 100 * 1024 * 1024;
/// Local copies are kept after editing stops, in case a save never reached the server.
const RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalState {
    modified: Option<SystemTime>,
    len: u64,
}

fn local_state(path: &Path) -> Option<LocalState> {
    let meta = std::fs::metadata(path).ok()?;
    Some(LocalState { modified: meta.modified().ok(), len: meta.len() })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RemoteState {
    size: u64,
    modified: Option<i64>,
}

impl From<&FileEntry> for RemoteState {
    fn from(entry: &FileEntry) -> Self {
        Self { size: entry.size, modified: entry.modified }
    }
}

struct Edit {
    remote_path: String,
    local_path: PathBuf,
    /// The server's copy as last downloaded or sent.
    remote: RemoteState,
    /// The local copy as last reported.
    known: Option<LocalState>,
    /// A change seen once, reported if still the same at the next look.
    pending: Option<LocalState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditInfo {
    pub id: String,
    pub remote_path: String,
    pub local_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditChanged {
    pub id: String,
    pub remote_path: String,
}

/// The files being edited on one connection.
#[derive(Default)]
pub struct Edits {
    files: Mutex<HashMap<String, Edit>>,
}

impl Edits {
    fn files(&self) -> std::sync::MutexGuard<'_, HashMap<String, Edit>> {
        self.files.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Downloads `remote_path` into `root/<id>/` and watches the copy. A file already
    /// being edited is not downloaded again, so unsent changes are never overwritten.
    pub fn open(&self, conn: &dyn Protocol, remote_path: &str, root: &Path, id: String) -> Result<EditInfo> {
        if let Some((id, edit)) = self.files().iter().find(|(_, e)| e.remote_path == remote_path) {
            return Ok(info(id, edit));
        }
        let entry = conn.stat(remote_path)?;
        if entry.is_dir {
            return Err(OmniError::Transfer(format!("{remote_path} is a folder")));
        }
        if entry.size > MAX_EDIT_SIZE {
            return Err(OmniError::Transfer(format!(
                "{remote_path} is too large to edit ({} MB at most); download it instead",
                MAX_EDIT_SIZE / (1024 * 1024)
            )));
        }
        let dir = root.join(&id);
        std::fs::create_dir_all(&dir).map_err(|e| OmniError::io_context(dir.display(), e))?;
        let local_path = dir.join(remote_file_name(remote_path));
        conn.download(remote_path, &local_path, &mut |_| true)?;

        let edit = Edit {
            remote_path: remote_path.to_string(),
            remote: RemoteState::from(&entry),
            known: local_state(&local_path),
            pending: None,
            local_path,
        };
        let result = info(&id, &edit);
        self.files().insert(id, edit);
        Ok(result)
    }

    /// Local copies saved since the last report, once the save looks finished.
    pub fn poll(&self) -> Vec<EditChanged> {
        let mut changed = Vec::new();
        for (id, edit) in self.files().iter_mut() {
            // Editors that save by replacing the file leave a moment without one.
            let Some(now) = local_state(&edit.local_path) else { continue };
            if Some(now) == edit.known {
                edit.pending = None;
            } else if Some(now) == edit.pending {
                edit.known = Some(now);
                edit.pending = None;
                changed.push(EditChanged { id: id.clone(), remote_path: edit.remote_path.clone() });
            } else {
                edit.pending = Some(now);
            }
        }
        changed
    }

    /// Sends the local copy back. Without `force`, refuses with `Conflict` if the
    /// server's copy changed or disappeared since it was downloaded or last sent.
    pub fn upload(&self, conn: &dyn Protocol, id: &str, force: bool) -> Result<()> {
        let (remote_path, local_path, expected) = {
            let files = self.files();
            let edit = files
                .get(id)
                .ok_or_else(|| OmniError::Transfer("This file is no longer being edited".into()))?;
            (edit.remote_path.clone(), edit.local_path.clone(), edit.remote)
        };
        if !force {
            match conn.stat(&remote_path) {
                Ok(entry) if RemoteState::from(&entry) == expected => {}
                Ok(_) => {
                    return Err(OmniError::Conflict(format!(
                        "{remote_path} was changed on the server since it was opened"
                    )));
                }
                Err(OmniError::ConnectionLost) => return Err(OmniError::ConnectionLost),
                Err(_) if !conn.exists(&remote_path) => {
                    return Err(OmniError::Conflict(format!("{remote_path} no longer exists on the server")));
                }
                Err(e) => return Err(e),
            }
        }
        conn.upload(&local_path, &remote_path, &mut |_| true)?;
        let after = RemoteState::from(&conn.stat(&remote_path)?);
        if let Some(edit) = self.files().get_mut(id) {
            edit.remote = after;
        }
        Ok(())
    }

    /// Stops watching; the local copy stays where it is.
    pub fn stop(&self, id: &str) {
        self.files().remove(id);
    }
}

fn info(id: &str, edit: &Edit) -> EditInfo {
    EditInfo {
        id: id.to_string(),
        remote_path: edit.remote_path.clone(),
        local_path: edit.local_path.to_string_lossy().into_owned(),
    }
}

/// Removes local copies from editing sessions older than a week.
pub fn clean_old(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > RETENTION);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
