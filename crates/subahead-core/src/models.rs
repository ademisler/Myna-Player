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
#[serde(rename_all = "camelCase")]
pub enum CueStatus {
    Queued,
    Transcribing,
    Transcribed,
    Translating,
    Ready,
    Failed,
}
