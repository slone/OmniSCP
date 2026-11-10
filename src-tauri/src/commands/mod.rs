// Tauri command handlers — bridge between Vue frontend and Rust backend.
// Commands are registered in lib.rs via tauri::generate_handler![].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{Emitter, Manager};

use crate::config::{AppConfig, AuthMethod, Profile, credentials};
use crate::error::OmniError;
use crate::logging::{self, LogEntry};
use crate::protocols::{
    ConnectParams, FileEntry, Protocol, for_profile, remote_join, remote_parent, sort_entries, validate_file_name,
};
use crate::sync::{self, SyncAction, SyncOptions, SyncPlan};
use crate::transfer::queue::{EventSink, PROGRESS_EVENT, SyncRequest, TransferQueue, TransferRequest};
use crate::transfer::{ConflictStrategy, TransferDirection};

pub struct ActiveConnection {
    /// Locked for the duration of network calls, which can be long (recursive delete).
    conn: Arc<Mutex<Box<dyn Protocol>>>,
    transfers: TransferQueue,
    /// For sessions opened on the side, such as long sync scans.
    params: ConnectParams,
    cancel_compare: Arc<AtomicBool>,
}

impl ActiveConnection {
    fn close(self) -> Result<(), OmniError> {
        self.cancel_compare.store(true, Ordering::Relaxed);
        drop(self.transfers);
        lock_conn(&self.conn).disconnect()
    }
}

fn lock_conn(conn: &Mutex<Box<dyn Protocol>>) -> MutexGuard<'_, Box<dyn Protocol>> {
    conn.lock().unwrap_or_else(|e| e.into_inner())
}

/// Never held across network I/O: sync commands take it on the main thread.
#[derive(Default)]
pub struct ConnectionState(Arc<Mutex<Option<ActiveConnection>>>);

#[tauri::command]
pub fn get_home_dir(app_handle: tauri::AppHandle) -> Result<String, OmniError> {
    let home = app_handle
        .path()
        .home_dir()
        .map_err(|e| OmniError::Config(format!("Could not determine home directory: {e}")))?;
    Ok(home.to_string_lossy().into_owned())
}

/// Parent of a local path, using the platform's separators. `None` at the root.
#[tauri::command]
pub fn get_parent_dir(path: String) -> Option<String> {
    Path::new(&path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn list_local_dir(path: String) -> Result<Vec<FileEntry>, OmniError> {
    let dir = PathBuf::from(&path);
    let mut entries = Vec::new();

    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let link_metadata = entry.metadata()?;
        let is_symlink = link_metadata.is_symlink();
        let name = entry.file_name().to_string_lossy().into_owned();
        let full_path = entry.path();
        // Describe the target of a symlink, unless it is dangling.
        let metadata = if is_symlink {
            std::fs::metadata(&full_path).unwrap_or(link_metadata)
        } else {
            link_metadata
        };
        let is_dir = metadata.is_dir();
        let size = if is_dir { 0 } else { metadata.len() };

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        let permissions = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                Some(crate::protocols::format_permissions(metadata.permissions().mode(), is_dir))
            }
            #[cfg(not(unix))]
            {
                if metadata.permissions().readonly() {
                    Some("r--".to_string())
                } else {
                    Some("rw-".to_string())
                }
            }
        };

        entries.push(FileEntry {
            name,
            path: full_path,
            is_dir,
            is_symlink,
            size,
            modified,
            permissions,
            owner: None,
            group: None,
        });
    }

    sort_entries(&mut entries);

    Ok(entries)
}

#[tauri::command]
pub fn load_profiles(app_handle: tauri::AppHandle) -> Result<Vec<Profile>, OmniError> {
    let config_dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| OmniError::Config(e.to_string()))?;
    let path = config_dir.join("profiles.json");

    if !path.exists() {
        return Ok(Vec::new());
    }

    let data = std::fs::read_to_string(&path)?;
    let profiles: Vec<Profile> =
        serde_json::from_str(&data).map_err(|e| OmniError::Config(e.to_string()))?;
    Ok(profiles)
}

#[tauri::command]
pub fn save_profiles(
    app_handle: tauri::AppHandle,
    profiles: Vec<Profile>,
) -> Result<(), OmniError> {
    let config_dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| OmniError::Config(e.to_string()))?;
    std::fs::create_dir_all(&config_dir)?;

    let tmp_path = config_dir.join("profiles.json.tmp");
    let final_path = config_dir.join("profiles.json");

    let data = serde_json::to_string_pretty(&profiles)
        .map_err(|e| OmniError::Config(e.to_string()))?;
    std::fs::write(&tmp_path, data)?;
    std::fs::rename(&tmp_path, &final_path)?;

    Ok(())
}

fn needs_secret(auth: &AuthMethod) -> bool {
    matches!(auth, AuthMethod::Password | AuthMethod::KeyWithPassphrase { .. })
}

/// Stores (`Some`) or forgets (`None`) the secret saved for a profile.
#[tauri::command]
pub async fn set_saved_password(profile_id: String, password: Option<String>) -> Result<(), OmniError> {
    run_blocking(move || match password {
        Some(p) => credentials::set(&profile_id, &p),
        None => credentials::delete(&profile_id),
    })
    .await
}

#[tauri::command]
pub async fn has_saved_password(profile_id: String) -> Result<bool, OmniError> {
    run_blocking(move || Ok(credentials::get(&profile_id)?.is_some())).await
}

#[tauri::command]
pub fn generate_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);

    format!("{timestamp:x}-{count:04x}")
}

#[tauri::command]
pub async fn connect(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, ConnectionState>,
    profile: Profile,
    password: Option<String>,
    trusted_fingerprint: Option<String>,
) -> Result<String, OmniError> {
    let password = match password {
        Some(p) => Some(p),
        None if profile.save_password && needs_secret(&profile.auth_method) => {
            let id = profile.id.clone();
            run_blocking(move || credentials::get(&id)).await?
        }
        None => None,
    };

    let config_dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| OmniError::Config(e.to_string()))?;
    let known_hosts_path = config_dir.join("known_hosts");
    // Unreadable settings must not prevent connecting.
    let workers = AppConfig::load(&config_dir.join(SETTINGS_FILE))
        .unwrap_or_default()
        .concurrent_transfers;

    let params = ConnectParams {
        profile,
        password,
        known_hosts_path,
        trusted_fingerprint,
    };

    // Connect outside the lock so other commands stay responsive and a failed
    // attempt leaves the current connection untouched.
    let sink: EventSink = Arc::new(move |event| {
        let _ = app_handle.emit(PROGRESS_EVENT, event);
    });
    let (conn, initial_path) = run_blocking(move || {
        let mut conn = for_profile(&params)?;
        conn.connect()?;
        let initial_path = match &params.profile.remote_directory {
            Some(dir) if !dir.is_empty() => dir.clone(),
            _ => conn.realpath(".").unwrap_or_else(|_| "/".to_string()),
        };
        Ok((
            ActiveConnection {
                conn: Arc::new(Mutex::new(conn)),
                transfers: TransferQueue::start(sink, params.clone(), workers),
                params,
                cancel_compare: Arc::default(),
            },
            initial_path,
        ))
    })
    .await?;

    let previous = lock(&state.0)?.replace(conn);
    if let Some(previous) = previous {
        let _ = run_blocking(move || previous.close()).await;
    }

    Ok(initial_path)
}

const SETTINGS_FILE: &str = "settings.json";

fn settings_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, OmniError> {
    let dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| OmniError::Config(e.to_string()))?;
    Ok(dir.join(SETTINGS_FILE))
}

#[tauri::command]
pub fn load_settings(app_handle: tauri::AppHandle) -> Result<AppConfig, OmniError> {
    AppConfig::load(&settings_path(&app_handle)?)
}

/// Returns what was saved, after bringing values within range.
#[tauri::command]
pub fn save_settings(app_handle: tauri::AppHandle, settings: AppConfig) -> Result<AppConfig, OmniError> {
    let settings = settings.sanitized();
    settings.save(&settings_path(&app_handle)?)?;
    Ok(settings)
}

#[tauri::command]
pub async fn disconnect(state: tauri::State<'_, ConnectionState>) -> Result<(), OmniError> {
    let previous = lock(&state.0)?.take();
    match previous {
        Some(active) => run_blocking(move || active.close()).await,
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn list_remote_dir(
    state: tauri::State<'_, ConnectionState>,
    path: String,
) -> Result<Vec<FileEntry>, OmniError> {
    with_connection(&state, move |conn| conn.list_dir(&path)).await
}

/// Runs `f` on the active connection, off the main thread. A failure caused by
/// a dead session is reported as such so the frontend can leave the connected state.
async fn with_connection<T, F>(state: &ConnectionState, f: F) -> Result<T, OmniError>
where
    T: Send + 'static,
    F: FnOnce(&dyn Protocol) -> Result<T, OmniError> + Send + 'static,
{
    let conn = {
        let guard = lock(&state.0)?;
        let active = guard
            .as_ref()
            .ok_or_else(|| OmniError::Protocol("Not connected".into()))?;
        Arc::clone(&active.conn)
    };
    run_blocking(move || {
        let conn = lock_conn(&conn);
        f(conn.as_ref()).map_err(|e| match e {
            OmniError::Permission(_) | OmniError::Config(_) => e,
            e if conn.is_alive() => e,
            _ => OmniError::ConnectionLost,
        })
    })
    .await
}

#[tauri::command]
pub async fn create_remote_dir(
    state: tauri::State<'_, ConnectionState>,
    parent: String,
    name: String,
) -> Result<String, OmniError> {
    validate_file_name(&name)?;
    with_connection(&state, move |conn| {
        let path = remote_join(&parent, name.trim());
        conn.mkdir(&path)?;
        Ok(path)
    })
    .await
}

#[tauri::command]
pub async fn rename_remote(
    state: tauri::State<'_, ConnectionState>,
    path: String,
    new_name: String,
) -> Result<String, OmniError> {
    validate_file_name(&new_name)?;
    with_connection(&state, move |conn| {
        let parent = remote_parent(&path)
            .ok_or_else(|| OmniError::Protocol("Cannot rename the root directory".into()))?;
        let target = remote_join(&parent, new_name.trim());
        if conn.exists(&target) {
            return Err(OmniError::Protocol(format!("{target} already exists")));
        }
        conn.rename(&path, &target)?;
        Ok(target)
    })
    .await
}

#[tauri::command]
pub async fn delete_remote(
    state: tauri::State<'_, ConnectionState>,
    path: String,
) -> Result<(), OmniError> {
    if remote_parent(&path).is_none() {
        return Err(OmniError::Protocol("Refusing to delete the root directory".into()));
    }
    with_connection(&state, move |conn| conn.delete(&path)).await
}

#[tauri::command]
pub fn create_local_dir(parent: String, name: String) -> Result<String, OmniError> {
    validate_file_name(&name)?;
    let path = Path::new(&parent).join(name.trim());
    std::fs::create_dir(&path).map_err(|e| OmniError::io_context(path.display(), e))?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn rename_local(path: String, new_name: String) -> Result<String, OmniError> {
    validate_file_name(&new_name)?;
    let from = Path::new(&path);
    let parent = from
        .parent()
        .ok_or_else(|| OmniError::Config("Cannot rename the root directory".into()))?;
    let target = parent.join(new_name.trim());
    if target.symlink_metadata().is_ok() {
        return Err(OmniError::Config(format!("{} already exists", target.display())));
    }
    std::fs::rename(from, &target).map_err(|e| OmniError::io_context(from.display(), e))?;
    Ok(target.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn delete_local(path: String) -> Result<(), OmniError> {
    run_blocking(move || {
        let target = Path::new(&path);
        if target.parent().is_none() {
            return Err(OmniError::Config("Refusing to delete the root directory".into()));
        }
        // symlink_metadata: a symlink to a directory is removed as a link, never followed.
        let meta = target
            .symlink_metadata()
            .map_err(|e| OmniError::io_context(target.display(), e))?;
        let result = if meta.is_dir() {
            std::fs::remove_dir_all(target)
        } else {
            std::fs::remove_file(target)
        };
        result.map_err(|e| OmniError::io_context(target.display(), e))
    })
    .await
}

#[tauri::command]
pub fn enqueue_transfer(
    state: tauri::State<'_, ConnectionState>,
    direction: TransferDirection,
    source: String,
    dest_dir: String,
    conflict: ConflictStrategy,
    dest_name: Option<String>,
) -> Result<String, OmniError> {
    if let Some(name) = &dest_name {
        validate_file_name(name)?;
    }
    let guard = lock(&state.0)?;
    let active = guard
        .as_ref()
        .ok_or_else(|| OmniError::Protocol("Not connected".into()))?;
    let id = generate_id();
    active.transfers.enqueue(TransferRequest {
        id: id.clone(),
        direction,
        source,
        dest_dir,
        conflict,
        dest_name,
    })?;
    Ok(id)
}

#[tauri::command]
pub fn cancel_transfer(state: tauri::State<'_, ConnectionState>, id: String) -> Result<(), OmniError> {
    if let Some(active) = lock(&state.0)?.as_ref() {
        active.transfers.cancel(&id);
    }
    Ok(())
}

/// Scans both sides on its own session, so browsing stays available meanwhile.
#[tauri::command]
pub async fn compare_sync(
    state: tauri::State<'_, ConnectionState>,
    local_root: String,
    remote_root: String,
    options: SyncOptions,
) -> Result<SyncPlan, OmniError> {
    let (params, cancel) = {
        let guard = lock(&state.0)?;
        let active = guard
            .as_ref()
            .ok_or_else(|| OmniError::Protocol("Not connected".into()))?;
        active.cancel_compare.store(false, Ordering::Relaxed);
        (active.params.clone(), Arc::clone(&active.cancel_compare))
    };
    run_blocking(move || {
        let local = sync::scan_local(Path::new(&local_root), &options.excludes, &cancel)?;
        let mut conn = for_profile(&params)?;
        conn.connect()?;
        let remote = sync::scan_remote(conn.as_ref(), &remote_root, &options.excludes, &cancel);
        let _ = conn.disconnect();
        Ok(sync::build_plan(local, remote?, &options))
    })
    .await
}

#[tauri::command]
pub fn cancel_sync_compare(state: tauri::State<'_, ConnectionState>) -> Result<(), OmniError> {
    if let Some(active) = lock(&state.0)?.as_ref() {
        active.cancel_compare.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub fn start_sync(
    state: tauri::State<'_, ConnectionState>,
    local_root: String,
    remote_root: String,
    actions: Vec<SyncAction>,
) -> Result<String, OmniError> {
    for action in &actions {
        sync::validate_relative_path(&action.path)?;
    }
    let guard = lock(&state.0)?;
    let active = guard
        .as_ref()
        .ok_or_else(|| OmniError::Protocol("Not connected".into()))?;
    let id = generate_id();
    active.transfers.enqueue_sync(SyncRequest {
        id: id.clone(),
        local_root,
        remote_root,
        actions,
    })?;
    Ok(id)
}

fn log_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, OmniError> {
    let dir = app_handle
        .path()
        .app_log_dir()
        .map_err(|e| OmniError::Config(e.to_string()))?;
    Ok(dir.join("omniscp.log"))
}

#[tauri::command]
pub fn append_log(app_handle: tauri::AppHandle, entry: LogEntry) -> Result<(), OmniError> {
    logging::append(&log_path(&app_handle)?, &entry)
}

#[tauri::command]
pub async fn load_logs(app_handle: tauri::AppHandle, limit: usize) -> Result<Vec<LogEntry>, OmniError> {
    let path = log_path(&app_handle)?;
    run_blocking(move || logging::read_recent(&path, limit)).await
}

#[tauri::command]
pub fn clear_logs(app_handle: tauri::AppHandle) -> Result<(), OmniError> {
    logging::clear(&log_path(&app_handle)?)
}

/// Copies the journal to the Downloads folder and returns the new file's path.
#[tauri::command]
pub fn export_logs(app_handle: tauri::AppHandle) -> Result<String, OmniError> {
    let source = log_path(&app_handle)?;
    let dir = app_handle
        .path()
        .download_dir()
        .map_err(|e| OmniError::Config(e.to_string()))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let target = dir.join(format!("omniscp-journal-{stamp}.jsonl"));
    std::fs::copy(&source, &target).map_err(|e| OmniError::io_context(target.display(), e))?;
    Ok(target.to_string_lossy().into_owned())
}

fn lock(
    shared: &Mutex<Option<ActiveConnection>>,
) -> Result<MutexGuard<'_, Option<ActiveConnection>>, OmniError> {
    shared
        .lock()
        .map_err(|e| OmniError::Protocol(format!("Failed to acquire connection lock: {e}")))
}

/// Runs blocking network I/O off the main thread.
async fn run_blocking<T, F>(f: F) -> Result<T, OmniError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, OmniError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| OmniError::Protocol(format!("Background task failed: {e}")))?
}
