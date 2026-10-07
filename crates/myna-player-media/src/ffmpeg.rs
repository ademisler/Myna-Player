use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use myna_player_core::{AudioWindowRequest, AudioWindowResult};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioExtractError {
    #[error("video path is empty")]
    EmptyPath,
    #[error("audio window duration must be greater than 0 and at most 120 seconds")]
    InvalidDuration,
    #[error("failed to prepare temporary directory: {0}")]
    TempDirectory(#[source] std::io::Error),
    #[error("failed to start ffmpeg: {0}")]
    Process(#[source] std::io::Error),
    #[error("ffmpeg failed: {0}")]
    Failed(String),
}

pub fn extract_audio_window(
    request: &AudioWindowRequest,
) -> Result<AudioWindowResult, AudioExtractError> {
    if request.path.trim().is_empty() {
        return Err(AudioExtractError::EmptyPath);
    }
    if !(1..=120_000).contains(&request.duration_ms) {
        return Err(AudioExtractError::InvalidDuration);
    }

    let cache_dir = std::env::temp_dir()
        .join("myna-player")
        .join("audio-windows");
    fs::create_dir_all(&cache_dir).map_err(AudioExtractError::TempDirectory)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let output_path: PathBuf = cache_dir.join(format!(
        "window-{}-{}-{}.wav",
        request.start_ms, request.duration_ms, nonce
    ));

    let mut command = Command::new(crate::ffmpeg_binary());
    command
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-ss", &format_seconds(request.start_ms)])
        .args(["-t", &format_seconds(request.duration_ms)])
        .arg("-i")
        .arg(&request.path);

    if let Some(relative_index) = request.audio_relative_index {
        command.args(["-map", &format!("0:a:{relative_index}")]);
    } else {
        command.args(["-map", "0:a:0"]);
    }

    let output = command
        .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(&output_path)
        .output()
        .map_err(AudioExtractError::Process)?;

    if !output.status.success() {
        return Err(AudioExtractError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }

    Ok(AudioWindowResult {
        output_path: output_path.to_string_lossy().into_owned(),
        start_ms: request.start_ms,
        end_ms: request.start_ms.saturating_add(request.duration_ms),
        sample_rate: 16_000,
        channels: 1,
    })
}

pub fn cleanup_audio_window(path: impl AsRef<Path>) -> std::io::Result<bool> {
    let path = path.as_ref();
    if !is_managed_audio_window(path) {
        return Ok(false);
    }

    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_managed_audio_window(path: &Path) -> bool {
    let cache_dir = std::env::temp_dir()
        .join("myna-player")
        .join("audio-windows");
    path.parent() == Some(cache_dir.as_path())
        && path.extension().and_then(|extension| extension.to_str()) == Some("wav")
}

fn format_seconds(milliseconds: u64) -> String {
    format!("{:.3}", milliseconds as f64 / 1_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_refuses_paths_outside_the_managed_audio_directory() {
        let unmanaged = std::env::temp_dir().join("unmanaged-audio.wav");

        assert!(!is_managed_audio_window(&unmanaged));
        assert!(!cleanup_audio_window(&unmanaged).unwrap());
    }
}
