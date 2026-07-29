mod commands;
mod model_manager;
mod native_surface;
mod state;
mod subtitle_io;

use std::sync::{Arc, Mutex};

use myna_player_player::create_default_player;
use myna_player_providers::{CredentialStore, KeyringCredentialStore, ProviderRegistry};
use myna_player_storage::Storage;
use tauri::Manager;

use crate::{
    model_manager::ModelManager,
    native_surface::NativeVideoSurface,
    state::{AppState, ProcessingService, player_clock_loop},
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let main_window = app
                .get_webview_window("main")
                .ok_or_else(|| "main window was not created".to_string())?;
            let native_surface =
                NativeVideoSurface::create(&main_window).map_err(std::io::Error::other)?;
            let player: Arc<dyn myna_player_player::PlayerEngine> =
                Arc::from(create_default_player());
            if player.available() {
                player
                    .attach_surface(native_surface.handle())
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
            }

            let app_data = app.path().app_data_dir()?;
            let storage = Arc::new(
                Storage::open(app_data.join("myna-player.sqlite3"))
                    .map_err(|error| std::io::Error::other(error.to_string()))?,
            );
            let credentials: Arc<dyn CredentialStore> = Arc::new(KeyringCredentialStore);
            let model_manager = Arc::new(
                ModelManager::new(app_data.join("models")).map_err(std::io::Error::other)?,
            );
            let processing = Arc::new(ProcessingService::new(
                Arc::clone(&storage),
                Arc::clone(&credentials),
            ));
            let state = Arc::new(AppState {
                player,
                storage,
                credentials,
                providers: ProviderRegistry::default(),
                model_manager,
                processing,
                player_subscribers: Mutex::new(Vec::new()),
                native_surface_handle: native_surface.handle(),
            });
            app.manage(Arc::clone(&state));
            tauri::async_runtime::spawn(player_clock_loop(state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::pick_video,
            commands::inspect_runtime,
            commands::list_models,
            commands::install_model,
            commands::verify_model,
            commands::delete_model,
            commands::export_subtitles,
            commands::update_subtitle_cue,
            commands::open_media,
            commands::player_snapshot,
            commands::player_command,
            commands::subscribe_player_events,
            commands::processing_snapshot,
            commands::processing_command,
            commands::subscribe_processing_events,
            commands::get_settings,
            commands::save_settings,
            commands::list_providers,
            commands::credential_status,
            commands::set_provider_credential,
            commands::delete_provider_credential,
            commands::set_video_surface_rect,
            commands::toggle_fullscreen,
            commands::show_main_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Myna Player");
}
