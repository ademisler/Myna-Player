use std::{
    collections::VecDeque,
    env,
    io::{BufRead, BufReader, Read},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use myna_player_core::{TranscriptSegment, TranscriptionRequest, TranscriptionResult};
use reqwest::blocking::{
    Client,
    multipart::{Form, Part},
};
use serde::Deserialize;

use crate::{
    PipelineError, default_whisper_model_path,
    subtitle_segmentation::{TimedWord, subtitle_segments_from_words},
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_WORKER_LOG_LINES: usize = 200;

struct ServerRuntime {
    model_path: PathBuf,
    vad_model_path: Option<PathBuf>,
    endpoint: String,
    process: Child,
}

impl Drop for ServerRuntime {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

#[derive(Debug, Clone)]
pub struct WhisperDiagnostics {
    pub logging_enabled: bool,
    pub worker_running: bool,
    pub model_path: Option<PathBuf>,
    pub recent_logs: Vec<String>,
}

#[derive(Default)]
struct WorkerLogBuffer {
    enabled: AtomicBool,
    lines: Mutex<VecDeque<String>>,
}

impl WorkerLogBuffer {
    fn push(&self, source: &str, line: String) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let line = redact_worker_log(&line);
        if line.trim().is_empty() {
            return;
        }
        if let Ok(mut lines) = self.lines.lock() {
            if lines.len() >= MAX_WORKER_LOG_LINES {
                lines.pop_front();
            }
            lines.push_back(format!("[{source}] {}", truncate_log_line(&line)));
        }
    }

    fn snapshot(&self) -> Vec<String> {
        self.lines
            .lock()
            .map(|lines| lines.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn clear(&self) {
        if let Ok(mut lines) = self.lines.lock() {
            lines.clear();
        }
    }
}

pub struct PersistentWhisper {
    server: Mutex<Option<ServerRuntime>>,
    logs: Arc<WorkerLogBuffer>,
}

impl Default for PersistentWhisper {
    fn default() -> Self {
        Self {
            server: Mutex::new(None),
            logs: Arc::new(WorkerLogBuffer::default()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ServerResponse {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    detected_language: Option<String>,
    #[serde(default)]
    detected_language_probability: Option<f32>,
    #[serde(default)]
    segments: Vec<ServerSegment>,
}

#[derive(Debug, Deserialize)]
struct ServerSegment {
    id: usize,
    start: f64,
    end: f64,
    text: String,
    #[serde(default)]
    words: Vec<ServerWord>,
}

#[derive(Debug, Deserialize)]
struct ServerWord {
    word: String,
    start: f64,
    end: f64,
    #[serde(default, rename = "probability")]
    _probability: Option<f32>,
}

impl PersistentWhisper {
    pub fn transcribe(
        &self,
        request: &TranscriptionRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<TranscriptionResult, PipelineError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(cancelled_error());
        }
        let audio_path = PathBuf::from(&request.audio_path);
        if !audio_path.is_file() {
            return Err(PipelineError::AsrUnavailable(format!(
                "audio file does not exist: {}",
                audio_path.display()
            )));
        }
        let model_path = resolve_model_path(request)?;
        let vad_model_path = resolve_vad_model_path(request)?;
        let endpoint = self.ensure_server(&model_path, vad_model_path.as_deref(), &cancelled)?;
        if cancelled.load(Ordering::Relaxed) {
            return Err(cancelled_error());
        }

        let audio = std::fs::read(&audio_path).map_err(|error| {
            PipelineError::AsrUnavailable(format!(
                "could not read extracted audio {}: {error}",
                audio_path.display()
            ))
        })?;
        let file_name = audio_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("window.wav")
            .to_owned();
        let language = normalize_language_hint(request.language_hint.as_deref()).to_owned();
        let mut form = Form::new().part(
            "file",
            Part::bytes(audio)
                .file_name(file_name)
                .mime_str("audio/wav")
                .map_err(|error| PipelineError::Provider(error.to_string()))?,
        );
        for (field, value) in inference_fields(&language) {
            form = form.text(field, value);
        }
        if let Some(prompt) = request
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
        {
            form = form.text("prompt", prompt.to_owned());
        }

        let started = Instant::now();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| PipelineError::Provider(error.to_string()))?;
        let response = client
            .post(endpoint)
            .multipart(form)
            .send()
            .map_err(|error| {
                if cancelled.load(Ordering::Relaxed) {
                    cancelled_error()
                } else {
                    PipelineError::Provider(format!("Whisper server request failed: {error}"))
                }
            })?;
        if cancelled.load(Ordering::Relaxed) {
            return Err(cancelled_error());
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(PipelineError::Provider(format!(
                "Whisper server returned {status}: {}",
                body.trim()
            )));
        }
        let parsed: ServerResponse = response.json().map_err(|error| {
            PipelineError::Provider(format!("invalid Whisper server response: {error}"))
        })?;
        let detected_language = parsed
            .detected_language
            .or(parsed.language)
            .or_else(|| (language != "auto").then_some(language));
        let has_word_timestamps = parsed
            .segments
            .iter()
            .any(|segment| !segment.words.is_empty());
        let segments = if has_word_timestamps {
            let words = parsed
                .segments
                .iter()
                .flat_map(|segment| segment.words.iter())
                .map(|word| {
                    let relative_start_ms = seconds_to_ms(word.start);
                    let relative_end_ms = seconds_to_ms(word.end).max(relative_start_ms);
                    TimedWord {
                        text: word.word.clone(),
                        start_ms: request.window_start_ms.saturating_add(relative_start_ms),
                        end_ms: request.window_start_ms.saturating_add(relative_end_ms),
                    }
                })
                .collect();
            subtitle_segments_from_words(
                words,
                &request.window_start_ms.to_string(),
                detected_language.clone(),
                parsed.detected_language_probability,
            )
        } else {
            parsed
                .segments
                .into_iter()
                .filter_map(|segment| {
                    let text = segment.text.trim().to_owned();
                    if text.is_empty() {
                        return None;
                    }
                    let relative_start_ms = seconds_to_ms(segment.start);
                    let relative_end_ms = seconds_to_ms(segment.end).max(relative_start_ms);
                    Some(TranscriptSegment {
                        id: format!(
                            "{}-{relative_start_ms}-{}",
                            request.window_start_ms, segment.id
                        ),
                        start_ms: request.window_start_ms.saturating_add(relative_start_ms),
                        end_ms: request.window_start_ms.saturating_add(relative_end_ms),
                        text,
                        detected_language: detected_language.clone(),
                        language_confidence: parsed.detected_language_probability,
                        is_final: true,
                    })
                })
                .collect()
        };

        Ok(TranscriptionResult {
            detected_language,
            segments,
            model_path: model_path.to_string_lossy().into_owned(),
            elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        })
    }

    pub fn cancel_current(&self) {
        if let Ok(mut server) = self.server.lock() {
            server.take();
        }
    }

    pub fn loaded_model_path(&self) -> Option<PathBuf> {
        self.server.lock().ok().and_then(|mut server| {
            server.as_mut().and_then(|runtime| {
                runtime
                    .process
                    .try_wait()
                    .ok()
                    .flatten()
                    .is_none()
                    .then(|| runtime.model_path.clone())
            })
        })
    }

    pub fn set_diagnostic_logging(&self, enabled: bool) {
        self.logs.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            self.logs.clear();
        }
    }

    pub fn diagnostics(&self) -> WhisperDiagnostics {
        let (worker_running, model_path) = self
            .server
            .lock()
            .map(|mut server| {
                server.as_mut().map_or((false, None), |runtime| {
                    let running = runtime.process.try_wait().ok().flatten().is_none();
                    (running, running.then(|| runtime.model_path.clone()))
                })
            })
            .unwrap_or((false, None));
        WhisperDiagnostics {
            logging_enabled: self.logs.enabled.load(Ordering::Relaxed),
            worker_running,
            model_path,
            recent_logs: self.logs.snapshot(),
        }
    }

    fn ensure_server(
        &self,
        model_path: &Path,
        vad_model_path: Option<&Path>,
        cancelled: &AtomicBool,
    ) -> Result<String, PipelineError> {
        let mut server = self.server.lock().map_err(|_| {
            PipelineError::Provider("Whisper server state lock was poisoned".into())
        })?;
        let can_reuse = server.as_mut().is_some_and(|runtime| {
            runtime.model_path == model_path
                && runtime.vad_model_path.as_deref() == vad_model_path
                && runtime.process.try_wait().ok().flatten().is_none()
        });
        if can_reuse {
            return Ok(server
                .as_ref()
                .expect("server checked above")
                .endpoint
                .clone());
        }
        server.take();

        let binary = find_whisper_server().ok_or_else(|| {
            PipelineError::AsrUnavailable(
                "whisper-server is unavailable; install whisper.cpp or package the Myna Player runtime"
                    .into(),
            )
        })?;
        let port = reserve_loopback_port()?;
        let endpoint = format!("http://127.0.0.1:{port}/inference");
        let health_endpoint = format!("http://127.0.0.1:{port}/health");
        let mut command = Command::new(&binary);
        command
            .arg("--model")
            .arg(model_path)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--language")
            .arg("auto")
            .arg("--suppress-nst")
            .arg("--no-language-probabilities");
        if let Some(vad_model_path) = vad_model_path {
            command
                .arg("--vad")
                .arg("--vad-model")
                .arg(vad_model_path)
                .arg("--vad-threshold")
                .arg("0.50")
                .arg("--vad-min-speech-duration-ms")
                .arg("250")
                .arg("--vad-min-silence-duration-ms")
                .arg("350")
                .arg("--vad-speech-pad-ms")
                .arg("120");
        }
        let mut process = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                PipelineError::AsrUnavailable(format!(
                    "could not start {}: {error}",
                    binary.display()
                ))
            })?;
        if let Some(stdout) = process.stdout.take() {
            spawn_worker_log_reader(stdout, "stdout", Arc::clone(&self.logs));
        }
        if let Some(stderr) = process.stderr.take() {
            spawn_worker_log_reader(stderr, "stderr", Arc::clone(&self.logs));
        }
        let probe = Client::builder()
            .connect_timeout(Duration::from_millis(250))
            .timeout(Duration::from_millis(500))
            .build()
            .map_err(|error| PipelineError::Provider(error.to_string()))?;
        let started = Instant::now();
        loop {
            if cancelled.load(Ordering::Relaxed) {
                let _ = process.kill();
                let _ = process.wait();
                return Err(cancelled_error());
            }
            if let Ok(Some(status)) = process.try_wait() {
                return Err(PipelineError::AsrUnavailable(format!(
                    "Whisper server exited during startup with {status}"
                )));
            }
            if probe
                .get(&health_endpoint)
                .send()
                .is_ok_and(|response| response.status().is_success())
            {
                break;
            }
            if started.elapsed() >= STARTUP_TIMEOUT {
                let _ = process.kill();
                let _ = process.wait();
                return Err(PipelineError::AsrUnavailable(
                    "Whisper model did not become ready within 30 seconds".into(),
                ));
            }
            thread::sleep(Duration::from_millis(75));
        }

        *server = Some(ServerRuntime {
            model_path: model_path.to_owned(),
            vad_model_path: vad_model_path.map(Path::to_owned),
            endpoint: endpoint.clone(),
            process,
        });
        Ok(endpoint)
    }
}

fn spawn_worker_log_reader<R>(reader: R, source: &'static str, logs: Arc<WorkerLogBuffer>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            logs.push(source, line);
        }
    });
}

fn truncate_log_line(value: &str) -> String {
    const MAX_CHARS: usize = 1_000;
    if value.chars().count() <= MAX_CHARS {
        value.to_owned()
    } else {
        value.chars().take(MAX_CHARS).collect::<String>() + "..."
    }
}

fn redact_worker_log(value: &str) -> String {
    let mut redacted = value.to_owned();
    if let Some(home) = env::var_os("HOME").and_then(|value| value.into_string().ok())
        && !home.is_empty()
    {
        redacted = redacted.replace(&home, "~");
    }
    redacted
}

fn inference_fields(language: &str) -> Vec<(&'static str, String)> {
    // whisper-server 1.9.x treats `detect_language=true` as a detection-only request,
    // returning an empty transcript. `language=auto` already enables detection while
    // preserving transcription, so that legacy form field must not be sent.
    vec![
        ("response_format", "verbose_json".into()),
        ("language", language.into()),
        ("translate", "false".into()),
        ("token_timestamps", "true".into()),
        ("suppress_nst", "true".into()),
    ]
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

fn resolve_vad_model_path(
    request: &TranscriptionRequest,
) -> Result<Option<PathBuf>, PipelineError> {
    if !request.vad_enabled {
        return Ok(None);
    }
    request
        .vad_model_path
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(default_vad_model_path)
        .map(Some)
        .ok_or_else(|| {
            PipelineError::AsrUnavailable(
                "VAD is enabled but the Silero VAD model is not installed".into(),
            )
        })
}

pub fn default_vad_model_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("MYNA_PLAYER_VAD_MODEL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let file_name = "ggml-silero-v6.2.0.bin";
    let mut candidates = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(
            home.join("Library")
                .join("Application Support")
                .join("com.mynaplayer.desktop")
                .join("models")
                .join(file_name),
        );
        candidates.push(
            home.join(".local")
                .join("share")
                .join("myna-player")
                .join("models")
                .join(file_name),
        );
    }
    if let Some(app_data) = env::var_os("APPDATA") {
        candidates.push(
            PathBuf::from(app_data)
                .join("com.mynaplayer.desktop")
                .join("models")
                .join(file_name),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn reserve_loopback_port() -> Result<u16, PipelineError> {
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| {
            PipelineError::AsrUnavailable(format!(
                "could not allocate a local Whisper worker port: {error}"
            ))
        })
}

fn find_whisper_server() -> Option<PathBuf> {
    env::var_os("MYNA_PLAYER_WHISPER_SERVER")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            env::current_exe().ok().and_then(|executable| {
                let parent = executable.parent()?;
                let direct = parent.join(if cfg!(windows) {
                    "whisper-server.exe"
                } else {
                    "whisper-server"
                });
                if direct.is_file() {
                    return Some(direct);
                }
                std::fs::read_dir(parent)
                    .ok()?
                    .flatten()
                    .map(|entry| entry.path())
                    .find(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("whisper-server"))
                            && path.is_file()
                    })
            })
        })
        .or_else(|| find_in_path("whisper-server"))
        .or_else(|| {
            [
                "/opt/homebrew/bin/whisper-server",
                "/usr/local/bin/whisper-server",
            ]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
        })
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn normalize_language_hint(language: Option<&str>) -> &str {
    match language.map(str::trim).filter(|value| !value.is_empty()) {
        Some("auto") | None => "auto",
        Some(value) => value,
    }
}

fn seconds_to_ms(seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        0
    } else {
        (seconds * 1_000.0).round().min(u64::MAX as f64) as u64
    }
}

fn cancelled_error() -> PipelineError {
    PipelineError::Provider("transcription cancelled".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_verbose_json_with_timestamps() {
        let parsed: ServerResponse = serde_json::from_str(
            r#"{
              "language": "English",
              "detected_language": "English",
              "detected_language_probability": 0.97,
              "segments": [{
                "id": 3,
                "start": 1.25,
                "end": 2.75,
                "text": " Hello world. ",
                "words": [
                  {"word": " Hello", "start": 1.25, "end": 1.75, "probability": 0.9},
                  {"word": " world", "start": 1.80, "end": 2.50, "probability": 0.9},
                  {"word": ".", "start": 2.50, "end": 2.55, "probability": 0.8}
                ]
              }]
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.segments[0].id, 3);
        assert_eq!(seconds_to_ms(parsed.segments[0].start), 1_250);
        assert_eq!(parsed.detected_language_probability, Some(0.97));
        assert_eq!(parsed.segments[0].words.len(), 3);
        assert_eq!(parsed.segments[0].words[0].word.trim(), "Hello");
    }

    #[test]
    fn language_hint_defaults_to_auto() {
        assert_eq!(normalize_language_hint(None), "auto");
        assert_eq!(normalize_language_hint(Some("tr")), "tr");
    }

    #[test]
    fn missing_vad_model_is_an_explicit_error() {
        let request = TranscriptionRequest {
            audio_path: String::new(),
            window_start_ms: 0,
            model_path: None,
            vad_enabled: true,
            vad_model_path: Some("/definitely/missing/vad.bin".into()),
            language_hint: None,
            prompt: None,
        };
        let result = resolve_vad_model_path(&request);
        if default_vad_model_path().is_none() {
            assert!(result.is_err());
        }
    }

    #[test]
    fn automatic_language_detection_does_not_enable_detection_only_mode() {
        let fields = inference_fields("auto");
        assert!(
            fields
                .iter()
                .any(|(key, value)| *key == "language" && value == "auto")
        );
        assert!(!fields.iter().any(|(key, _)| *key == "detect_language"));
    }

    #[test]
    fn diagnostic_log_buffer_is_bounded_and_redacts_home() {
        let logs = WorkerLogBuffer::default();
        logs.enabled.store(true, Ordering::Relaxed);
        for index in 0..(MAX_WORKER_LOG_LINES + 10) {
            logs.push("stderr", format!("line {index}"));
        }
        let snapshot = logs.snapshot();
        assert_eq!(snapshot.len(), MAX_WORKER_LOG_LINES);
        assert!(
            snapshot
                .first()
                .is_some_and(|line| line.contains("line 10"))
        );
    }

    #[test]
    #[ignore = "requires a local whisper-server binary, model, and test WAV"]
    fn transcribes_with_long_lived_local_server() {
        let audio_path = env::var_os("MYNA_PLAYER_WHISPER_TEST_AUDIO")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(
                    "/opt/homebrew/opt/whisper-cpp/share/whisper-cpp/for-tests-ggml-tiny.bin",
                )
                .with_file_name("jfk.wav")
            });
        let model_path = default_whisper_model_path().expect("Whisper model is required");
        let request = TranscriptionRequest {
            audio_path: audio_path.to_string_lossy().into_owned(),
            window_start_ms: 30_000,
            model_path: Some(model_path.to_string_lossy().into_owned()),
            vad_enabled: false,
            vad_model_path: None,
            language_hint: Some("en".into()),
            prompt: None,
        };
        let worker = PersistentWhisper::default();
        let result = worker
            .transcribe(&request, Arc::new(AtomicBool::new(false)))
            .unwrap();
        assert!(!result.segments.is_empty());
        assert_eq!(worker.loaded_model_path(), Some(model_path));
        assert!(result.segments[0].start_ms >= 30_000);
    }
}
