use async_trait::async_trait;
use subahead_core::{SubtitleCue, TranscriptSegment};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("speech recognition provider is unavailable: {0}")]
    AsrUnavailable(String),
    #[error("translation provider is unavailable: {0}")]
    TranslationUnavailable(String),
    #[error("provider failed: {0}")]
    Provider(String),
}

#[derive(Debug, Clone)]
pub struct TranscriptionRequest {
    pub audio_path: String,
    pub language_hint: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TranslationRequest {
    pub segments: Vec<TranscriptSegment>,
    pub source_language: Option<String>,
    pub target_language: String,
    pub previous_context: Vec<String>,
}

#[async_trait]
pub trait AsrEngine: Send + Sync {
    fn id(&self) -> &'static str;

    async fn transcribe(
        &self,
        request: TranscriptionRequest,
    ) -> Result<Vec<TranscriptSegment>, PipelineError>;
}

#[async_trait]
pub trait TranslationProvider: Send + Sync {
    fn id(&self) -> &'static str;

    async fn translate(
        &self,
        request: TranslationRequest,
    ) -> Result<Vec<SubtitleCue>, PipelineError>;
}

#[derive(Debug, Default)]
pub struct UnconfiguredWhisper;

#[async_trait]
impl AsrEngine for UnconfiguredWhisper {
    fn id(&self) -> &'static str {
        "whisper.cpp"
    }

    async fn transcribe(
        &self,
        _request: TranscriptionRequest,
    ) -> Result<Vec<TranscriptSegment>, PipelineError> {
        Err(PipelineError::AsrUnavailable(
            "whisper.cpp model and executable are not configured yet".into(),
        ))
    }
}
