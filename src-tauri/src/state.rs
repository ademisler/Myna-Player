use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use myna_player_core::{
    AppSettingsV1, MediaMetadata, PlayerEvent, PlayerSnapshot, ProcessingEvent, ProcessingSnapshot,
    ProcessingStage, ProcessingWindow, SubtitleCue, SubtitleEditRequest, TranscriptSegment,
    TranscriptionRequest, TranslationBatchRequest, TranslationProviderKind,
};
use myna_player_jobs::{ProcessingQueue, ScheduledWindow};
use myna_player_player::PlayerEngine;
use myna_player_providers::{CredentialStore, ProviderRegistry};
use myna_player_storage::{MediaIdentity, Storage};
use tauri::ipc::Channel;

const PIPELINE_VERSION: &str = "stream-v3";
const EXTRACTION_CONTEXT_MS: u64 = 2_000;

pub struct AppState {
    pub player: Arc<dyn PlayerEngine>,
    pub storage: Arc<Storage>,
    pub credentials: Arc<dyn CredentialStore>,
    pub providers: ProviderRegistry,
    pub model_manager: Arc<crate::model_manager::ModelManager>,
    pub processing: Arc<ProcessingService>,
    pub player_subscribers: Mutex<Vec<Channel<PlayerEvent>>>,
    pub native_surface_handle: usize,
}

impl AppState {
    pub fn broadcast_player(&self, snapshot: PlayerSnapshot) {
        let Ok(mut subscribers) = self.player_subscribers.lock() else {
            return;
        };
        subscribers.retain(|channel| {
            channel
                .send(PlayerEvent::Snapshot {
                    snapshot: snapshot.clone(),
                })
                .is_ok()
        });
    }
}

struct ProcessingSession {
    session_id: String,
    path: String,
    metadata: MediaMetadata,
    identity: MediaIdentity,
    duration_ms: u64,
    audio_track: u32,
    settings: AppSettingsV1,
    queue: ProcessingQueue,
    paused: bool,
    worker_running: bool,
    playback_position_ms: u64,
    cancel_token: Arc<AtomicBool>,
    force_translation: bool,
}

struct ProcessingInner {
    session: Option<ProcessingSession>,
    snapshot: ProcessingSnapshot,
    subscribers: Vec<Channel<ProcessingEvent>>,
}

pub struct ProcessingService {
    storage: Arc<Storage>,
    credentials: Arc<dyn CredentialStore>,
    asr: Arc<myna_player_pipeline::PersistentWhisper>,
    inner: Mutex<ProcessingInner>,
}

impl ProcessingService {
    pub fn new(storage: Arc<Storage>, credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            storage,
            credentials,
            asr: Arc::new(myna_player_pipeline::PersistentWhisper::default()),
            inner: Mutex::new(ProcessingInner {
                session: None,
                snapshot: ProcessingSnapshot::default(),
                subscribers: Vec::new(),
            }),
        }
    }

    pub fn snapshot(&self) -> ProcessingSnapshot {
        self.inner
            .lock()
            .map(|inner| inner.snapshot.clone())
            .unwrap_or_else(|_| ProcessingSnapshot {
                stage: ProcessingStage::Failed,
                error: Some("processing state lock was poisoned".into()),
                status_message: "Subtitle processing is unavailable.".into(),
                ..ProcessingSnapshot::default()
            })
    }

    pub fn update_subtitle_cue(
        &self,
        edit: SubtitleEditRequest,
    ) -> Result<ProcessingSnapshot, String> {
        let (
            session_id,
            generation,
            fingerprint,
            audio_track,
            duration_ms,
            settings,
            mut source,
            mut translated,
        ) = {
            let inner = self
                .inner
                .lock()
                .map_err(|_| "processing state lock was poisoned".to_string())?;
            let session = inner
                .session
                .as_ref()
                .ok_or_else(|| "Open a video before editing subtitles.".to_string())?;
            (
                session.session_id.clone(),
                session.queue.generation(),
                session.identity.fingerprint.clone(),
                session.audio_track,
                session.duration_ms,
                session.settings.clone(),
                inner.snapshot.source_segments.clone(),
                inner.snapshot.translated_cues.clone(),
            )
        };

        let source_text = edit.source_text.trim().to_owned();
        if source_text.is_empty() {
            return Err("Source subtitle text cannot be empty.".into());
        }
        if edit.end_ms <= edit.start_ms {
            return Err("Subtitle end time must be after its start time.".into());
        }
        if edit.end_ms > duration_ms {
            return Err("Subtitle timing cannot exceed the media duration.".into());
        }
        let segment = source
            .iter_mut()
            .find(|segment| segment.id == edit.id)
            .ok_or_else(|| "Subtitle cue was not found.".to_string())?;
        segment.start_ms = edit.start_ms;
        segment.end_ms = edit.end_ms;
        segment.text = source_text.clone();
        segment.is_final = true;
        source.sort_by_key(|segment| (segment.start_ms, segment.end_ms));
        if source
            .windows(2)
            .any(|pair| pair[0].end_ms > pair[1].start_ms)
        {
            return Err("Subtitle timing overlaps a neighboring cue.".into());
        }
        let updated_segment = source
            .iter()
            .find(|segment| segment.id == edit.id)
            .cloned()
            .ok_or_else(|| "Subtitle cue disappeared during validation.".to_string())?;

        let translated_text = edit
            .translated_text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned);
        let mut updated_translation = None;
        if let Some(cue) = translated.iter_mut().find(|cue| cue.id == edit.id) {
            cue.start_ms = edit.start_ms;
            cue.end_ms = edit.end_ms;
            cue.source_text = source_text;
            cue.translated_text = translated_text;
            updated_translation = Some(cue.clone());
        }

        self.storage
            .store_transcript_segments(
                &fingerprint,
                audio_track,
                PIPELINE_VERSION,
                std::slice::from_ref(&updated_segment),
            )
            .map_err(|error| error.to_string())?;
        if let Some(cue) = updated_translation.as_ref()
            && settings.translation.provider_id != "none"
        {
            self.storage
                .store_translations(
                    &fingerprint,
                    audio_track,
                    PIPELINE_VERSION,
                    &settings.translation.provider_id,
                    &settings.translation.target_language,
                    std::slice::from_ref(cue),
                )
                .map_err(|error| error.to_string())?;
        }

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "processing state lock was poisoned".to_string())?;
        if !inner.session.as_ref().is_some_and(|session| {
            session.session_id == session_id && session.queue.generation() == generation
        }) {
            return Err("The media session changed while saving the cue.".into());
        }
        inner.snapshot.source_segments = source;
        inner.snapshot.translated_cues = translated;
        inner.snapshot.status_message = "Subtitle correction saved.".into();
        inner.snapshot.error = None;
        broadcast_processing_locked(&mut inner);
        Ok(inner.snapshot.clone())
    }

    pub fn subscribe(&self, channel: Channel<ProcessingEvent>) {
        let snapshot = self.snapshot();
        let _ = channel.send(ProcessingEvent::Snapshot { snapshot });
        if let Ok(mut inner) = self.inner.lock() {
            inner.subscribers.push(channel);
        }
    }

    pub fn prepare(
        self: &Arc<Self>,
        metadata: &MediaMetadata,
        identity: MediaIdentity,
        settings: AppSettingsV1,
        audio_track: u32,
        resume_position_ms: u64,
    ) -> Result<ProcessingSnapshot, String> {
        self.asr.cancel_current();
        let completed = self
            .storage
            .completed_windows(&identity.fingerprint, audio_track, PIPELINE_VERSION)
            .map_err(|error| error.to_string())?;
        let source_segments = self
            .storage
            .load_transcript_segments(&identity.fingerprint, audio_track, PIPELINE_VERSION)
            .map_err(|error| error.to_string())?;
        let translated_cues = if settings.translation.provider_id != "none" {
            self.storage
                .load_translated_cues(
                    &identity.fingerprint,
                    audio_track,
                    PIPELINE_VERSION,
                    &settings.translation.provider_id,
                    &settings.translation.target_language,
                )
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut queue = ProcessingQueue::new(
            metadata.duration_ms,
            settings.transcription.chunk_duration_ms,
            settings.transcription.lookahead_ms,
            settings.transcription.process_full_media,
        );
        queue.restore_completed(completed);
        queue.schedule_initial(resume_position_ms);
        let ready_until_ms = queue.ready_until_from(resume_position_ms);
        let session_id = uuid::Uuid::new_v4().to_string();
        let snapshot = ProcessingSnapshot {
            session_id: Some(session_id.clone()),
            stage: if settings.transcription.auto_start {
                ProcessingStage::Queued
            } else {
                ProcessingStage::Idle
            },
            media_path: Some(metadata.path.clone()),
            generation: queue.generation(),
            current_window: None,
            completed_windows: queue.completed_windows(),
            total_windows: queue.total_windows(),
            ready_until_ms,
            source_segments,
            translated_cues,
            status_message: if settings.transcription.auto_start {
                "Preparing the first subtitle window…".into()
            } else {
                "Automatic transcription is disabled.".into()
            },
            error: None,
        };

        {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "processing state lock was poisoned".to_string())?;
            if let Some(previous) = inner.session.as_ref() {
                previous.cancel_token.store(true, Ordering::Relaxed);
            }
            inner.session = Some(ProcessingSession {
                session_id,
                path: metadata.path.clone(),
                metadata: metadata.clone(),
                identity,
                duration_ms: metadata.duration_ms,
                audio_track,
                settings: settings.clone(),
                queue,
                paused: !settings.transcription.auto_start,
                worker_running: false,
                playback_position_ms: resume_position_ms,
                cancel_token: Arc::new(AtomicBool::new(false)),
                force_translation: false,
            });
            inner.snapshot = snapshot.clone();
            broadcast_processing_locked(&mut inner);
        }

        if settings.transcription.auto_start {
            self.spawn_worker();
        }
        Ok(snapshot)
    }

    pub fn select_audio_track(
        self: &Arc<Self>,
        audio_track: u32,
    ) -> Result<ProcessingSnapshot, String> {
        let selection = self
            .inner
            .lock()
            .map_err(|_| "processing state lock was poisoned".to_string())?
            .session
            .as_ref()
            .map(|session| {
                (
                    session.audio_track,
                    session.metadata.clone(),
                    session.identity.clone(),
                    session.settings.clone(),
                    session.playback_position_ms,
                )
            });
        let Some((current, metadata, identity, settings, position_ms)) = selection else {
            return Ok(self.snapshot());
        };
        if current == audio_track {
            return Ok(self.snapshot());
        }
        self.prepare(&metadata, identity, settings, audio_track, position_ms)
    }

    pub fn update_settings(&self, settings: AppSettingsV1) {
        if let Ok(mut inner) = self.inner.lock()
            && let Some(session) = inner.session.as_mut()
        {
            session.settings = settings;
        }
    }

    pub fn start_or_resume(self: &Arc<Self>) -> ProcessingSnapshot {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(session) = inner.session.as_mut() {
                session.paused = false;
                if session.cancel_token.load(Ordering::Relaxed) {
                    session.cancel_token = Arc::new(AtomicBool::new(false));
                }
                inner.snapshot.stage = ProcessingStage::Queued;
                inner.snapshot.status_message = "Subtitle processing queued…".into();
                inner.snapshot.error = None;
                broadcast_processing_locked(&mut inner);
            } else {
                inner.snapshot.status_message =
                    "Open a video before starting transcription.".into();
                inner.snapshot.error = Some("no active media".into());
                broadcast_processing_locked(&mut inner);
                return inner.snapshot.clone();
            }
        }
        self.spawn_worker();
        self.snapshot()
    }

    pub fn pause(&self) -> ProcessingSnapshot {
        self.asr.cancel_current();
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(session) = inner.session.as_mut() {
                session.paused = true;
                session.cancel_token.store(true, Ordering::Relaxed);
                let position = session.playback_position_ms;
                session.queue.seek(position);
                inner.snapshot.generation = session.queue.generation();
                inner.snapshot.stage = ProcessingStage::Paused;
                inner.snapshot.status_message = "Subtitle processing paused.".into();
            }
            broadcast_processing_locked(&mut inner);
            return inner.snapshot.clone();
        }
        self.snapshot()
    }

    pub fn translate_now(self: &Arc<Self>) -> ProcessingSnapshot {
        if let Ok(mut inner) = self.inner.lock()
            && let Some(session) = inner.session.as_mut()
        {
            if session.settings.translation.provider_id == "none" {
                inner.snapshot.error =
                    Some("Choose a translation provider in Settings first.".into());
                inner.snapshot.status_message = "Translation provider is disabled.".into();
                broadcast_processing_locked(&mut inner);
                return inner.snapshot.clone();
            }
            session.force_translation = true;
            session.paused = false;
            inner.snapshot.stage = ProcessingStage::Queued;
            inner.snapshot.status_message = "Translation queued…".into();
            inner.snapshot.error = None;
            broadcast_processing_locked(&mut inner);
        }
        self.spawn_worker();
        self.snapshot()
    }

    pub fn seek(self: &Arc<Self>, position_ms: u64) -> ProcessingSnapshot {
        self.asr.cancel_current();
        let should_run = if let Ok(mut inner) = self.inner.lock() {
            let mut should_run = false;
            let mut generation = None;
            if let Some(session) = inner.session.as_mut() {
                session.cancel_token.store(true, Ordering::Relaxed);
                session.cancel_token = Arc::new(AtomicBool::new(false));
                session.playback_position_ms = position_ms.min(session.duration_ms);
                session.queue.seek(session.playback_position_ms);
                generation = Some(session.queue.generation());
                should_run = !session.paused;
            }
            if let Some(generation) = generation {
                inner.snapshot.generation = generation;
                inner.snapshot.stage = ProcessingStage::Queued;
                inner.snapshot.current_window = None;
                inner.snapshot.status_message =
                    "Prioritizing subtitles at the new playback position…".into();
                inner.snapshot.error = None;
            }
            broadcast_processing_locked(&mut inner);
            should_run
        } else {
            false
        };
        if should_run {
            self.spawn_worker();
        }
        self.snapshot()
    }

    pub fn update_playback_position(&self, position_ms: u64) {
        if let Ok(mut inner) = self.inner.lock()
            && let Some(session) = inner.session.as_mut()
        {
            session.playback_position_ms = position_ms.min(session.duration_ms);
            session
                .queue
                .promote_lookahead(session.playback_position_ms);
        }
    }

    fn spawn_worker(self: &Arc<Self>) {
        let generation = {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            let Some(session) = inner.session.as_mut() else {
                return;
            };
            if session.paused || session.worker_running {
                return;
            }
            session.worker_running = true;
            session.queue.generation()
        };

        let service = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            service.run_worker(generation).await;
            let restart = {
                let Ok(mut inner) = service.inner.lock() else {
                    return;
                };
                let Some(session) = inner.session.as_mut() else {
                    return;
                };
                session.worker_running = false;
                !session.paused
                    && session.queue.generation() != generation
                    && !matches!(
                        inner.snapshot.stage,
                        ProcessingStage::Failed | ProcessingStage::Unavailable
                    )
            };
            if restart {
                service.spawn_worker();
            }
        });
    }

    async fn run_worker(self: &Arc<Self>, generation: u64) {
        loop {
            let action = {
                let Ok(mut inner) = self.inner.lock() else {
                    return;
                };
                let Some(session) = inner.session.as_mut() else {
                    return;
                };
                if session.paused || session.queue.generation() != generation {
                    return;
                }
                if session.force_translation {
                    session.force_translation = false;
                    let context = TranslationContext {
                        session_id: session.session_id.clone(),
                        fingerprint: session.identity.fingerprint.clone(),
                        audio_track: session.audio_track,
                        settings: session.settings.clone(),
                        generation,
                    };
                    inner.snapshot.stage = ProcessingStage::Translating;
                    inner.snapshot.current_window = None;
                    inner.snapshot.status_message =
                        "Translating completed transcript segments…".into();
                    inner.snapshot.error = None;
                    broadcast_processing_locked(&mut inner);
                    WorkerAction::Translate(context)
                } else if let Some(job) = session.queue.pop() {
                    let context = JobContext {
                        session_id: session.session_id.clone(),
                        path: session.path.clone(),
                        fingerprint: session.identity.fingerprint.clone(),
                        duration_ms: session.duration_ms,
                        audio_track: session.audio_track,
                        settings: session.settings.clone(),
                        job: job.clone(),
                        cancel_token: Arc::clone(&session.cancel_token),
                    };
                    inner.snapshot.stage = ProcessingStage::Extracting;
                    inner.snapshot.current_window = Some(job.window.clone());
                    inner.snapshot.status_message = format!(
                        "Extracting audio {}–{}…",
                        format_time(job.window.start_ms),
                        format_time(job.window.end_ms)
                    );
                    inner.snapshot.error = None;
                    broadcast_processing_locked(&mut inner);
                    WorkerAction::Window(context)
                } else {
                    inner.snapshot.stage = ProcessingStage::Ready;
                    inner.snapshot.current_window = None;
                    inner.snapshot.status_message = "Transcript processing complete.".into();
                    broadcast_processing_locked(&mut inner);
                    WorkerAction::Finished
                }
            };

            let WorkerAction::Window(job_context) = action else {
                match action {
                    WorkerAction::Translate(context) => {
                        if let Err(error) = self.translate_remaining(&context).await {
                            self.report_translation_error(&context, error);
                        }
                        continue;
                    }
                    WorkerAction::Finished => return,
                    WorkerAction::Window(_) => unreachable!(),
                }
            };

            if let Err(error) = self.storage.mark_window_running(
                &job_context.fingerprint,
                job_context.audio_track,
                PIPELINE_VERSION,
                job_context.job.window.start_ms,
                job_context.job.window.end_ms,
                generation,
            ) {
                self.fail_job(&job_context, error.to_string(), false);
                return;
            }

            match self.process_window(&job_context).await {
                Ok(result) => {
                    if !self.is_current(&job_context.session_id, generation) {
                        return;
                    }
                    if let Err(error) = self.complete_job(&job_context, result) {
                        self.fail_job(&job_context, error, false);
                        return;
                    }
                }
                Err(WindowError::Cancelled) => return,
                Err(WindowError::Unavailable(error)) => {
                    self.fail_job(&job_context, error, true);
                    return;
                }
                Err(WindowError::Failed(error)) => {
                    self.fail_job(&job_context, error, false);
                    return;
                }
            }
        }
    }

    async fn translate_remaining(&self, context: &TranslationContext) -> Result<(), String> {
        if context.settings.translation.provider_id == "none" {
            return Err("Select a translation provider first.".into());
        }
        let source = self
            .storage
            .load_transcript_segments(&context.fingerprint, context.audio_track, PIPELINE_VERSION)
            .map_err(|error| error.to_string())?;
        let existing = self
            .storage
            .load_translated_cues(
                &context.fingerprint,
                context.audio_track,
                PIPELINE_VERSION,
                &context.settings.translation.provider_id,
                &context.settings.translation.target_language,
            )
            .unwrap_or_default();
        let translated_ids = existing
            .iter()
            .map(|cue| cue.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let pending = source
            .iter()
            .filter(|segment| !translated_ids.contains(segment.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return Ok(());
        }
        let cues = self
            .translate_segments(
                &context.settings,
                pending.clone(),
                pending
                    .iter()
                    .find_map(|segment| segment.detected_language.clone()),
                Vec::new(),
            )
            .await?;
        self.storage
            .store_translations(
                &context.fingerprint,
                context.audio_track,
                PIPELINE_VERSION,
                &context.settings.translation.provider_id,
                &context.settings.translation.target_language,
                &cues,
            )
            .map_err(|error| error.to_string())?;
        let all_translations = self
            .storage
            .load_translated_cues(
                &context.fingerprint,
                context.audio_track,
                PIPELINE_VERSION,
                &context.settings.translation.provider_id,
                &context.settings.translation.target_language,
            )
            .map_err(|error| error.to_string())?;
        if let Ok(mut inner) = self.inner.lock()
            && inner.session.as_ref().is_some_and(|session| {
                session.session_id == context.session_id
                    && session.queue.generation() == context.generation
            })
        {
            inner.snapshot.translated_cues = all_translations;
            inner.snapshot.status_message =
                format!("Translated {} transcript segment(s).", cues.len());
            inner.snapshot.error = None;
            broadcast_processing_locked(&mut inner);
        }
        Ok(())
    }

    async fn translate_segments(
        &self,
        settings: &AppSettingsV1,
        segments: Vec<TranscriptSegment>,
        source_language: Option<String>,
        previous_context: Vec<String>,
    ) -> Result<Vec<SubtitleCue>, String> {
        let provider_id = settings.translation.provider_id.as_str();
        if provider_id == "none" {
            return Ok(Vec::new());
        }
        let api_key = self
            .credential_for_provider(provider_id.to_owned())
            .await?
            .ok_or_else(|| {
                format!(
                    "{} is selected but no credential is configured.",
                    provider_display_name(provider_id)
                )
            })?;
        if provider_id == "deepl" {
            let provider = if settings.translation.endpoint == "pro" {
                TranslationProviderKind::DeeplPro
            } else {
                TranslationProviderKind::DeeplFree
            };
            return myna_player_pipeline::translate_with_deepl(&TranslationBatchRequest {
                source_language,
                previous_context,
                target_language: settings.translation.target_language.clone(),
                provider,
                api_key,
                segments,
            })
            .await
            .map(|result| result.cues)
            .map_err(|error| error.to_string());
        }
        let model = if settings.translation.model.trim().is_empty() {
            default_provider_model(provider_id).to_owned()
        } else {
            settings.translation.model.trim().to_owned()
        };
        myna_player_pipeline::translate_with_llm(&myna_player_pipeline::LlmTranslationRequest {
            provider_id: provider_id.to_owned(),
            model,
            api_key,
            source_language,
            target_language: settings.translation.target_language.clone(),
            segments,
            previous_context,
        })
        .await
        .map_err(|error| error.to_string())
    }

    async fn credential_for_provider(&self, provider_id: String) -> Result<Option<String>, String> {
        if let Some(secret) = provider_environment_credential(&provider_id) {
            return Ok(Some(secret));
        }
        let credentials = Arc::clone(&self.credentials);
        let task = tauri::async_runtime::spawn_blocking(move || credentials.get(&provider_id));
        match tokio::time::timeout(Duration::from_secs(5), task).await {
            Ok(Ok(result)) => result.map_err(|error| error.to_string()),
            Ok(Err(error)) => Err(format!("credential worker failed: {error}")),
            Err(_) => Err("credential store did not respond within 5 seconds".into()),
        }
    }

    fn report_translation_error(&self, context: &TranslationContext, error: String) {
        if let Ok(mut inner) = self.inner.lock()
            && inner.session.as_ref().is_some_and(|session| {
                session.session_id == context.session_id
                    && session.queue.generation() == context.generation
            })
        {
            inner.snapshot.status_message =
                "Source transcript is safe; translation needs attention.".into();
            inner.snapshot.error = Some(error);
            broadcast_processing_locked(&mut inner);
        }
    }

    async fn process_window(&self, context: &JobContext) -> Result<WindowResult, WindowError> {
        let canonical = &context.job.window;
        let extraction_start = canonical.start_ms.saturating_sub(EXTRACTION_CONTEXT_MS);
        let extraction_end = canonical
            .end_ms
            .saturating_add(EXTRACTION_CONTEXT_MS)
            .min(context.duration_ms);
        let audio_request = myna_player_core::AudioWindowRequest {
            path: context.path.clone(),
            start_ms: extraction_start,
            duration_ms: extraction_end.saturating_sub(extraction_start),
            audio_relative_index: Some(context.audio_track),
        };

        let audio = tauri::async_runtime::spawn_blocking(move || {
            myna_player_media::extract_audio_window(&audio_request)
        })
        .await
        .map_err(|error| WindowError::Failed(error.to_string()))?
        .map_err(|error| WindowError::Unavailable(error.to_string()))?;
        if !self.is_current(&context.session_id, context.job.generation) {
            let _ = myna_player_media::cleanup_audio_window(&audio.output_path);
            return Err(WindowError::Cancelled);
        }

        self.set_stage(
            ProcessingStage::Transcribing,
            format!(
                "Transcribing {}–{} locally…",
                format_time(canonical.start_ms),
                format_time(canonical.end_ms)
            ),
        );
        let transcription_request = TranscriptionRequest {
            audio_path: audio.output_path.clone(),
            window_start_ms: audio.start_ms,
            model_path: context.settings.transcription.model_path.clone(),
            vad_enabled: context.settings.transcription.vad_enabled,
            vad_model_path: context.settings.transcription.vad_model_path.clone(),
            language_hint: Some(context.settings.transcription.spoken_language.clone()),
            prompt: self.previous_context(
                &context.fingerprint,
                context.audio_track,
                canonical.start_ms,
            ),
        };
        let asr = Arc::clone(&self.asr);
        let cancel_token = Arc::clone(&context.cancel_token);
        let transcription = tauri::async_runtime::spawn_blocking(move || {
            asr.transcribe(&transcription_request, cancel_token)
        })
        .await
        .map_err(|error| WindowError::Failed(error.to_string()))?;
        let _ = myna_player_media::cleanup_audio_window(&audio.output_path);
        let transcription = transcription.map_err(|error| {
            if context.cancel_token.load(Ordering::Relaxed) {
                WindowError::Cancelled
            } else {
                match error {
                    myna_player_pipeline::PipelineError::AsrUnavailable(message) => {
                        WindowError::Unavailable(message)
                    }
                    other => WindowError::Failed(other.to_string()),
                }
            }
        })?;
        if !self.is_current(&context.session_id, context.job.generation) {
            return Err(WindowError::Cancelled);
        }

        let segments = canonical_segments(
            &context.fingerprint,
            context.audio_track,
            canonical,
            transcription.segments,
        );
        let mut translated_cues = Vec::new();
        let mut translation_error = None;
        if context.settings.translation.auto_start
            && context.settings.translation.provider_id != "none"
            && !segments.is_empty()
        {
            self.set_stage(
                ProcessingStage::Translating,
                format!("Translating {} subtitle segment(s)…", segments.len()),
            );
            match self
                .translate_segments(
                    &context.settings,
                    segments.clone(),
                    transcription.detected_language,
                    self.previous_context_segments(
                        &context.fingerprint,
                        context.audio_track,
                        canonical.start_ms,
                    ),
                )
                .await
            {
                Ok(cues) => translated_cues = cues,
                Err(error) => translation_error = Some(error),
            }
        }

        Ok(WindowResult {
            segments,
            translated_cues,
            translation_error,
        })
    }

    fn complete_job(&self, context: &JobContext, result: WindowResult) -> Result<(), String> {
        self.storage
            .store_transcript_segments(
                &context.fingerprint,
                context.audio_track,
                PIPELINE_VERSION,
                &result.segments,
            )
            .map_err(|error| error.to_string())?;
        if !result.translated_cues.is_empty() {
            self.storage
                .store_translations(
                    &context.fingerprint,
                    context.audio_track,
                    PIPELINE_VERSION,
                    &context.settings.translation.provider_id,
                    &context.settings.translation.target_language,
                    &result.translated_cues,
                )
                .map_err(|error| error.to_string())?;
        }
        self.storage
            .mark_window_completed(
                &context.fingerprint,
                context.audio_track,
                PIPELINE_VERSION,
                context.job.window.start_ms,
                context.job.window.end_ms,
                context.job.generation,
            )
            .map_err(|error| error.to_string())?;

        let all_segments = self
            .storage
            .load_transcript_segments(&context.fingerprint, context.audio_track, PIPELINE_VERSION)
            .map_err(|error| error.to_string())?;
        let all_translations = if context.settings.translation.provider_id != "none" {
            self.storage
                .load_translated_cues(
                    &context.fingerprint,
                    context.audio_track,
                    PIPELINE_VERSION,
                    &context.settings.translation.provider_id,
                    &context.settings.translation.target_language,
                )
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "processing state lock was poisoned".to_string())?;
        let (completed_windows, total_windows, ready_until_ms) = {
            let session = inner
                .session
                .as_mut()
                .ok_or_else(|| "processing session disappeared".to_string())?;
            if session.session_id != context.session_id
                || session.queue.generation() != context.job.generation
            {
                return Ok(());
            }
            session.queue.mark_completed(&context.job.window);
            (
                session.queue.completed_windows(),
                session.queue.total_windows(),
                session.queue.ready_until_from(session.playback_position_ms),
            )
        };
        inner.snapshot.completed_windows = completed_windows;
        inner.snapshot.total_windows = total_windows;
        inner.snapshot.ready_until_ms = ready_until_ms;
        inner.snapshot.source_segments = all_segments;
        inner.snapshot.translated_cues = all_translations;
        inner.snapshot.current_window = None;
        inner.snapshot.stage = ProcessingStage::Queued;
        inner.snapshot.status_message = if let Some(ref error) = result.translation_error {
            format!("Transcript saved. Translation needs attention: {error}")
        } else {
            format!("Subtitle window ready · {completed_windows}/{total_windows}")
        };
        inner.snapshot.error = result.translation_error;
        broadcast_processing_locked(&mut inner);
        Ok(())
    }

    fn fail_job(&self, context: &JobContext, error: String, unavailable: bool) {
        let _ = self.storage.mark_window_failed(
            &context.fingerprint,
            context.audio_track,
            PIPELINE_VERSION,
            context.job.window.start_ms,
            context.job.window.end_ms,
            context.job.generation,
            &error,
        );
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(session) = inner.session.as_mut()
                && session.session_id == context.session_id
                && session.queue.generation() == context.job.generation
            {
                session.queue.requeue(context.job.clone());
                session.paused = true;
            }
            inner.snapshot.stage = if unavailable {
                ProcessingStage::Unavailable
            } else {
                ProcessingStage::Failed
            };
            inner.snapshot.current_window = None;
            inner.snapshot.status_message = if unavailable {
                "Local subtitle engine is unavailable.".into()
            } else {
                "Subtitle processing stopped.".into()
            };
            inner.snapshot.error = Some(error);
            broadcast_processing_locked(&mut inner);
        }
    }

    fn is_current(&self, session_id: &str, generation: u64) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| {
                inner.session.as_ref().map(|session| {
                    session.session_id == session_id
                        && session.queue.generation() == generation
                        && !session.paused
                })
            })
            .unwrap_or(false)
    }

    fn set_stage(&self, stage: ProcessingStage, message: String) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.snapshot.stage = stage;
            inner.snapshot.status_message = message;
            broadcast_processing_locked(&mut inner);
        }
    }

    fn previous_context(
        &self,
        fingerprint: &str,
        audio_track: u32,
        before_ms: u64,
    ) -> Option<String> {
        self.previous_context_segments(fingerprint, audio_track, before_ms)
            .last()
            .cloned()
    }

    fn previous_context_segments(
        &self,
        fingerprint: &str,
        audio_track: u32,
        before_ms: u64,
    ) -> Vec<String> {
        self.storage
            .load_transcript_segments(fingerprint, audio_track, PIPELINE_VERSION)
            .unwrap_or_default()
            .into_iter()
            .filter(|segment| segment.end_ms <= before_ms)
            .rev()
            .take(8)
            .map(|segment| segment.text)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

#[derive(Clone)]
struct JobContext {
    session_id: String,
    path: String,
    fingerprint: String,
    duration_ms: u64,
    audio_track: u32,
    settings: AppSettingsV1,
    job: ScheduledWindow,
    cancel_token: Arc<AtomicBool>,
}

struct TranslationContext {
    session_id: String,
    fingerprint: String,
    audio_track: u32,
    settings: AppSettingsV1,
    generation: u64,
}

enum WorkerAction {
    Window(JobContext),
    Translate(TranslationContext),
    Finished,
}

struct WindowResult {
    segments: Vec<TranscriptSegment>,
    translated_cues: Vec<SubtitleCue>,
    translation_error: Option<String>,
}

enum WindowError {
    Cancelled,
    Unavailable(String),
    Failed(String),
}

fn canonical_segments(
    fingerprint: &str,
    audio_track: u32,
    window: &ProcessingWindow,
    segments: Vec<TranscriptSegment>,
) -> Vec<TranscriptSegment> {
    segments
        .into_iter()
        .filter_map(|mut segment| {
            let midpoint = segment.start_ms.saturating_add(segment.end_ms) / 2;
            let is_last_edge = segment.start_ms < window.end_ms && segment.end_ms == window.end_ms;
            if midpoint < window.start_ms
                || (midpoint >= window.end_ms && !is_last_edge)
                || segment.text.trim().is_empty()
            {
                return None;
            }
            segment.start_ms = segment.start_ms.max(window.start_ms);
            segment.end_ms = segment.end_ms.min(window.end_ms);
            segment.id = format!(
                "{}:{}:{}:{}",
                &fingerprint[..fingerprint.len().min(12)],
                audio_track,
                segment.start_ms,
                segment.end_ms
            );
            segment.is_final = true;
            Some(segment)
        })
        .collect()
}

fn provider_environment_credential(provider_id: &str) -> Option<String> {
    let variable = match provider_id {
        "deepl" => "DEEPL_AUTH_KEY",
        "openai" => "OPENAI_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        _ => return None,
    };
    std::env::var(variable)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn default_provider_model(provider_id: &str) -> &'static str {
    match provider_id {
        "openai" => "gpt-5-mini",
        "gemini" => "gemini-3.6-flash",
        "openrouter" => "openai/gpt-4.1-mini",
        "minimax" => "MiniMax-M2.7",
        _ => "",
    }
}

fn provider_display_name(provider_id: &str) -> &str {
    match provider_id {
        "deepl" => "DeepL",
        "openai" => "OpenAI",
        "gemini" => "Google Gemini",
        "openrouter" => "OpenRouter",
        "minimax" => "MiniMax",
        _ => provider_id,
    }
}

fn broadcast_processing_locked(inner: &mut ProcessingInner) {
    let snapshot = inner.snapshot.clone();
    inner.subscribers.retain(|channel| {
        channel
            .send(ProcessingEvent::Snapshot {
                snapshot: snapshot.clone(),
            })
            .is_ok()
    });
}

fn format_time(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

pub async fn player_clock_loop(state: Arc<AppState>) {
    let mut ticks = 0_u8;
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let snapshot = state.player.snapshot();
        state
            .processing
            .update_playback_position(snapshot.position_ms);
        state.broadcast_player(snapshot.clone());

        ticks = ticks.wrapping_add(1);
        if ticks.is_multiple_of(20)
            && let Some(fingerprint) = state.processing.inner.lock().ok().and_then(|inner| {
                inner
                    .session
                    .as_ref()
                    .map(|session| session.identity.fingerprint.clone())
            })
        {
            let _ = state
                .storage
                .save_playback_position(&fingerprint, snapshot.position_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_is_clipped_and_owned_by_one_canonical_window() {
        let window = ProcessingWindow {
            start_ms: 30_000,
            end_ms: 60_000,
            priority: myna_player_core::ProcessingPriority::Urgent,
        };
        let segments = canonical_segments(
            "1234567890abcdef",
            0,
            &window,
            vec![
                TranscriptSegment {
                    id: "old".into(),
                    start_ms: 28_000,
                    end_ms: 30_500,
                    text: "previous".into(),
                    detected_language: Some("en".into()),
                    language_confidence: None,
                    is_final: true,
                },
                TranscriptSegment {
                    id: "keep".into(),
                    start_ms: 29_900,
                    end_ms: 31_000,
                    text: "boundary".into(),
                    detected_language: Some("en".into()),
                    language_confidence: None,
                    is_final: true,
                },
            ],
        );

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_ms, 30_000);
        assert_eq!(segments[0].text, "boundary");
    }

    #[test]
    fn silent_window_produces_an_empty_checkpoint_payload() {
        let window = ProcessingWindow {
            start_ms: 0,
            end_ms: 30_000,
            priority: myna_player_core::ProcessingPriority::Urgent,
        };
        assert!(canonical_segments("media", 0, &window, Vec::new()).is_empty());
    }
}
