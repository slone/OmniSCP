pub mod commands;
pub mod config;
pub mod error;
pub mod logging;
pub mod protocols;
pub mod sync;
pub mod transfer;

use commands::ConnectionState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ConnectionState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_home_dir,
            commands::get_parent_dir,
            commands::list_local_dir,
            commands::load_profiles,
            commands::save_profiles,
            commands::generate_id,
            commands::connect,
            commands::disconnect,
            commands::list_remote_dir,
            commands::create_remote_dir,
            commands::rename_remote,
            commands::delete_remote,
            commands::create_local_dir,
            commands::rename_local,
            commands::delete_local,
            commands::enqueue_transfer,
            commands::cancel_transfer,
            commands::compare_sync,
            commands::cancel_sync_compare,
            commands::start_sync,
            commands::append_log,
            commands::load_logs,
            commands::clear_logs,
            commands::export_logs,
            commands::set_saved_password,
            commands::has_saved_password,
            commands::load_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
