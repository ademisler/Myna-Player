use gloo_timers::callback::Timeout;
use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use myna_player_core::{
    AppSettingsV1, CredentialStatus, DiagnosticSnapshot, MediaMetadata, ModelDescriptor,
    PlayerCommand, PlayerEvent, PlayerSnapshot, PlayerState, ProcessingCommand, ProcessingEvent,
    ProcessingPatch, ProcessingSnapshot, ProcessingStage, ProviderDescriptor, RuntimeStatus,
    SubtitleEditRequest, SubtitleExportFormat, SubtitleExportTrack, TrackKind,
};
use serde::{Deserialize, Serialize};

use crate::{
    bridge::{EmptyArgs, install_drag_drop, install_video_surface_sync, invoke_typed, subscribe},
    components::{PlayerShell, SettingsModal},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenMediaResult {
    metadata: MediaMetadata,
    player: PlayerSnapshot,
    processing: ProcessingSnapshot,
    resumed_at_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetMediaResult {
    player: PlayerSnapshot,
    processing: ProcessingSnapshot,
}

#[derive(Debug, Deserialize)]
struct DragMessage {
    kind: String,
    path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PathArgs {
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerCommandArgs {
    command: PlayerCommand,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessingCommandArgs {
    command: ProcessingCommand,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsArgs {
    settings: AppSettingsV1,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderArgs {
    provider_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelArgs {
    model_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportArgs {
    format: SubtitleExportFormat,
    track: SubtitleExportTrack,
}

#[derive(Serialize)]
struct EditCueArgs {
    edit: SubtitleEditRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialArgs {
    provider_id: String,
    secret: String,
}

fn apply_processing_patch(snapshot: &mut ProcessingSnapshot, patch: ProcessingPatch) {
    if !patch.removed_segment_ids.is_empty() {
        let removed = patch
            .removed_segment_ids
            .iter()
            .collect::<std::collections::HashSet<_>>();
        snapshot
            .source_segments
            .retain(|segment| !removed.contains(&segment.id));
        snapshot
            .translated_cues
            .retain(|cue| !removed.contains(&cue.id));
    }
    for segment in patch.source_upserts {
        if let Some(existing) = snapshot
            .source_segments
            .iter_mut()
            .find(|existing| existing.id == segment.id)
        {
            *existing = segment;
        } else {
            snapshot.source_segments.push(segment);
        }
    }
    for cue in patch.translated_upserts {
        if let Some(existing) = snapshot
            .translated_cues
            .iter_mut()
            .find(|existing| existing.id == cue.id)
        {
            *existing = cue;
        } else {
            snapshot.translated_cues.push(cue);
        }
    }
    snapshot
        .source_segments
        .sort_by_key(|segment| (segment.start_ms, segment.end_ms));
    snapshot
        .translated_cues
        .sort_by_key(|cue| (cue.start_ms, cue.end_ms));
    snapshot.completed_windows = patch.completed_windows;
    snapshot.total_windows = patch.total_windows;
    snapshot.ready_until_ms = patch.ready_until_ms;
    snapshot.stage = patch.stage;
    snapshot.translation_running = patch.translation_running;
    snapshot.status_message = patch.status_message;
    snapshot.error = patch.error;
    snapshot.translation_error = patch.translation_error;
}

#[component]
pub fn App() -> impl IntoView {
    let (player, set_player) = signal(PlayerSnapshot::unavailable(
        "Connecting to the native player…",
    ));
    let (processing, set_processing) = signal(ProcessingSnapshot::default());
    let (metadata, set_metadata) = signal::<Option<MediaMetadata>>(None);
    let (runtime, set_runtime) = signal::<Option<RuntimeStatus>>(None);
    let (diagnostics, set_diagnostics) = signal::<Option<DiagnosticSnapshot>>(None);
    let (providers, set_providers) = signal::<Vec<ProviderDescriptor>>(Vec::new());
    let (models, set_models) = signal::<Vec<ModelDescriptor>>(Vec::new());
    let (model_busy, set_model_busy) = signal::<Option<String>>(None);
    let settings = RwSignal::new(AppSettingsV1::default());
    let settings_draft = RwSignal::new(AppSettingsV1::default());
    let (credential, set_credential) = signal(CredentialStatus {
        provider_id: "deepl".into(),
        configured: false,
    });
    let credential_input = RwSignal::new(String::new());
    let (settings_open, set_settings_open) = signal(false);
    let (drop_active, set_drop_active) = signal(false);
    let (controls_visible, set_controls_visible) = signal(true);
    let (pointer_generation, set_pointer_generation) = signal(0_u64);
    let (wants_playing, set_wants_playing) = signal(false);
    let (toast, set_toast) = signal::<Option<String>>(None);

    install_video_surface_sync();
    subscribe::<PlayerEvent>("subscribe_player_events", move |event| match event {
        PlayerEvent::Snapshot { snapshot } => set_player.set(snapshot),
    });
    subscribe::<ProcessingEvent>("subscribe_processing_events", move |event| match event {
        ProcessingEvent::Snapshot { snapshot } => set_processing.set(snapshot),
        ProcessingEvent::Patch { patch } => {
            set_processing.update(|snapshot| apply_processing_patch(snapshot, patch));
        }
    });
    install_drag_drop::<DragMessage>(move |event| match event.kind.as_str() {
        "over" => set_drop_active.set(true),
        "leave" => set_drop_active.set(false),
        "drop" => {
            set_drop_active.set(false);
            if let Some(path) = event.path {
                open_media(
                    path,
                    set_player,
                    set_processing,
                    set_metadata,
                    set_wants_playing,
                    settings,
                    set_toast,
                );
            }
        }
        _ => {}
    });

    spawn_local(async move {
        if let Ok(loaded) = invoke_typed::<AppSettingsV1, _>("get_settings", &EmptyArgs {}).await {
            settings.set(loaded.clone());
            settings_draft.set(loaded);
        }
    });
    spawn_local(async move {
        if let Ok(available) =
            invoke_typed::<Vec<ProviderDescriptor>, _>("list_providers", &EmptyArgs {}).await
        {
            set_providers.set(available);
        }
    });
    spawn_local(async move {
        if let Ok(available) =
            invoke_typed::<Vec<ModelDescriptor>, _>("list_models", &EmptyArgs {}).await
        {
            set_models.set(available);
        }
    });
    spawn_local(async move {
        if let Ok(status) = invoke_typed::<RuntimeStatus, _>("inspect_runtime", &EmptyArgs {}).await
        {
            set_runtime.set(Some(status));
        }
    });
    refresh_diagnostics(set_diagnostics);

    let open_picker = Callback::new(move |()| {
        spawn_local(async move {
            match invoke_typed::<Option<String>, _>("pick_video", &EmptyArgs {}).await {
                Ok(Some(path)) => open_media(
                    path,
                    set_player,
                    set_processing,
                    set_metadata,
                    set_wants_playing,
                    settings,
                    set_toast,
                ),
                Ok(None) => {}
                Err(error) => show_error(set_toast, error),
            }
        });
    });

    let toggle_playback = Callback::new(move |()| {
        let snapshot = player.get_untracked();
        if matches!(
            snapshot.state,
            PlayerState::Playing | PlayerState::Buffering
        ) {
            set_wants_playing.set(false);
            send_player_command(PlayerCommand::Pause, set_player, set_toast);
            return;
        }
        if snapshot.media_path.is_none() {
            open_picker.run(());
            return;
        }

        let app_settings = settings.get_untracked();
        let process = processing.get_untracked();
        let buffer_ms = process.ready_until_ms.saturating_sub(snapshot.position_ms);
        let media_duration_ms = metadata
            .get_untracked()
            .map(|media| media.duration_ms)
            .unwrap_or(snapshot.duration_ms);
        let required_ms = app_settings
            .transcription
            .initial_buffer_ms
            .min(media_duration_ms.saturating_sub(snapshot.position_ms));
        let can_wait = !matches!(
            process.stage,
            ProcessingStage::Failed | ProcessingStage::Unavailable
        );
        if app_settings.subtitles.smart_wait_enabled && buffer_ms < required_ms && can_wait {
            set_wants_playing.set(true);
            send_processing_command(ProcessingCommand::Start, set_processing, set_toast);
        } else {
            set_wants_playing.set(false);
            send_player_command(PlayerCommand::Play, set_player, set_toast);
        }
    });

    let play_now = Callback::new(move |()| {
        set_wants_playing.set(false);
        send_player_command(PlayerCommand::Play, set_player, set_toast);
    });

    Effect::new(move |_| {
        if !wants_playing.get() {
            return;
        }
        let process = processing.get();
        let snapshot = player.get();
        let media_duration_ms = metadata
            .get()
            .map(|media| media.duration_ms)
            .unwrap_or(snapshot.duration_ms);
        let required = settings
            .get()
            .transcription
            .initial_buffer_ms
            .min(media_duration_ms.saturating_sub(snapshot.position_ms));
        let ready = process.ready_until_ms.saturating_sub(snapshot.position_ms) >= required;
        let cannot_wait = matches!(
            process.stage,
            ProcessingStage::Failed | ProcessingStage::Unavailable
        );
        if ready || cannot_wait {
            set_wants_playing.set(false);
            send_player_command(PlayerCommand::Play, set_player, set_toast);
        }
    });

    Effect::new(move |_| {
        if !matches!(
            player.get().state,
            PlayerState::Playing | PlayerState::Buffering
        ) {
            set_controls_visible.set(true);
        }
    });

    let pointer_activity = Callback::new(move |()| {
        let next = pointer_generation.get_untracked().saturating_add(1);
        set_pointer_generation.set(next);
        set_controls_visible.set(true);
        let delay = settings.get_untracked().playback.controls_hide_delay_ms as u32;
        Timeout::new(delay, move || {
            if pointer_generation.get_untracked() == next
                && matches!(
                    player.get_untracked().state,
                    PlayerState::Playing | PlayerState::Buffering
                )
                && !settings_open.get_untracked()
            {
                set_controls_visible.set(false);
            }
        })
        .forget();
    });

    let on_seek = Callback::new(move |position_ms| {
        send_player_command(PlayerCommand::Seek { position_ms }, set_player, set_toast);
    });
    let on_volume = Callback::new(move |volume| {
        send_player_command(PlayerCommand::SetVolume { volume }, set_player, set_toast);
    });
    let on_toggle_mute = Callback::new(move |()| {
        send_player_command(
            PlayerCommand::SetMuted {
                muted: !player.get_untracked().muted,
            },
            set_player,
            set_toast,
        );
    });
    let on_rate = Callback::new(move |rate| {
        send_player_command(PlayerCommand::SetRate { rate }, set_player, set_toast);
    });
    let on_select_track = Callback::new(move |(kind, id): (TrackKind, i32)| {
        send_player_command(
            PlayerCommand::SelectTrack { kind, id },
            set_player,
            set_toast,
        );
    });
    let on_fullscreen = Callback::new(move |()| {
        spawn_local(async move {
            if let Err(error) = invoke_typed::<bool, _>("toggle_fullscreen", &EmptyArgs {}).await {
                show_error(set_toast, error);
            }
        });
    });
    let open_settings = Callback::new(move |()| {
        settings_draft.set(settings.get_untracked());
        credential_input.set(String::new());
        set_settings_open.set(true);
        set_controls_visible.set(true);
    });
    Effect::new(move |_| {
        if !settings_open.get() {
            return;
        }
        let provider_id = settings_draft.get().translation.provider_id;
        if provider_id == "none" {
            set_credential.set(CredentialStatus {
                provider_id,
                configured: false,
            });
            credential_input.set(String::new());
            return;
        }
        if credential.get_untracked().provider_id != provider_id {
            credential_input.set(String::new());
            refresh_credential_status(provider_id, set_credential, set_toast);
        }
    });

    let close_settings = Callback::new(move |()| {
        credential_input.set(String::new());
        settings_draft.set(settings.get_untracked());
        set_settings_open.set(false);
    });
    let save_settings_callback =
        Callback::new(move |(next_settings, secret): (AppSettingsV1, String)| {
            spawn_local(async move {
                if !secret.trim().is_empty() {
                    match invoke_typed::<CredentialStatus, _>(
                        "set_provider_credential",
                        &CredentialArgs {
                            provider_id: next_settings.translation.provider_id.clone(),
                            secret,
                        },
                    )
                    .await
                    {
                        Ok(status) => set_credential.set(status),
                        Err(error) => {
                            show_error(set_toast, error);
                            return;
                        }
                    }
                }
                match invoke_typed::<AppSettingsV1, _>(
                    "save_settings",
                    &SettingsArgs {
                        settings: next_settings,
                    },
                )
                .await
                {
                    Ok(saved) => {
                        settings.set(saved.clone());
                        settings_draft.set(saved);
                        credential_input.set(String::new());
                        set_settings_open.set(false);
                        set_toast.set(Some("Settings saved.".into()));
                        clear_toast_later(set_toast);
                    }
                    Err(error) => show_error(set_toast, error),
                }
            });
        });
    let delete_credential = Callback::new(move |()| {
        spawn_local(async move {
            match invoke_typed::<CredentialStatus, _>(
                "delete_provider_credential",
                &ProviderArgs {
                    provider_id: settings_draft.get_untracked().translation.provider_id,
                },
            )
            .await
            {
                Ok(status) => {
                    set_credential.set(status);
                    credential_input.set(String::new());
                }
                Err(error) => show_error(set_toast, error),
            }
        });
    });
    let install_model = Callback::new(move |model_id: String| {
        if model_busy.get_untracked().is_some() {
            return;
        }
        set_model_busy.set(Some(model_id.clone()));
        set_toast.set(Some("Downloading and verifying model…".into()));
        poll_model_progress(model_id.clone(), set_models);
        spawn_local(async move {
            let result = invoke_typed::<ModelDescriptor, _>(
                "install_model",
                &ModelArgs {
                    model_id: model_id.clone(),
                },
            )
            .await;
            match result {
                Ok(model) => {
                    set_models.update(|models| replace_model(models, model.clone()));
                    if let Ok(status) =
                        invoke_typed::<RuntimeStatus, _>("inspect_runtime", &EmptyArgs {}).await
                    {
                        set_runtime.set(Some(status));
                    }
                    set_toast.set(Some(format!(
                        "{} installed and verified.",
                        model.display_name
                    )));
                    clear_toast_later(set_toast);
                }
                Err(error) => show_error(set_toast, error),
            }
            set_model_busy.set(None);
        });
    });
    let verify_model = Callback::new(move |model_id: String| {
        if model_busy.get_untracked().is_some() {
            return;
        }
        set_model_busy.set(Some(model_id.clone()));
        spawn_local(async move {
            match invoke_typed::<ModelDescriptor, _>(
                "verify_model",
                &ModelArgs {
                    model_id: model_id.clone(),
                },
            )
            .await
            {
                Ok(model) => {
                    set_models.update(|models| replace_model(models, model.clone()));
                    set_toast.set(Some(format!("{} checksum verified.", model.display_name)));
                    clear_toast_later(set_toast);
                }
                Err(error) => show_error(set_toast, error),
            }
            set_model_busy.set(None);
        });
    });
    let delete_model = Callback::new(move |model_id: String| {
        if model_busy.get_untracked().is_some() {
            return;
        }
        set_model_busy.set(Some(model_id.clone()));
        spawn_local(async move {
            match invoke_typed::<ModelDescriptor, _>(
                "delete_model",
                &ModelArgs {
                    model_id: model_id.clone(),
                },
            )
            .await
            {
                Ok(model) => {
                    set_models.update(|models| replace_model(models, model.clone()));
                    if let Ok(status) =
                        invoke_typed::<RuntimeStatus, _>("inspect_runtime", &EmptyArgs {}).await
                    {
                        set_runtime.set(Some(status));
                    }
                    set_toast.set(Some(format!("{} removed.", model.display_name)));
                    clear_toast_later(set_toast);
                }
                Err(error) => show_error(set_toast, error),
            }
            set_model_busy.set(None);
        });
    });

    let export_subtitles = Callback::new(
        move |(format, track): (SubtitleExportFormat, SubtitleExportTrack)| {
            spawn_local(async move {
                match invoke_typed::<Option<String>, _>(
                    "export_subtitles",
                    &ExportArgs { format, track },
                )
                .await
                {
                    Ok(Some(path)) => {
                        set_toast.set(Some(format!("Subtitle file saved to {path}.")));
                        clear_toast_later(set_toast);
                    }
                    Ok(None) => {}
                    Err(error) => show_error(set_toast, error),
                }
            });
        },
    );
    let reset_current_media = Callback::new(move |()| {
        set_wants_playing.set(false);
        set_toast.set(Some("Resetting this video's generated data…".into()));
        spawn_local(async move {
            match invoke_typed::<ResetMediaResult, _>("reset_current_media", &EmptyArgs {}).await {
                Ok(result) => {
                    set_player.set(result.player);
                    set_processing.set(result.processing);
                    set_toast.set(Some(
                        "Transcript, translations and processing cache were reset.".into(),
                    ));
                    clear_toast_later(set_toast);
                }
                Err(error) => show_error(set_toast, error),
            }
        });
    });

    let update_subtitle_cue = Callback::new(move |edit: SubtitleEditRequest| {
        spawn_local(async move {
            match invoke_typed::<ProcessingSnapshot, _>(
                "update_subtitle_cue",
                &EditCueArgs { edit },
            )
            .await
            {
                Ok(snapshot) => {
                    set_processing.set(snapshot);
                    set_toast.set(Some("Subtitle correction saved.".into()));
                    clear_toast_later(set_toast);
                }
                Err(error) => show_error(set_toast, error),
            }
        });
    });

    let process_start = Callback::new(move |()| {
        send_processing_command(ProcessingCommand::Start, set_processing, set_toast)
    });
    let process_pause = Callback::new(move |()| {
        send_processing_command(ProcessingCommand::Pause, set_processing, set_toast)
    });
    let process_translate = Callback::new(move |()| {
        send_processing_command(ProcessingCommand::Translate, set_processing, set_toast)
    });
    let process_retry = Callback::new(move |()| {
        send_processing_command(ProcessingCommand::Retry, set_processing, set_toast)
    });

    let _keyboard_listener = window_event_listener(ev::keydown, move |event| {
        if settings_open.get_untracked() {
            if event.key() == "Escape" {
                set_settings_open.set(false);
            }
            return;
        }
        match event.key().as_str() {
            " " => {
                event.prevent_default();
                toggle_playback.run(());
            }
            "ArrowLeft" => {
                event.prevent_default();
                on_seek.run(player.get_untracked().position_ms.saturating_sub(5_000));
            }
            "ArrowRight" => {
                event.prevent_default();
                on_seek.run(player.get_untracked().position_ms.saturating_add(5_000));
            }
            "m" | "M" => on_toggle_mute.run(()),
            "f" | "F" => on_fullscreen.run(()),
            "," => open_settings.run(()),
            _ => {}
        }
    });

    view! {
        <PlayerShell
            player=player
            processing=processing
            metadata=metadata
            settings=settings
            controls_visible=controls_visible
            drop_active=drop_active
            settings_open=settings_open
            on_open=open_picker
            on_toggle_playback=toggle_playback
            on_play_now=play_now
            on_seek=on_seek
            on_volume=on_volume
            on_toggle_mute=on_toggle_mute
            on_rate=on_rate
            on_select_track=on_select_track
            on_fullscreen=on_fullscreen
            on_settings=open_settings
            on_pointer_activity=pointer_activity
        />

        <SettingsModal
            open=settings_open
            draft=settings_draft
            metadata=metadata
            player=player
            processing=processing
            providers=providers
            credential=credential
            credential_input=credential_input
            runtime=runtime
            diagnostics=diagnostics
            models=models
            model_busy=model_busy
            on_install_model=install_model
            on_verify_model=verify_model
            on_delete_model=delete_model
            on_close=close_settings
            on_save=save_settings_callback
            on_delete_credential=delete_credential
            on_process_start=process_start
            on_process_pause=process_pause
            on_process_retry=process_retry
            on_process_translate=process_translate
            on_reset_current_media=reset_current_media
            on_export_subtitles=export_subtitles
            on_update_subtitle_cue=update_subtitle_cue
            on_select_track=on_select_track
        />

        <Show when=move || toast.get().is_some()>
            <div class=move || {
                if toast.get().as_deref().is_some_and(|message| {
                    message.to_ascii_lowercase().contains("failed")
                        || message.to_ascii_lowercase().contains("error")
                        || message.to_ascii_lowercase().contains("unavailable")
                }) {
                    "toast toast--error"
                } else {
                    "toast"
                }
            }>
                {move || toast.get().unwrap_or_default()}
            </div>
        </Show>
    }
}

fn open_media(
    path: String,
    set_player: WriteSignal<PlayerSnapshot>,
    set_processing: WriteSignal<ProcessingSnapshot>,
    set_metadata: WriteSignal<Option<MediaMetadata>>,
    set_wants_playing: WriteSignal<bool>,
    settings: RwSignal<AppSettingsV1>,
    set_toast: WriteSignal<Option<String>>,
) {
    set_toast.set(Some("Opening video…".into()));
    spawn_local(async move {
        match invoke_typed::<OpenMediaResult, _>("open_media", &PathArgs { path }).await {
            Ok(result) => {
                let resumed = result.resumed_at_ms;
                set_player.set(result.player);
                set_processing.set(result.processing);
                set_metadata.set(Some(result.metadata));
                let current_settings = settings.get_untracked();
                set_wants_playing.set(
                    current_settings.playback.autoplay_after_initial_buffer
                        && current_settings.transcription.auto_start,
                );
                set_toast.set(if resumed > 0 {
                    Some(format!("Resumed at {}.", format_time(resumed)))
                } else {
                    None
                });
                if resumed > 0 {
                    clear_toast_later(set_toast);
                }
            }
            Err(error) => show_error(set_toast, error),
        }
    });
}

fn send_player_command(
    command: PlayerCommand,
    set_player: WriteSignal<PlayerSnapshot>,
    set_toast: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        match invoke_typed::<PlayerSnapshot, _>("player_command", &PlayerCommandArgs { command })
            .await
        {
            Ok(snapshot) => set_player.set(snapshot),
            Err(error) => show_error(set_toast, error),
        }
    });
}

fn send_processing_command(
    command: ProcessingCommand,
    set_processing: WriteSignal<ProcessingSnapshot>,
    set_toast: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        match invoke_typed::<ProcessingSnapshot, _>(
            "processing_command",
            &ProcessingCommandArgs { command },
        )
        .await
        {
            Ok(snapshot) => set_processing.set(snapshot),
            Err(error) => show_error(set_toast, error),
        }
    });
}

fn refresh_credential_status(
    provider_id: String,
    set_credential: WriteSignal<CredentialStatus>,
    set_toast: WriteSignal<Option<String>>,
) {
    spawn_local(async move {
        match invoke_typed::<CredentialStatus, _>(
            "credential_status",
            &ProviderArgs { provider_id },
        )
        .await
        {
            Ok(status) => set_credential.set(status),
            Err(error) => show_error(set_toast, error),
        }
    });
}

fn poll_model_progress(model_id: String, set_models: WriteSignal<Vec<ModelDescriptor>>) {
    Timeout::new(400, move || {
        spawn_local(async move {
            if let Ok(models) =
                invoke_typed::<Vec<ModelDescriptor>, _>("list_models", &EmptyArgs {}).await
            {
                let still_installing = models
                    .iter()
                    .any(|model| model.id == model_id && model.installing);
                set_models.set(models);
                if still_installing {
                    poll_model_progress(model_id, set_models);
                }
            }
        });
    })
    .forget();
}

fn refresh_diagnostics(set_diagnostics: WriteSignal<Option<DiagnosticSnapshot>>) {
    spawn_local(async move {
        if let Ok(snapshot) =
            invoke_typed::<DiagnosticSnapshot, _>("diagnostic_snapshot", &EmptyArgs {}).await
        {
            set_diagnostics.set(Some(snapshot));
        }
        Timeout::new(1_500, move || refresh_diagnostics(set_diagnostics)).forget();
    });
}

fn replace_model(models: &mut Vec<ModelDescriptor>, updated: ModelDescriptor) {
    if let Some(existing) = models.iter_mut().find(|model| model.id == updated.id) {
        *existing = updated;
    } else {
        models.push(updated);
    }
}

fn show_error(set_toast: WriteSignal<Option<String>>, error: String) {
    set_toast.set(Some(error));
    clear_toast_later(set_toast);
}

fn clear_toast_later(set_toast: WriteSignal<Option<String>>) {
    Timeout::new(5_000, move || set_toast.set(None)).forget();
}

fn format_time(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use myna_player_core::{CueStatus, ProcessingStage, SubtitleCue, TranscriptSegment};

    #[test]
    fn processing_patch_replaces_removed_cues_and_upserts_translation() {
        let mut snapshot = ProcessingSnapshot::default();
        snapshot.source_segments.push(TranscriptSegment {
            id: "old".into(),
            start_ms: 0,
            end_ms: 500,
            text: "old".into(),
            detected_language: Some("en".into()),
            language_confidence: None,
            is_final: true,
        });
        let patch = ProcessingPatch {
            source_upserts: vec![TranscriptSegment {
                id: "new".into(),
                start_ms: 100,
                end_ms: 900,
                text: "new".into(),
                detected_language: Some("en".into()),
                language_confidence: None,
                is_final: true,
            }],
            translated_upserts: vec![SubtitleCue {
                id: "new".into(),
                start_ms: 100,
                end_ms: 900,
                source_text: "new".into(),
                translated_text: Some("yeni".into()),
                source_language: Some("en".into()),
                target_language: Some("tr".into()),
                status: CueStatus::Ready,
            }],
            removed_segment_ids: vec!["old".into()],
            completed_windows: 1,
            total_windows: 3,
            ready_until_ms: 30_000,
            stage: ProcessingStage::Queued,
            translation_running: true,
            status_message: "updated".into(),
            error: None,
            translation_error: None,
        };
        apply_processing_patch(&mut snapshot, patch);
        assert_eq!(snapshot.source_segments.len(), 1);
        assert_eq!(snapshot.source_segments[0].id, "new");
        assert_eq!(
            snapshot.translated_cues[0].translated_text.as_deref(),
            Some("yeni")
        );
        assert_eq!(snapshot.completed_windows, 1);
        assert!(snapshot.translation_running);
    }
}
