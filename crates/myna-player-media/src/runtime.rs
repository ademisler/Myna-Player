use std::{env, fs, path::PathBuf, process::Command};

use myna_player_core::{RuntimeDependency, RuntimeStatus};

pub fn runtime_status() -> RuntimeStatus {
    RuntimeStatus {
        ffmpeg: inspect_binary("ffmpeg", &["-version"]),
        ffprobe: inspect_binary("ffprobe", &["-version"]),
        whisper: ["whisper-server", "whisper-cli", "whisper-cpp", "main"]
            .iter()
            .map(|name| inspect_binary(name, &["--help"]))
            .find(|dependency| dependency.available)
            .unwrap_or_else(|| RuntimeDependency {
                name: "whisper.cpp".into(),
                available: false,
                path: None,
                version: None,
            }),
        whisper_model: inspect_whisper_model(),
        vad_model: inspect_vad_model(),
    }
}

fn inspect_vad_model() -> RuntimeDependency {
    let model_path = default_vad_model_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file());
    let version = model_path.as_ref().and_then(|path| {
        fs::metadata(path)
            .ok()
            .map(|metadata| format!("{:.1} MB", metadata.len() as f64 / 1_048_576.0))
    });
    RuntimeDependency {
        name: "Silero VAD model".into(),
        available: model_path.is_some(),
        path: model_path.map(|path| path.to_string_lossy().into_owned()),
        version,
    }
}

fn inspect_whisper_model() -> RuntimeDependency {
    let model_path = default_model_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file());
    let version = model_path.as_ref().and_then(|path| {
        fs::metadata(path)
            .ok()
            .map(|metadata| format!("{:.0} MB", metadata.len() as f64 / 1_048_576.0))
    });

    RuntimeDependency {
        name: "Whisper base model".into(),
        available: model_path.is_some(),
        path: model_path.map(|path| path.to_string_lossy().into_owned()),
        version,
    }
}

fn default_model_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("MYNA_PLAYER_WHISPER_MODEL") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(
            home.join("Library")
                .join("Application Support")
                .join("com.mynaplayer.desktop")
                .join("models")
                .join("ggml-base.bin"),
        );
        candidates.push(
            home.join(".local")
                .join("share")
                .join("myna-player")
                .join("models")
                .join("ggml-base.bin"),
        );
    }
    if let Some(app_data) = env::var_os("APPDATA") {
        candidates.push(
            PathBuf::from(app_data)
                .join("com.mynaplayer.desktop")
                .join("models")
                .join("ggml-base.bin"),
        );
    }
    candidates
}

fn default_vad_model_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("MYNA_PLAYER_VAD_MODEL") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(
            home.join("Library")
                .join("Application Support")
                .join("com.mynaplayer.desktop")
                .join("models")
                .join("ggml-silero-v6.2.0.bin"),
        );
        candidates.push(
            home.join(".local")
                .join("share")
                .join("myna-player")
                .join("models")
                .join("ggml-silero-v6.2.0.bin"),
        );
    }
    if let Some(app_data) = env::var_os("APPDATA") {
        candidates.push(
            PathBuf::from(app_data)
                .join("com.mynaplayer.desktop")
                .join("models")
                .join("ggml-silero-v6.2.0.bin"),
        );
    }
    candidates
}

fn inspect_binary(name: &str, version_args: &[&str]) -> RuntimeDependency {
    let path = find_in_path(name);
    let version = path.as_ref().and_then(|binary| {
        Command::new(binary)
            .args(version_args)
            .output()
            .ok()
            .and_then(|output| {
                let text = if output.stdout.is_empty() {
                    String::from_utf8_lossy(&output.stderr).into_owned()
                } else {
                    String::from_utf8_lossy(&output.stdout).into_owned()
                };
                text.lines()
                    .find(|line| !line.trim().is_empty())
                    .map(str::trim)
                    .map(str::to_owned)
            })
    });

    RuntimeDependency {
        name: name.to_owned(),
        available: path.is_some(),
        path: path.map(|value| value.to_string_lossy().into_owned()),
        version,
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(name);
    if candidate.is_absolute() && candidate.is_file() {
        return Some(candidate);
    }

    env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(|parent| parent.join(name)))
        .filter(|path| path.is_file())
        .or_else(|| {
            env::var_os("PATH").and_then(|paths| {
                env::split_paths(&paths)
                    .map(|directory| directory.join(name))
                    .find(|path| path.is_file())
            })
        })
}
