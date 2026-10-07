use leptos::prelude::*;

#[component]
pub fn Icon(name: &'static str) -> impl IntoView {
    let path = icon_path(name);
    view! {
        <svg
            class="ui-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            focusable="false"
        >
            <path d=path></path>
        </svg>
    }
}

fn icon_path(name: &str) -> &'static str {
    match name {
        "open" => "M3.5 7.5h6l2-2h9v13h-17z M12 10v6 M9 13h6",
        "settings" => {
            "M12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z M19 13.5l1.5 1.1-1.8 3.1-1.8-.7a7 7 0 0 1-2.2 1.3l-.3 1.9h-3.6l-.3-1.9A7 7 0 0 1 8.3 17l-1.8.7-1.8-3.1L6.2 13a7 7 0 0 1 0-2.5L4.7 9.4l1.8-3.1 1.8.7a7 7 0 0 1 2.2-1.3l.3-1.9h3.6l.3 1.9A7 7 0 0 1 16.9 7l1.8-.7 1.8 3.1-1.5 1.1a7 7 0 0 1 0 3z"
        }
        "play" => "M8.5 5.5v13l10-6.5z",
        "pause" => "M9 5.5v13 M15 5.5v13",
        "volume" => {
            "M4 10v4h3l4 3.5v-11L7 10z M15 9.2a4 4 0 0 1 0 5.6 M17.8 6.5a7.5 7.5 0 0 1 0 11"
        }
        "mute" => "M4 10v4h3l4 3.5v-11L7 10z M16 9l5 6 M21 9l-5 6",
        "fullscreen" => "M8 4H4v4 M16 4h4v4 M8 20H4v-4 M20 16v4h-4",
        "captions" | "subtitles" => {
            "M4 6h16v12H4z M7.5 11.2a2 2 0 1 0 0 2.6 M14.5 11.2a2 2 0 1 0 0 2.6"
        }
        "close" => "M6 6l12 12 M18 6 6 18",
        "current-media" => "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16z M10 8.5l5.5 3.5-5.5 3.5z",
        "general" => "M4 10.5 12 4l8 6.5V20h-5v-6H9v6H4z",
        "playback" => "M7 4.5v15l12-7.5z",
        "transcription" => "M4 12h2 M8 8v8 M12 5v14 M16 8v8 M20 11v2",
        "translation" => {
            "M4 5h8 M8 3v2 M6 5c.5 3 2.2 5.3 5 7 M10.5 6c-.7 3.5-2.5 6-5.5 7 M14 19l3.3-9h1.4l3.3 9 M15.2 16h5.6"
        }
        "storage" => {
            "M5 5c0-1.1 3.1-2 7-2s7 .9 7 2-3.1 2-7 2-7-.9-7-2z M5 5v7c0 1.1 3.1 2 7 2s7-.9 7-2V5 M5 12v7c0 1.1 3.1 2 7 2s7-.9 7-2v-7"
        }
        "advanced" => "M5 12h.01 M12 12h.01 M19 12h.01",
        "chevron-down" => "M7 9.5l5 5 5-5",
        "download" => "M12 4v11 M8 11l4 4 4-4 M5 20h14",
        _ => "M12 12h.01",
    }
}
