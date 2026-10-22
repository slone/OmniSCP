pub mod commands;
pub mod config;
pub mod error;
pub mod logging;
pub mod protocols;
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
            commands::connect_sftp,
            commands::disconnect_sftp,
            commands::list_remote_dir,
            commands::get_connection_status,
            commands::create_remote_dir,
            commands::rename_remote,
            commands::delete_remote,
            commands::create_local_dir,
            commands::rename_local,
            commands::delete_local,
            commands::enqueue_transfer,
            commands::cancel_transfer,
            commands::set_saved_password,
            commands::has_saved_password,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
