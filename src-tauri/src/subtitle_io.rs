use myna_player_core::{SubtitleCue, SubtitleExportFormat, SubtitleExportTrack, TranscriptSegment};

pub fn render_subtitles(
    format: SubtitleExportFormat,
    track: SubtitleExportTrack,
    source: &[TranscriptSegment],
    translated: &[SubtitleCue],
) -> Result<String, String> {
    let cues = export_cues(track, source, translated)?;
    match format {
        SubtitleExportFormat::Srt => Ok(render_srt(&cues)),
        SubtitleExportFormat::Vtt => Ok(render_vtt(&cues)),
    }
}

fn export_cues(
    track: SubtitleExportTrack,
    source: &[TranscriptSegment],
    translated: &[SubtitleCue],
) -> Result<Vec<ExportCue>, String> {
    if source.is_empty() {
        return Err("There is no transcript to export yet.".into());
    }
    let translated_by_id = translated
        .iter()
        .map(|cue| (cue.id.as_str(), cue))
        .collect::<std::collections::HashMap<_, _>>();
    let mut cues = Vec::with_capacity(source.len());
    for segment in source {
        let translation = translated_by_id
            .get(segment.id.as_str())
            .and_then(|cue| cue.translated_text.as_deref())
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let text = match track {
            SubtitleExportTrack::Source => segment.text.trim().to_owned(),
            SubtitleExportTrack::Translated => translation
                .ok_or_else(|| format!("Translation is missing for cue {}.", segment.id))?
                .to_owned(),
            SubtitleExportTrack::Dual => match translation {
                Some(translation) => format!("{}\n{}", segment.text.trim(), translation),
                None => segment.text.trim().to_owned(),
            },
        };
        if text.is_empty() || segment.end_ms <= segment.start_ms {
            continue;
        }
        cues.push(ExportCue {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            text,
        });
    }
    if cues.is_empty() {
        return Err("There are no valid subtitle cues to export.".into());
    }
    Ok(cues)
}

struct ExportCue {
    start_ms: u64,
    end_ms: u64,
    text: String,
}

fn render_srt(cues: &[ExportCue]) -> String {
    let mut output = String::new();
    for (index, cue) in cues.iter().enumerate() {
        output.push_str(&(index + 1).to_string());
        output.push('\n');
        output.push_str(&format_srt_time(cue.start_ms));
        output.push_str(" --> ");
        output.push_str(&format_srt_time(cue.end_ms));
        output.push('\n');
        output.push_str(&normalize_text(&cue.text));
        output.push_str("\n\n");
    }
    output
}

fn render_vtt(cues: &[ExportCue]) -> String {
    let mut output = String::from("WEBVTT\n\n");
    for cue in cues {
        output.push_str(&format_vtt_time(cue.start_ms));
        output.push_str(" --> ");
        output.push_str(&format_vtt_time(cue.end_ms));
        output.push('\n');
        output.push_str(&normalize_text(&cue.text));
        output.push_str("\n\n");
    }
    output
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned()
}

fn format_srt_time(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = milliseconds / 60_000 % 60;
    let seconds = milliseconds / 1_000 % 60;
    let millis = milliseconds % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn format_vtt_time(milliseconds: u64) -> String {
    format_srt_time(milliseconds).replace(',', ".")
}

#[cfg(test)]
mod tests {
    use myna_player_core::{CueStatus, SubtitleCue, TranscriptSegment};

    use super::*;

    fn source() -> Vec<TranscriptSegment> {
        vec![
            TranscriptSegment {
                id: "a".into(),
                start_ms: 220,
                end_ms: 3_800,
                text: "Hello".into(),
                detected_language: Some("en".into()),
                language_confidence: None,
                is_final: true,
            },
            TranscriptSegment {
                id: "b".into(),
                start_ms: 4_030,
                end_ms: 7_610,
                text: "How are you?".into(),
                detected_language: Some("en".into()),
                language_confidence: None,
                is_final: true,
            },
        ]
    }

    fn translated() -> Vec<SubtitleCue> {
        source()
            .into_iter()
            .map(|segment| SubtitleCue {
                id: segment.id,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                source_text: segment.text,
                translated_text: Some("Çeviri".into()),
                source_language: Some("en".into()),
                target_language: Some("TR".into()),
                status: CueStatus::Ready,
            })
            .collect()
    }

    #[test]
    fn srt_preserves_millisecond_timing() {
        let output = render_subtitles(
            SubtitleExportFormat::Srt,
            SubtitleExportTrack::Translated,
            &source(),
            &translated(),
        )
        .unwrap();
        assert!(output.contains("00:00:00,220 --> 00:00:03,800"));
        assert!(output.contains("00:00:04,030 --> 00:00:07,610"));
    }

    #[test]
    fn vtt_has_header_and_period_separator() {
        let output = render_subtitles(
            SubtitleExportFormat::Vtt,
            SubtitleExportTrack::Dual,
            &source(),
            &translated(),
        )
        .unwrap();
        assert!(output.starts_with("WEBVTT\n\n"));
        assert!(output.contains("00:00:00.220 --> 00:00:03.800"));
        assert!(output.contains("Hello\nÇeviri"));
    }

    #[test]
    fn translated_export_fails_when_a_cue_is_missing() {
        let error = render_subtitles(
            SubtitleExportFormat::Srt,
            SubtitleExportTrack::Translated,
            &source(),
            &translated()[..1],
        )
        .unwrap_err();
        assert!(error.contains("Translation is missing"));
    }
}
