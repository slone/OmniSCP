pub mod commands;
pub mod config;
pub mod edit;
pub mod error;
pub mod logging;
pub mod protocols;
pub mod sync;
pub mod transfer;
pub mod tree;

use commands::ConnectionState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(ConnectionState::default())
        .setup(|app| {
            use tauri::Manager;
            if let Ok(cache) = app.path().app_cache_dir() {
                edit::clean_old(&cache.join(commands::EDITS_DIR));
            }
            Ok(())
        })
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
            commands::pause_transfer,
            commands::resume_transfer,
            commands::set_queue_paused,
            commands::edit_remote_file,
            commands::upload_edited_file,
            commands::stop_editing,
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
            commands::set_local_permissions,
            commands::set_remote_permissions,
            commands::local_link_target,
            commands::remote_link_target,
            commands::local_folder_size,
            commands::remote_folder_size,
            commands::cancel_folder_size,
            commands::save_settings,
            commands::export_profiles,
            commands::import_profiles,
            commands::test_connection,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
