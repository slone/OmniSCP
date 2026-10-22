// Background transfer queue: one worker thread with its own SSH session, so
// browsing on the main connection stays responsive while files are copied.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::{OmniError, Result};
use crate::protocols::sftp::{ConnectParams, SftpConnection};
use crate::protocols::{Protocol, remote_file_name, remote_join, unique_name};
use crate::transfer::{ConflictStrategy, TransferDirection, TransferStatus};

pub const PROGRESS_EVENT: &str = "transfer-progress";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

pub struct TransferRequest {
    pub id: String,
    pub direction: TransferDirection,
    /// Local path for uploads, remote path for downloads.
    pub source: String,
    /// Directory receiving the item, on the other side.
    pub dest_dir: String,
    pub conflict: ConflictStrategy,
}

#[derive(Clone, Serialize)]
pub struct TransferEvent {
    pub id: String,
    pub status: TransferStatus,
    pub destination: Option<String>,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub error: Option<String>,
}

type CancelFlags = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

pub struct TransferQueue {
    sender: Sender<TransferRequest>,
    cancels: CancelFlags,
}

impl TransferQueue {
    pub fn start(app: AppHandle, params: ConnectParams) -> Self {
        let (sender, receiver) = mpsc::channel();
        let cancels: CancelFlags = Arc::default();
        let worker_cancels = Arc::clone(&cancels);
        std::thread::spawn(move || run_worker(app, params, receiver, worker_cancels));
        Self { sender, cancels }
    }

    pub fn enqueue(&self, request: TransferRequest) -> Result<()> {
        self.flags()
            .insert(request.id.clone(), Arc::new(AtomicBool::new(false)));
        self.sender
            .send(request)
            .map_err(|_| OmniError::Transfer("Transfer worker is not running".into()))
    }

    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.flags().get(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn flags(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.cancels.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for TransferQueue {
    /// Disconnecting cancels everything still queued or running.
    fn drop(&mut self) {
        for flag in self.flags().values() {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

fn run_worker(
    app: AppHandle,
    params: ConnectParams,
    receiver: Receiver<TransferRequest>,
    cancels: CancelFlags,
) {
    let mut conn: Option<SftpConnection> = None;

    // Ends once the queue is dropped and every buffered request was handled.
    for request in receiver {
        let cancel = cancels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&request.id)
            .cloned()
            .unwrap_or_default();

        let mut job = Job {
            app: &app,
            id: &request.id,
            cancel: &cancel,
            destination: None,
            total: 0,
            transferred: 0,
            last_emit: Instant::now(),
        };

        let outcome = if cancel.load(Ordering::Relaxed) {
            Err(OmniError::Cancelled)
        } else {
            ensure_connected(&mut conn, &params).and_then(|c| job.run(c, &request))
        };

        let (status, error) = match outcome {
            Ok(status) => (status, None),
            Err(OmniError::Cancelled) => (TransferStatus::Cancelled, None),
            Err(e) => {
                // The session may be broken; reconnect for the next job.
                if conn.as_ref().is_some_and(|c| !c.is_connected()) {
                    conn = None;
                }
                (TransferStatus::Failed, Some(e.to_string()))
            }
        };
        job.emit(status, error);

        cancels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&request.id);
    }

    if let Some(mut c) = conn {
        let _ = c.disconnect();
    }
}

fn ensure_connected<'a>(
    conn: &'a mut Option<SftpConnection>,
    params: &ConnectParams,
) -> Result<&'a SftpConnection> {
    if conn.is_none() {
        let mut c = SftpConnection::new(params);
        c.connect()?;
        *conn = Some(c);
    }
    Ok(conn.as_ref().expect("connection set above"))
}

struct Job<'a> {
    app: &'a AppHandle,
    id: &'a str,
    cancel: &'a AtomicBool,
    destination: Option<String>,
    total: u64,
    transferred: u64,
    last_emit: Instant,
}

impl Job<'_> {
    fn emit(&self, status: TransferStatus, error: Option<String>) {
        let _ = self.app.emit(
            PROGRESS_EVENT,
            TransferEvent {
                id: self.id.to_string(),
                status,
                destination: self.destination.clone(),
                total_bytes: self.total,
                transferred_bytes: self.transferred,
                error,
            },
        );
    }

    /// Called after each copied chunk; throttles events and checks for cancellation.
    fn on_progress(&mut self, bytes: u64) -> bool {
        self.transferred += bytes;
        if self.last_emit.elapsed() >= PROGRESS_INTERVAL {
            self.last_emit = Instant::now();
            self.emit(TransferStatus::InProgress, None);
        }
        !self.cancel.load(Ordering::Relaxed)
    }

    fn check_cancel(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(OmniError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn run(&mut self, conn: &SftpConnection, request: &TransferRequest) -> Result<TransferStatus> {
        match request.direction {
            TransferDirection::Upload => self.upload(conn, request),
            TransferDirection::Download => self.download(conn, request),
        }
    }

    fn upload(&mut self, conn: &SftpConnection, request: &TransferRequest) -> Result<TransferStatus> {
        let source = PathBuf::from(&request.source);
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| OmniError::Transfer(format!("Invalid source path {}", request.source)))?;

        let exists = |n: &str| conn.exists(&remote_join(&request.dest_dir, n));
        let name = match resolve_conflict(&name, request.conflict, exists) {
            Some(name) => name,
            None => return Ok(TransferStatus::Skipped),
        };
        let dest = remote_join(&request.dest_dir, &name);
        self.destination = Some(dest.clone());

        self.total = local_tree_size(&source)?;
        self.emit(TransferStatus::InProgress, None);
        self.upload_tree(conn, &source, &dest)?;
        Ok(TransferStatus::Completed)
    }

    fn upload_tree(&mut self, conn: &SftpConnection, local: &Path, remote: &str) -> Result<()> {
        self.check_cancel()?;
        let meta = std::fs::symlink_metadata(local)
            .map_err(|e| OmniError::io_context(local.display(), e))?;

        if meta.is_dir() {
            if !conn.exists(remote) {
                conn.mkdir(remote)?;
            }
            for entry in std::fs::read_dir(local).map_err(|e| OmniError::io_context(local.display(), e))? {
                let entry = entry?;
                let child = remote_join(remote, &entry.file_name().to_string_lossy());
                self.upload_tree(conn, &entry.path(), &child)?;
            }
            Ok(())
        } else if meta.is_symlink() && std::fs::metadata(local).is_ok_and(|m| m.is_dir()) {
            // Never follow directory symlinks: they can loop.
            Ok(())
        } else {
            conn.upload(local, remote, &mut |n| self.on_progress(n))
        }
    }

    fn download(&mut self, conn: &SftpConnection, request: &TransferRequest) -> Result<TransferStatus> {
        let name = remote_file_name(&request.source).to_string();
        let dest_dir = PathBuf::from(&request.dest_dir);

        let exists = |n: &str| dest_dir.join(n).symlink_metadata().is_ok();
        let name = match resolve_conflict(&name, request.conflict, exists) {
            Some(name) => name,
            None => return Ok(TransferStatus::Skipped),
        };
        let dest = dest_dir.join(&name);
        self.destination = Some(dest.to_string_lossy().into_owned());

        let entry = conn.stat(&request.source)?;
        self.total = remote_tree_size(conn, &request.source, entry.is_dir, entry.size)?;
        self.emit(TransferStatus::InProgress, None);
        self.download_tree(conn, &request.source, entry.is_dir, &dest)?;
        Ok(TransferStatus::Completed)
    }

    fn download_tree(&mut self, conn: &SftpConnection, remote: &str, is_dir: bool, local: &Path) -> Result<()> {
        self.check_cancel()?;
        if is_dir {
            std::fs::create_dir_all(local).map_err(|e| OmniError::io_context(local.display(), e))?;
            for entry in conn.list_dir(remote)? {
                let child_remote = entry.path.to_string_lossy();
                self.download_tree(conn, &child_remote, entry.is_dir, &local.join(&entry.name))?;
            }
            Ok(())
        } else {
            conn.download(remote, local, &mut |n| self.on_progress(n))
        }
    }
}

/// Returns the name to write to, or `None` to skip.
fn resolve_conflict(name: &str, strategy: ConflictStrategy, exists: impl Fn(&str) -> bool) -> Option<String> {
    if !exists(name) {
        return Some(name.to_string());
    }
    match strategy {
        ConflictStrategy::Overwrite => Some(name.to_string()),
        ConflictStrategy::Rename => Some(unique_name(name, exists)),
        // The frontend resolves "Ask" before enqueueing; never overwrite silently.
        ConflictStrategy::Skip | ConflictStrategy::Ask => None,
    }
}

fn local_tree_size(path: &Path) -> Result<u64> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| OmniError::io_context(path.display(), e))?;
    if !meta.is_dir() {
        return Ok(if meta.is_symlink() {
            std::fs::metadata(path).map(|m| if m.is_dir() { 0 } else { m.len() }).unwrap_or(0)
        } else {
            meta.len()
        });
    }
    let mut total = 0;
    for entry in std::fs::read_dir(path).map_err(|e| OmniError::io_context(path.display(), e))? {
        total += local_tree_size(&entry?.path())?;
    }
    Ok(total)
}

fn remote_tree_size(conn: &SftpConnection, path: &str, is_dir: bool, size: u64) -> Result<u64> {
    if !is_dir {
        return Ok(size);
    }
    let mut total = 0;
    for entry in conn.list_dir(path)? {
        total += remote_tree_size(conn, &entry.path.to_string_lossy(), entry.is_dir, entry.size)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_resolution() {
        let taken = |n: &str| n == "a.txt";
        assert_eq!(resolve_conflict("b.txt", ConflictStrategy::Skip, taken), Some("b.txt".into()));
        assert_eq!(resolve_conflict("a.txt", ConflictStrategy::Skip, taken), None);
        assert_eq!(resolve_conflict("a.txt", ConflictStrategy::Ask, taken), None);
        assert_eq!(resolve_conflict("a.txt", ConflictStrategy::Overwrite, taken), Some("a.txt".into()));
        assert_eq!(resolve_conflict("a.txt", ConflictStrategy::Rename, taken), Some("a (1).txt".into()));
    }
}
