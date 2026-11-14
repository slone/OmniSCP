// Tauri command handlers — bridge between Vue frontend and Rust backend.
// Commands are registered in lib.rs via tauri::generate_handler![].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{Emitter, Manager};

use crate::edit::{self, EditInfo, Edits};
use crate::config::{AppConfig, AuthMethod, Profile, credentials, exchange};
use crate::error::OmniError;
use crate::logging::{self, LogEntry};
use crate::protocols::{
    AccountNames, ConnectParams, FileEntry, ProbeResult, Protocol, for_profile, probe, remote_join, remote_parent, sort_entries, validate_file_name,
};
use crate::sync::{self, SyncAction, SyncOptions, SyncPlan};
use crate::transfer::queue::{EventSink, PROGRESS_EVENT, SyncRequest, TransferQueue, TransferRequest};
use crate::transfer::{ConflictStrategy, TransferDirection};
use crate::tree::{self, TreeSize};

pub struct ActiveConnection {
    /// Locked for the duration of network calls, which can be long (recursive delete).
    conn: Arc<Mutex<Box<dyn Protocol>>>,
    transfers: TransferQueue,
    /// For sessions opened on the side, such as long sync scans.
    params: ConnectParams,
    cancel_compare: Arc<AtomicBool>,
    cancel_size: Arc<AtomicBool>,
    /// Read once at connection, to show owners by name.
    names: Arc<AccountNames>,
    /// Remote files open in local applications; their watcher stops when this is dropped.
    edits: Arc<Edits>,
}

impl ActiveConnection {
    fn close(self) -> Result<(), OmniError> {
        self.cancel_compare.store(true, Ordering::Relaxed);
        self.cancel_size.store(true, Ordering::Relaxed);
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
    let mut names = local_names::Cache::default();

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

        #[cfg(unix)]
        let (permissions, mode, uid, gid) = {
            use std::os::unix::fs::MetadataExt;
            let mode = metadata.mode() & 0o7777;
            (Some(crate::protocols::format_permissions(mode, is_dir)), Some(mode), Some(metadata.uid()), Some(metadata.gid()))
        };
        #[cfg(not(unix))]
        let (permissions, mode, uid, gid) = {
            let flags = if metadata.permissions().readonly() { "r--" } else { "rw-" };
            (Some(flags.to_string()), None, None, None)
        };

        entries.push(FileEntry {
            name,
            path: full_path,
            is_dir,
            is_symlink,
            size,
            modified,
            permissions,
            mode,
            uid,
            gid,
            owner: uid.and_then(|id| local_names::user(&mut names, id)),
            group: gid.and_then(|id| local_names::group(&mut names, id)),
        });
    }

    sort_entries(&mut entries);

    Ok(entries)
}

/// Local account names, looked up through the system so directory services count too.
mod local_names {
    use std::collections::HashMap;

    #[derive(Default)]
    pub struct Cache {
        users: HashMap<u32, Option<String>>,
        groups: HashMap<u32, Option<String>>,
    }

    pub fn user(cache: &mut Cache, id: u32) -> Option<String> {
        cache.users.entry(id).or_insert_with(|| lookup_user(id)).clone()
    }

    pub fn group(cache: &mut Cache, id: u32) -> Option<String> {
        cache.groups.entry(id).or_insert_with(|| lookup_group(id)).clone()
    }

    #[cfg(unix)]
    fn lookup_user(id: u32) -> Option<String> {
        let mut buf = vec![0 as libc::c_char; 4096];
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result = std::ptr::null_mut();
        // SAFETY: every pointer refers to live memory of the stated size; `pw_name`
        // points into `buf`, which outlives the read below.
        let rc = unsafe { libc::getpwuid_r(id, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
        if rc != 0 || result.is_null() {
            return None;
        }
        Some(unsafe { std::ffi::CStr::from_ptr(pwd.pw_name) }.to_string_lossy().into_owned())
    }

    #[cfg(unix)]
    fn lookup_group(id: u32) -> Option<String> {
        let mut buf = vec![0 as libc::c_char; 16384];
        let mut grp: libc::group = unsafe { std::mem::zeroed() };
        let mut result = std::ptr::null_mut();
        // SAFETY: as in `lookup_user`.
        let rc = unsafe { libc::getgrgid_r(id, &mut grp, buf.as_mut_ptr(), buf.len(), &mut result) };
        if rc != 0 || result.is_null() {
            return None;
        }
        Some(unsafe { std::ffi::CStr::from_ptr(grp.gr_name) }.to_string_lossy().into_owned())
    }

    #[cfg(not(unix))]
    fn lookup_user(_: u32) -> Option<String> {
        None
    }

    #[cfg(not(unix))]
    fn lookup_group(_: u32) -> Option<String> {
        None
    }
}

#[tauri::command]
pub fn set_local_permissions(path: String, mode: u32) -> Result<(), OmniError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o7777))
            .map_err(|e| OmniError::io_context(&path, e))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Err(OmniError::Protocol("Unix permissions do not exist on this system".into()))
    }
}

#[tauri::command]
pub fn local_link_target(path: String) -> Result<String, OmniError> {
    std::fs::read_link(&path)
        .map(|t| t.to_string_lossy().into_owned())
        .map_err(|e| OmniError::io_context(&path, e))
}

static LOCAL_SIZE_CANCEL: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn local_folder_size(paths: Vec<String>) -> Result<TreeSize, OmniError> {
    LOCAL_SIZE_CANCEL.store(false, Ordering::Relaxed);
    run_blocking(move || tree::local_tree_size(&paths, &LOCAL_SIZE_CANCEL)).await
}

/// On its own session, so browsing stays available meanwhile.
#[tauri::command]
pub async fn remote_folder_size(
    state: tauri::State<'_, ConnectionState>,
    paths: Vec<String>,
) -> Result<TreeSize, OmniError> {
    let (params, cancel) = {
        let guard = lock(&state.0)?;
        let active = guard
            .as_ref()
            .ok_or_else(|| OmniError::Protocol("Not connected".into()))?;
        active.cancel_size.store(false, Ordering::Relaxed);
        (active.params.clone(), Arc::clone(&active.cancel_size))
    };
    run_blocking(move || {
        let mut conn = for_profile(&params)?;
        conn.connect()?;
        let total = tree::remote_tree_size(conn.as_ref(), &paths, &cancel);
        let _ = conn.disconnect();
        total
    })
    .await
}

/// Stops both size computations; a finished one is unaffected.
#[tauri::command]
pub fn cancel_folder_size(state: tauri::State<'_, ConnectionState>) -> Result<(), OmniError> {
    LOCAL_SIZE_CANCEL.store(true, Ordering::Relaxed);
    if let Some(active) = lock(&state.0)?.as_ref() {
        active.cancel_size.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_remote_permissions(
    state: tauri::State<'_, ConnectionState>,
    path: String,
    mode: u32,
) -> Result<(), OmniError> {
    with_connection(&state, move |conn| conn.set_permissions(&path, mode)).await
}

#[tauri::command]
pub async fn remote_link_target(state: tauri::State<'_, ConnectionState>, path: String) -> Result<String, OmniError> {
    with_connection(&state, move |conn| conn.read_link(&path)).await
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

/// Writes the given profiles to a file for another machine. Returns how many were written.
#[tauri::command]
pub async fn export_profiles(path: String, profiles: Vec<Profile>) -> Result<usize, OmniError> {
    run_blocking(move || exchange::export(Path::new(&path), &profiles)).await
}

/// Reads a profile file; the profiles come back with new ids and are not saved yet.
#[tauri::command]
pub async fn import_profiles(path: String) -> Result<Vec<Profile>, OmniError> {
    run_blocking(move || exchange::import(Path::new(&path), generate_id)).await
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
    let edits = Arc::new(Edits::default());
    watch_edits(app_handle.clone(), Arc::downgrade(&edits));
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
        let names = Arc::new(conn.account_names());
        Ok((
            ActiveConnection {
                conn: Arc::new(Mutex::new(conn)),
                transfers: TransferQueue::start(sink, params.clone(), workers),
                params,
                cancel_compare: Arc::default(),
                cancel_size: Arc::default(),
                names,
                edits,
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

/// Tries a profile, possibly unsaved, without touching the current connection.
/// `use_saved`: take the secret from the credential store when `password` is absent.
#[tauri::command]
pub async fn test_connection(
    app_handle: tauri::AppHandle,
    profile: Profile,
    password: Option<String>,
    use_saved: bool,
    trusted_fingerprint: Option<String>,
) -> Result<ProbeResult, OmniError> {
    let password = match password {
        Some(p) => Some(p),
        None if use_saved && needs_secret(&profile.auth_method) => {
            let id = profile.id.clone();
            run_blocking(move || credentials::get(&id)).await?
        }
        None => None,
    };
    let known_hosts_path = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| OmniError::Config(e.to_string()))?
        .join("known_hosts");
    let params = ConnectParams {
        profile,
        password,
        known_hosts_path,
        trusted_fingerprint,
    };
    run_blocking(move || probe(&params)).await
}

/// Reports saved local copies until the connection that owns `edits` is gone.
fn watch_edits(app_handle: tauri::AppHandle, edits: std::sync::Weak<Edits>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(edit::POLL_INTERVAL);
            let Some(edits) = edits.upgrade() else { break };
            for change in edits.poll() {
                let _ = app_handle.emit(edit::CHANGED_EVENT, change);
            }
        }
    });
}

/// Under the app's cache folder.
pub const EDITS_DIR: &str = "edits";

fn active_edits(state: &ConnectionState) -> Result<Arc<Edits>, OmniError> {
    let guard = lock(&state.0)?;
    let active = guard
        .as_ref()
        .ok_or_else(|| OmniError::Protocol("Not connected".into()))?;
    Ok(Arc::clone(&active.edits))
}

/// Downloads a remote file and opens it with the system's default application.
/// Each save is reported with `remote-edit-changed`; `upload_edited_file` sends it.
#[tauri::command]
pub async fn edit_remote_file(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, ConnectionState>,
    path: String,
) -> Result<EditInfo, OmniError> {
    use tauri_plugin_opener::OpenerExt;

    let root = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| OmniError::Config(e.to_string()))?
        .join(EDITS_DIR);
    let edits = active_edits(&state)?;
    let info = with_connection(&state, move |conn| edits.open(conn, &path, &root, generate_id())).await?;
    app_handle
        .opener()
        .open_path(&info.local_path, None::<&str>)
        .map_err(|e| OmniError::Config(format!("No application could open {}: {e}", info.local_path)))?;
    Ok(info)
}

/// `force`: send even if the server's copy changed since it was opened.
#[tauri::command]
pub async fn upload_edited_file(
    state: tauri::State<'_, ConnectionState>,
    id: String,
    force: bool,
) -> Result<(), OmniError> {
    let edits = active_edits(&state)?;
    with_connection(&state, move |conn| edits.upload(conn, &id, force)).await
}

#[tauri::command]
pub fn stop_editing(state: tauri::State<'_, ConnectionState>, id: String) -> Result<(), OmniError> {
    if let Ok(edits) = active_edits(&state) {
        edits.stop(&id);
    }
    Ok(())
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
    let names = {
        let guard = lock(&state.0)?;
        guard.as_ref().map(|active| Arc::clone(&active.names)).unwrap_or_default()
    };
    let mut entries = with_connection(&state, move |conn| conn.list_dir(&path)).await?;
    names.name_entries(&mut entries);
    Ok(entries)
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
    delete_source: Option<bool>,
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
        delete_source: delete_source.unwrap_or(false),
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

#[tauri::command]
pub fn pause_transfer(state: tauri::State<'_, ConnectionState>, id: String) -> Result<(), OmniError> {
    if let Some(active) = lock(&state.0)?.as_ref() {
        active.transfers.pause(&id);
    }
    Ok(())
}

#[tauri::command]
pub fn resume_transfer(state: tauri::State<'_, ConnectionState>, id: String) -> Result<(), OmniError> {
    if let Some(active) = lock(&state.0)?.as_ref() {
        active.transfers.resume(&id);
    }
    Ok(())
}

/// Holds or releases the whole queue, running transfers included.
#[tauri::command]
pub fn set_queue_paused(state: tauri::State<'_, ConnectionState>, paused: bool) -> Result<(), OmniError> {
    if let Some(active) = lock(&state.0)?.as_ref() {
        active.transfers.set_paused(paused);
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
