// Tauri command handlers — bridge between Vue frontend and Rust backend.
// Commands are registered in lib.rs via tauri::generate_handler![].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use tauri::Manager;

use crate::config::{AuthMethod, Profile, credentials};
use crate::error::OmniError;
use crate::protocols::sftp::{ConnectParams, SftpConnection};
use crate::protocols::{
    FileEntry, Protocol, ProtocolType, remote_join, remote_parent, sort_entries, validate_file_name,
};
use crate::transfer::queue::{TransferQueue, TransferRequest};
use crate::transfer::{ConflictStrategy, TransferDirection};

pub struct ActiveConnection {
    conn: SftpConnection,
    profile_name: String,
    transfers: TransferQueue,
}

/// Shared with blocking tasks, hence the `Arc`.
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
        let metadata = entry.metadata()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let full_path = entry.path();
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
pub async fn connect_sftp(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, ConnectionState>,
    profile: Profile,
    password: Option<String>,
    trusted_fingerprint: Option<String>,
) -> Result<String, OmniError> {
    if profile.protocol != ProtocolType::Sftp {
        return Err(OmniError::Protocol(format!(
            "Protocol {:?} is not supported yet (only SFTP is available)",
            profile.protocol
        )));
    }

    let password = match password {
        Some(p) => Some(p),
        None if profile.save_password && needs_secret(&profile.auth_method) => {
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

    // Connect outside the lock so other commands stay responsive and a failed
    // attempt leaves the current connection untouched.
    let (conn, initial_path) = run_blocking(move || {
        let mut conn = SftpConnection::new(&params);
        conn.connect()?;
        let initial_path = match &params.profile.remote_directory {
            Some(dir) if !dir.is_empty() => dir.clone(),
            _ => conn.realpath(".").unwrap_or_else(|_| "/".to_string()),
        };
        Ok((
            ActiveConnection {
                conn,
                profile_name: params.profile.name.clone(),
                transfers: TransferQueue::start(app_handle, params),
            },
            initial_path,
        ))
    })
    .await?;

    let previous = lock(&state.0)?.replace(conn);
    if let Some(mut previous) = previous {
        let _ = run_blocking(move || previous.conn.disconnect()).await;
    }

    Ok(initial_path)
}

#[tauri::command]
pub async fn disconnect_sftp(state: tauri::State<'_, ConnectionState>) -> Result<(), OmniError> {
    let previous = lock(&state.0)?.take();
    match previous {
        Some(mut active) => run_blocking(move || active.conn.disconnect()).await,
        None => Ok(()),
    }
}

#[tauri::command]
pub async fn list_remote_dir(
    state: tauri::State<'_, ConnectionState>,
    path: String,
) -> Result<Vec<FileEntry>, OmniError> {
    let shared = Arc::clone(&state.0);
    run_blocking(move || {
        let guard = lock(&shared)?;
        let active = guard
            .as_ref()
            .ok_or_else(|| OmniError::Protocol("Not connected".into()))?;
        active.conn.list_dir(&path)
    })
    .await
}

/// Runs `f` on the active connection, off the main thread.
async fn with_connection<T, F>(state: &ConnectionState, f: F) -> Result<T, OmniError>
where
    T: Send + 'static,
    F: FnOnce(&SftpConnection) -> Result<T, OmniError> + Send + 'static,
{
    let shared = Arc::clone(&state.0);
    run_blocking(move || {
        let guard = lock(&shared)?;
        let active = guard
            .as_ref()
            .ok_or_else(|| OmniError::Protocol("Not connected".into()))?;
        f(&active.conn)
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
) -> Result<String, OmniError> {
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
pub fn get_connection_status(
    state: tauri::State<'_, ConnectionState>,
) -> Result<Option<String>, OmniError> {
    let guard = lock(&state.0)?;
    match guard.as_ref() {
        Some(active) if active.conn.is_connected() => Ok(Some(active.profile_name.clone())),
        _ => Ok(None),
    }
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
