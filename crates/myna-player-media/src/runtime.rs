use std::{env, fs, path::PathBuf, process::Command};

use myna_player_core::{RuntimeDependency, RuntimeStatus};

pub fn runtime_status() -> RuntimeStatus {
    RuntimeStatus {
        ffmpeg: inspect_named_binary("ffmpeg", "MYNA_PLAYER_FFMPEG", &["-version"]),
        ffprobe: inspect_named_binary("ffprobe", "MYNA_PLAYER_FFPROBE", &["-version"]),
        whisper: ["whisper-server", "whisper-cli", "whisper-cpp", "main"]
            .iter()
            .map(|name| inspect_named_binary(name, "MYNA_PLAYER_WHISPER_SERVER", &["--help"]))
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

pub fn ffmpeg_binary() -> PathBuf {
    resolve_binary("ffmpeg", "MYNA_PLAYER_FFMPEG").unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

pub fn ffprobe_binary() -> PathBuf {
    resolve_binary("ffprobe", "MYNA_PLAYER_FFPROBE").unwrap_or_else(|| PathBuf::from("ffprobe"))
}

pub fn resolve_binary(name: &str, override_variable: &str) -> Option<PathBuf> {
    if let Some(path) = env::var_os(override_variable).map(PathBuf::from)
        && path.is_file()
    {
        return Some(path);
    }
    binary_candidates(name)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

fn binary_candidates(name: &str) -> Vec<PathBuf> {
    let executable_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let mut candidates = Vec::new();
    if let Ok(executable) = env::current_exe()
        && let Some(directory) = executable.parent()
    {
        candidates.push(directory.join(&executable_name));
        candidates.push(directory.join("resources").join(&executable_name));
        candidates.push(directory.join("../Resources").join(&executable_name));
    }
    if let Some(paths) = env::var_os("PATH") {
        candidates
            .extend(env::split_paths(&paths).map(|directory| directory.join(&executable_name)));
    }
    candidates
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
    model_candidates("MYNA_PLAYER_WHISPER_MODEL", "ggml-base.bin")
}

fn default_vad_model_candidates() -> Vec<PathBuf> {
    model_candidates("MYNA_PLAYER_VAD_MODEL", "ggml-silero-v6.2.0.bin")
}

fn model_candidates(variable: &str, file_name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os(variable) {
        candidates.push(PathBuf::from(path));
    }
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(
            home.join("Library/Application Support/com.mynaplayer.desktop/models")
                .join(file_name),
        );
        candidates.push(home.join(".local/share/myna-player/models").join(file_name));
    }
    if let Some(app_data) = env::var_os("APPDATA") {
        candidates.push(
            PathBuf::from(app_data)
                .join("com.mynaplayer.desktop/models")
                .join(file_name),
        );
    }
    candidates
}

fn inspect_named_binary(name: &str, variable: &str, version_args: &[&str]) -> RuntimeDependency {
    let path = resolve_binary(name, variable);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_binary_override_has_priority() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        });
        std::fs::write(&binary, b"test").unwrap();
        unsafe { env::set_var("MYNA_PLAYER_FFMPEG", &binary) };
        assert_eq!(resolve_binary("ffmpeg", "MYNA_PLAYER_FFMPEG"), Some(binary));
        unsafe { env::remove_var("MYNA_PLAYER_FFMPEG") };
    }
}
