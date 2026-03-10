mod commands;
mod session_watcher;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_models,
            commands::list_sessions,
            commands::start_recording,
            commands::stop_recording,
            commands::get_session_transcript,
            commands::get_active_session,
            commands::debug_paths,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
