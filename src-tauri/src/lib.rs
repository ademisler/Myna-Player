use subahead_core::{
    AudioWindowRequest, AudioWindowResult, LookAheadPlan, LookAheadRequest, MediaMetadata,
    RuntimeStatus, TranscriptionRequest, TranscriptionResult, TranslationBatchRequest,
    TranslationBatchResult,
};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
async fn pick_video(app: AppHandle) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter(
            "Video files",
            &["mp4", "mkv", "avi", "mov", "m4v", "webm", "mpeg", "mpg"],
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
async fn inspect_runtime() -> RuntimeStatus {
    tauri::async_runtime::spawn_blocking(subahead_media::runtime_status)
        .await
        .unwrap_or_else(|_| subahead_media::runtime_status())
}

#[tauri::command]
async fn probe_media(path: String) -> Result<MediaMetadata, String> {
    tauri::async_runtime::spawn_blocking(move || subahead_media::probe_media(path))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn plan_lookahead(request: LookAheadRequest) -> LookAheadPlan {
    subahead_core::plan_lookahead(&request)
}

#[tauri::command]
async fn extract_audio_window(request: AudioWindowRequest) -> Result<AudioWindowResult, String> {
    tauri::async_runtime::spawn_blocking(move || subahead_media::extract_audio_window(&request))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn transcribe_audio(request: TranscriptionRequest) -> Result<TranscriptionResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        subahead_pipeline::transcribe_with_whisper_cli(&request)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn translate_segments(
    mut request: TranslationBatchRequest,
) -> Result<TranslationBatchResult, String> {
    if request.api_key.trim().is_empty() {
        if let Ok(api_key) = std::env::var("DEEPL_AUTH_KEY") {
            request.api_key = api_key;
        }
    }

    subahead_pipeline::translate_with_deepl(&request)
        .await
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            pick_video,
            inspect_runtime,
            probe_media,
            plan_lookahead,
            extract_audio_window,
            transcribe_audio,
            translate_segments
        ])
        .run(tauri::generate_context!())
        .expect("error while running SubAhead");
}
