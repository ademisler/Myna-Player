use std::sync::Arc;

use myna_player_core::{
    AppSettingsV1, CredentialStatus, MediaMetadata, ModelDescriptor, PlayerCommand, PlayerEvent,
    PlayerSnapshot, ProcessingCommand, ProcessingEvent, ProcessingSnapshot, ProviderDescriptor,
    RuntimeStatus, SubtitleEditRequest, SubtitleExportFormat, SubtitleExportTrack, TrackKind,
    VideoSurfaceRect,
};
use myna_player_storage::media_identity;
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow, ipc::Channel};
use tauri_plugin_dialog::DialogExt;

use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenMediaResult {
    pub metadata: MediaMetadata,
    pub player: PlayerSnapshot,
    pub processing: ProcessingSnapshot,
    pub resumed_at_ms: u64,
}

#[tauri::command]
pub async fn pick_video(app: AppHandle) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter(
            "Video files",
            &[
                "mp4", "mkv", "avi", "mov", "m4v", "webm", "mpeg", "mpg", "ts", "m2ts",
            ],
        )
        .blocking_pick_file();

    selected
        .map(|file| {
            file.into_path()
                .map(|path| path.to_string_lossy().into_owned())
                .map_err(|error| error.to_string())
        })
        .transpose()
}

#[tauri::command]
pub async fn inspect_runtime() -> RuntimeStatus {
    tauri::async_runtime::spawn_blocking(myna_player_media::runtime_status)
        .await
        .unwrap_or_else(|_| myna_player_media::runtime_status())
}

#[tauri::command]
pub fn list_models(state: State<'_, Arc<AppState>>) -> Vec<ModelDescriptor> {
    state.model_manager.list()
}

#[tauri::command]
pub async fn install_model(
    model_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ModelDescriptor, String> {
    state.model_manager.install(&model_id).await
}

#[tauri::command]
pub fn verify_model(
    model_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ModelDescriptor, String> {
    state.model_manager.verify(&model_id)
}

#[tauri::command]
pub fn delete_model(
    model_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<ModelDescriptor, String> {
    state.model_manager.delete(&model_id)
}

#[tauri::command]
pub async fn export_subtitles(
    format: SubtitleExportFormat,
    track: SubtitleExportTrack,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<String>, String> {
    let snapshot = state.processing.snapshot();
    let content = crate::subtitle_io::render_subtitles(
        format,
        track,
        &snapshot.source_segments,
        &snapshot.translated_cues,
    )?;
    let extension = match format {
        SubtitleExportFormat::Srt => "srt",
        SubtitleExportFormat::Vtt => "vtt",
    };
    let file_stem = snapshot
        .media_path
        .as_deref()
        .and_then(|path| std::path::Path::new(path).file_stem())
        .and_then(|name| name.to_str())
        .unwrap_or("subtitles");
    let suffix = match track {
        SubtitleExportTrack::Source => "source",
        SubtitleExportTrack::Translated => "translated",
        SubtitleExportTrack::Dual => "dual",
    };
    let selected = app
        .dialog()
        .file()
        .add_filter("Subtitle file", &[extension])
        .set_file_name(format!("{file_stem}.{suffix}.{extension}"))
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected.into_path().map_err(|error| error.to_string())?;
    std::fs::write(&path, content).map_err(|error| error.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn update_subtitle_cue(
    edit: SubtitleEditRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<ProcessingSnapshot, String> {
    state.processing.update_subtitle_cue(edit)
}

#[tauri::command]
pub async fn open_media(
    path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<OpenMediaResult, String> {
    let probe_path = path.clone();
    let metadata =
        tauri::async_runtime::spawn_blocking(move || myna_player_media::probe_media(probe_path))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
    let identity_path = path.clone();
    let identity = tauri::async_runtime::spawn_blocking(move || media_identity(identity_path))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    state
        .storage
        .upsert_media(&identity, metadata.duration_ms)
        .map_err(|error| error.to_string())?;
    let settings = state
        .storage
        .load_settings()
        .map_err(|error| error.to_string())?;
    let resumed_at_ms = if settings.playback.remember_position {
        state
            .storage
            .playback_position(&identity.fingerprint)
            .map_err(|error| error.to_string())?
            .min(metadata.duration_ms.saturating_sub(1_000))
    } else {
        0
    };

    let mut player = state
        .player
        .open(&path)
        .map_err(|error| error.to_string())?;
    if resumed_at_ms > 0 {
        player = state
            .player
            .command(PlayerCommand::Seek {
                position_ms: resumed_at_ms,
            })
            .map_err(|error| error.to_string())?;
    }
    let processing = state
        .processing
        .prepare(&metadata, identity, settings, 0, resumed_at_ms)?;
    state.broadcast_player(player.clone());

    Ok(OpenMediaResult {
        metadata,
        player,
        processing,
        resumed_at_ms,
    })
}

#[tauri::command]
pub fn player_snapshot(state: State<'_, Arc<AppState>>) -> PlayerSnapshot {
    state.player.snapshot()
}

#[tauri::command]
pub fn player_command(
    command: PlayerCommand,
    state: State<'_, Arc<AppState>>,
) -> Result<PlayerSnapshot, String> {
    let seek_position = match &command {
        PlayerCommand::Seek { position_ms } => Some(*position_ms),
        _ => None,
    };
    let selected_audio_id = match &command {
        PlayerCommand::SelectTrack {
            kind: TrackKind::Audio,
            id,
        } => Some(*id),
        _ => None,
    };
    let snapshot = state
        .player
        .command(command)
        .map_err(|error| error.to_string())?;
    if let Some(position_ms) = seek_position {
        state.processing.seek(position_ms);
    }
    if let Some(selected_audio_id) = selected_audio_id
        && let Some(relative_index) = snapshot
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Audio && track.id >= 0)
            .position(|track| track.id == selected_audio_id)
    {
        state.processing.select_audio_track(relative_index as u32)?;
    }
    state.broadcast_player(snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
pub fn subscribe_player_events(channel: Channel<PlayerEvent>, state: State<'_, Arc<AppState>>) {
    let _ = channel.send(PlayerEvent::Snapshot {
        snapshot: state.player.snapshot(),
    });
    if let Ok(mut subscribers) = state.player_subscribers.lock() {
        subscribers.push(channel);
    }
}

#[tauri::command]
pub fn subscribe_processing_events(
    channel: Channel<ProcessingEvent>,
    state: State<'_, Arc<AppState>>,
) {
    state.processing.subscribe(channel);
}

#[tauri::command]
pub fn processing_snapshot(state: State<'_, Arc<AppState>>) -> ProcessingSnapshot {
    state.processing.snapshot()
}

#[tauri::command]
pub fn processing_command(
    command: ProcessingCommand,
    state: State<'_, Arc<AppState>>,
) -> ProcessingSnapshot {
    match command {
        ProcessingCommand::Start | ProcessingCommand::Resume | ProcessingCommand::Retry => {
            state.processing.start_or_resume()
        }
        ProcessingCommand::Translate => state.processing.translate_now(),
        ProcessingCommand::Pause => state.processing.pause(),
        ProcessingCommand::Seek { position_ms } => state.processing.seek(position_ms),
        ProcessingCommand::PlayNow => state.processing.snapshot(),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Result<AppSettingsV1, String> {
    state
        .storage
        .load_settings()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_settings(
    settings: AppSettingsV1,
    state: State<'_, Arc<AppState>>,
) -> Result<AppSettingsV1, String> {
    if settings.translation.provider_id != "none" {
        let provider = state
            .providers
            .get(&settings.translation.provider_id)
            .ok_or_else(|| "unknown translation provider".to_string())?;
        if !provider.available {
            return Err(provider
                .unavailable_reason
                .unwrap_or_else(|| "translation provider is unavailable".into()));
        }
    }
    state
        .storage
        .save_settings(&settings)
        .map_err(|error| error.to_string())?;
    state.processing.update_settings(settings.clone());
    if settings.translation.auto_start && settings.translation.provider_id != "none" {
        state.processing.translate_now();
    }
    Ok(settings)
}

#[tauri::command]
pub fn list_providers(state: State<'_, Arc<AppState>>) -> Vec<ProviderDescriptor> {
    state.providers.list()
}

const CREDENTIAL_OPERATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[tauri::command]
pub async fn credential_status(
    provider_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<CredentialStatus, String> {
    let credentials = Arc::clone(&state.credentials);
    run_credential_operation(move || credentials.status(&provider_id)).await
}

#[tauri::command]
pub async fn set_provider_credential(
    provider_id: String,
    secret: String,
    state: State<'_, Arc<AppState>>,
) -> Result<CredentialStatus, String> {
    let credentials = Arc::clone(&state.credentials);
    run_credential_operation(move || {
        credentials.set(&provider_id, &secret)?;
        credentials.status(&provider_id)
    })
    .await
}

#[tauri::command]
pub async fn delete_provider_credential(
    provider_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<CredentialStatus, String> {
    let credentials = Arc::clone(&state.credentials);
    run_credential_operation(move || {
        credentials.delete(&provider_id)?;
        credentials.status(&provider_id)
    })
    .await
}

async fn run_credential_operation(
    operation: impl FnOnce() -> Result<CredentialStatus, myna_player_providers::CredentialError>
    + Send
    + 'static,
) -> Result<CredentialStatus, String> {
    let task = tauri::async_runtime::spawn_blocking(operation);
    match tokio::time::timeout(CREDENTIAL_OPERATION_TIMEOUT, task).await {
        Ok(Ok(result)) => result.map_err(|error| error.to_string()),
        Ok(Err(error)) => Err(format!("credential worker failed: {error}")),
        Err(_) => Err("credential store did not respond within 5 seconds".into()),
    }
}

#[tauri::command]
pub fn set_video_surface_rect(
    rect: VideoSurfaceRect,
    window: WebviewWindow,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    crate::native_surface::NativeVideoSurface::from_handle(state.native_surface_handle)
        .set_rect(&window, rect)
}

#[tauri::command]
pub fn toggle_fullscreen(window: WebviewWindow) -> Result<bool, String> {
    let next = !window.is_fullscreen().map_err(|error| error.to_string())?;
    window
        .set_fullscreen(next)
        .map_err(|error| error.to_string())?;
    Ok(next)
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window was not found".to_string())?;
    window.show().map_err(|error| error.to_string())
}
