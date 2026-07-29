use leptos::prelude::*;

use super::Icon;
use myna_player_core::{
    AppSettingsV1, MediaMetadata, PlayerSnapshot, PlayerState, ProcessingSnapshot, ProcessingStage,
    SubtitleCue, TrackKind,
};

#[component]
pub fn PlayerShell(
    player: ReadSignal<PlayerSnapshot>,
    processing: ReadSignal<ProcessingSnapshot>,
    metadata: ReadSignal<Option<MediaMetadata>>,
    settings: RwSignal<AppSettingsV1>,
    controls_visible: ReadSignal<bool>,
    drop_active: ReadSignal<bool>,
    settings_open: ReadSignal<bool>,
    on_open: Callback<()>,
    on_toggle_playback: Callback<()>,
    on_play_now: Callback<()>,
    on_seek: Callback<u64>,
    on_volume: Callback<u8>,
    on_toggle_mute: Callback<()>,
    on_rate: Callback<f32>,
    on_select_track: Callback<(TrackKind, i32)>,
    on_fullscreen: Callback<()>,
    on_settings: Callback<()>,
    on_pointer_activity: Callback<()>,
) -> impl IntoView {
    let has_media = move || player.get().media_path.is_some();
    let is_playing = move || {
        matches!(
            player.get().state,
            PlayerState::Playing | PlayerState::Buffering
        )
    };
    let active_cue = move || {
        let snapshot = player.get();
        let processing = processing.get();
        find_active_cue(
            snapshot.position_ms,
            &processing.translated_cues,
            &processing.source_segments,
            &settings.get().subtitles.preferred_track,
        )
    };
    let waiting_for_first_buffer = move || {
        let snapshot = player.get();
        let processing = processing.get();
        let media_duration_ms = metadata
            .get()
            .map(|media| media.duration_ms)
            .unwrap_or(snapshot.duration_ms);
        let required_ms = settings
            .get()
            .transcription
            .initial_buffer_ms
            .min(media_duration_ms.saturating_sub(snapshot.position_ms));
        snapshot.media_path.is_some()
            && processing
                .ready_until_ms
                .saturating_sub(snapshot.position_ms)
                < required_ms
            && matches!(
                processing.stage,
                ProcessingStage::Queued
                    | ProcessingStage::Extracting
                    | ProcessingStage::Transcribing
                    | ProcessingStage::Translating
            )
    };

    view! {
        <main
            class=move || {
                let mut class = "player-shell".to_string();
                if !has_media() {
                    class.push_str(" player-shell--empty");
                }
                if drop_active.get() {
                    class.push_str(" player-shell--drop");
                }
                if settings_open.get() {
                    class.push_str(" player-shell--settings");
                }
                class
            }
            on:mousemove=move |_| on_pointer_activity.run(())
        >
            <div class="video-viewport" aria-label="Video surface">
                <div class="ambient-shade"></div>

                <header class=move || control_class("player-topbar", controls_visible.get(), has_media())>
                    <div class="window-title">
                        <img class="app-glyph" src="myna_player_icon.svg" alt="" aria-hidden="true"/>
                        <div>
                            <strong>{move || player.get().file_name.unwrap_or_else(|| "Myna Player".into())}</strong>
                            <small>{move || media_details(metadata.get())}</small>
                        </div>
                    </div>
                    <div class="top-actions">
                        <ProcessingPill processing=processing/>
                        <button class="icon-button" title="Open video" on:click=move |_| on_open.run(())>
                            <Icon name="open"/>
                        </button>
                        <button class="icon-button" title="Settings" on:click=move |_| on_settings.run(())>
                            <Icon name="settings"/>
                        </button>
                    </div>
                </header>

                <Show
                    when=move || !has_media()
                    fallback=move || view! {
                        <div
                            class="subtitle-layer"
                            style=move || {
                                let subtitle = settings.get().subtitles;
                                format!(
                                    "bottom: calc(74px + {}%); --subtitle-scale: {};",
                                    subtitle.vertical_offset_percent,
                                    subtitle.font_scale
                                )
                            }
                        >
                            {move || active_cue().map(|cue| {
                                view! {
                                    <div class="subtitle-card">
                                        {cue.secondary.map(|source| view! {
                                            <span class="subtitle-source">{source}</span>
                                        })}
                                        <strong class="subtitle-primary">{cue.primary}</strong>
                                    </div>
                                }
                            })}
                        </div>

                        <Show when=waiting_for_first_buffer>
                            <div class="smart-wait">
                                <span class="smart-wait__spinner"></span>
                                <div>
                                    <strong>"Preparing subtitles"</strong>
                                    <small>{move || processing.get().status_message}</small>
                                </div>
                                <button on:click=move |_| on_play_now.run(())>"Play now"</button>
                            </div>
                        </Show>
                    }
                >
                    <button class="empty-state" on:click=move |_| on_open.run(())>
                        <span class="empty-state__mark"><Icon name="play"/></span>
                        <strong>"Drop a video anywhere"</strong>
                        <small>"or click to open a local file"</small>
                        <span class="empty-state__formats">"MP4 · MKV · MOV · AVI · WEBM"</span>
                    </button>
                </Show>

                <Show when=move || drop_active.get()>
                    <div class="drop-overlay">
                        <span><Icon name="download"/></span>
                        <strong>"Release to open"</strong>
                    </div>
                </Show>

                <Show when=move || has_media()>
                    <footer class=move || control_class("player-controls", controls_visible.get(), true)>
                        <div class="timeline-wrap">
                            <input
                                class="timeline"
                                aria-label="Seek"
                                type="range"
                                min="0"
                                max=move || {
                                    player_duration(player.get(), metadata.get())
                                        .max(1)
                                        .to_string()
                                }
                                step="250"
                                prop:value=move || player.get().position_ms.to_string()
                                on:change=move |event| {
                                    if let Ok(value) = event_target_value(&event).parse::<u64>() {
                                        on_seek.run(value);
                                    }
                                }
                            />
                            <div
                                class="timeline__processed"
                                style:width=move || percent(
                                    processing.get().ready_until_ms,
                                    player_duration(player.get(), metadata.get())
                                )
                            ></div>
                        </div>

                        <div class="control-row">
                            <div class="control-group control-group--left">
                                <button
                                    class="transport-button"
                                    title=move || if is_playing() { "Pause" } else { "Play" }
                                    on:click=move |_| on_toggle_playback.run(())
                                >
                                    {move || if is_playing() { view! { <Icon name="pause"/> }.into_any() } else { view! { <Icon name="play"/> }.into_any() }}
                                </button>
                                <button
                                    class="icon-button icon-button--control"
                                    title="Mute"
                                    on:click=move |_| on_toggle_mute.run(())
                                >
                                    {move || if player.get().muted { view! { <Icon name="mute"/> }.into_any() } else { view! { <Icon name="volume"/> }.into_any() }}
                                </button>
                                <input
                                    class="volume"
                                    aria-label="Volume"
                                    type="range"
                                    min="0"
                                    max="100"
                                    prop:value=move || player.get().volume.to_string()
                                    on:input=move |event| {
                                        if let Ok(value) = event_target_value(&event).parse::<u8>() {
                                            on_volume.run(value);
                                        }
                                    }
                                />
                                <span class="timecode">
                                    {move || format!(
                                        "{} / {}",
                                        format_time(player.get().position_ms),
                                        format_time(player_duration(player.get(), metadata.get()))
                                    )}
                                </span>
                            </div>

                            <div class="control-group control-group--right">
                                <TrackSelect
                                    player=player
                                    kind=TrackKind::Audio
                                    label="Audio track"
                                    on_select=on_select_track
                                />
                                <TrackSelect
                                    player=player
                                    kind=TrackKind::Subtitle
                                    label="Embedded subtitles"
                                    on_select=on_select_track
                                />
                                <select
                                    class="control-select"
                                    aria-label="Playback speed"
                                    prop:value=move || format!("{:.2}", player.get().rate)
                                    on:change=move |event| {
                                        if let Ok(rate) = event_target_value(&event).parse::<f32>() {
                                            on_rate.run(rate);
                                        }
                                    }
                                >
                                    <option value="0.50">"0.5×"</option>
                                    <option value="0.75">"0.75×"</option>
                                    <option value="1.00">"1×"</option>
                                    <option value="1.25">"1.25×"</option>
                                    <option value="1.50">"1.5×"</option>
                                    <option value="2.00">"2×"</option>
                                </select>
                                <button
                                    class="icon-button icon-button--control"
                                    title="Subtitle settings"
                                    on:click=move |_| on_settings.run(())
                                >
                                    <Icon name="captions"/>
                                </button>
                                <button
                                    class="icon-button icon-button--control"
                                    title="Fullscreen"
                                    on:click=move |_| on_fullscreen.run(())
                                >
                                    <Icon name="fullscreen"/>
                                </button>
                            </div>
                        </div>
                    </footer>
                </Show>
            </div>
        </main>
    }
}

#[component]
fn TrackSelect(
    player: ReadSignal<PlayerSnapshot>,
    kind: TrackKind,
    label: &'static str,
    on_select: Callback<(TrackKind, i32)>,
) -> impl IntoView {
    let tracks = move || {
        player
            .get()
            .tracks
            .into_iter()
            .filter(|track| track.kind == kind)
            .filter(|track| kind == TrackKind::Subtitle || track.id >= 0)
            .map(|mut track| {
                if track.id < 0 {
                    track.label = "Embedded subtitles off".into();
                }
                track
            })
            .collect::<Vec<_>>()
    };
    view! {
        <Show when=move || !tracks().is_empty()>
            <select
                class="control-select control-select--track"
                aria-label=label
                prop:value=move || {
                    tracks()
                        .into_iter()
                        .find(|track| track.selected)
                        .or_else(|| tracks().into_iter().find(|track| track.id >= 0))
                        .map(|track| track.id.to_string())
                        .unwrap_or_default()
                }
                on:change=move |event| {
                    if let Ok(id) = event_target_value(&event).parse::<i32>() {
                        on_select.run((kind, id));
                    }
                }
            >
                {move || tracks().into_iter().map(|track| view! {
                    <option value=track.id.to_string()>{track.label}</option>
                }).collect_view()}
            </select>
        </Show>
    }
}

#[component]
fn ProcessingPill(processing: ReadSignal<ProcessingSnapshot>) -> impl IntoView {
    view! {
        <div class=move || {
            let state = processing.get().stage;
            let suffix = match state {
                ProcessingStage::Failed | ProcessingStage::Unavailable => " status-pill--error",
                ProcessingStage::Ready => " status-pill--ready",
                ProcessingStage::Idle | ProcessingStage::Paused => "",
                _ => " status-pill--working",
            };
            format!("status-pill{suffix}")
        }>
            <span></span>
            <strong>{move || processing_label(processing.get().stage)}</strong>
            <small>{move || {
                let state = processing.get();
                if state.total_windows == 0 {
                    String::new()
                } else {
                    format!("{}/{}", state.completed_windows, state.total_windows)
                }
            }}</small>
        </div>
    }
}

struct ActiveCue {
    primary: String,
    secondary: Option<String>,
}

fn find_active_cue(
    position_ms: u64,
    translated: &[SubtitleCue],
    source: &[myna_player_core::TranscriptSegment],
    preferred_track: &str,
) -> Option<ActiveCue> {
    let source_cue = || {
        source
            .iter()
            .find(|segment| position_ms >= segment.start_ms && position_ms < segment.end_ms)
    };
    if preferred_track == "source" {
        return source_cue().map(|segment| ActiveCue {
            primary: segment.text.clone(),
            secondary: None,
        });
    }
    if let Some(cue) = translated
        .iter()
        .find(|cue| position_ms >= cue.start_ms && position_ms < cue.end_ms)
        && let Some(text) = cue.translated_text.clone()
    {
        return Some(ActiveCue {
            primary: text,
            secondary: (preferred_track == "dual").then(|| cue.source_text.clone()),
        });
    }
    source_cue().map(|segment| ActiveCue {
        primary: segment.text.clone(),
        secondary: (preferred_track != "source")
            .then(|| "Source transcript · translation not ready".into()),
    })
}

fn control_class(base: &str, visible: bool, has_media: bool) -> String {
    if !has_media || visible {
        format!("{base} {base}--visible")
    } else {
        base.into()
    }
}

fn media_details(metadata: Option<MediaMetadata>) -> String {
    metadata
        .map(|media| {
            let dimensions = media
                .video_streams
                .first()
                .and_then(|stream| Some(format!("{}×{}", stream.width?, stream.height?)))
                .unwrap_or_else(|| "Video".into());
            format!("{dimensions} · {}", format_time(media.duration_ms))
        })
        .unwrap_or_else(|| "Local-first subtitle player".into())
}

fn player_duration(player: PlayerSnapshot, metadata: Option<MediaMetadata>) -> u64 {
    if player.duration_ms > 0 {
        player.duration_ms
    } else {
        metadata.map(|media| media.duration_ms).unwrap_or(0)
    }
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

fn percent(value: u64, total: u64) -> String {
    if total == 0 {
        return "0%".into();
    }
    format!("{:.3}%", value.min(total) as f64 / total as f64 * 100.0)
}

fn processing_label(stage: ProcessingStage) -> &'static str {
    match stage {
        ProcessingStage::Idle => "Idle",
        ProcessingStage::Queued => "Queued",
        ProcessingStage::Extracting => "Audio",
        ProcessingStage::Transcribing => "Transcribing",
        ProcessingStage::Translating => "Translating",
        ProcessingStage::Ready => "Ready",
        ProcessingStage::Paused => "Paused",
        ProcessingStage::Failed => "Needs attention",
        ProcessingStage::Unavailable => "Unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use myna_player_core::{CueStatus, TranscriptSegment};

    #[test]
    fn translated_cue_keeps_source_as_secondary_text() {
        let cue = SubtitleCue {
            id: "1".into(),
            start_ms: 0,
            end_ms: 1_000,
            source_text: "Hello".into(),
            translated_text: Some("Merhaba".into()),
            source_language: Some("en".into()),
            target_language: Some("TR".into()),
            status: CueStatus::Ready,
        };
        let active = find_active_cue(500, &[cue], &[], "dual").unwrap();
        assert_eq!(active.primary, "Merhaba");
        assert_eq!(active.secondary.as_deref(), Some("Hello"));
    }

    #[test]
    fn source_is_used_only_when_no_translation_exists() {
        let source = TranscriptSegment {
            id: "1".into(),
            start_ms: 0,
            end_ms: 1_000,
            text: "Hello".into(),
            detected_language: Some("en".into()),
            language_confidence: None,
            is_final: true,
        };
        let active = find_active_cue(500, &[], &[source], "translated").unwrap();
        assert_eq!(active.primary, "Hello");
        assert_eq!(
            active.secondary.as_deref(),
            Some("Source transcript · translation not ready")
        );
    }
}
