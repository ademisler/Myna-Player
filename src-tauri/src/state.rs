use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use myna_player_core::{
    AppSettingsV1, DiagnosticSnapshot, MediaMetadata, PlayerEvent, PlayerSnapshot, ProcessingEvent,
    ProcessingPatch, ProcessingSnapshot, ProcessingStage, ProcessingWindow, SubtitleCue,
    SubtitleEditRequest, TranscriptSegment, TranscriptionRequest, TranslationBatchRequest,
    TranslationProviderKind,
};
use myna_player_jobs::{ProcessingQueue, ScheduledWindow};
use myna_player_player::PlayerEngine;
use myna_player_providers::{CredentialStore, ProviderRegistry};
use myna_player_storage::{MediaIdentity, Storage};
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;

const PIPELINE_FORMAT_VERSION: &str = "stream-v4";
const SEGMENTATION_VERSION: &str = "word-timing-dp-v1";
const WHISPER_RUNTIME_VERSION: &str = "whisper.cpp-v1.9.1-f049fff95a089aa9969deb009cdd4892b3e74916";
const EXTRACTION_CONTEXT_MS: u64 = 2_000;

pub struct AppState {
    pub player: Arc<dyn PlayerEngine>,
    pub storage: Arc<Storage>,
    pub credentials: Arc<dyn CredentialStore>,
    pub providers: ProviderRegistry,
    pub model_manager: Arc<crate::model_manager::ModelManager>,
    pub processing: Arc<ProcessingService>,
    pub player_subscribers: Mutex<Vec<Channel<PlayerEvent>>>,
    pub current_media: Mutex<Option<MediaMetadata>>,
    pub audio_track_map: Mutex<HashMap<i32, u32>>,
    pub pending_audio_relative: Mutex<Option<u32>>,
    pub native_surface_handle: usize,
}

impl AppState {
    pub fn set_media_context(
        &self,
        metadata: MediaMetadata,
        snapshot: &PlayerSnapshot,
        preferred_relative: u32,
    ) {
        if let Ok(mut current) = self.current_media.lock() {
            *current = Some(metadata.clone());
        }
        let mapping = match_audio_tracks(&metadata, snapshot);
        if let Ok(mut stored) = self.audio_track_map.lock() {
            *stored = mapping.clone();
        }
        if let Ok(mut pending) = self.pending_audio_relative.lock() {
            *pending = (!mapping
                .values()
                .any(|relative| *relative == preferred_relative))
            .then_some(preferred_relative);
        }
    }

    pub fn audio_relative_for_player_track(
        &self,
        snapshot: &PlayerSnapshot,
        player_track_id: i32,
    ) -> Option<u32> {
        if let Ok(mapping) = self.audio_track_map.lock()
            && let Some(relative) = mapping.get(&player_track_id)
        {
            return Some(*relative);
        }
        let metadata = self.current_media.lock().ok()?.clone()?;
        let mapping = match_audio_tracks(&metadata, snapshot);
        let relative = mapping.get(&player_track_id).copied();
        if let Ok(mut stored) = self.audio_track_map.lock() {
            *stored = mapping;
        }
        relative
    }

    fn apply_pending_audio_selection(&self, snapshot: &PlayerSnapshot) -> Option<PlayerSnapshot> {
        let pending = self
            .pending_audio_relative
            .lock()
            .ok()
            .and_then(|value| *value)?;
        let metadata = self.current_media.lock().ok()?.clone()?;
        let mapping = match_audio_tracks(&metadata, snapshot);
        let player_id = mapping
            .iter()
            .find_map(|(player_id, relative)| (*relative == pending).then_some(*player_id))?;
        let selected = snapshot.tracks.iter().any(|track| {
            track.kind == myna_player_core::TrackKind::Audio
                && track.id == player_id
                && track.selected
        });
        let updated = if selected {
            snapshot.clone()
        } else {
            self.player
                .command(myna_player_core::PlayerCommand::SelectTrack {
                    kind: myna_player_core::TrackKind::Audio,
                    id: player_id,
                })
                .ok()?
        };
        if let Ok(mut stored) = self.audio_track_map.lock() {
            *stored = mapping;
        }
        if let Ok(mut value) = self.pending_audio_relative.lock() {
            *value = None;
        }
        Some(updated)
    }

    pub fn apply_preferred_audio_language(
        &self,
        preferred_language: &str,
    ) -> Result<Option<u32>, String> {
        let metadata = self
            .current_media
            .lock()
            .map_err(|_| "media metadata lock was poisoned".to_string())?
            .clone();
        let Some(metadata) = metadata else {
            return Ok(None);
        };
        let snapshot = self.player.snapshot();
        let relative = preferred_audio_relative_index(&metadata, &snapshot, preferred_language);
        let mapping = match_audio_tracks(&metadata, &snapshot);
        if let Ok(mut stored) = self.audio_track_map.lock() {
            *stored = mapping.clone();
        }
        if let Some(player_id) = mapping
            .iter()
            .find_map(|(player_id, stream)| (*stream == relative).then_some(*player_id))
        {
            if !snapshot.tracks.iter().any(|track| {
                track.kind == myna_player_core::TrackKind::Audio
                    && track.id == player_id
                    && track.selected
            }) {
                let updated = self
                    .player
                    .command(myna_player_core::PlayerCommand::SelectTrack {
                        kind: myna_player_core::TrackKind::Audio,
                        id: player_id,
                    })
                    .map_err(|error| error.to_string())?;
                self.broadcast_player(updated);
            }
            if let Ok(mut pending) = self.pending_audio_relative.lock() {
                *pending = None;
            }
        } else if let Ok(mut pending) = self.pending_audio_relative.lock() {
            *pending = Some(relative);
        }
        Ok(Some(relative))
    }

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
    pipeline_key: String,
    settings: AppSettingsV1,
    queue: ProcessingQueue,
    paused: bool,
    worker_running: bool,
    translation_running: bool,
    translation_requested: bool,
    playback_position_ms: u64,
    cancel_token: Arc<AtomicBool>,
}

struct ProcessingInner {
    session: Option<ProcessingSession>,
    snapshot: ProcessingSnapshot,
    subscribers: Vec<Channel<ProcessingEvent>>,
}

pub struct ProcessingService {
    storage: Arc<Storage>,
    credentials: Arc<dyn CredentialStore>,
    model_manager: Arc<crate::model_manager::ModelManager>,
    asr: Arc<myna_player_pipeline::PersistentWhisper>,
    inner: Mutex<ProcessingInner>,
}

impl ProcessingService {
    pub fn new(
        storage: Arc<Storage>,
        credentials: Arc<dyn CredentialStore>,
        model_manager: Arc<crate::model_manager::ModelManager>,
    ) -> Self {
        Self {
            storage,
            credentials,
            model_manager,
            asr: Arc::new(myna_player_pipeline::PersistentWhisper::default()),
            inner: Mutex::new(ProcessingInner {
                session: None,
                snapshot: ProcessingSnapshot::default(),
                subscribers: Vec::new(),
            }),
        }
    }

    pub fn set_diagnostic_logging(&self, enabled: bool) {
        self.asr.set_diagnostic_logging(enabled);
    }

    pub fn diagnostics(&self) -> DiagnosticSnapshot {
        let worker = self.asr.diagnostics();
        DiagnosticSnapshot {
            diagnostic_logging: worker.logging_enabled,
            worker_running: worker.worker_running,
            worker_model_path: worker
                .model_path
                .map(|path| path.to_string_lossy().into_owned()),
            worker_logs: worker.recent_logs,
            cache_usage_bytes: self.storage.cache_usage_bytes().unwrap_or(0),
            database_path: self.storage.path().to_string_lossy().into_owned(),
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
            pipeline_key,
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
                session.pipeline_key.clone(),
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
                &pipeline_key,
                std::slice::from_ref(&updated_segment),
            )
            .map_err(|error| error.to_string())?;
        self.storage
            .invalidate_translations_for_segment(
                &fingerprint,
                audio_track,
                &pipeline_key,
                &updated_segment.id,
            )
            .map_err(|error| error.to_string())?;
        if let Some(cue) = updated_translation.as_ref()
            && settings.translation.provider_id != "none"
        {
            self.storage
                .store_translations(
                    &fingerprint,
                    audio_track,
                    &pipeline_key,
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

    fn pipeline_key(&self, settings: &AppSettingsV1) -> Result<String, String> {
        let whisper = model_identity(
            settings.transcription.model_path.as_deref(),
            self.model_manager
                .list()
                .into_iter()
                .find(|model| model.id == "whisper-base"),
        )?;
        let vad = if settings.transcription.vad_enabled {
            model_identity(
                settings.transcription.vad_model_path.as_deref(),
                self.model_manager
                    .list()
                    .into_iter()
                    .find(|model| model.id == "silero-vad"),
            )?
        } else {
            "disabled".to_owned()
        };
        let payload = serde_json::json!({
            "format": PIPELINE_FORMAT_VERSION,
            "segmentation": SEGMENTATION_VERSION,
            "runtime": WHISPER_RUNTIME_VERSION,
            "whisperModel": whisper,
            "vadEnabled": settings.transcription.vad_enabled,
            "vadModel": vad,
            "spokenLanguage": settings.transcription.spoken_language,
            "chunkDurationMs": settings.transcription.chunk_duration_ms,
            "extractionContextMs": EXTRACTION_CONTEXT_MS,
        });
        let encoded = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
        let digest = Sha256::digest(encoded);
        Ok(format!("{PIPELINE_FORMAT_VERSION}:{digest:x}"))
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
        let previous_ephemeral = self.inner.lock().ok().and_then(|inner| {
            inner.session.as_ref().and_then(|session| {
                (!session.settings.storage.keep_completed_transcripts
                    && session.identity.fingerprint != identity.fingerprint)
                    .then(|| session.identity.fingerprint.clone())
            })
        });
        if let Some(fingerprint) = previous_ephemeral {
            self.storage
                .purge_media_cache(&fingerprint)
                .map_err(|error| error.to_string())?;
        }
        self.storage
            .set_media_cache_policy(
                &identity.fingerprint,
                settings.storage.keep_completed_transcripts,
            )
            .map_err(|error| error.to_string())?;
        let pipeline_key = self.pipeline_key(&settings)?;
        let completed = self
            .storage
            .completed_windows(&identity.fingerprint, audio_track, &pipeline_key)
            .map_err(|error| error.to_string())?;
        let source_segments = self
            .storage
            .load_transcript_segments(&identity.fingerprint, audio_track, &pipeline_key)
            .map_err(|error| error.to_string())?;
        let translated_cues = if settings.translation.provider_id != "none" {
            self.storage
                .load_translated_cues(
                    &identity.fingerprint,
                    audio_track,
                    &pipeline_key,
                    &settings.translation.provider_id,
                    &settings.translation.target_language,
                )
                .map_err(|error| error.to_string())?
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
            translation_running: false,
            translation_error: None,
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
                pipeline_key,
                settings: settings.clone(),
                queue,
                paused: !settings.transcription.auto_start,
                worker_running: false,
                translation_running: false,
                translation_requested: false,
                playback_position_ms: resume_position_ms,
                cancel_token: Arc::new(AtomicBool::new(false)),
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

    pub fn update_settings(
        self: &Arc<Self>,
        settings: AppSettingsV1,
        selected_audio_track: Option<u32>,
    ) -> Result<ProcessingSnapshot, String> {
        let active = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "processing state lock was poisoned".to_string())?;
            let Some(session) = inner.session.as_mut() else {
                return Ok(inner.snapshot.clone());
            };
            let audio_track = selected_audio_track.unwrap_or(session.audio_track);
            let needs_restart = audio_track != session.audio_track
                || processing_settings_changed(&session.settings, &settings);
            if !needs_restart {
                session.settings = settings;
                return Ok(inner.snapshot.clone());
            }
            Some((
                session.metadata.clone(),
                session.identity.clone(),
                audio_track,
                session.playback_position_ms,
            ))
        };
        let Some((metadata, identity, audio_track, position_ms)) = active else {
            return Ok(self.snapshot());
        };
        self.prepare(&metadata, identity, settings, audio_track, position_ms)
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
            session.translation_requested = true;
            inner.snapshot.status_message = "Translation queued…".into();
            inner.snapshot.translation_error = None;
            broadcast_processing_locked(&mut inner);
        }
        self.spawn_translation_worker();
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
                if let Some(job) = session.queue.pop() {
                    let context = JobContext {
                        session_id: session.session_id.clone(),
                        path: session.path.clone(),
                        fingerprint: session.identity.fingerprint.clone(),
                        duration_ms: session.duration_ms,
                        audio_track: session.audio_track,
                        pipeline_key: session.pipeline_key.clone(),
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
                    WorkerAction::Window(Box::new(context))
                } else {
                    inner.snapshot.stage = ProcessingStage::Ready;
                    inner.snapshot.current_window = None;
                    inner.snapshot.status_message = "Transcript processing complete.".into();
                    broadcast_processing_locked(&mut inner);
                    WorkerAction::Finished
                }
            };

            let WorkerAction::Window(job_context) = action else {
                return;
            };

            if let Err(error) = self.storage.mark_window_running(
                &job_context.fingerprint,
                job_context.audio_track,
                &job_context.pipeline_key,
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

    fn spawn_translation_worker(self: &Arc<Self>) {
        let context = {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            let context = {
                let Some(session) = inner.session.as_mut() else {
                    return;
                };
                if session.translation_running
                    || !session.translation_requested
                    || session.settings.translation.provider_id == "none"
                {
                    return;
                }
                session.translation_running = true;
                session.translation_requested = false;
                TranslationContext {
                    session_id: session.session_id.clone(),
                    fingerprint: session.identity.fingerprint.clone(),
                    audio_track: session.audio_track,
                    pipeline_key: session.pipeline_key.clone(),
                    settings: session.settings.clone(),
                    generation: session.queue.generation(),
                }
            };
            inner.snapshot.translation_running = true;
            inner.snapshot.translation_error = None;
            broadcast_processing_locked(&mut inner);
            context
        };
        let service = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let result = service.translate_remaining(&context).await;
            let restart = {
                let Ok(mut inner) = service.inner.lock() else {
                    return;
                };
                let restart = {
                    let Some(session) = inner.session.as_mut() else {
                        return;
                    };
                    if session.session_id != context.session_id
                        || session.queue.generation() != context.generation
                    {
                        return;
                    }
                    session.translation_running = false;
                    session.translation_requested
                };
                inner.snapshot.translation_running = false;
                match result {
                    Ok(()) => inner.snapshot.translation_error = None,
                    Err(error) => inner.snapshot.translation_error = Some(error),
                }
                broadcast_processing_locked(&mut inner);
                restart
            };
            if restart {
                service.spawn_translation_worker();
            }
        });
    }

    async fn translate_remaining(&self, context: &TranslationContext) -> Result<(), String> {
        if context.settings.translation.provider_id == "none" {
            return Err("Select a translation provider first.".into());
        }
        let source = self
            .storage
            .load_transcript_segments(
                &context.fingerprint,
                context.audio_track,
                &context.pipeline_key,
            )
            .map_err(|error| error.to_string())?;
        let existing = self
            .storage
            .load_translated_cues(
                &context.fingerprint,
                context.audio_track,
                &context.pipeline_key,
                &context.settings.translation.provider_id,
                &context.settings.translation.target_language,
            )
            .map_err(|error| error.to_string())?;
        let translated_ids = existing
            .iter()
            .map(|cue| cue.id.as_str())
            .collect::<HashSet<_>>();
        let pending = source
            .iter()
            .filter(|segment| !translated_ids.contains(segment.id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return Ok(());
        }

        const TRANSLATION_BATCH_SIZE: usize = 32;
        let mut translated_count = 0_usize;
        for batch in pending.chunks(TRANSLATION_BATCH_SIZE) {
            if !self.is_session_current(&context.session_id, context.generation) {
                return Ok(());
            }
            let first_start = batch.first().map(|segment| segment.start_ms).unwrap_or(0);
            let previous_context = source
                .iter()
                .filter(|segment| segment.end_ms <= first_start)
                .rev()
                .take(8)
                .map(|segment| segment.text.clone())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let cues = self
                .translate_segments(
                    &context.settings,
                    batch.to_vec(),
                    batch
                        .iter()
                        .find_map(|segment| segment.detected_language.clone()),
                    previous_context,
                )
                .await?;
            if !self.is_session_current(&context.session_id, context.generation) {
                return Ok(());
            }
            self.storage
                .store_translations(
                    &context.fingerprint,
                    context.audio_track,
                    &context.pipeline_key,
                    &context.settings.translation.provider_id,
                    &context.settings.translation.target_language,
                    &cues,
                )
                .map_err(|error| error.to_string())?;
            translated_count += cues.len();
            if let Ok(mut inner) = self.inner.lock()
                && inner.session.as_ref().is_some_and(|session| {
                    session.session_id == context.session_id
                        && session.queue.generation() == context.generation
                })
            {
                upsert_translated_cues(&mut inner.snapshot.translated_cues, &cues);
                inner.snapshot.status_message =
                    format!("Translated {translated_count} subtitle segment(s)…");
                let patch = ProcessingPatch {
                    source_upserts: Vec::new(),
                    translated_upserts: cues,
                    removed_segment_ids: Vec::new(),
                    completed_windows: inner.snapshot.completed_windows,
                    total_windows: inner.snapshot.total_windows,
                    ready_until_ms: inner.snapshot.ready_until_ms,
                    stage: inner.snapshot.stage,
                    translation_running: true,
                    status_message: inner.snapshot.status_message.clone(),
                    error: inner.snapshot.error.clone(),
                    translation_error: None,
                };
                broadcast_processing_patch_locked(&mut inner, patch);
            }
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
                &context.pipeline_key,
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
        Ok(WindowResult { segments })
    }

    fn complete_job(
        self: &Arc<Self>,
        context: &JobContext,
        result: WindowResult,
    ) -> Result<(), String> {
        self.storage
            .replace_window_segments(
                &context.fingerprint,
                context.audio_track,
                &context.pipeline_key,
                context.job.window.start_ms,
                context.job.window.end_ms,
                context.job.generation,
                &result.segments,
            )
            .map_err(|error| error.to_string())?;

        let max_cache_bytes = context
            .settings
            .storage
            .cache_limit_mb
            .saturating_mul(1024 * 1024);
        self.storage
            .enforce_cache_limit(max_cache_bytes, Some(&context.fingerprint))
            .map_err(|error| error.to_string())?;

        let should_translate = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "processing state lock was poisoned".to_string())?;
            let (completed_windows, total_windows, ready_until_ms, auto_translate) = {
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
                let auto_translate = session.settings.translation.auto_start
                    && session.settings.translation.provider_id != "none"
                    && !result.segments.is_empty();
                if auto_translate {
                    session.translation_requested = true;
                }
                (
                    session.queue.completed_windows(),
                    session.queue.total_windows(),
                    session.queue.ready_until_from(session.playback_position_ms),
                    auto_translate,
                )
            };
            let removed_segment_ids = inner
                .snapshot
                .source_segments
                .iter()
                .filter(|segment| {
                    segment.start_ms < context.job.window.end_ms
                        && segment.end_ms > context.job.window.start_ms
                })
                .map(|segment| segment.id.clone())
                .collect::<Vec<_>>();
            let removed = removed_segment_ids.iter().collect::<HashSet<_>>();
            inner
                .snapshot
                .source_segments
                .retain(|segment| !removed.contains(&segment.id));
            inner
                .snapshot
                .translated_cues
                .retain(|cue| !removed.contains(&cue.id));
            inner
                .snapshot
                .source_segments
                .extend(result.segments.clone());
            inner
                .snapshot
                .source_segments
                .sort_by_key(|segment| (segment.start_ms, segment.end_ms));
            inner.snapshot.completed_windows = completed_windows;
            inner.snapshot.total_windows = total_windows;
            inner.snapshot.ready_until_ms = ready_until_ms;
            inner.snapshot.current_window = None;
            inner.snapshot.stage = ProcessingStage::Queued;
            inner.snapshot.status_message =
                format!("Subtitle window ready · {completed_windows}/{total_windows}");
            inner.snapshot.error = None;
            let patch = ProcessingPatch {
                source_upserts: result.segments,
                translated_upserts: Vec::new(),
                removed_segment_ids,
                completed_windows,
                total_windows,
                ready_until_ms,
                stage: inner.snapshot.stage,
                translation_running: inner.snapshot.translation_running,
                status_message: inner.snapshot.status_message.clone(),
                error: None,
                translation_error: inner.snapshot.translation_error.clone(),
            };
            broadcast_processing_patch_locked(&mut inner, patch);
            auto_translate
        };
        if should_translate {
            self.spawn_translation_worker();
        }
        Ok(())
    }

    fn fail_job(&self, context: &JobContext, error: String, unavailable: bool) {
        let _ = self.storage.mark_window_failed(
            &context.fingerprint,
            context.audio_track,
            &context.pipeline_key,
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

    fn is_session_current(&self, session_id: &str, generation: u64) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| {
                inner.session.as_ref().map(|session| {
                    session.session_id == session_id && session.queue.generation() == generation
                })
            })
            .unwrap_or(false)
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
        pipeline_key: &str,
        before_ms: u64,
    ) -> Option<String> {
        self.previous_context_segments(fingerprint, audio_track, pipeline_key, before_ms)
            .last()
            .cloned()
    }

    fn previous_context_segments(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_key: &str,
        before_ms: u64,
    ) -> Vec<String> {
        self.storage
            .load_transcript_segments(fingerprint, audio_track, pipeline_key)
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
    pipeline_key: String,
    settings: AppSettingsV1,
    job: ScheduledWindow,
    cancel_token: Arc<AtomicBool>,
}

struct TranslationContext {
    session_id: String,
    fingerprint: String,
    audio_track: u32,
    pipeline_key: String,
    settings: AppSettingsV1,
    generation: u64,
}

enum WorkerAction {
    Window(Box<JobContext>),
    Finished,
}

struct WindowResult {
    segments: Vec<TranscriptSegment>,
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

fn processing_settings_changed(old: &AppSettingsV1, new: &AppSettingsV1) -> bool {
    old.transcription.auto_start != new.transcription.auto_start
        || old.transcription.spoken_language != new.transcription.spoken_language
        || old.transcription.model_path != new.transcription.model_path
        || old.transcription.vad_enabled != new.transcription.vad_enabled
        || old.transcription.vad_model_path != new.transcription.vad_model_path
        || old.transcription.chunk_duration_ms != new.transcription.chunk_duration_ms
        || old.transcription.lookahead_ms != new.transcription.lookahead_ms
        || old.transcription.process_full_media != new.transcription.process_full_media
        || old.translation.provider_id != new.translation.provider_id
        || old.translation.endpoint != new.translation.endpoint
        || old.translation.model != new.translation.model
        || old.translation.target_language != new.translation.target_language
        || old.storage.keep_completed_transcripts != new.storage.keep_completed_transcripts
        || old.storage.cache_limit_mb != new.storage.cache_limit_mb
}

pub(crate) fn preferred_audio_relative_index(
    metadata: &MediaMetadata,
    snapshot: &PlayerSnapshot,
    preferred_language: &str,
) -> u32 {
    let preferred = normalize_language(preferred_language);
    if preferred_language != "auto"
        && let Some(stream) = metadata.audio_streams.iter().find(|stream| {
            stream
                .language
                .as_deref()
                .map(normalize_language)
                .is_some_and(|language| language == preferred)
                || stream
                    .title
                    .as_deref()
                    .is_some_and(|title| normalize_text(title).contains(&preferred))
        })
    {
        return stream.relative_index;
    }
    let mapping = match_audio_tracks(metadata, snapshot);
    snapshot
        .tracks
        .iter()
        .find(|track| track.kind == myna_player_core::TrackKind::Audio && track.selected)
        .and_then(|track| mapping.get(&track.id).copied())
        .or_else(|| {
            metadata
                .audio_streams
                .first()
                .map(|stream| stream.relative_index)
        })
        .unwrap_or(0)
}

pub(crate) fn match_audio_tracks(
    metadata: &MediaMetadata,
    snapshot: &PlayerSnapshot,
) -> HashMap<i32, u32> {
    let player_tracks = snapshot
        .tracks
        .iter()
        .filter(|track| track.kind == myna_player_core::TrackKind::Audio && track.id >= 0)
        .collect::<Vec<_>>();
    let mut mapping = HashMap::new();
    let mut used = HashSet::new();
    for track in &player_tracks {
        let label = normalize_text(&track.label);
        let language = track.language.as_deref().map(normalize_language);
        let mut candidates = metadata
            .audio_streams
            .iter()
            .filter(|stream| !used.contains(&stream.relative_index))
            .map(|stream| {
                let mut score = 0_i32;
                if let (Some(player_language), Some(stream_language)) = (
                    language.as_deref(),
                    stream
                        .language
                        .as_deref()
                        .map(normalize_language)
                        .as_deref(),
                ) && player_language == stream_language
                {
                    score += 100;
                }
                if let Some(stream_language) = stream.language.as_deref().map(normalize_language)
                    && !stream_language.is_empty()
                    && label.contains(&stream_language)
                {
                    score += 40;
                }
                if let Some(title) = stream.title.as_deref() {
                    let title = normalize_text(title);
                    if !title.is_empty() && (label.contains(&title) || title.contains(&label)) {
                        score += 60;
                    }
                }
                (score, stream.relative_index)
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.cmp(left));
        if let Some((score, relative)) = candidates.first().copied()
            && score > 0
        {
            mapping.insert(track.id, relative);
            used.insert(relative);
        }
    }
    let remaining_players = player_tracks
        .iter()
        .filter(|track| !mapping.contains_key(&track.id))
        .map(|track| track.id)
        .collect::<Vec<_>>();
    let remaining_streams = metadata
        .audio_streams
        .iter()
        .filter(|stream| !used.contains(&stream.relative_index))
        .map(|stream| stream.relative_index)
        .collect::<Vec<_>>();
    for (player_id, relative_index) in remaining_players.into_iter().zip(remaining_streams) {
        mapping.insert(player_id, relative_index);
    }
    mapping
}

fn normalize_language(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase().replace('_', "-");
    match value.split('-').next().unwrap_or("") {
        "eng" | "english" => "en".into(),
        "fra" | "fre" | "french" => "fr".into(),
        "tur" | "turkish" => "tr".into(),
        "deu" | "ger" | "german" => "de".into(),
        "spa" | "spanish" => "es".into(),
        "ita" | "italian" => "it".into(),
        "por" | "portuguese" => "pt".into(),
        other => other.to_owned(),
    }
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn model_identity(
    configured_path: Option<&str>,
    catalog: Option<myna_player_core::ModelDescriptor>,
) -> Result<String, String> {
    if let Some(path) = configured_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return sha256_file(Path::new(path));
    }
    match catalog {
        Some(model) if model.installed && model.verified => Ok(model.sha256),
        Some(model) if model.installed => Err(format!(
            "{} is installed but failed verification",
            model.display_name
        )),
        Some(model) => Ok(format!("missing:{}:{}", model.id, model.sha256)),
        None => Ok("missing:unknown".into()),
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("could not open model {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("could not hash model {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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
        "gemini" => "gemini-3.5-flash",
        "openrouter" => "openai/gpt-4.1-mini",
        "minimax" => "MiniMax-M3",
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

fn upsert_translated_cues(target: &mut Vec<SubtitleCue>, cues: &[SubtitleCue]) {
    for cue in cues {
        if let Some(existing) = target.iter_mut().find(|existing| existing.id == cue.id) {
            *existing = cue.clone();
        } else {
            target.push(cue.clone());
        }
    }
    target.sort_by_key(|cue| (cue.start_ms, cue.end_ms));
}

fn broadcast_processing_patch_locked(inner: &mut ProcessingInner, patch: ProcessingPatch) {
    inner.subscribers.retain(|channel| {
        channel
            .send(ProcessingEvent::Patch {
                patch: patch.clone(),
            })
            .is_ok()
    });
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
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut snapshot = state.player.snapshot();
        if let Some(updated) = state.apply_pending_audio_selection(&snapshot) {
            snapshot = updated;
        }
        state
            .processing
            .update_playback_position(snapshot.position_ms);
        state.broadcast_player(snapshot.clone());

        ticks = ticks.wrapping_add(1);
        if ticks.is_multiple_of(50)
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
    fn processing_restarts_only_for_settings_that_change_cached_or_queued_work() {
        let base = AppSettingsV1::default();
        let mut subtitle_only = base.clone();
        subtitle_only.subtitles.font_scale = 1.4;
        assert!(!processing_settings_changed(&base, &subtitle_only));

        let mut language = base.clone();
        language.transcription.spoken_language = "fr".into();
        assert!(processing_settings_changed(&base, &language));

        let mut provider = base.clone();
        provider.translation.provider_id = "deepl".into();
        assert!(processing_settings_changed(&base, &provider));
    }

    #[test]
    fn preferred_language_and_track_metadata_choose_the_same_audio_stream() {
        let metadata = MediaMetadata {
            path: "/tmp/movie.mkv".into(),
            file_name: "movie.mkv".into(),
            duration_ms: 60_000,
            size_bytes: Some(1),
            format_name: Some("matroska".into()),
            video_streams: Vec::new(),
            audio_streams: vec![
                myna_player_core::AudioStream {
                    index: 1,
                    relative_index: 0,
                    codec: Some("aac".into()),
                    channels: Some(2),
                    sample_rate: Some(48_000),
                    language: Some("eng".into()),
                    title: Some("English".into()),
                    player_track_id: None,
                },
                myna_player_core::AudioStream {
                    index: 2,
                    relative_index: 1,
                    codec: Some("aac".into()),
                    channels: Some(2),
                    sample_rate: Some(48_000),
                    language: Some("tur".into()),
                    title: Some("Turkish dub".into()),
                    player_track_id: None,
                },
            ],
            subtitle_streams: Vec::new(),
        };
        let snapshot = PlayerSnapshot {
            available: true,
            backend: "test".into(),
            state: myna_player_core::PlayerState::Paused,
            media_path: Some(metadata.path.clone()),
            file_name: Some(metadata.file_name.clone()),
            position_ms: 0,
            duration_ms: metadata.duration_ms,
            volume: 100,
            muted: false,
            rate: 1.0,
            tracks: vec![
                myna_player_core::TrackDescriptor {
                    id: 10,
                    kind: myna_player_core::TrackKind::Audio,
                    label: "English".into(),
                    language: Some("en".into()),
                    selected: true,
                },
                myna_player_core::TrackDescriptor {
                    id: 21,
                    kind: myna_player_core::TrackKind::Audio,
                    label: "Turkish dub".into(),
                    language: Some("tr".into()),
                    selected: false,
                },
            ],
            error: None,
        };
        let mapping = match_audio_tracks(&metadata, &snapshot);
        assert_eq!(mapping.get(&10), Some(&0));
        assert_eq!(mapping.get(&21), Some(&1));
        assert_eq!(
            preferred_audio_relative_index(&metadata, &snapshot, "tr"),
            1
        );
        assert_eq!(
            preferred_audio_relative_index(&metadata, &snapshot, "auto"),
            0
        );
    }

    #[test]
    fn pipeline_fingerprint_changes_with_model_language_and_chunking() {
        let directory = tempfile::tempdir().unwrap();
        let storage = Arc::new(Storage::open(directory.path().join("cache.sqlite3")).unwrap());
        let credentials: Arc<dyn CredentialStore> =
            Arc::new(myna_player_providers::MemoryCredentialStore::default());
        let models = Arc::new(
            crate::model_manager::ModelManager::new(directory.path().join("models")).unwrap(),
        );
        let service = ProcessingService::new(storage, credentials, models);
        let base = AppSettingsV1::default();
        let base_key = service.pipeline_key(&base).unwrap();

        let mut language = base.clone();
        language.transcription.spoken_language = "fr".into();
        assert_ne!(base_key, service.pipeline_key(&language).unwrap());

        let mut chunk = base.clone();
        chunk.transcription.chunk_duration_ms = 45_000;
        assert_ne!(base_key, service.pipeline_key(&chunk).unwrap());

        let custom_model = directory.path().join("custom-model.bin");
        std::fs::write(&custom_model, b"model-v1").unwrap();
        let mut custom = base.clone();
        custom.transcription.model_path = Some(custom_model.to_string_lossy().into_owned());
        let first = service.pipeline_key(&custom).unwrap();
        std::fs::write(&custom_model, b"model-v2").unwrap();
        let second = service.pipeline_key(&custom).unwrap();
        assert_ne!(first, second);
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
