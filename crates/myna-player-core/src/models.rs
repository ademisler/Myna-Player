use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDependency {
    pub name: String,
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub ffmpeg: RuntimeDependency,
    pub ffprobe: RuntimeDependency,
    pub whisper: RuntimeDependency,
    pub whisper_model: RuntimeDependency,
    pub vad_model: RuntimeDependency,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub installed: bool,
    pub verified: bool,
    pub installing: bool,
    pub downloaded_bytes: u64,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSnapshot {
    pub diagnostic_logging: bool,
    pub worker_running: bool,
    pub worker_model_path: Option<String>,
    pub worker_logs: Vec<String>,
    pub cache_usage_bytes: u64,
    pub database_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadata {
    pub path: String,
    pub file_name: String,
    pub duration_ms: u64,
    pub size_bytes: Option<u64>,
    pub format_name: Option<String>,
    pub video_streams: Vec<VideoStream>,
    pub audio_streams: Vec<AudioStream>,
    pub subtitle_streams: Vec<SubtitleStream>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoStream {
    pub index: u32,
    pub codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioStream {
    pub index: u32,
    pub relative_index: u32,
    pub codec: Option<String>,
    pub channels: Option<u32>,
    pub sample_rate: Option<u32>,
    pub language: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub player_track_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleStream {
    pub index: u32,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioWindowRequest {
    pub path: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub audio_relative_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioWindowResult {
    pub output_path: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub sample_rate: u32,
    pub channels: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionRequest {
    pub audio_path: String,
    pub window_start_ms: u64,
    pub model_path: Option<String>,
    #[serde(default)]
    pub vad_enabled: bool,
    #[serde(default)]
    pub vad_model_path: Option<String>,
    pub language_hint: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
    pub detected_language: Option<String>,
    pub segments: Vec<TranscriptSegment>,
    pub model_path: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TranslationProviderKind {
    None,
    DeeplFree,
    DeeplPro,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranslationBatchRequest {
    pub segments: Vec<TranscriptSegment>,
    pub source_language: Option<String>,
    pub target_language: String,
    pub provider: TranslationProviderKind,
    pub api_key: String,
    pub previous_context: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranslationBatchResult {
    pub cues: Vec<SubtitleCue>,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LookAheadRequest {
    pub playback_position_ms: u64,
    pub ready_until_ms: u64,
    pub media_duration_ms: u64,
    pub target_buffer_ms: Option<u64>,
    pub urgent_buffer_ms: Option<u64>,
    pub chunk_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingWindow {
    pub start_ms: u64,
    pub end_ms: u64,
    pub priority: ProcessingPriority,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProcessingPriority {
    Urgent,
    Normal,
    Background,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LookAheadPlan {
    pub current_buffer_ms: u64,
    pub target_buffer_ms: u64,
    pub windows: Vec<ProcessingWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub detected_language: Option<String>,
    pub language_confidence: Option<f32>,
    pub is_final: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCue {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: Option<String>,
    pub source_language: Option<String>,
    pub target_language: Option<String>,
    pub status: CueStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleExportFormat {
    Srt,
    Vtt,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleExportTrack {
    Source,
    Translated,
    Dual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleEditRequest {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CueStatus {
    Queued,
    Transcribing,
    Transcribed,
    Translating,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlayerState {
    Unavailable,
    Idle,
    Opening,
    Buffering,
    Playing,
    Paused,
    Stopped,
    Ended,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Audio,
    Subtitle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackDescriptor {
    pub id: i32,
    pub kind: TrackKind,
    pub label: String,
    pub language: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub available: bool,
    pub backend: String,
    pub state: PlayerState,
    pub media_path: Option<String>,
    pub file_name: Option<String>,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: u8,
    pub muted: bool,
    pub rate: f32,
    pub tracks: Vec<TrackDescriptor>,
    pub error: Option<String>,
}

impl PlayerSnapshot {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            backend: "unavailable".into(),
            state: PlayerState::Unavailable,
            media_path: None,
            file_name: None,
            position_ms: 0,
            duration_ms: 0,
            volume: 100,
            muted: false,
            rate: 1.0,
            tracks: Vec::new(),
            error: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlayerCommand {
    Play,
    Pause,
    TogglePlayback,
    Stop,
    Seek { position_ms: u64 },
    SetVolume { volume: u8 },
    SetMuted { muted: bool },
    SetRate { rate: f32 },
    SelectTrack { kind: TrackKind, id: i32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoSurfaceRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PlayerEvent {
    Snapshot { snapshot: PlayerSnapshot },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    pub theme: String,
    pub interface_language: String,
    pub preferred_subtitle_language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSettings {
    pub autoplay_after_initial_buffer: bool,
    pub remember_position: bool,
    pub preferred_audio_language: String,
    pub controls_hide_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSettings {
    pub preferred_track: String,
    pub font_scale: f32,
    pub vertical_offset_percent: u8,
    pub smart_wait_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSettings {
    pub auto_start: bool,
    pub spoken_language: String,
    pub model_path: Option<String>,
    #[serde(default = "default_vad_enabled")]
    pub vad_enabled: bool,
    #[serde(default)]
    pub vad_model_path: Option<String>,
    pub chunk_duration_ms: u64,
    pub initial_buffer_ms: u64,
    pub lookahead_ms: u64,
    pub process_full_media: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSettings {
    pub auto_start: bool,
    pub provider_id: String,
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
    pub target_language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StorageSettings {
    pub keep_completed_transcripts: bool,
    pub cache_limit_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedSettings {
    pub diagnostic_logging: bool,
    pub max_asr_workers: u8,
    pub max_translation_workers: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsV1 {
    pub schema_version: u32,
    pub general: GeneralSettings,
    pub playback: PlaybackSettings,
    pub subtitles: SubtitleSettings,
    pub transcription: TranscriptionSettings,
    pub translation: TranslationSettings,
    pub storage: StorageSettings,
    pub advanced: AdvancedSettings,
}

fn default_vad_enabled() -> bool {
    true
}

impl Default for AppSettingsV1 {
    fn default() -> Self {
        Self {
            schema_version: 1,
            general: GeneralSettings {
                theme: "dark".into(),
                interface_language: "en".into(),
                preferred_subtitle_language: "TR".into(),
            },
            playback: PlaybackSettings {
                autoplay_after_initial_buffer: true,
                remember_position: true,
                preferred_audio_language: "auto".into(),
                controls_hide_delay_ms: 2_200,
            },
            subtitles: SubtitleSettings {
                preferred_track: "translated".into(),
                font_scale: 1.0,
                vertical_offset_percent: 12,
                smart_wait_enabled: true,
            },
            transcription: TranscriptionSettings {
                auto_start: true,
                spoken_language: "auto".into(),
                model_path: None,
                vad_enabled: true,
                vad_model_path: None,
                chunk_duration_ms: 30_000,
                initial_buffer_ms: 30_000,
                lookahead_ms: 90_000,
                process_full_media: true,
            },
            translation: TranslationSettings {
                auto_start: false,
                provider_id: "none".into(),
                endpoint: "free".into(),
                model: String::new(),
                target_language: "TR".into(),
            },
            storage: StorageSettings {
                keep_completed_transcripts: true,
                cache_limit_mb: 2_048,
            },
            advanced: AdvancedSettings {
                diagnostic_logging: false,
                max_asr_workers: 1,
                max_translation_workers: 1,
            },
        }
    }
}

impl AppSettingsV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("unsupported settings schema version".into());
        }
        if !(5_000..=120_000).contains(&self.transcription.chunk_duration_ms) {
            return Err("transcription chunk duration must be between 5 and 120 seconds".into());
        }
        if self.transcription.initial_buffer_ms < 5_000
            || self.transcription.initial_buffer_ms > self.transcription.lookahead_ms
        {
            return Err("initial subtitle buffer must be between 5 seconds and look-ahead".into());
        }
        if !(10_000..=600_000).contains(&self.transcription.lookahead_ms) {
            return Err("look-ahead must be between 10 seconds and 10 minutes".into());
        }
        if !(0.6..=2.5).contains(&self.subtitles.font_scale) {
            return Err("subtitle font scale must be between 0.6 and 2.5".into());
        }
        if !(4..=35).contains(&self.subtitles.vertical_offset_percent) {
            return Err("subtitle vertical position must be between 4 and 35 percent".into());
        }
        if !matches!(
            self.subtitles.preferred_track.as_str(),
            "translated" | "source" | "dual"
        ) {
            return Err("unsupported generated subtitle track preference".into());
        }
        if self.playback.controls_hide_delay_ms > 30_000 {
            return Err("control hide delay cannot exceed 30 seconds".into());
        }
        if !(1..=4).contains(&self.advanced.max_asr_workers)
            || !(1..=4).contains(&self.advanced.max_translation_workers)
        {
            return Err("worker counts must be between 1 and 4".into());
        }
        if !matches!(
            self.translation.provider_id.as_str(),
            "none" | "deepl" | "openai" | "gemini" | "openrouter" | "minimax"
        ) {
            return Err("unsupported translation provider".into());
        }
        if self.translation.provider_id == "deepl"
            && !matches!(self.translation.endpoint.as_str(), "free" | "pro")
        {
            return Err("DeepL endpoint must be free or pro".into());
        }
        if self.translation.target_language.trim().is_empty() {
            return Err("translation target language cannot be empty".into());
        }
        if self.translation.auto_start && self.translation.provider_id == "none" {
            return Err("automatic translation requires a provider".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDescriptor {
    pub id: String,
    pub display_name: String,
    pub cloud: bool,
    pub requires_credential: bool,
    pub supported_endpoints: Vec<String>,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatus {
    pub provider_id: String,
    pub configured: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProcessingStage {
    Idle,
    Queued,
    Extracting,
    Transcribing,
    Translating,
    Ready,
    Paused,
    Failed,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingSnapshot {
    pub session_id: Option<String>,
    pub stage: ProcessingStage,
    pub media_path: Option<String>,
    pub generation: u64,
    pub current_window: Option<ProcessingWindow>,
    pub completed_windows: usize,
    pub total_windows: usize,
    pub ready_until_ms: u64,
    pub source_segments: Vec<TranscriptSegment>,
    pub translated_cues: Vec<SubtitleCue>,
    pub translation_running: bool,
    pub translation_error: Option<String>,
    pub status_message: String,
    pub error: Option<String>,
}

impl Default for ProcessingSnapshot {
    fn default() -> Self {
        Self {
            session_id: None,
            stage: ProcessingStage::Idle,
            media_path: None,
            generation: 0,
            current_window: None,
            completed_windows: 0,
            total_windows: 0,
            ready_until_ms: 0,
            source_segments: Vec::new(),
            translated_cues: Vec::new(),
            translation_running: false,
            translation_error: None,
            status_message: "Drop a video to begin.".into(),
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProcessingCommand {
    Start,
    Translate,
    Pause,
    Resume,
    Retry,
    Seek { position_ms: u64 },
    PlayNow,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub struct ProcessingPatch {
    pub source_upserts: Vec<TranscriptSegment>,
    pub translated_upserts: Vec<SubtitleCue>,
    pub removed_segment_ids: Vec<String>,
    pub completed_windows: usize,
    pub total_windows: usize,
    pub ready_until_ms: u64,
    pub stage: ProcessingStage,
    pub translation_running: bool,
    pub status_message: String,
    pub error: Option<String>,
    pub translation_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProcessingEvent {
    Snapshot { snapshot: ProcessingSnapshot },
    Patch { patch: ProcessingPatch },
}
