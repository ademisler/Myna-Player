use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use myna_player_core::{TranscriptSegment, TranscriptionRequest, TranscriptionResult};
use serde::Deserialize;

use crate::PipelineError;

#[derive(Debug, Deserialize)]
struct WhisperOutput {
    result: Option<WhisperResult>,
    #[serde(default)]
    transcription: Vec<WhisperSegment>,
}

#[derive(Debug, Deserialize)]
struct WhisperResult {
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WhisperSegment {
    offsets: WhisperOffsets,
    text: String,
}

#[derive(Debug, Deserialize)]
struct WhisperOffsets {
    from: i64,
    to: i64,
}

pub fn default_whisper_model_path() -> Option<PathBuf> {
    default_model_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file())
}

pub fn transcribe_with_whisper_cli(
    request: &TranscriptionRequest,
) -> Result<TranscriptionResult, PipelineError> {
    let audio_path = PathBuf::from(&request.audio_path);
    if !audio_path.is_file() {
        return Err(PipelineError::AsrUnavailable(format!(
            "audio file does not exist: {}",
            audio_path.display()
        )));
    }

    let model_path = request
        .model_path
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(default_whisper_model_path)
        .ok_or_else(|| {
            PipelineError::AsrUnavailable(
                "no Whisper model found; expected ggml-base.bin in the Myna Player model directory"
                    .into(),
            )
        })?;
    let whisper_binary = find_in_path("whisper-cli")
        .or_else(|| find_in_path("whisper-cpp"))
        .or_else(|| find_in_path("main"))
        .ok_or_else(|| {
            PipelineError::AsrUnavailable("whisper-cli is not available in PATH".into())
        })?;

    let output_dir = env::temp_dir().join("myna-player").join("transcriptions");
    fs::create_dir_all(&output_dir).map_err(|error| PipelineError::Provider(error.to_string()))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let output_base = output_dir.join(format!("window-{}-{nonce}", request.window_start_ms));

    let mut command = Command::new(&whisper_binary);
    command
        .arg("-m")
        .arg(&model_path)
        .arg("-f")
        .arg(&audio_path)
        .arg("-l")
        .arg(normalize_language_hint(request.language_hint.as_deref()))
        .args(["-oj", "-np", "-sow", "-of"])
        .arg(&output_base);

    if let Some(prompt) = request
        .prompt
        .as_deref()
        .filter(|prompt| !prompt.trim().is_empty())
    {
        command.arg("--prompt").arg(prompt);
    }

    let started = Instant::now();
    let output = command
        .output()
        .map_err(|error| PipelineError::AsrUnavailable(error.to_string()))?;
    if !output.status.success() {
        return Err(PipelineError::Provider(format!(
            "whisper-cli failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let json_path = output_base.with_extension("json");
    let raw = fs::read(&json_path).map_err(|error| {
        PipelineError::Provider(format!(
            "Whisper did not produce {}: {error}",
            json_path.display()
        ))
    })?;
    let parsed: WhisperOutput = serde_json::from_slice(&raw)
        .map_err(|error| PipelineError::Provider(format!("invalid Whisper JSON: {error}")))?;
    let _ = fs::remove_file(&json_path);

    let detected_language = parsed.result.and_then(|result| result.language);
    let segments = parsed
        .transcription
        .into_iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            let text = segment.text.trim().to_owned();
            if text.is_empty() {
                return None;
            }
            let relative_start = segment.offsets.from.max(0) as u64;
            let relative_end = segment.offsets.to.max(segment.offsets.from).max(0) as u64;
            let start_ms = request.window_start_ms.saturating_add(relative_start);
            let end_ms = request.window_start_ms.saturating_add(relative_end);
            Some(TranscriptSegment {
                id: format!("{}-{relative_start}-{index}", request.window_start_ms),
                start_ms,
                end_ms,
                text,
                detected_language: detected_language.clone(),
                language_confidence: None,
                is_final: true,
            })
        })
        .collect();

    Ok(TranscriptionResult {
        detected_language,
        segments,
        model_path: model_path.to_string_lossy().into_owned(),
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    })
}

fn normalize_language_hint(language: Option<&str>) -> &str {
    match language.map(str::trim).filter(|value| !value.is_empty()) {
        Some("auto") | None => "auto",
        Some(value) => value,
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

fn find_in_path(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.is_absolute() && direct.is_file() {
        return Some(direct.to_path_buf());
    }
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_offsets_whisper_segments() {
        let raw = br#"{
          "result": {"language": "en"},
          "transcription": [{
            "offsets": {"from": 120, "to": 1450},
            "text": " Hello there. "
          }]
        }"#;
        let parsed: WhisperOutput = serde_json::from_slice(raw).unwrap();
        let language = parsed.result.unwrap().language;
        let segment = &parsed.transcription[0];
        assert_eq!(language.as_deref(), Some("en"));
        assert_eq!(segment.offsets.from, 120);
        assert_eq!(segment.text.trim(), "Hello there.");
    }
}
