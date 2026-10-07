use leptos::prelude::*;

use super::Icon;
use myna_player_core::{
    AppSettingsV1, CredentialStatus, DiagnosticSnapshot, MediaMetadata, ModelDescriptor,
    PlayerSnapshot, ProcessingSnapshot, ProcessingStage, ProviderDescriptor, RuntimeStatus,
    SubtitleEditRequest, SubtitleExportFormat, SubtitleExportTrack, TrackKind,
};

const TABS: [(&str, &str); 8] = [
    ("current-media", "Current media"),
    ("general", "General"),
    ("playback", "Playback"),
    ("subtitles", "Subtitles"),
    ("transcription", "Transcription"),
    ("translation", "Translation & providers"),
    ("storage", "Storage & privacy"),
    ("advanced", "Advanced & diagnostics"),
];

#[component]
pub fn SettingsModal(
    open: ReadSignal<bool>,
    draft: RwSignal<AppSettingsV1>,
    metadata: ReadSignal<Option<MediaMetadata>>,
    player: ReadSignal<PlayerSnapshot>,
    processing: ReadSignal<ProcessingSnapshot>,
    providers: ReadSignal<Vec<ProviderDescriptor>>,
    credential: ReadSignal<CredentialStatus>,
    credential_input: RwSignal<String>,
    runtime: ReadSignal<Option<RuntimeStatus>>,
    diagnostics: ReadSignal<Option<DiagnosticSnapshot>>,
    models: ReadSignal<Vec<ModelDescriptor>>,
    model_busy: ReadSignal<Option<String>>,
    on_install_model: Callback<String>,
    on_verify_model: Callback<String>,
    on_delete_model: Callback<String>,
    on_close: Callback<()>,
    on_save: Callback<(AppSettingsV1, String)>,
    on_delete_credential: Callback<()>,
    on_process_start: Callback<()>,
    on_process_pause: Callback<()>,
    on_process_retry: Callback<()>,
    on_process_translate: Callback<()>,
    on_reset_current_media: Callback<()>,
    on_export_subtitles: Callback<(SubtitleExportFormat, SubtitleExportTrack)>,
    on_update_subtitle_cue: Callback<SubtitleEditRequest>,
    on_select_track: Callback<(TrackKind, i32)>,
) -> impl IntoView {
    let active_tab = RwSignal::new("media".to_string());
    let validation_error = move || draft.get().validate().err();

    view! {
        <Show when=move || open.get()>
            <div class="modal-backdrop" on:click=move |_| on_close.run(())></div>
            <section class="settings-modal" role="dialog" aria-modal="true" aria-label="Settings">
                <aside class="settings-sidebar" tabindex="0" aria-label="Settings navigation">
                    <div class="settings-brand">
                        <img class="app-glyph" src="myna_player_icon.svg" alt="" aria-hidden="true"/>
                        <div>
                            <strong>"Myna Player"</strong>
                            <small>"Settings"</small>
                        </div>
                    </div>
                    <nav>
                        {TABS.into_iter().map(|(id, label)| {
                            let id_for_click = id.to_string();
                            let tab_id = if id == "current-media" { "media" } else { id };
                            view! {
                                <button
                                    class=move || if active_tab.get() == tab_id {
                                        "settings-nav__item settings-nav__item--active"
                                    } else {
                                        "settings-nav__item"
                                    }
                                    on:click=move |_| {
                                        active_tab.set(if id_for_click == "current-media" { "media".into() } else { id_for_click.clone() })
                                    }
                                >
                                    <span><Icon name=id/></span>
                                    <strong>{label}</strong>
                                </button>
                            }
                        }).collect_view()}
                    </nav>
                    <div class="settings-sidebar__privacy">
                        <span></span>
                        <small>"Local processing is the default. Cloud providers are always opt-in."</small>
                    </div>
                </aside>

                <div class="settings-content">
                    <header class="settings-header">
                        <div>
                            <small>"Preferences"</small>
                            <h2>{move || tab_title(&active_tab.get())}</h2>
                        </div>
                        <button class="modal-close" title="Close" on:click=move |_| on_close.run(())><Icon name="close"/></button>
                    </header>

                    <div class="settings-scroll" tabindex="0" aria-label="Settings content">
                        <Show when=move || active_tab.get() == "media">
                            <CurrentMediaTab
                                metadata=metadata
                                player=player
                                processing=processing
                                on_start=on_process_start
                                on_pause=on_process_pause
                                on_retry=on_process_retry
                                on_translate=on_process_translate
                                on_reset=on_reset_current_media
                                on_export=on_export_subtitles
                                on_update_cue=on_update_subtitle_cue
                                on_select_track=on_select_track
                            />
                        </Show>

                        <Show when=move || active_tab.get() == "general">
                            <SettingsSection
                                title="Appearance & language"
                                description="Defaults that apply to every new media session."
                            >
                                <SettingRow label="My subtitle language" help="Translation target used for newly opened videos.">
                                    <select
                                        prop:value=move || draft.get().general.preferred_subtitle_language
                                        on:change=move |event| {
                                            let value = event_target_value(&event);
                                            draft.update(|settings| {
                                                settings.general.preferred_subtitle_language = value.clone();
                                                settings.translation.target_language = value;
                                            });
                                        }
                                    >
                                        {language_options()}
                                    </select>
                                </SettingRow>
                            </SettingsSection>
                        </Show>

                        <Show when=move || active_tab.get() == "playback">
                            <SettingsSection
                                title="Playback behavior"
                                description="Controls stay lightweight while remembering the useful choices."
                            >
                                <ToggleRow
                                    label="Remember playback position"
                                    help="Resume a previously opened file from its last saved position."
                                    checked=Signal::derive(move || draft.get().playback.remember_position)
                                    on_change=Callback::new(move |value| draft.update(|settings| {
                                        settings.playback.remember_position = value;
                                    }))
                                />
                                <ToggleRow
                                    label="Autoplay after initial buffer"
                                    help="Start automatically when the first subtitle window becomes ready."
                                    checked=Signal::derive(move || draft.get().playback.autoplay_after_initial_buffer)
                                    on_change=Callback::new(move |value| draft.update(|settings| {
                                        settings.playback.autoplay_after_initial_buffer = value;
                                    }))
                                />
                                <SettingRow label="Preferred audio language" help="Used when a file offers several audio tracks.">
                                    <select
                                        prop:value=move || draft.get().playback.preferred_audio_language
                                        on:change=move |event| draft.update(|settings| {
                                            settings.playback.preferred_audio_language = event_target_value(&event);
                                        })
                                    >
                                        <option value="auto">"Automatic"</option>
                                        <option value="en">"English"</option>
                                        <option value="tr">"Turkish"</option>
                                        <option value="fr">"French"</option>
                                        <option value="de">"German"</option>
                                        <option value="es">"Spanish"</option>
                                    </select>
                                </SettingRow>
                            </SettingsSection>
                        </Show>

                        <Show when=move || active_tab.get() == "subtitles">
                            <SettingsSection
                                title="Subtitle presentation"
                                description="AI cues remain separate from embedded subtitle tracks."
                            >
                                <SettingRow label="Preferred cue track" help="Translated text is never replaced silently by source text.">
                                    <select
                                        prop:value=move || draft.get().subtitles.preferred_track
                                        on:change=move |event| draft.update(|settings| {
                                            settings.subtitles.preferred_track = event_target_value(&event);
                                        })
                                    >
                                        <option value="translated">"Translated (label source fallback)"</option>
                                        <option value="source">"Source transcript"</option>
                                        <option value="dual">"Source + translated"</option>
                                    </select>
                                </SettingRow>
                                <SettingRow label="Text size" help="Scales the subtitle overlay without affecting the player UI.">
                                    <div class="range-setting">
                                        <input
                                            type="range"
                                            min="0.6"
                                            max="2.5"
                                            step="0.1"
                                            prop:value=move || draft.get().subtitles.font_scale.to_string()
                                            on:input=move |event| {
                                                if let Ok(value) = event_target_value(&event).parse::<f32>() {
                                                    draft.update(|settings| settings.subtitles.font_scale = value);
                                                }
                                            }
                                        />
                                        <strong>{move || format!("{:.1}×", draft.get().subtitles.font_scale)}</strong>
                                    </div>
                                </SettingRow>
                                <SettingRow label="Vertical position" help="Moves generated cues above the player controls.">
                                    <div class="range-setting">
                                        <input
                                            type="range"
                                            min="4"
                                            max="35"
                                            step="1"
                                            prop:value=move || draft.get().subtitles.vertical_offset_percent.to_string()
                                            on:input=move |event| {
                                                if let Ok(value) = event_target_value(&event).parse::<u8>() {
                                                    draft.update(|settings| settings.subtitles.vertical_offset_percent = value);
                                                }
                                            }
                                        />
                                        <strong>{move || format!("{}%", draft.get().subtitles.vertical_offset_percent)}</strong>
                                    </div>
                                </SettingRow>
                                <ToggleRow
                                    label="Smart start"
                                    help="Wait for the first subtitle window, with an always-available Play now override."
                                    checked=Signal::derive(move || draft.get().subtitles.smart_wait_enabled)
                                    on_change=Callback::new(move |value| draft.update(|settings| {
                                        settings.subtitles.smart_wait_enabled = value;
                                    }))
                                />
                            </SettingsSection>
                        </Show>

                        <Show when=move || active_tab.get() == "transcription">
                            <SettingsSection
                                title="Local transcription"
                                description="FFmpeg and Whisper process audio locally in resumable windows."
                            >
                                <ToggleRow
                                    label="Transcribe new videos automatically"
                                    help="Starts the first 30-second window immediately after opening a video."
                                    checked=Signal::derive(move || draft.get().transcription.auto_start)
                                    on_change=Callback::new(move |value| draft.update(|settings| {
                                        settings.transcription.auto_start = value;
                                    }))
                                />
                                <ToggleRow
                                    label="Finish the complete video"
                                    help="After the urgent look-ahead, continue the rest at background priority."
                                    checked=Signal::derive(move || draft.get().transcription.process_full_media)
                                    on_change=Callback::new(move |value| draft.update(|settings| {
                                        settings.transcription.process_full_media = value;
                                    }))
                                />
                                <ToggleRow
                                    label="Voice activity detection"
                                    help="Use the local Silero model to skip silence and process only spoken regions."
                                    checked=Signal::derive(move || draft.get().transcription.vad_enabled)
                                    on_change=Callback::new(move |value| draft.update(|settings| {
                                        settings.transcription.vad_enabled = value;
                                    }))
                                />
                                <div class="model-list">
                                    <For
                                        each=move || models.get()
                                        key=|model| model.id.clone()
                                        children=move |model| view! {
                                            <ModelCard
                                                model=model
                                                model_busy=model_busy
                                                on_install=on_install_model
                                                on_verify=on_verify_model
                                                on_delete=on_delete_model
                                            />
                                        }
                                    />
                                </div>
                                <SettingRow label="Spoken language" help="Automatic detection is best when files contain multiple languages.">
                                    <select
                                        prop:value=move || draft.get().transcription.spoken_language
                                        on:change=move |event| draft.update(|settings| {
                                            settings.transcription.spoken_language = event_target_value(&event);
                                        })
                                    >
                                        <option value="auto">"Auto detect"</option>
                                        <option value="en">"English"</option>
                                        <option value="tr">"Turkish"</option>
                                        <option value="fr">"French"</option>
                                        <option value="de">"German"</option>
                                        <option value="es">"Spanish"</option>
                                        <option value="it">"Italian"</option>
                                    </select>
                                </SettingRow>
                                <div class="settings-metrics">
                                    <Metric label="Window" value=Signal::derive(move || format!("{} sec", draft.get().transcription.chunk_duration_ms / 1_000))/>
                                    <Metric label="Initial buffer" value=Signal::derive(move || format!("{} sec", draft.get().transcription.initial_buffer_ms / 1_000))/>
                                    <Metric label="Look-ahead" value=Signal::derive(move || format!("{} sec", draft.get().transcription.lookahead_ms / 1_000))/>
                                </div>
                            </SettingsSection>
                        </Show>

                        <Show when=move || active_tab.get() == "translation">
                            <SettingsSection
                                title="Translation providers"
                                description="Only finalized source text is sent, and only after explicit opt-in."
                            >
                                <ToggleRow
                                    label="Translate automatically"
                                    help="Translate each completed transcript window with the selected provider."
                                    checked=Signal::derive(move || draft.get().translation.auto_start)
                                    on_change=Callback::new(move |value| draft.update(|settings| {
                                        settings.translation.auto_start = value;
                                    }))
                                />
                                <SettingRow label="Provider" help="Unavailable adapters are shown honestly and cannot be selected.">
                                    <select
                                        prop:value=move || draft.get().translation.provider_id
                                        on:change=move |event| {
                                            let provider_id = event_target_value(&event);
                                            let default_model = providers
                                                .get_untracked()
                                                .into_iter()
                                                .find(|provider| provider.id == provider_id)
                                                .and_then(|provider| provider.supported_endpoints.first().cloned())
                                                .unwrap_or_default();
                                            draft.update(|settings| {
                                                settings.translation.provider_id = provider_id;
                                                settings.translation.model = default_model;
                                            });
                                        }
                                    >
                                        <For
                                            each=move || providers.get()
                                            key=|provider| provider.id.clone()
                                            children=move |provider| view! {
                                                <option
                                                    value=provider.id
                                                    disabled=!provider.available
                                                >
                                                    {provider.display_name}
                                                </option>
                                            }
                                        />
                                    </select>
                                </SettingRow>
                                <Show when=move || draft.get().translation.provider_id == "deepl">
                                    <SettingRow label="DeepL endpoint" help="Free and Pro accounts use separate endpoints.">
                                        <select
                                            prop:value=move || draft.get().translation.endpoint
                                            on:change=move |event| draft.update(|settings| {
                                                settings.translation.endpoint = event_target_value(&event);
                                            })
                                        >
                                            <option value="free">"API Free"</option>
                                            <option value="pro">"API Pro"</option>
                                        </select>
                                    </SettingRow>
                                </Show>
                                <Show when=move || {
                                    let provider = draft.get().translation.provider_id;
                                    provider != "none" && provider != "deepl"
                                }>
                                    <SettingRow label="Model" help="Provider model identifier. Change this only when your account uses another model.">
                                        <input
                                            type="text"
                                            autocomplete="off"
                                            prop:value=move || draft.get().translation.model
                                            on:input=move |event| draft.update(|settings| {
                                                settings.translation.model = event_target_value(&event);
                                            })
                                        />
                                    </SettingRow>
                                </Show>
                                <Show when=move || draft.get().translation.provider_id != "none">
                                    <SettingRow label="API credential" help="Saved in the operating system keychain and never returned to the UI.">
                                        <div class="credential-field">
                                            <input
                                                type="password"
                                                autocomplete="off"
                                                placeholder=move || if credential.get().configured {
                                                    "Configured · enter to replace"
                                                } else {
                                                    "Paste a new API key"
                                                }
                                                prop:value=move || credential_input.get()
                                                on:input=move |event| credential_input.set(event_target_value(&event))
                                            />
                                            <Show when=move || credential.get().configured>
                                                <button class="text-button text-button--danger" on:click=move |_| on_delete_credential.run(())>
                                                    "Remove"
                                                </button>
                                            </Show>
                                        </div>
                                    </SettingRow>
                                </Show>
                                <SettingRow label="Translate to" help="Defaults to the language selected under General.">
                                    <select
                                        prop:value=move || draft.get().translation.target_language
                                        on:change=move |event| draft.update(|settings| {
                                            settings.translation.target_language = event_target_value(&event);
                                        })
                                    >
                                        {language_options()}
                                    </select>
                                </SettingRow>
                            </SettingsSection>
                        </Show>

                        <Show when=move || active_tab.get() == "storage">
                            <SettingsSection
                                title="Storage & privacy"
                                description="Transcripts survive restarts without storing provider credentials."
                            >
                                <ToggleRow
                                    label="Keep completed transcripts"
                                    help="Reuse source and translated cues when the same media is opened again."
                                    checked=Signal::derive(move || draft.get().storage.keep_completed_transcripts)
                                    on_change=Callback::new(move |value| draft.update(|settings| {
                                        settings.storage.keep_completed_transcripts = value;
                                    }))
                                />
                                <SettingRow label="Cache limit" help="Temporary WAV windows are deleted immediately after ASR.">
                                    <select
                                        prop:value=move || draft.get().storage.cache_limit_mb.to_string()
                                        on:change=move |event| {
                                            if let Ok(value) = event_target_value(&event).parse::<u64>() {
                                                draft.update(|settings| settings.storage.cache_limit_mb = value);
                                            }
                                        }
                                    >
                                        <option value="512">"512 MB"</option>
                                        <option value="1024">"1 GB"</option>
                                        <option value="2048">"2 GB"</option>
                                        <option value="4096">"4 GB"</option>
                                    </select>
                                </SettingRow>
                                <div class="privacy-card">
                                    <strong>"What leaves this Mac?"</strong>
                                    <p>"Video and audio never leave the device. When cloud translation is enabled, only finalized source transcript text is sent to the selected provider."</p>
                                </div>
                            </SettingsSection>
                        </Show>

                        <Show when=move || active_tab.get() == "advanced">
                            <SettingsSection
                                title="Runtime diagnostics"
                                description="Missing dependencies are explicit unavailable states, never simulated output."
                            >
                                <div class="runtime-grid">
                                    {move || runtime.get().map(|runtime| view! {
                                        <RuntimeItem name="FFmpeg" ready=runtime.ffmpeg.available detail=runtime.ffmpeg.version/>
                                        <RuntimeItem name="FFprobe" ready=runtime.ffprobe.available detail=runtime.ffprobe.version/>
                                        <RuntimeItem name="Whisper" ready=runtime.whisper.available detail=runtime.whisper.version/>
                                        <RuntimeItem name="Whisper model" ready=runtime.whisper_model.available detail=runtime.whisper_model.path/>
                                        <RuntimeItem name="Silero VAD" ready=runtime.vad_model.available detail=runtime.vad_model.path/>
                                    })}
                                </div>
                                <ToggleRow
                                    label="Diagnostic logging"
                                    help="Keep disabled unless investigating a reproducible runtime issue."
                                    checked=Signal::derive(move || draft.get().advanced.diagnostic_logging)
                                    on_change=Callback::new(move |value| draft.update(|settings| {
                                        settings.advanced.diagnostic_logging = value;
                                    }))
                                />
                                {move || diagnostics.get().map(|snapshot| view! {
                                    <DiagnosticCard snapshot=snapshot/>
                                })}
                            </SettingsSection>
                        </Show>
                    </div>

                    <footer class="settings-footer">
                        <div class="settings-validation">
                            {move || validation_error().map(|error| view! { <span>{error}</span> })}
                        </div>
                        <button class="button button--ghost" on:click=move |_| on_close.run(())>"Cancel"</button>
                        <button
                            class="button button--primary"
                            disabled=move || validation_error().is_some()
                            on:click=move |_| on_save.run((draft.get_untracked(), credential_input.get_untracked()))
                        >
                            "Save changes"
                        </button>
                    </footer>
                </div>
            </section>
        </Show>
    }
}

#[component]
fn DiagnosticCard(snapshot: DiagnosticSnapshot) -> impl IntoView {
    let worker_status = if snapshot.worker_running {
        "Running".to_owned()
    } else {
        "Stopped".to_owned()
    };
    let cache_usage = format_bytes(snapshot.cache_usage_bytes);
    let model_path = snapshot
        .worker_model_path
        .unwrap_or_else(|| "No model loaded".into());
    let logs = if snapshot.diagnostic_logging {
        if snapshot.worker_logs.is_empty() {
            view! { <span>"No worker output captured yet."</span> }.into_any()
        } else {
            snapshot
                .worker_logs
                .into_iter()
                .map(|line| view! { <span>{line}</span> })
                .collect_view()
                .into_any()
        }
    } else {
        view! { <span>"Enable diagnostic logging to capture worker output."</span> }.into_any()
    };
    view! {
        <div class="diagnostic-card">
            <div class="diagnostic-card__metrics">
                <Metric label="Whisper worker" value=Signal::derive(move || worker_status.clone())/>
                <Metric label="Transcript cache" value=Signal::derive(move || cache_usage.clone())/>
            </div>
            <small>{model_path}</small>
            <code>{snapshot.database_path}</code>
            <div class="diagnostic-log" aria-label="Recent Whisper worker logs">
                {logs}
            </div>
        </div>
    }
}

#[component]
fn ModelCard(
    model: ModelDescriptor,
    model_busy: ReadSignal<Option<String>>,
    on_install: Callback<String>,
    on_verify: Callback<String>,
    on_delete: Callback<String>,
) -> impl IntoView {
    let installed = model.installed;
    let verified = model.verified;
    let installing = model.installing;
    let downloaded_bytes = model.downloaded_bytes;
    let size_bytes = model.size_bytes;
    let progress_percent = if size_bytes == 0 {
        0.0
    } else {
        (downloaded_bytes as f64 / size_bytes as f64 * 100.0).clamp(0.0, 100.0)
    };
    let install_id = model.id.clone();
    let busy_id = model.id.clone();
    let verify_id = model.id.clone();
    let delete_id = model.id.clone();
    let status_class = if verified {
        "model-status model-status--ready"
    } else if installed {
        "model-status model-status--warning"
    } else {
        "model-status"
    };
    let status_text = if installing {
        "Downloading"
    } else if verified {
        "Verified"
    } else if installed {
        "Needs verification"
    } else {
        "Not installed"
    };

    view! {
        <div class="model-card">
            <div class="model-card__icon"><Icon name="download"/></div>
            <div class="model-card__copy">
                <div class="model-card__title">
                    <strong>{model.display_name}</strong>
                    <span class=status_class>{status_text}</span>
                </div>
                <small>{model.description}</small>
                <code>{format!("SHA-256 {}…", &model.sha256[..12])}</code>
                <Show when=move || installing>
                    <div class="model-progress" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow=progress_percent.round().to_string()>
                        <span style=format!("width: {progress_percent:.1}%")></span>
                    </div>
                    <small>{format!("{} / {} · {:.0}%", format_bytes(downloaded_bytes), format_bytes(size_bytes), progress_percent)}</small>
                </Show>
            </div>
            <div class="model-card__actions">
                {if installed {
                    view! {
                        <button
                            class="button button--ghost"
                            disabled=move || model_busy.get().is_some()
                            on:click=move |_| on_verify.run(verify_id.clone())
                        >"Verify"</button>
                        <button
                            class="text-button text-button--danger"
                            disabled=move || model_busy.get().is_some()
                            on:click=move |_| on_delete.run(delete_id.clone())
                        >"Remove"</button>
                    }.into_any()
                } else {
                    view! {
                        <button
                            class="button button--primary"
                            disabled=move || model_busy.get().is_some()
                            on:click=move |_| on_install.run(install_id.clone())
                        >
                            {move || if installing || model_busy.get().as_deref() == Some(busy_id.as_str()) {
                                "Installing…"
                            } else {
                                "Install"
                            }}
                        </button>
                    }.into_any()
                }}
            </div>
        </div>
    }
}

#[component]
fn CurrentMediaTab(
    metadata: ReadSignal<Option<MediaMetadata>>,
    player: ReadSignal<PlayerSnapshot>,
    processing: ReadSignal<ProcessingSnapshot>,
    on_start: Callback<()>,
    on_pause: Callback<()>,
    on_retry: Callback<()>,
    on_translate: Callback<()>,
    on_reset: Callback<()>,
    on_export: Callback<(SubtitleExportFormat, SubtitleExportTrack)>,
    on_update_cue: Callback<SubtitleEditRequest>,
    on_select_track: Callback<(TrackKind, i32)>,
) -> impl IntoView {
    let editor_open = RwSignal::new(false);
    let selected_id = RwSignal::new(String::new());
    let start_seconds = RwSignal::new(String::new());
    let end_seconds = RwSignal::new(String::new());
    let source_text = RwSignal::new(String::new());
    let translated_text = RwSignal::new(String::new());
    let export_track = RwSignal::new("source".to_string());
    let reset_confirming = RwSignal::new(false);

    view! {
        <SettingsSection
            title="Current media"
            description="Per-file tracks, processing progress and recovery actions."
        >
            <Show
                when=move || metadata.get().is_some()
                fallback=|| view! {
                    <div class="empty-settings-state">
                        <strong>"No video open"</strong>
                        <p>"Drop a local video into the player to see its audio tracks and processing session."</p>
                    </div>
                }
            >
                {move || metadata.get().map(|media| view! {
                    <div class="media-card">
                        <div>
                            <small>"Playing"</small>
                            <strong>{media.file_name}</strong>
                            <span>{format!(
                                "{} · {} audio · {} embedded subtitle track(s)",
                                format_duration(media.duration_ms),
                                media.audio_streams.len(),
                                media.subtitle_streams.len()
                            )}</span>
                        </div>
                    </div>
                    <div class="track-list">
                        <small class="group-label">"Audio tracks"</small>
                        {media.audio_streams.into_iter().map(|track| {
                            let player_track_id = track.player_track_id;
                            let selected = player_track_id.is_some_and(|id| {
                                player.get().tracks.into_iter().any(|candidate| {
                                    candidate.kind == TrackKind::Audio
                                        && candidate.id == id
                                        && candidate.selected
                                })
                            });
                            view! {
                            <button
                                class=if selected { "track-row track-row--selected" } else { "track-row" }
                                disabled=player_track_id.is_none()
                                on:click=move |_| {
                                    if let Some(id) = player_track_id {
                                        on_select_track.run((TrackKind::Audio, id));
                                    }
                                }
                            >
                                <span>{format!("{}", track.relative_index + 1)}</span>
                                <div>
                                    <strong>{track.title.unwrap_or_else(|| format!("Audio {}", track.relative_index + 1))}</strong>
                                    <small>{format!(
                                        "{} · {} channel(s)",
                                        track.language.unwrap_or_else(|| "Unknown language".into()),
                                        track.channels.unwrap_or(0)
                                    )}</small>
                                </div>
                            </button>
                        }}).collect_view()}
                    </div>
                    <div class="track-list">
                        <small class="group-label">"Embedded subtitle tracks"</small>
                        {move || player.get().tracks.into_iter()
                            .filter(|track| track.kind == TrackKind::Subtitle)
                            .map(|track| {
                                let id = track.id;
                                view! {
                                    <button
                                        class=if track.selected { "track-row track-row--selected" } else { "track-row" }
                                        on:click=move |_| on_select_track.run((TrackKind::Subtitle, id))
                                    >
                                        <span><Icon name="captions"/></span>
                                        <div>
                                            <strong>{track.label}</strong>
                                            <small>{track.language.unwrap_or_else(|| "Embedded subtitle".into())}</small>
                                        </div>
                                    </button>
                                }
                            }).collect_view()}
                    </div>
                })}
                <div class="processing-card">
                    <div class="processing-card__head">
                        <div>
                            <small>"Subtitle engine"</small>
                            <strong>{move || processing.get().status_message}</strong>
                        </div>
                        <span>{move || {
                            let state = processing.get();
                            if state.total_windows == 0 {
                                "—".into()
                            } else {
                                format!("{:.0}%", state.completed_windows as f64 / state.total_windows as f64 * 100.0)
                            }
                        }}</span>
                    </div>
                    <div class="processing-progress">
                        <i style:width=move || {
                            let state = processing.get();
                            let percentage = state
                                .completed_windows
                                .saturating_mul(100)
                                .checked_div(state.total_windows)
                                .unwrap_or_default();
                            format!("{percentage}%")
                        }></i>
                    </div>
                    <div class="processing-actions">
                        <button class="button button--primary" on:click=move |_| on_start.run(())>
                            {move || if processing.get().stage == ProcessingStage::Paused { "Resume" } else { "Start / continue" }}
                        </button>
                        <button class="button button--ghost" on:click=move |_| on_pause.run(())>"Pause"</button>
                        <button class="button button--ghost" on:click=move |_| on_translate.run(())>"Translate transcript"</button>
                        <Show when=move || matches!(processing.get().stage, ProcessingStage::Failed | ProcessingStage::Unavailable)>
                            <button class="button button--ghost" on:click=move |_| on_retry.run(())>"Retry"</button>
                        </Show>
                    </div>
                    <Show when=move || processing.get().error.is_some()>
                        <p class="inline-error">{move || processing.get().error.unwrap_or_default()}</p>
                    </Show>
                </div>
                <div class="video-reset-card">
                    <div>
                        <small class="group-label">"Per-video data"</small>
                        <strong>"Start fresh with this video"</strong>
                        <p>
                            "Deletes its transcript, translations, processing checkpoints and remembered playback position. The video stays open and returns to the beginning."
                        </p>
                    </div>
                    <Show
                        when=move || reset_confirming.get()
                        fallback=move || view! {
                            <button
                                class="button button--danger-ghost"
                                on:click=move |_| reset_confirming.set(true)
                            >
                                "Reset this video"
                            </button>
                        }
                    >
                        <div class="reset-confirmation">
                            <button
                                class="button button--ghost"
                                on:click=move |_| reset_confirming.set(false)
                            >
                                "Cancel"
                            </button>
                            <button
                                class="button button--danger"
                                on:click=move |_| {
                                    reset_confirming.set(false);
                                    editor_open.set(false);
                                    selected_id.set(String::new());
                                    on_reset.run(());
                                }
                            >
                                "Confirm reset"
                            </button>
                        </div>
                    </Show>
                </div>

                <div class="subtitle-tools">
                    <div class="subtitle-tools__head">
                        <div>
                            <small class="group-label">"Subtitle files & corrections"</small>
                            <strong>"Export or fine-tune generated cues"</strong>
                        </div>
                        <button
                            class="button button--ghost"
                            on:click=move |_| editor_open.update(|value| *value = !*value)
                        >
                            {move || if editor_open.get() { "Close editor" } else { "Edit subtitles" }}
                        </button>
                    </div>
                    <div class="export-row">
                        <select
                            class="control-select"
                            prop:value=move || export_track.get()
                            on:change=move |event| export_track.set(event_target_value(&event))
                        >
                            <option value="source">"Source transcript"</option>
                            <option value="translated">"Translated subtitles"</option>
                            <option value="dual">"Source + translation"</option>
                        </select>
                        <button
                            class="button button--ghost"
                            on:click=move |_| on_export.run((SubtitleExportFormat::Srt, export_track_value(export_track.get_untracked())))
                        >"Export SRT"</button>
                        <button
                            class="button button--ghost"
                            on:click=move |_| on_export.run((SubtitleExportFormat::Vtt, export_track_value(export_track.get_untracked())))
                        >"Export VTT"</button>
                    </div>
                    <Show when=move || editor_open.get()>
                        <div class="subtitle-editor">
                            <div class="subtitle-editor__list">
                                <For
                                    each=move || processing.get().source_segments
                                    key=|segment| segment.id.clone()
                                    children=move |segment| {
                                        let id = segment.id.clone();
                                        let segment_for_click = segment.clone();
                                        view! {
                                            <button
                                                class=move || if selected_id.get() == id { "cue-row cue-row--selected" } else { "cue-row" }
                                                on:click=move |_| {
                                                    let translation = processing
                                                        .get_untracked()
                                                        .translated_cues
                                                        .into_iter()
                                                        .find(|cue| cue.id == segment_for_click.id)
                                                        .and_then(|cue| cue.translated_text)
                                                        .unwrap_or_default();
                                                    selected_id.set(segment_for_click.id.clone());
                                                    start_seconds.set(format_seconds_input(segment_for_click.start_ms));
                                                    end_seconds.set(format_seconds_input(segment_for_click.end_ms));
                                                    source_text.set(segment_for_click.text.clone());
                                                    translated_text.set(translation);
                                                }
                                            >
                                                <span>{format!("{} → {}", format_editor_time(segment.start_ms), format_editor_time(segment.end_ms))}</span>
                                                <strong>{segment.text}</strong>
                                            </button>
                                        }
                                    }
                                />
                            </div>
                            <div class="subtitle-editor__form">
                                <Show
                                    when=move || !selected_id.get().is_empty()
                                    fallback=|| view! { <p class="editor-placeholder">"Select a cue to edit its text and timing."</p> }
                                >
                                    <div class="editor-time-grid">
                                        <label>
                                            <span>"Start (seconds)"</span>
                                            <input type="text" prop:value=move || start_seconds.get() on:input=move |event| start_seconds.set(event_target_value(&event))/>
                                        </label>
                                        <label>
                                            <span>"End (seconds)"</span>
                                            <input type="text" prop:value=move || end_seconds.get() on:input=move |event| end_seconds.set(event_target_value(&event))/>
                                        </label>
                                    </div>
                                    <label>
                                        <span>"Source transcript"</span>
                                        <textarea prop:value=move || source_text.get() on:input=move |event| source_text.set(event_target_value(&event))></textarea>
                                    </label>
                                    <label>
                                        <span>"Translation"</span>
                                        <textarea prop:value=move || translated_text.get() on:input=move |event| translated_text.set(event_target_value(&event))></textarea>
                                    </label>
                                    <button
                                        class="button button--primary"
                                        on:click=move |_| {
                                            if let (Ok(start_ms), Ok(end_ms)) = (
                                                parse_seconds_input(&start_seconds.get_untracked()),
                                                parse_seconds_input(&end_seconds.get_untracked()),
                                            ) {
                                                on_update_cue.run(SubtitleEditRequest {
                                                    id: selected_id.get_untracked(),
                                                    start_ms,
                                                    end_ms,
                                                    source_text: source_text.get_untracked(),
                                                    translated_text: Some(translated_text.get_untracked()),
                                                });
                                            }
                                        }
                                    >"Save correction"</button>
                                </Show>
                            </div>
                        </div>
                    </Show>
                </div>
            </Show>
        </SettingsSection>
    }
}

#[component]
fn SettingsSection(
    title: &'static str,
    description: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <section class="settings-section">
            <div class="settings-section__intro">
                <h3>{title}</h3>
                <p>{description}</p>
            </div>
            <div class="settings-section__body">{children()}</div>
        </section>
    }
}

#[component]
fn SettingRow(label: &'static str, help: &'static str, children: Children) -> impl IntoView {
    view! {
        <label class="setting-row">
            <div>
                <strong>{label}</strong>
                <small>{help}</small>
            </div>
            <div class="setting-control">{children()}</div>
        </label>
    }
}

#[component]
fn ToggleRow(
    label: &'static str,
    help: &'static str,
    checked: Signal<bool>,
    on_change: Callback<bool>,
) -> impl IntoView {
    view! {
        <label class="setting-row">
            <div>
                <strong>{label}</strong>
                <small>{help}</small>
            </div>
            <button
                type="button"
                role="switch"
                aria-checked=move || checked.get().to_string()
                class=move || if checked.get() { "switch switch--on" } else { "switch" }
                on:click=move |_| on_change.run(!checked.get_untracked())
            >
                <span></span>
            </button>
        </label>
    }
}

#[component]
fn Metric(label: &'static str, #[prop(into)] value: Signal<String>) -> impl IntoView {
    view! {
        <div>
            <small>{label}</small>
            <strong>{move || value.get()}</strong>
        </div>
    }
}

#[component]
fn RuntimeItem(name: &'static str, ready: bool, detail: Option<String>) -> impl IntoView {
    view! {
        <div class=if ready { "runtime-item runtime-item--ready" } else { "runtime-item runtime-item--missing" }>
            <span></span>
            <div>
                <strong>{name}</strong>
                <small>{detail.unwrap_or_else(|| "Not available".into())}</small>
            </div>
        </div>
    }
}

fn export_track_value(value: String) -> SubtitleExportTrack {
    match value.as_str() {
        "source" => SubtitleExportTrack::Source,
        "dual" => SubtitleExportTrack::Dual,
        _ => SubtitleExportTrack::Translated,
    }
}

fn format_seconds_input(milliseconds: u64) -> String {
    format!("{:.3}", milliseconds as f64 / 1_000.0)
}

fn parse_seconds_input(value: &str) -> Result<u64, ()> {
    let seconds = value
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .map_err(|_| ())?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(());
    }
    Ok((seconds * 1_000.0).round().min(u64::MAX as f64) as u64)
}

fn format_editor_time(milliseconds: u64) -> String {
    let minutes = milliseconds / 60_000;
    let seconds = milliseconds / 1_000 % 60;
    let millis = milliseconds % 1_000;
    format!("{minutes:02}:{seconds:02}.{millis:03}")
}

fn language_options() -> impl IntoView {
    view! {
        <option value="TR">"Turkish"</option>
        <option value="EN">"English"</option>
        <option value="FR">"French"</option>
        <option value="DE">"German"</option>
        <option value="ES">"Spanish"</option>
        <option value="IT">"Italian"</option>
        <option value="PT">"Portuguese"</option>
        <option value="NL">"Dutch"</option>
        <option value="JA">"Japanese"</option>
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KB", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} B")
    }
}

fn tab_title(tab: &str) -> &'static str {
    TABS.iter()
        .find(|(id, _)| (*id == "current-media" && tab == "media") || *id == tab)
        .map(|(_, title)| *title)
        .unwrap_or("Settings")
}

fn format_duration(milliseconds: u64) -> String {
    let total_seconds = milliseconds / 1_000;
    format!(
        "{:02}:{:02}:{:02}",
        total_seconds / 3_600,
        (total_seconds % 3_600) / 60,
        total_seconds % 60
    )
}
