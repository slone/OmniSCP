// Background transfer queue: worker threads, each with its own session, so
// browsing on the main connection stays responsive while files are copied.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{Msg, OmniError, Result};
use crate::protocols::{ConnectParams, Protocol, for_profile, remote_file_name, remote_join, remote_parent, unique_name};
use crate::sync::{self, SyncAction, SyncActionKind};
use crate::transfer::{ConflictStrategy, TransferDirection, TransferStatus};

pub const PROGRESS_EVENT: &str = "transfer-progress";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);
/// How often a paused job checks whether it may go on.
const PAUSE_POLL: Duration = Duration::from_millis(100);
/// A paused job keeps its session open; this stops the server from closing it as idle.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

pub struct TransferRequest {
    pub id: String,
    pub direction: TransferDirection,
    /// Local path for uploads, remote path for downloads.
    pub source: String,
    /// Directory receiving the item, on the other side.
    pub dest_dir: String,
    pub conflict: ConflictStrategy,
    /// Name to use at the destination, when retrying a job that already chose one.
    pub dest_name: Option<String>,
    /// Move: once everything is copied, remove what was copied from the source side.
    /// Nothing is removed if any item failed or was left out.
    pub delete_source: bool,
}

/// Executes actions of a plan the user reviewed; shows as one job in the queue.
pub struct SyncRequest {
    pub id: String,
    pub local_root: String,
    pub remote_root: String,
    pub actions: Vec<SyncAction>,
}

enum Request {
    Transfer(TransferRequest),
    Sync(SyncRequest),
}

impl Request {
    fn id(&self) -> &str {
        match self {
            Request::Transfer(r) => &r.id,
            Request::Sync(r) => &r.id,
        }
    }
}

/// Receives every status change of every job.
pub type EventSink = Arc<dyn Fn(TransferEvent) + Send + Sync>;

/// How many problems a finished job spells out.
const MAX_LISTED_PROBLEMS: usize = 5;

#[derive(Clone, Serialize)]
pub struct TransferEvent {
    pub id: String,
    pub status: TransferStatus,
    pub destination: Option<String>,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    /// Why the job failed, or what went wrong in a job that finished anyway.
    pub errors: Vec<OmniError>,
    /// Problems beyond those listed in `errors`.
    pub more_errors: usize,
}

/// Switches for one job, shared with the worker running it.
#[derive(Default)]
struct JobControl {
    cancel: AtomicBool,
    paused: AtomicBool,
}

#[derive(Default)]
struct State {
    /// Jobs not started yet, in order.
    pending: VecDeque<Request>,
    controls: HashMap<String, Arc<JobControl>>,
    /// The queue was dropped: workers finish what is left (as cancelled) and stop.
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    /// Signalled whenever a job may have become ready to start.
    wake: Condvar,
    sink: EventSink,
    /// The whole queue on hold: nothing starts, and running jobs wait between chunks.
    paused: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Blocks until a job may start; `None` once the queue is closed and empty.
    fn next_request(&self) -> Option<(Request, Arc<JobControl>)> {
        let mut state = self.lock();
        loop {
            let st = &*state;
            let ready = if st.closed {
                if st.pending.is_empty() {
                    return None;
                }
                Some(0)
            } else if self.paused.load(Ordering::Relaxed) {
                None
            } else {
                st.pending
                    .iter()
                    .position(|r| !st.controls.get(r.id()).is_some_and(|c| c.paused.load(Ordering::Relaxed)))
            };
            if let Some(i) = ready {
                let request = state.pending.remove(i).expect("index found above");
                let control = state.controls.get(request.id()).cloned().unwrap_or_default();
                return Some((request, control));
            }
            state = self.wake.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// For a job that has not started: only its status is known.
    fn emit_waiting(&self, id: &str, status: TransferStatus) {
        (self.sink)(TransferEvent {
            id: id.to_string(),
            status,
            destination: None,
            total_bytes: 0,
            transferred_bytes: 0,
            errors: Vec::new(),
            more_errors: 0,
        });
    }
}

pub struct TransferQueue {
    shared: Arc<Shared>,
}

impl TransferQueue {
    /// Runs up to `workers` jobs at once. A worker opens its session on its first job.
    pub fn start(sink: EventSink, params: ConnectParams, workers: u32) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            wake: Condvar::new(),
            sink,
            paused: AtomicBool::new(false),
        });
        for _ in 0..workers.max(1) {
            let (shared, params) = (Arc::clone(&shared), params.clone());
            std::thread::spawn(move || run_worker(shared, params));
        }
        Self { shared }
    }

    pub fn enqueue(&self, request: TransferRequest) -> Result<()> {
        self.send(Request::Transfer(request))
    }

    pub fn enqueue_sync(&self, request: SyncRequest) -> Result<()> {
        self.send(Request::Sync(request))
    }

    fn send(&self, request: Request) -> Result<()> {
        let mut state = self.shared.lock();
        if state.closed {
            return Err(OmniError::Transfer(Msg::new("queue.closed")));
        }
        state.controls.insert(request.id().to_string(), Arc::default());
        state.pending.push_back(request);
        self.shared.wake.notify_all();
        Ok(())
    }

    /// A job that has not started is dropped at once; a running one stops at its next chunk.
    pub fn cancel(&self, id: &str) {
        let mut state = self.shared.lock();
        if let Some(control) = state.controls.get(id) {
            control.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(i) = state.pending.iter().position(|r| r.id() == id) {
            state.pending.remove(i);
            state.controls.remove(id);
            self.shared.emit_waiting(id, TransferStatus::Cancelled);
        }
    }

    /// A waiting job is held back; a running one stops between two chunks, its session kept open.
    pub fn pause(&self, id: &str) {
        let state = self.shared.lock();
        if let Some(control) = state.controls.get(id) {
            control.paused.store(true, Ordering::Relaxed);
            if state.pending.iter().any(|r| r.id() == id) {
                self.shared.emit_waiting(id, TransferStatus::Paused);
            }
        }
    }

    pub fn resume(&self, id: &str) {
        let state = self.shared.lock();
        if let Some(control) = state.controls.get(id) {
            control.paused.store(false, Ordering::Relaxed);
            if state.pending.iter().any(|r| r.id() == id) {
                self.shared.emit_waiting(id, TransferStatus::Pending);
            }
        }
        self.shared.wake.notify_all();
    }

    /// Holds or releases the whole queue, running jobs included.
    pub fn set_paused(&self, paused: bool) {
        let _state = self.shared.lock();
        self.shared.paused.store(paused, Ordering::Relaxed);
        self.shared.wake.notify_all();
    }
}

impl Drop for TransferQueue {
    /// Disconnecting cancels everything still queued or running.
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.closed = true;
        for control in state.controls.values() {
            control.cancel.store(true, Ordering::Relaxed);
        }
        self.shared.wake.notify_all();
    }
}

fn run_worker(shared: Arc<Shared>, params: ConnectParams) {
    let mut conn: Option<Box<dyn Protocol>> = None;

    // Ends once the queue is dropped and every waiting request was handled.
    while let Some((request, control)) = shared.next_request() {
        let mut job = Job {
            sink: &shared.sink,
            id: request.id(),
            control: &control,
            queue_paused: &shared.paused,
            destination: None,
            total: 0,
            transferred: 0,
            last_emit: Instant::now(),
            files_done: 0,
            already_present: 0,
            merge: false,
            failures: Vec::new(),
            skipped_links: Vec::new(),
            copied: Vec::new(),
        };

        let outcome = if job.cancelled() {
            Err(OmniError::Cancelled)
        } else {
            ensure_connected(&mut conn, &params).and_then(|c| job.run(c, &request))
        };

        let (status, errors, more) = match outcome {
            Ok(status) => {
                let (errors, more) = job.problem_summary();
                (status, errors, more)
            }
            Err(OmniError::Cancelled) => (TransferStatus::Cancelled, Vec::new(), 0),
            Err(e) => (TransferStatus::Failed, vec![e], 0),
        };
        job.emit_with(status, errors, more);

        shared.lock().controls.remove(request.id());
    }

    if let Some(mut c) = conn {
        let _ = c.disconnect();
    }
}

fn ensure_connected<'a>(
    conn: &'a mut Option<Box<dyn Protocol>>,
    params: &ConnectParams,
) -> Result<&'a dyn Protocol> {
    // The session may have died since the previous job.
    if conn.as_ref().is_some_and(|c| !c.is_alive()) {
        *conn = None;
    }
    if conn.is_none() {
        let mut c = for_profile(params)?;
        c.connect()?;
        *conn = Some(c);
    }
    Ok(conn.as_deref().expect("connection set above"))
}

struct Job<'a> {
    sink: &'a EventSink,
    id: &'a str,
    control: &'a JobControl,
    queue_paused: &'a AtomicBool,
    destination: Option<String>,
    total: u64,
    transferred: u64,
    last_emit: Instant,
    files_done: u64,
    /// Files a `Merge` found already at the destination.
    already_present: u64,
    merge: bool,
    failures: Vec<OmniError>,
    skipped_links: Vec<String>,
    /// Sources fully copied, children before their folder: what a move removes.
    copied: Vec<(String, bool)>,
}

impl Job<'_> {
    fn emit(&self, status: TransferStatus) {
        self.emit_with(status, Vec::new(), 0);
    }

    fn emit_with(&self, status: TransferStatus, errors: Vec<OmniError>, more_errors: usize) {
        (self.sink)(TransferEvent {
            id: self.id.to_string(),
            status,
            destination: self.destination.clone(),
            total_bytes: self.total,
            transferred_bytes: self.transferred,
            errors,
            more_errors,
        });
    }

    fn cancelled(&self) -> bool {
        self.control.cancel.load(Ordering::Relaxed)
    }

    fn held(&self) -> bool {
        self.control.paused.load(Ordering::Relaxed) || self.queue_paused.load(Ordering::Relaxed)
    }

    /// Waits while the job or the queue is paused. Returns false once the job is cancelled.
    fn wait_if_paused(&mut self, conn: &dyn Protocol) -> bool {
        if self.held() && !self.cancelled() {
            self.emit(TransferStatus::Paused);
            let mut last_keepalive = Instant::now();
            while self.held() && !self.cancelled() {
                std::thread::sleep(PAUSE_POLL);
                if last_keepalive.elapsed() >= KEEPALIVE_INTERVAL {
                    conn.keepalive();
                    last_keepalive = Instant::now();
                }
            }
            if !self.cancelled() {
                self.last_emit = Instant::now();
                self.emit(TransferStatus::InProgress);
            }
        }
        !self.cancelled()
    }

    /// Called after each copied chunk; throttles events, pauses and checks for cancellation.
    fn on_progress(&mut self, conn: &dyn Protocol, bytes: u64) -> bool {
        self.transferred += bytes;
        if self.last_emit.elapsed() >= PROGRESS_INTERVAL {
            self.last_emit = Instant::now();
            self.emit(TransferStatus::InProgress);
        }
        self.wait_if_paused(conn)
    }

    fn check_cancel(&mut self, conn: &dyn Protocol) -> Result<()> {
        if self.wait_if_paused(conn) {
            Ok(())
        } else {
            Err(OmniError::Cancelled)
        }
    }

    /// Keeps going after a failed item, unless nothing further can succeed.
    fn record_failure(&mut self, conn: &dyn Protocol, err: OmniError) -> Result<()> {
        if matches!(err, OmniError::Cancelled) || !conn.is_alive() {
            return Err(err);
        }
        self.failures.push(err);
        Ok(())
    }

    fn final_status(&self) -> TransferStatus {
        if self.failures.is_empty() && self.skipped_links.is_empty() {
            if self.files_done == 0 && self.already_present > 0 {
                TransferStatus::Skipped
            } else {
                TransferStatus::Completed
            }
        } else if self.files_done == 0 && !self.failures.is_empty() {
            TransferStatus::Failed
        } else {
            TransferStatus::CompletedWithWarnings
        }
    }

    /// The first problems, and how many more there were.
    fn problem_summary(&self) -> (Vec<OmniError>, usize) {
        let skipped = self.skipped_links.iter().map(|path| link_not_followed(path));
        let mut problems: Vec<OmniError> = self.failures.iter().cloned().chain(skipped).collect();
        let more = problems.len().saturating_sub(MAX_LISTED_PROBLEMS);
        problems.truncate(MAX_LISTED_PROBLEMS);
        (problems, more)
    }

    fn run(&mut self, conn: &dyn Protocol, request: &Request) -> Result<TransferStatus> {
        match request {
            Request::Sync(request) => self.sync(conn, request),
            Request::Transfer(request) => {
                self.merge = request.conflict == ConflictStrategy::Merge;
                match request.direction {
                    TransferDirection::Upload => self.upload(conn, request),
                    TransferDirection::Download => self.download(conn, request),
                }
            }
        }
    }

    fn sync(&mut self, conn: &dyn Protocol, request: &SyncRequest) -> Result<TransferStatus> {
        self.destination = Some(request.remote_root.clone());
        self.total = request
            .actions
            .iter()
            .map(|a| match a.kind {
                SyncActionKind::Upload => a.local.map_or(0, |m| m.size),
                SyncActionKind::Download => a.remote.map_or(0, |m| m.size),
                _ => 0,
            })
            .sum();
        self.emit(TransferStatus::InProgress);

        // Shortest paths first to create parents; deletes come last, deepest first,
        // so a folder is empty by the time it is removed.
        let is_delete = |a: &SyncAction| matches!(a.kind, SyncActionKind::DeleteLocal | SyncActionKind::DeleteRemote);
        let (mut deletes, mut others): (Vec<&SyncAction>, Vec<&SyncAction>) =
            request.actions.iter().partition(|a| is_delete(a));
        others.sort_by_key(|a| a.path.matches('/').count());
        deletes.sort_by_key(|a| std::cmp::Reverse(a.path.matches('/').count()));

        for action in others.into_iter().chain(deletes) {
            self.check_cancel(conn)?;
            if let Err(e) = self.sync_action(conn, request, action) {
                self.record_failure(conn, e)?;
            }
        }
        Ok(self.final_status())
    }

    fn sync_action(&mut self, conn: &dyn Protocol, request: &SyncRequest, action: &SyncAction) -> Result<()> {
        let local = Path::new(&request.local_root).join(&action.path);
        let remote = remote_join(&request.remote_root, &action.path);
        let changed = || OmniError::Transfer(Msg::new("sync.changedSincePreview").with("path", &action.path));

        match action.kind {
            SyncActionKind::Upload => {
                if !sync::unchanged_since_preview(action.remote, sync::remote_meta(conn, &remote)) {
                    return Err(changed());
                }
                ensure_remote_parents(conn, &request.remote_root, &remote)?;
                conn.upload(&local, &remote, &mut |n| self.on_progress(conn, n))?;
            }
            SyncActionKind::Download => {
                if !sync::unchanged_since_preview(action.local, sync::local_meta(&local)) {
                    return Err(changed());
                }
                if let Some(parent) = local.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| OmniError::io_context(parent.display(), e))?;
                }
                conn.download(&remote, &local, &mut |n| self.on_progress(conn, n))?;
            }
            SyncActionKind::CreateRemoteDir => {
                ensure_remote_parents(conn, &request.remote_root, &remote)?;
                if !conn.exists(&remote) {
                    conn.mkdir(&remote)?;
                }
            }
            SyncActionKind::CreateLocalDir => {
                std::fs::create_dir_all(&local).map_err(|e| OmniError::io_context(local.display(), e))?;
            }
            SyncActionKind::DeleteRemote => {
                if !sync::unchanged_since_preview(action.remote, sync::remote_meta(conn, &remote)) {
                    return Err(changed());
                }
                if action.remote.is_some_and(|m| m.is_dir) {
                    conn.remove_empty_dir(&remote)?;
                } else {
                    conn.delete(&remote)?;
                }
            }
            SyncActionKind::DeleteLocal => {
                if !sync::unchanged_since_preview(action.local, sync::local_meta(&local)) {
                    return Err(changed());
                }
                let removed = if action.local.is_some_and(|m| m.is_dir) {
                    std::fs::remove_dir(&local)
                } else {
                    std::fs::remove_file(&local)
                };
                removed.map_err(|e| OmniError::io_context(local.display(), e))?;
            }
            // The preview turns a resolved conflict into a copy; anything left is not to be touched.
            SyncActionKind::Conflict => return Ok(()),
        }
        self.files_done += 1;
        Ok(())
    }

    fn upload(&mut self, conn: &dyn Protocol, request: &TransferRequest) -> Result<TransferStatus> {
        let source = PathBuf::from(&request.source);
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| OmniError::Transfer(Msg::new("transfer.invalidSource").with("path", &request.source)))?;
        if is_local_dir_link(&source) {
            return Err(link_not_followed(&request.source));
        }
        let name = request.dest_name.clone().unwrap_or(name);

        let exists = |n: &str| conn.exists(&remote_join(&request.dest_dir, n));
        let name = match resolve_conflict(&name, request.conflict, exists) {
            Some(name) => name,
            None => return Ok(TransferStatus::Skipped),
        };
        let dest = remote_join(&request.dest_dir, &name);
        self.destination = Some(dest.clone());

        self.total = local_tree_size(&source)?;
        self.emit(TransferStatus::InProgress);
        self.upload_tree(conn, &source, &dest)?;
        if request.delete_source && self.final_status() == TransferStatus::Completed {
            self.remove_copied(|path, is_dir| {
                let path = Path::new(path);
                let removed = if is_dir { std::fs::remove_dir(path) } else { std::fs::remove_file(path) };
                removed.map_err(|e| OmniError::io_context(path.display(), e))
            });
        }
        Ok(self.final_status())
    }

    /// Removes the copied sources, children first. A folder that holds anything
    /// not copied (added meanwhile) stays, and is reported.
    fn remove_copied(&mut self, mut remove: impl FnMut(&str, bool) -> Result<()>) {
        for (path, is_dir) in std::mem::take(&mut self.copied) {
            if let Err(e) = remove(&path, is_dir) {
                self.failures.push(OmniError::Transfer(Msg::new("transfer.sourceNotRemoved").cause(e)));
            }
        }
    }

    fn upload_tree(&mut self, conn: &dyn Protocol, local: &Path, remote: &str) -> Result<()> {
        self.check_cancel(conn)?;
        let meta = std::fs::symlink_metadata(local)
            .map_err(|e| OmniError::io_context(local.display(), e))?;

        if meta.is_dir() {
            if !conn.exists(remote) {
                conn.mkdir(remote)?;
            }
            for entry in std::fs::read_dir(local).map_err(|e| OmniError::io_context(local.display(), e))? {
                let copied = entry.map_err(OmniError::from).and_then(|entry| {
                    let child = remote_join(remote, &entry.file_name().to_string_lossy());
                    self.upload_tree(conn, &entry.path(), &child)
                });
                if let Err(e) = copied {
                    self.record_failure(conn, e)?;
                }
            }
            self.copied.push((local.display().to_string(), true));
            Ok(())
        } else if is_local_dir_link(local) {
            self.skipped_links.push(local.display().to_string());
            Ok(())
        } else {
            if self.merge && conn.exists(remote) {
                self.already_present += 1;
                self.transferred += meta.len();
            } else {
                conn.upload(local, remote, &mut |n| self.on_progress(conn, n))?;
                self.files_done += 1;
            }
            self.copied.push((local.display().to_string(), false));
            Ok(())
        }
    }

    fn download(&mut self, conn: &dyn Protocol, request: &TransferRequest) -> Result<TransferStatus> {
        let name = request
            .dest_name
            .clone()
            .unwrap_or_else(|| remote_file_name(&request.source).to_string());
        let dest_dir = PathBuf::from(&request.dest_dir);
        let entry = conn.stat(&request.source)?;
        if entry.is_symlink && entry.is_dir {
            return Err(link_not_followed(&request.source));
        }

        let exists = |n: &str| dest_dir.join(n).symlink_metadata().is_ok();
        let name = match resolve_conflict(&name, request.conflict, exists) {
            Some(name) => name,
            None => return Ok(TransferStatus::Skipped),
        };
        let dest = dest_dir.join(&name);
        self.destination = Some(dest.to_string_lossy().into_owned());

        self.total = remote_tree_size(conn, &request.source, entry.is_dir, entry.size)?;
        self.emit(TransferStatus::InProgress);
        self.download_tree(conn, &request.source, entry.is_dir, &dest)?;
        if request.delete_source && self.final_status() == TransferStatus::Completed {
            self.remove_copied(|path, is_dir| if is_dir { conn.remove_empty_dir(path) } else { conn.delete(path) });
        }
        Ok(self.final_status())
    }

    fn download_tree(&mut self, conn: &dyn Protocol, remote: &str, is_dir: bool, local: &Path) -> Result<()> {
        self.check_cancel(conn)?;
        if is_dir {
            std::fs::create_dir_all(local).map_err(|e| OmniError::io_context(local.display(), e))?;
            for entry in conn.list_dir(remote)? {
                let child_remote = entry.path.to_string_lossy();
                if entry.is_symlink && entry.is_dir {
                    self.skipped_links.push(child_remote.into_owned());
                } else if let Err(e) =
                    self.download_tree(conn, &child_remote, entry.is_dir, &local.join(&entry.name))
                {
                    self.record_failure(conn, e)?;
                }
            }
            self.copied.push((remote.to_string(), true));
            Ok(())
        } else {
            if self.merge && local.symlink_metadata().is_ok() {
                self.already_present += 1;
                self.transferred += std::fs::metadata(local).map_or(0, |m| m.len());
            } else {
                conn.download(remote, local, &mut |n| self.on_progress(conn, n))?;
                self.files_done += 1;
            }
            self.copied.push((remote.to_string(), false));
            Ok(())
        }
    }
}

/// Returns the name to write to, or `None` to skip.
fn resolve_conflict(name: &str, strategy: ConflictStrategy, exists: impl Fn(&str) -> bool) -> Option<String> {
    if !exists(name) {
        return Some(name.to_string());
    }
    match strategy {
        ConflictStrategy::Overwrite | ConflictStrategy::Merge => Some(name.to_string()),
        ConflictStrategy::Rename => Some(unique_name(name, exists)),
        // The frontend resolves "Ask" before enqueueing; never overwrite silently.
        ConflictStrategy::Skip | ConflictStrategy::Ask => None,
    }
}

/// Creates the folders missing between the sync root and `remote`.
fn ensure_remote_parents(conn: &dyn Protocol, root: &str, remote: &str) -> Result<()> {
    let Some(parent) = remote_parent(remote) else { return Ok(()) };
    if parent.len() <= root.len() || conn.exists(&parent) {
        return Ok(());
    }
    ensure_remote_parents(conn, root, &parent)?;
    conn.mkdir(&parent)
}

/// Links to directories are never followed: they can loop.
fn is_local_dir_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_symlink())
        && std::fs::metadata(path).is_ok_and(|m| m.is_dir())
}

fn link_not_followed(path: &str) -> OmniError {
    OmniError::Transfer(Msg::new("transfer.linkNotFollowed").with("path", path))
}

/// Unreadable subfolders only make the total an underestimate; copying them reports the error.
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
    for entry in std::fs::read_dir(path).map_err(|e| OmniError::io_context(path.display(), e))?.flatten() {
        total += local_tree_size(&entry.path()).unwrap_or(0);
    }
    Ok(total)
}

/// Same as `local_tree_size`, unless the session died.
fn remote_tree_size(conn: &dyn Protocol, path: &str, is_dir: bool, size: u64) -> Result<u64> {
    if !is_dir {
        return Ok(size);
    }
    let mut total = 0;
    for entry in conn.list_dir(path)? {
        if entry.is_symlink && entry.is_dir {
            continue;
        }
        match remote_tree_size(conn, &entry.path.to_string_lossy(), entry.is_dir, entry.size) {
            Ok(size) => total += size,
            Err(e) if !conn.is_alive() => return Err(e),
            Err(_) => {}
        }
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
        assert_eq!(resolve_conflict("a.txt", ConflictStrategy::Merge, taken), Some("a.txt".into()));
    }
}
