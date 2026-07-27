use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Serialize, de::DeserializeOwned};
use subahead_core::{
    AudioWindowRequest, AudioWindowResult, LookAheadPlan, LookAheadRequest, MediaMetadata,
    RuntimeDependency, RuntimeStatus,
};
use wasm_bindgen::prelude::*;

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
    let (last_audio, set_last_audio) = signal::<Option<AudioWindowResult>>(None);
    let (source_language, set_source_language) = signal("Auto detect".to_string());
    let (target_language, set_target_language) = signal("Turkish".to_string());

    spawn_local(async move {
        match invoke_typed::<RuntimeStatus, _>("inspect_runtime", &EmptyArgs {}).await {
            Ok(status) => set_runtime.set(Some(status)),
            Err(error) => set_activity.set(format!("Runtime check failed: {error}")),
        }
    });

    let open_video = move |_| {
        spawn_local(async move {
            set_is_busy.set(true);
            set_activity.set("Opening video…".into());
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
                    set_plan.set(None);
                    set_last_audio.set(None);
                    set_selected_video.set(Some(SelectedVideo { path, source_url }));
                    set_metadata.set(Some(media));
                    set_activity.set("Video ready. Prepare the first audio window.".into());
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
            target_buffer_ms: Some(90_000),
            urgent_buffer_ms: Some(20_000),
            chunk_duration_ms: Some(30_000),
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

    let prepare_audio = move |_| {
        let Some(video) = selected_video.get_untracked() else {
            return;
        };
        let start_ms = playback_position_ms.get_untracked();
        let request = AudioWindowRequest {
            path: video.path,
            start_ms,
            duration_ms: 30_000,
            audio_relative_index: Some(0),
        };

        spawn_local(async move {
            set_is_busy.set(true);
            set_activity.set(format!(
                "Extracting audio window at {}…",
                format_time(start_ms)
            ));
            match invoke_typed::<AudioWindowResult, _>(
                "extract_audio_window",
                &AudioWindowArgs { request: &request },
            )
            .await
            {
                Ok(audio) => {
                    set_ready_until_ms.update(|ready| *ready = (*ready).max(audio.end_ms));
                    set_activity.set(
                        "Audio window ready: 16 kHz mono PCM. Whisper adapter is next.".into(),
                    );
                    set_last_audio.set(Some(audio));
                    refresh_plan();
                }
                Err(error) => set_activity.set(format!("Audio extraction failed: {error}")),
            }
            set_is_busy.set(false);
        });
    };

    let on_time_update = move |_| {
        if let Some(video) = video_ref.get() {
            set_playback_position_ms.set((video.current_time().max(0.0) * 1_000.0) as u64);
        }
    };

    Effect::new(move |_| {
        let _ = playback_position_ms.get();
        let _ = ready_until_ms.get();
        if metadata.get().is_some() {
            refresh_plan();
        }
    });

    let buffer_ms = move || {
        ready_until_ms
            .get()
            .saturating_sub(playback_position_ms.get())
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
                                />
                            }.into_any(),
                            None => view! {
                                <button class="empty-player" on:click=open_video>
                                    <span class="empty-player__icon">"＋"</span>
                                    <strong>"Drop-in AI subtitles begin with a video"</strong>
                                    <small>"MP4, MKV, AVI, MOV, M4V or WebM"</small>
                                </button>
                            }.into_any(),
                        }}
                        <div class="subtitle-preview">
                            <span>"Original speech will appear here"</span>
                            <strong>"Türkçe çeviri burada görünecek"</strong>
                        </div>
                    </div>

                    <div class="timeline-card">
                        <div class="timeline-head">
                            <div>
                                <span>"Playback"</span>
                                <strong>{move || format_time(playback_position_ms.get())}</strong>
                            </div>
                            <div class="timeline-ready">
                                <span>"Translated buffer"</span>
                                <strong>{move || format!("+{}", format_time(buffer_ms()))}</strong>
                            </div>
                        </div>
                        <div class="buffer-track">
                            <div
                                class="buffer-track__fill"
                                style:width=move || format!("{}%", ((buffer_ms() as f64 / 90_000.0) * 100.0).clamp(0.0, 100.0))
                            ></div>
                        </div>
                        <div class="pipeline-steps">
                            <span class="pipeline-step pipeline-step--active">"1  Audio"</span>
                            <span class="pipeline-step">"2  Speech to text"</span>
                            <span class="pipeline-step">"3  Language"</span>
                            <span class="pipeline-step">"4  Translation"</span>
                            <span class="pipeline-step">"5  Subtitle"</span>
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
                            <span class=move || if is_busy.get() { "status status--busy" } else { "status" }>
                                {move || if is_busy.get() { "Working" } else { "Idle" }}
                            </span>
                        </div>

                        <label class="field">
                            <span>"Spoken language"</span>
                            <select
                                prop:value=move || source_language.get()
                                on:change=move |event| set_source_language.set(event_target_value(&event))
                            >
                                <option>"Auto detect"</option>
                                <option>"English"</option>
                                <option>"French"</option>
                                <option>"German"</option>
                                <option>"Spanish"</option>
                            </select>
                        </label>

                        <label class="field">
                            <span>"Translate to"</span>
                            <select
                                prop:value=move || target_language.get()
                                on:change=move |event| set_target_language.set(event_target_value(&event))
                            >
                                <option>"Turkish"</option>
                                <option>"English"</option>
                                <option>"French"</option>
                                <option>"German"</option>
                            </select>
                        </label>

                        <div class="provider-grid">
                            <button class="provider provider--selected">
                                <span>"ASR"</span>
                                <strong>"whisper.cpp"</strong>
                                <small>"Local · not configured"</small>
                            </button>
                            <button class="provider">
                                <span>"Translate"</span>
                                <strong>"DeepL / AI"</strong>
                                <small>"Provider adapter"</small>
                            </button>
                        </div>

                        <button
                            class="button button--wide button--accent"
                            on:click=prepare_audio
                            disabled=move || selected_video.get().is_none() || is_busy.get()
                        >
                            "Prepare next 30 seconds"
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
                                            "{}×{} · {}",
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
                                        <span></span><strong>"Whisper"</strong><small>{if status.whisper.available { "Ready" } else { "Next step" }}</small>
                                    </div>
                                </div>
                            }.into_any(),
                            None => view! { <p class="muted">"Checking local dependencies…"</p> }.into_any(),
                        }}
                    </section>

                    <section class="panel panel--compact">
                        <div class="panel-heading">
                            <div>
                                <span class="eyebrow">"Scheduler"</span>
                                <h2>"Next windows"</h2>
                            </div>
                        </div>
                        {move || match plan.get() {
                            Some(current_plan) if !current_plan.windows.is_empty() => view! {
                                <div class="window-list">
                                    {current_plan.windows.into_iter().map(|window| view! {
                                        <div class="window-row">
                                            <span>{format!("{}–{}", format_time(window.start_ms), format_time(window.end_ms))}</span>
                                            <strong>{format!("{:?}", window.priority)}</strong>
                                        </div>
                                    }).collect_view()}
                                </div>
                            }.into_any(),
                            _ => view! { <p class="muted">"Open a video to generate the look-ahead queue."</p> }.into_any(),
                        }}
                        {move || last_audio.get().map(|audio| view! {
                            <p class="audio-result">{format!("Last PCM window: {}–{}", format_time(audio.start_ms), format_time(audio.end_ms))}</p>
                        })}
                    </section>
                </aside>
            </section>
        </main>
    }
}
