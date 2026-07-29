use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use myna_player_core::{TranscriptSegment, TranscriptionRequest, TranscriptionResult};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
    convert_integer_to_float_audio, get_lang_str,
};

use crate::{PipelineError, default_whisper_model_path};

struct LoadedModel {
    path: PathBuf,
    context: WhisperContext,
}

#[derive(Default)]
pub struct NativeWhisper {
    model: Mutex<Option<LoadedModel>>,
}

impl NativeWhisper {
    pub fn transcribe(
        &self,
        request: &TranscriptionRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<TranscriptionResult, PipelineError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(PipelineError::Provider("transcription cancelled".into()));
        }
        let model_path = resolve_model_path(request)?;
        let audio = read_wav(&request.audio_path)?;
        let mut loaded = self
            .model
            .lock()
            .map_err(|_| PipelineError::Provider("Whisper model lock was poisoned".into()))?;
        let should_reload = loaded
            .as_ref()
            .is_none_or(|current| current.path != model_path);
        if should_reload {
            let parameters = WhisperContextParameters::default();
            let context =
                WhisperContext::new_with_params(&model_path, parameters).map_err(|error| {
                    PipelineError::AsrUnavailable(format!(
                        "could not load Whisper model {}: {error}",
                        model_path.display()
                    ))
                })?;
            *loaded = Some(LoadedModel {
                path: model_path.clone(),
                context,
            });
        }
        let loaded = loaded
            .as_ref()
            .ok_or_else(|| PipelineError::AsrUnavailable("Whisper model is unavailable".into()))?;
        let mut state = loaded.context.create_state().map_err(|error| {
            PipelineError::AsrUnavailable(format!("could not create Whisper state: {error}"))
        })?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        let language = request
            .language_hint
            .as_deref()
            .map(str::trim)
            .filter(|language| !language.is_empty() && *language != "auto");
        params.set_language(language);
        params.set_detect_language(language.is_none());
        if let Some(prompt) = request
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
        {
            params.set_initial_prompt(prompt);
        }
        let cancel_for_callback = Arc::clone(&cancelled);
        let abort_callback: Box<dyn FnMut() -> bool> =
            Box::new(move || cancel_for_callback.load(Ordering::Relaxed));
        params
            .set_abort_callback_safe::<Option<Box<dyn FnMut() -> bool>>, Box<dyn FnMut() -> bool>>(
                Some(abort_callback),
            );

        let started = Instant::now();
        state.full(params, &audio).map_err(|error| {
            PipelineError::Provider(format!("Whisper inference failed: {error}"))
        })?;
        if cancelled.load(Ordering::Relaxed) {
            return Err(PipelineError::Provider("transcription cancelled".into()));
        }

        let detected_language = get_lang_str(state.full_lang_id_from_state())
            .map(str::to_owned)
            .or_else(|| language.map(str::to_owned));
        let segments = state
            .as_iter()
            .enumerate()
            .filter_map(|(index, segment)| {
                let text = segment.to_str_lossy().ok()?.trim().to_owned();
                if text.is_empty() {
                    return None;
                }
                let relative_start_ms = segment.start_timestamp().max(0) as u64 * 10;
                let relative_end_ms = segment
                    .end_timestamp()
                    .max(segment.start_timestamp())
                    .max(0) as u64
                    * 10;
                Some(TranscriptSegment {
                    id: format!("{}-{relative_start_ms}-{index}", request.window_start_ms),
                    start_ms: request.window_start_ms.saturating_add(relative_start_ms),
                    end_ms: request.window_start_ms.saturating_add(relative_end_ms),
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

    pub fn loaded_model_path(&self) -> Option<PathBuf> {
        self.model
            .lock()
            .ok()
            .and_then(|loaded| loaded.as_ref().map(|model| model.path.clone()))
    }
}

fn resolve_model_path(request: &TranscriptionRequest) -> Result<PathBuf, PipelineError> {
    request
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
        })
}

fn read_wav(path: impl AsRef<Path>) -> Result<Vec<f32>, PipelineError> {
    let path = path.as_ref();
    let mut reader = hound::WavReader::open(path).map_err(|error| {
        PipelineError::AsrUnavailable(format!("could not read {}: {error}", path.display()))
    })?;
    let specification = reader.spec();
    if specification.channels != 1
        || specification.sample_rate != 16_000
        || specification.bits_per_sample != 16
    {
        return Err(PipelineError::Provider(format!(
            "Whisper requires mono 16 kHz 16-bit PCM, got {} channel(s), {} Hz, {} bit",
            specification.channels, specification.sample_rate, specification.bits_per_sample
        )));
    }
    let integer = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| PipelineError::Provider(format!("invalid WAV samples: {error}")))?;
    let mut floating = vec![0.0; integer.len()];
    convert_integer_to_float_audio(&integer, &mut floating)
        .map_err(|error| PipelineError::Provider(format!("audio conversion failed: {error}")))?;
    Ok(floating)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn rejects_non_whisper_audio_format_before_loading_model() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("stereo.wav");
        let specification = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, specification).unwrap();
        writer.write_sample(0_i16).unwrap();
        writer.write_sample(0_i16).unwrap();
        writer.finalize().unwrap();

        let error = read_wav(&path).unwrap_err();
        assert!(error.to_string().contains("mono 16 kHz"));
    }
}
