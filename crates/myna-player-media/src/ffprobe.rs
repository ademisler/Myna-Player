use std::{fs, path::Path, process::Command};

use myna_player_core::{AudioStream, MediaMetadata, SubtitleStream, VideoStream};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("media file does not exist: {0}")]
    MissingFile(String),
    #[error("failed to start ffprobe: {0}")]
    Process(#[from] std::io::Error),
    #[error("ffprobe failed: {0}")]
    Failed(String),
    #[error("invalid ffprobe JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<RawStream>,
    format: Option<RawFormat>,
}

#[derive(Debug, Deserialize)]
struct RawStream {
    index: u32,
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u32>,
    sample_rate: Option<String>,
    avg_frame_rate: Option<String>,
    tags: Option<RawTags>,
}

#[derive(Debug, Deserialize)]
struct RawTags {
    language: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawFormat {
    duration: Option<String>,
    size: Option<String>,
    format_name: Option<String>,
}

pub fn probe_media(path: impl AsRef<Path>) -> Result<MediaMetadata, ProbeError> {
    let path = path.as_ref();
    if !path.is_file() {
        return Err(ProbeError::MissingFile(path.display().to_string()));
    }

    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;

    if !output.status.success() {
        return Err(ProbeError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }

    let probe: ProbeOutput = serde_json::from_slice(&output.stdout)?;
    let format = probe.format;
    let mut audio_relative_index = 0_u32;
    let mut video_streams = Vec::new();
    let mut audio_streams = Vec::new();
    let mut subtitle_streams = Vec::new();

    for stream in probe.streams {
        let tags = stream.tags.unwrap_or(RawTags {
            language: None,
            title: None,
        });

        match stream.codec_type.as_deref() {
            Some("video") => video_streams.push(VideoStream {
                index: stream.index,
                codec: stream.codec_name,
                width: stream.width,
                height: stream.height,
                frame_rate: stream.avg_frame_rate,
            }),
            Some("audio") => {
                audio_streams.push(AudioStream {
                    index: stream.index,
                    relative_index: audio_relative_index,
                    codec: stream.codec_name,
                    channels: stream.channels,
                    sample_rate: stream.sample_rate.and_then(|value| value.parse().ok()),
                    language: tags.language,
                    title: tags.title,
                });
                audio_relative_index += 1;
            }
            Some("subtitle") => subtitle_streams.push(SubtitleStream {
                index: stream.index,
                codec: stream.codec_name,
                language: tags.language,
                title: tags.title,
            }),
            _ => {}
        }
    }

    let duration_ms = format
        .as_ref()
        .and_then(|value| value.duration.as_deref())
        .and_then(|value| value.parse::<f64>().ok())
        .map(|seconds| (seconds * 1_000.0).round() as u64)
        .unwrap_or(0);
    let reported_size = format
        .as_ref()
        .and_then(|value| value.size.as_deref())
        .and_then(|value| value.parse::<u64>().ok());
    let size_bytes = reported_size.or_else(|| fs::metadata(path).ok().map(|value| value.len()));

    Ok(MediaMetadata {
        path: path.to_string_lossy().into_owned(),
        file_name: path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        duration_ms,
        size_bytes,
        format_name: format.and_then(|value| value.format_name),
        video_streams,
        audio_streams,
        subtitle_streams,
    })
}
