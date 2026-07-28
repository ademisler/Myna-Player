use std::{env, fs, path::PathBuf, process::Command};

use subahead_core::{RuntimeDependency, RuntimeStatus};

pub fn runtime_status() -> RuntimeStatus {
    RuntimeStatus {
        ffmpeg: inspect_binary("ffmpeg", &["-version"]),
        ffprobe: inspect_binary("ffprobe", &["-version"]),
        whisper: ["whisper-cli", "whisper-cpp", "main"]
            .iter()
            .map(|name| inspect_binary(name, &["--version"]))
            .find(|dependency| dependency.available)
            .unwrap_or_else(|| RuntimeDependency {
                name: "whisper.cpp".into(),
                available: false,
                path: None,
                version: None,
            }),
        whisper_model: inspect_whisper_model(),
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
    if let Some(path) = env::var_os("SUBAHEAD_WHISPER_MODEL") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(
            home.join("Library")
                .join("Application Support")
                .join("com.subahead.desktop")
                .join("models")
                .join("ggml-base.bin"),
        );
        candidates.push(
            home.join(".local")
                .join("share")
                .join("subahead")
                .join("models")
                .join("ggml-base.bin"),
        );
    }
    if let Some(app_data) = env::var_os("APPDATA") {
        candidates.push(
            PathBuf::from(app_data)
                .join("com.subahead.desktop")
                .join("models")
                .join("ggml-base.bin"),
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

    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|directory| directory.join(name))
            .find(|path| path.is_file())
    })
}
