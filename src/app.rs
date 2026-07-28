use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Serialize, de::DeserializeOwned};
use subahead_core::{
    AudioWindowRequest, AudioWindowResult, CueStatus, LookAheadPlan, LookAheadRequest,
    MediaMetadata, RuntimeDependency, RuntimeStatus, SubtitleCue, TranscriptionRequest,
    TranscriptionResult, TranslationBatchRequest, TranslationBatchResult, TranslationProviderKind,
};
use wasm_bindgen::prelude::*;

const TARGET_BUFFER_MS: u64 = 90_000;
const CHUNK_DURATION_MS: u64 = 30_000;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = convertFileSrc)]
    fn convert_file_src(path: &str) -> String;
}

#[derive(Debug, Clone)]
struct SelectedVideo {
    path: String,
    source_url: String,
}

#[derive(Serialize)]
struct EmptyArgs {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PathArgs<'a> {
    path: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LookAheadArgs<'a> {
    request: &'a LookAheadRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioWindowArgs<'a> {
    request: &'a AudioWindowRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionArgs<'a> {
    request: &'a TranscriptionRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TranslationArgs<'a> {
    request: &'a TranslationBatchRequest,
}

async fn invoke_typed<T, A>(command: &str, args: &A) -> Result<T, String>
where
    T: DeserializeOwned,
    A: Serialize,
{
    let args = serde_wasm_bindgen::to_value(args).map_err(|error| error.to_string())?;
    let value = invoke(command, args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

fn format_time(milliseconds: u64) -> String {
    let total_seconds = milliseconds / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn format_bytes(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return "Unknown size".into();
    };
    let gb = bytes as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.2} GB")
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}

fn dependency_class(dependency: &RuntimeDependency) -> &'static str {
    if dependency.available {
        "dependency dependency--ready"
    } else {
        "dependency dependency--missing"
    }
}

fn source_cues(result: &TranscriptionResult) -> Vec<SubtitleCue> {
    result
        .segments
        .iter()
        .map(|segment| SubtitleCue {
            id: segment.id.clone(),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            source_text: segment.text.clone(),
            translated_text: None,
            source_language: segment.detected_language.clone(),
            target_language: None,
            status: CueStatus::Transcribed,
        })
        .collect()
}

fn merge_cues(existing: &mut Vec<SubtitleCue>, incoming: Vec<SubtitleCue>) {
    for cue in incoming {
        if let Some(current) = existing.iter_mut().find(|current| current.id == cue.id) {
            *current = cue;
        } else {
            existing.push(cue);
        }
    }
    existing.sort_by_key(|cue| (cue.start_ms, cue.end_ms));
}

fn translation_provider(value: &str) -> TranslationProviderKind {
    match value {
        "deepl-free" => TranslationProviderKind::DeeplFree,
        "deepl-pro" => TranslationProviderKind::DeeplPro,
        _ => TranslationProviderKind::None,
    }
}

#[component]
pub fn App() -> impl IntoView {
    let video_ref = NodeRef::<leptos::html::Video>::new();
    let (selected_video, set_selected_video) = signal::<Option<SelectedVideo>>(None);
    let (metadata, set_metadata) = signal::<Option<MediaMetadata>>(None);
    let (runtime, set_runtime) = signal::<Option<RuntimeStatus>>(None);
    let (playback_position_ms, set_playback_position_ms) = signal(0_u64);
    let (ready_until_ms, set_ready_until_ms) = signal(0_u64);
    let (plan, set_plan) = signal::<Option<LookAheadPlan>>(None);
    let (activity, set_activity) = signal("Choose a local video to begin.".to_string());
    let (is_busy, set_is_busy) = signal(false);
    let (pipeline_enabled, set_pipeline_enabled) = signal(false);
    let (last_audio, set_last_audio) = signal::<Option<AudioWindowResult>>(None);
    let (last_transcription, set_last_transcription) = signal::<Option<TranscriptionResult>>(None);
    let (cues, set_cues) = signal::<Vec<SubtitleCue>>(Vec::new());
    let (source_language, set_source_language) = signal("auto".to_string());
    let (target_language, set_target_language) = signal("TR".to_string());
    let (translation_mode, set_translation_mode) = signal("none".to_string());
    let (deepl_api_key, set_deepl_api_key) = signal(String::new());
    let (audio_relative_index, set_audio_relative_index) = signal(0_u32);

    spawn_local(async move {
        match invoke_typed::<RuntimeStatus, _>("inspect_runtime", &EmptyArgs {}).await {
            Ok(status) => set_runtime.set(Some(status)),
            Err(error) => set_activity.set(format!("Runtime check failed: {error}")),
        }
    });

    let open_video = move |_| {
        spawn_local(async move {
            set_pipeline_enabled.set(false);
            set_is_busy.set(true);
            set_activity.set("Opening video...".into());
            let result = async {
                let path = invoke_typed::<Option<String>, _>("pick_video", &EmptyArgs {})
                    .await?
                    .ok_or_else(|| "No file selected".to_string())?;
                let media =
                    invoke_typed::<MediaMetadata, _>("probe_media", &PathArgs { path: &path })
                        .await?;
                let source_url = convert_file_src(&path);
                Ok::<_, String>((path, source_url, media))
            }
            .await;

            match result {
                Ok((path, source_url, media)) => {
                    set_playback_position_ms.set(0);
                    set_ready_until_ms.set(0);
                    set_audio_relative_index.set(0);
                    set_plan.set(None);
                    set_last_audio.set(None);
                    set_last_transcription.set(None);
                    set_cues.set(Vec::new());
                    set_selected_video.set(Some(SelectedVideo { path, source_url }));
                    set_metadata.set(Some(media));
                    set_activity.set("Video ready. Start the live subtitle pipeline.".into());
                }
                Err(error) if error == "No file selected" => {
                    set_activity.set("File selection cancelled.".into());
                }
                Err(error) => set_activity.set(format!("Could not open video: {error}")),
            }
            set_is_busy.set(false);
        });
    };

    let refresh_plan = move || {
        let Some(media) = metadata.get_untracked() else {
            return;
        };
        let request = LookAheadRequest {
            playback_position_ms: playback_position_ms.get_untracked(),
            ready_until_ms: ready_until_ms.get_untracked(),
            media_duration_ms: media.duration_ms,
            target_buffer_ms: Some(TARGET_BUFFER_MS),
            urgent_buffer_ms: Some(20_000),
            chunk_duration_ms: Some(CHUNK_DURATION_MS),
        };
        spawn_local(async move {
            if let Ok(next_plan) = invoke_typed::<LookAheadPlan, _>(
                "plan_lookahead",
                &LookAheadArgs { request: &request },
            )
            .await
            {
                set_plan.set(Some(next_plan));
            }
        });
    };

    let on_time_update = move |_| {
        if let Some(video) = video_ref.get() {
            set_playback_position_ms.set((video.current_time().max(0.0) * 1_000.0) as u64);
        }
    };

    let on_seeking = move |_| {
        if let Some(video) = video_ref.get() {
            let position = (video.current_time().max(0.0) * 1_000.0) as u64;
            set_playback_position_ms.set(position);
            if position > ready_until_ms.get_untracked() {
                set_ready_until_ms.set(position);
            }
        }
    };

    Effect::new(move |_| {
        let _ = playback_position_ms.get();
        let _ = ready_until_ms.get();
        if metadata.get().is_some() {
            refresh_plan();
        }
    });

    Effect::new(move |_| {
        let enabled = pipeline_enabled.get();
        let busy = is_busy.get();
        let position = playback_position_ms.get();
        let ready_until = ready_until_ms.get();
        let Some(media) = metadata.get() else {
            return;
        };
        let Some(video) = selected_video.get() else {
            return;
        };

        if !enabled || busy || ready_until.saturating_sub(position) >= TARGET_BUFFER_MS {
            return;
        }

        let start_ms = ready_until.max(position).min(media.duration_ms);
        if start_ms >= media.duration_ms {
            set_pipeline_enabled.set(false);
            set_activity.set("Subtitle pipeline reached the end of the video.".into());
            return;
        }
        let duration_ms = CHUNK_DURATION_MS.min(media.duration_ms.saturating_sub(start_ms));
        if duration_ms < 1_000 {
            set_pipeline_enabled.set(false);
            return;
        }

        let audio_request = AudioWindowRequest {
            path: video.path,
            start_ms,
            duration_ms,
            audio_relative_index: Some(audio_relative_index.get_untracked()),
        };
        let language_hint = source_language.get_untracked();
        let target_language = target_language.get_untracked();
        let provider_value = translation_mode.get_untracked();
        let provider = translation_provider(&provider_value);
        let api_key = deepl_api_key.get_untracked();
        let context = cues
            .get_untracked()
            .into_iter()
            .filter(|cue| cue.end_ms <= start_ms)
            .rev()
            .take(8)
            .map(|cue| cue.source_text)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();

        set_is_busy.set(true);
        set_activity.set(format!(
            "Preparing {}-{}...",
            format_time(start_ms),
            format_time(start_ms.saturating_add(duration_ms))
        ));

        spawn_local(async move {
            let result = async {
                let audio = invoke_typed::<AudioWindowResult, _>(
                    "extract_audio_window",
                    &AudioWindowArgs {
                        request: &audio_request,
                    },
                )
                .await?;
                set_activity.set(format!(
                    "Transcribing {}-{} with Whisper...",
                    format_time(audio.start_ms),
                    format_time(audio.end_ms)
                ));
                let transcription_request = TranscriptionRequest {
                    audio_path: audio.output_path.clone(),
                    window_start_ms: audio.start_ms,
                    model_path: None,
                    language_hint: Some(language_hint),
                    prompt: context.last().cloned(),
                };
                let transcription = invoke_typed::<TranscriptionResult, _>(
                    "transcribe_audio",
                    &TranscriptionArgs {
                        request: &transcription_request,
                    },
                )
                .await?;

                let mut next_cues = source_cues(&transcription);
                let should_translate =
                    provider != TranslationProviderKind::None && !transcription.segments.is_empty();
                if should_translate {
                    set_activity.set(format!(
                        "Translating {} subtitle segment(s)...",
                        transcription.segments.len()
                    ));
                    let translation_request = TranslationBatchRequest {
                        segments: transcription.segments.clone(),
                        source_language: transcription.detected_language.clone(),
                        target_language,
                        provider,
                        api_key,
                        previous_context: context,
                    };
                    let translation = invoke_typed::<TranslationBatchResult, _>(
                        "translate_segments",
                        &TranslationArgs {
                            request: &translation_request,
                        },
                    )
                    .await?;
                    next_cues = translation.cues;
                }

                Ok::<_, String>((audio, transcription, next_cues, should_translate))
            }
            .await;

            match result {
                Ok((audio, transcription, next_cues, translated)) => {
                    let segment_count = transcription.segments.len();
                    let elapsed = transcription.elapsed_ms;
                    set_last_audio.set(Some(audio.clone()));
                    set_last_transcription.set(Some(transcription));
                    set_cues.update(|existing| merge_cues(existing, next_cues));
                    set_ready_until_ms.update(|ready| *ready = (*ready).max(audio.end_ms));
                    let mode = if translated {
                        "transcribed and translated"
                    } else {
                        "transcribed"
                    };
                    set_activity.set(format!(
                        "Window {mode}: {segment_count} segment(s) in {:.1}s.",
                        elapsed as f64 / 1_000.0
                    ));
                }
                Err(error) => {
                    set_pipeline_enabled.set(false);
                    set_activity.set(format!("Pipeline stopped: {error}"));
                }
            }
            set_is_busy.set(false);
        });
    });

    let buffer_ms = move || {
        ready_until_ms
            .get()
            .saturating_sub(playback_position_ms.get())
    };
    let active_cue = move || {
        let current = playback_position_ms.get();
        cues.get()
            .into_iter()
            .find(|cue| current >= cue.start_ms && current < cue.end_ms)
    };

    view! {
        <main class="app-shell">
            <header class="topbar">
                <div class="brand-lockup">
                    <div class="brand-mark">"S"</div>
                    <div>
                        <strong>"SubAhead"</strong>
                        <span>"Subtitles before the scene"</span>
                    </div>
                </div>
                <div class="topbar-actions">
                    <span class="local-pill">"Local-first"</span>
                    <button class="button button--primary" on:click=open_video disabled=move || is_busy.get()>
                        {move || if selected_video.get().is_some() { "Open another video" } else { "Open video" }}
                    </button>
                </div>
            </header>

            <section class="workspace">
                <div class="player-column">
                    <div class="player-frame">
                        {move || match selected_video.get() {
                            Some(video) => view! {
                                <video
                                    node_ref=video_ref
                                    class="video-player"
                                    controls
                                    src=video.source_url
                                    on:timeupdate=on_time_update
                                    on:seeking=on_seeking
                                />
                            }.into_any(),
                            None => view! {
                                <button class="empty-player" on:click=open_video>
                                    <span class="empty-player__icon">"+"</span>
                                    <strong>"Drop-in AI subtitles begin with a video"</strong>
                                    <small>"MP4, MKV, AVI, MOV, M4V or WebM"</small>
                                </button>
                            }.into_any(),
                        }}
                        <div class="subtitle-preview">
                            {move || match active_cue() {
                                Some(cue) => view! {
                                    <span>{cue.source_text}</span>
                                    {cue.translated_text.map(|text| view! { <strong>{text}</strong> })}
                                }.into_any(),
                                None if pipeline_enabled.get() => view! {
                                    <small>"SubAhead is preparing the next spoken line..."</small>
                                }.into_any(),
                                None => view! {
                                    <small>"Start live subtitles to analyze speech."</small>
                                }.into_any(),
                            }}
                        </div>
                    </div>

                    <div class="timeline-card">
                        <div class="timeline-head">
                            <div>
                                <span>"Playback"</span>
                                <strong>{move || format_time(playback_position_ms.get())}</strong>
                            </div>
                            <div class="timeline-ready">
                                <span>"Subtitle buffer"</span>
                                <strong>{move || format!("+{}", format_time(buffer_ms()))}</strong>
                            </div>
                        </div>
                        <div class="buffer-track">
                            <div
                                class="buffer-track__fill"
                                style:width=move || format!("{}%", ((buffer_ms() as f64 / TARGET_BUFFER_MS as f64) * 100.0).clamp(0.0, 100.0))
                            ></div>
                        </div>
                        <div class="pipeline-steps">
                            <span class="pipeline-step pipeline-step--active">"1  Audio"</span>
                            <span class=move || if last_transcription.get().is_some() { "pipeline-step pipeline-step--active" } else { "pipeline-step" }>"2  Speech to text"</span>
                            <span class=move || if last_transcription.get().and_then(|value| value.detected_language).is_some() { "pipeline-step pipeline-step--active" } else { "pipeline-step" }>"3  Language"</span>
                            <span class=move || if cues.get().iter().any(|cue| cue.translated_text.is_some()) { "pipeline-step pipeline-step--active" } else { "pipeline-step" }>"4  Translation"</span>
                            <span class=move || if !cues.get().is_empty() { "pipeline-step pipeline-step--active" } else { "pipeline-step" }>"5  Subtitle"</span>
                        </div>
                    </div>
                </div>

                <aside class="control-column">
                    <section class="panel">
                        <div class="panel-heading">
                            <div>
                                <span class="eyebrow">"Live pipeline"</span>
                                <h2>"Subtitle engine"</h2>
                            </div>
                            <span class=move || if is_busy.get() { "status status--busy" } else if pipeline_enabled.get() { "status status--running" } else { "status" }>
                                {move || if is_busy.get() { "Working" } else if pipeline_enabled.get() { "Watching" } else { "Idle" }}
                            </span>
                        </div>

                        <label class="field">
                            <span>"Spoken language"</span>
                            <select
                                prop:value=move || source_language.get()
                                on:change=move |event| set_source_language.set(event_target_value(&event))
                            >
                                <option value="auto">"Auto detect"</option>
                                <option value="en">"English"</option>
                                <option value="fr">"French"</option>
                                <option value="de">"German"</option>
                                <option value="es">"Spanish"</option>
                                <option value="it">"Italian"</option>
                            </select>
                        </label>

                        <label class="field">
                            <span>"Translate to"</span>
                            <select
                                prop:value=move || target_language.get()
                                on:change=move |event| set_target_language.set(event_target_value(&event))
                            >
                                <option value="TR">"Turkish"</option>
                                <option value="EN">"English"</option>
                                <option value="FR">"French"</option>
                                <option value="DE">"German"</option>
                                <option value="ES">"Spanish"</option>
                            </select>
                        </label>

                        <label class="field">
                            <span>"Translation provider"</span>
                            <select
                                prop:value=move || translation_mode.get()
                                on:change=move |event| set_translation_mode.set(event_target_value(&event))
                            >
                                <option value="none">"Transcript only"</option>
                                <option value="deepl-free">"DeepL API Free"</option>
                                <option value="deepl-pro">"DeepL API Pro"</option>
                            </select>
                        </label>

                        {move || if translation_mode.get() != "none" {
                            view! {
                                <label class="field">
                                    <span>"DeepL API key (kept in memory only)"</span>
                                    <input
                                        class="text-input"
                                        type="password"
                                        autocomplete="off"
                                        placeholder="Enter DeepL key"
                                        prop:value=move || deepl_api_key.get()
                                        on:input=move |event| set_deepl_api_key.set(event_target_value(&event))
                                    />
                                </label>
                            }.into_any()
                        } else {
                            view! { <p class="provider-note">"Whisper stays fully local. Enable DeepL only when you want translated subtitles."</p> }.into_any()
                        }}

                        {move || metadata.get().map(|media| view! {
                            <label class="field">
                                <span>"Audio track"</span>
                                <select
                                    prop:value=move || audio_relative_index.get().to_string()
                                    on:change=move |event| {
                                        if let Ok(value) = event_target_value(&event).parse::<u32>() {
                                            set_audio_relative_index.set(value);
                                        }
                                    }
                                >
                                    {media.audio_streams.into_iter().map(|stream| {
                                        let label = stream.title
                                            .or(stream.language)
                                            .unwrap_or_else(|| format!("Audio {}", stream.relative_index + 1));
                                        view! { <option value=stream.relative_index.to_string()>{label}</option> }
                                    }).collect_view()}
                                </select>
                            </label>
                        })}

                        <button
                            class=move || if pipeline_enabled.get() { "button button--wide" } else { "button button--wide button--accent" }
                            on:click=move |_| {
                                if selected_video.get_untracked().is_some() {
                                    set_pipeline_enabled.update(|enabled| *enabled = !*enabled);
                                    set_activity.set(if pipeline_enabled.get_untracked() {
                                        "Live subtitle pipeline started.".into()
                                    } else {
                                        "Live subtitle pipeline paused.".into()
                                    });
                                }
                            }
                            disabled=move || selected_video.get().is_none()
                        >
                            {move || if pipeline_enabled.get() { "Pause live subtitles" } else { "Start live subtitles" }}
                        </button>
                        <p class="activity">{move || activity.get()}</p>
                    </section>

                    <section class="panel panel--compact">
                        <div class="panel-heading">
                            <div>
                                <span class="eyebrow">"Media"</span>
                                <h2>"Current file"</h2>
                            </div>
                        </div>
                        {move || match metadata.get() {
                            Some(media) => {
                                let primary_video = media.video_streams.first().cloned();
                                view! {
                                    <div class="file-summary">
                                        <strong>{media.file_name}</strong>
                                        <span>{format!("{} · {}", format_time(media.duration_ms), format_bytes(media.size_bytes))}</span>
                                        <span>{format!(
                                            "{} video · {} audio · {} embedded subtitles",
                                            media.video_streams.len(),
                                            media.audio_streams.len(),
                                            media.subtitle_streams.len()
                                        )}</span>
                                        <span>{primary_video.map(|stream| format!(
                                            "{}x{} · {}",
                                            stream.width.unwrap_or(0),
                                            stream.height.unwrap_or(0),
                                            stream.codec.unwrap_or_else(|| "unknown codec".into())
                                        )).unwrap_or_else(|| "No video stream detected".into())}</span>
                                    </div>
                                }.into_any()
                            }
                            None => view! { <p class="muted">"No media selected."</p> }.into_any(),
                        }}
                    </section>

                    <section class="panel panel--compact">
                        <div class="panel-heading">
                            <div>
                                <span class="eyebrow">"Environment"</span>
                                <h2>"Local engines"</h2>
                            </div>
                        </div>
                        {move || match runtime.get() {
                            Some(status) => view! {
                                <div class="dependency-list">
                                    <div class=dependency_class(&status.ffmpeg)>
                                        <span></span><strong>"FFmpeg"</strong><small>{if status.ffmpeg.available { "Ready" } else { "Missing" }}</small>
                                    </div>
                                    <div class=dependency_class(&status.ffprobe)>
                                        <span></span><strong>"FFprobe"</strong><small>{if status.ffprobe.available { "Ready" } else { "Missing" }}</small>
                                    </div>
                                    <div class=dependency_class(&status.whisper)>
                                        <span></span><strong>"Whisper CLI"</strong><small>{if status.whisper.available { "Ready" } else { "Missing" }}</small>
                                    </div>
                                    <div class=dependency_class(&status.whisper_model)>
                                        <span></span><strong>"Base model"</strong><small>{status.whisper_model.version.clone().unwrap_or_else(|| "Missing".into())}</small>
                                    </div>
                                </div>
                            }.into_any(),
                            None => view! { <p class="muted">"Checking local dependencies..."</p> }.into_any(),
                        }}
                    </section>

                    <section class="panel panel--compact">
                        <div class="panel-heading">
                            <div>
                                <span class="eyebrow">"Scheduler"</span>
                                <h2>"Next windows"</h2>
                            </div>
                            <span class="cue-count">{move || format!("{} cues", cues.get().len())}</span>
                        </div>
                        {move || match plan.get() {
                            Some(current_plan) if !current_plan.windows.is_empty() => view! {
                                <div class="window-list">
                                    {current_plan.windows.into_iter().map(|window| view! {
                                        <div class="window-row">
                                            <span>{format!("{}-{}", format_time(window.start_ms), format_time(window.end_ms))}</span>
                                            <strong>{format!("{:?}", window.priority)}</strong>
                                        </div>
                                    }).collect_view()}
                                </div>
                            }.into_any(),
                            _ => view! { <p class="muted">"The 90-second buffer is ready or no video is open."</p> }.into_any(),
                        }}
                        {move || last_audio.get().map(|audio| view! {
                            <p class="audio-result">{format!("Last PCM: {}-{}", format_time(audio.start_ms), format_time(audio.end_ms))}</p>
                        })}
                        {move || last_transcription.get().map(|result| view! {
                            <p class="audio-result">{format!(
                                "Whisper: {} · {} segment(s) · {:.1}s",
                                result.detected_language.unwrap_or_else(|| "unknown".into()),
                                result.segments.len(),
                                result.elapsed_ms as f64 / 1_000.0
                            )}</p>
                        })}
                    </section>
                </aside>
            </section>
        </main>
    }
}
