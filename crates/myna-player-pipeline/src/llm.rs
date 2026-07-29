use std::collections::{HashMap, HashSet};

use myna_player_core::{CueStatus, SubtitleCue, TranscriptSegment};
use serde::{Deserialize, Serialize};

use crate::PipelineError;

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
const MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct LlmTranslationRequest {
    pub provider_id: String,
    pub model: String,
    pub api_key: String,
    pub source_language: Option<String>,
    pub target_language: String,
    pub segments: Vec<TranscriptSegment>,
    pub previous_context: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ChatResponseFormat>,
}

#[derive(Debug, Serialize)]
struct ChatResponseFormat {
    r#type: &'static str,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    system_instruction: GeminiContent,
    contents: Vec<GeminiContent>,
    generation_config: GeminiGenerationConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiContent {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    temperature: f32,
    response_mime_type: &'static str,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiResponseContent,
}

#[derive(Debug, Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
struct TranslationEnvelope {
    translations: Vec<TranslationItem>,
}

#[derive(Debug, Deserialize)]
struct TranslationItem {
    id: String,
    text: String,
}

pub async fn translate_with_llm(
    request: &LlmTranslationRequest,
) -> Result<Vec<SubtitleCue>, PipelineError> {
    validate_request(request)?;
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| PipelineError::Provider(error.to_string()))?;
    let system = system_prompt(&request.target_language);
    let user = user_prompt(request)?;
    let content = match request.provider_id.as_str() {
        "openai" => {
            request_openai_compatible(
                &client,
                "https://api.openai.com/v1/chat/completions",
                request,
                system,
                user,
                false,
                false,
            )
            .await?
        }
        "openrouter" => {
            request_openai_compatible(
                &client,
                "https://openrouter.ai/api/v1/chat/completions",
                request,
                system,
                user,
                true,
                false,
            )
            .await?
        }
        "minimax" => {
            request_openai_compatible(
                &client,
                "https://api.minimax.io/v1/chat/completions",
                request,
                system,
                user,
                false,
                true,
            )
            .await?
        }
        "gemini" => request_gemini(&client, request, system, user).await?,
        provider => {
            return Err(PipelineError::TranslationUnavailable(format!(
                "unsupported LLM translation provider: {provider}"
            )));
        }
    };
    map_llm_translations(request, &content)
}

fn validate_request(request: &LlmTranslationRequest) -> Result<(), PipelineError> {
    if request.api_key.trim().is_empty() {
        return Err(PipelineError::TranslationUnavailable(format!(
            "{} API key is empty",
            request.provider_id
        )));
    }
    if request.model.trim().is_empty() {
        return Err(PipelineError::TranslationUnavailable(format!(
            "{} model is empty",
            request.provider_id
        )));
    }
    if request.segments.is_empty() {
        return Err(PipelineError::TranslationUnavailable(
            "there are no subtitle segments to translate".into(),
        ));
    }
    Ok(())
}

async fn request_openai_compatible(
    client: &reqwest::Client,
    endpoint: &str,
    request: &LlmTranslationRequest,
    system: String,
    user: String,
    openrouter_headers: bool,
    force_json_object: bool,
) -> Result<String, PipelineError> {
    let body = ChatRequest {
        model: request.model.trim().to_owned(),
        messages: vec![
            ChatMessage {
                role: "system",
                content: system,
            },
            ChatMessage {
                role: "user",
                content: user,
            },
        ],
        // MiniMax rejects zero; a low positive value is accepted across all compatible APIs.
        temperature: 0.1,
        response_format: force_json_object.then_some(ChatResponseFormat {
            r#type: "json_object",
        }),
    };
    let mut builder = client
        .post(endpoint)
        .bearer_auth(request.api_key.trim())
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    if openrouter_headers {
        builder = builder
            .header("HTTP-Referer", "https://github.com/ademisler/Myna-Player")
            .header("X-Title", "Myna Player");
    }
    let value: ChatResponse = send_json_with_retry(builder, &body, &request.provider_id).await?;
    value
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| PipelineError::Provider("provider returned an empty response".into()))
}

async fn request_gemini(
    client: &reqwest::Client,
    request: &LlmTranslationRequest,
    system: String,
    user: String,
) -> Result<String, PipelineError> {
    let model = request.model.trim().trim_start_matches("models/");
    let endpoint =
        format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent");
    let body = GeminiRequest {
        system_instruction: GeminiContent {
            role: None,
            parts: vec![GeminiPart { text: system }],
        },
        contents: vec![GeminiContent {
            role: Some("user"),
            parts: vec![GeminiPart { text: user }],
        }],
        generation_config: GeminiGenerationConfig {
            temperature: 0.1,
            response_mime_type: "application/json",
        },
    };
    let builder = client
        .post(endpoint)
        .header("x-goog-api-key", request.api_key.trim())
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    let value: GeminiResponse = send_json_with_retry(builder, &body, "gemini").await?;
    let content = value
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| PipelineError::Provider("Gemini returned no candidates".into()))?
        .content
        .parts
        .into_iter()
        .map(|part| part.text)
        .collect::<String>();
    if content.trim().is_empty() {
        return Err(PipelineError::Provider(
            "Gemini returned an empty response".into(),
        ));
    }
    Ok(content)
}

async fn send_json_with_retry<T, B>(
    builder: reqwest::RequestBuilder,
    body: &B,
    provider: &str,
) -> Result<T, PipelineError>
where
    T: for<'de> Deserialize<'de>,
    B: Serialize + ?Sized,
{
    for attempt in 0..MAX_ATTEMPTS {
        let request = builder
            .try_clone()
            .ok_or_else(|| PipelineError::Provider("could not clone provider request".into()))?;
        let response = request.json(body).send().await;
        let response = match response {
            Ok(response) => response,
            Err(error)
                if (error.is_timeout() || error.is_connect()) && attempt + 1 < MAX_ATTEMPTS =>
            {
                retry_delay(attempt).await;
                continue;
            }
            Err(error) => {
                return Err(PipelineError::Provider(format!(
                    "{provider} request failed: {error}"
                )));
            }
        };
        let status = response.status();
        let text = response.text().await.map_err(|error| {
            PipelineError::Provider(format!("{provider} response failed: {error}"))
        })?;
        if status.is_success() {
            return serde_json::from_str(&text).map_err(|error| {
                PipelineError::Provider(format!("invalid {provider} response JSON: {error}"))
            });
        }
        if is_retryable(status) && attempt + 1 < MAX_ATTEMPTS {
            retry_delay(attempt).await;
            continue;
        }
        return Err(PipelineError::Provider(format!(
            "{provider} returned {status}: {}",
            text.trim()
        )));
    }
    Err(PipelineError::Provider(format!(
        "{provider} retry budget was exhausted"
    )))
}

fn system_prompt(target_language: &str) -> String {
    format!(
        "You are a professional film subtitle translator. Translate into {target_language}. Preserve meaning, tone, names, register, jokes and continuity. Each input item is already a timed subtitle cue. Never merge, split, reorder, omit or invent cue IDs. Return only valid JSON in exactly this shape: {{\"translations\":[{{\"id\":\"cue-id\",\"text\":\"translated subtitle\"}}]}}. No markdown and no commentary."
    )
}

fn user_prompt(request: &LlmTranslationRequest) -> Result<String, PipelineError> {
    #[derive(Serialize)]
    struct PromptPayload<'a> {
        source_language: Option<&'a str>,
        target_language: &'a str,
        previous_context: &'a [String],
        cues: Vec<PromptCue<'a>>,
    }
    #[derive(Serialize)]
    struct PromptCue<'a> {
        id: &'a str,
        text: &'a str,
    }
    serde_json::to_string(&PromptPayload {
        source_language: request.source_language.as_deref(),
        target_language: &request.target_language,
        previous_context: &request.previous_context,
        cues: request
            .segments
            .iter()
            .map(|segment| PromptCue {
                id: &segment.id,
                text: &segment.text,
            })
            .collect(),
    })
    .map_err(|error| PipelineError::Provider(error.to_string()))
}

fn map_llm_translations(
    request: &LlmTranslationRequest,
    content: &str,
) -> Result<Vec<SubtitleCue>, PipelineError> {
    let json = extract_json_object(content).ok_or_else(|| {
        PipelineError::Provider("provider response did not contain a JSON object".into())
    })?;
    let envelope: TranslationEnvelope = serde_json::from_str(&json)
        .map_err(|error| PipelineError::Provider(format!("invalid translation JSON: {error}")))?;
    let expected = request
        .segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect::<HashSet<_>>();
    let mut values = HashMap::with_capacity(envelope.translations.len());
    for item in envelope.translations {
        if !expected.contains(item.id.as_str()) {
            return Err(PipelineError::Provider(format!(
                "provider returned unknown cue id '{}'",
                item.id
            )));
        }
        let text = item.text.trim().to_owned();
        if text.is_empty() {
            return Err(PipelineError::Provider(format!(
                "provider returned empty text for cue '{}'",
                item.id
            )));
        }
        if values.insert(item.id.clone(), text).is_some() {
            return Err(PipelineError::Provider(format!(
                "provider returned duplicate cue id '{}'",
                item.id
            )));
        }
    }
    if values.len() != request.segments.len() {
        let missing = request
            .segments
            .iter()
            .filter(|segment| !values.contains_key(&segment.id))
            .map(|segment| segment.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(PipelineError::Provider(format!(
            "provider omitted cue id(s): {missing}"
        )));
    }
    Ok(request
        .segments
        .iter()
        .map(|segment| SubtitleCue {
            id: segment.id.clone(),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            source_text: segment.text.clone(),
            translated_text: values.remove(&segment.id),
            source_language: segment.detected_language.clone(),
            target_language: Some(request.target_language.clone()),
            status: CueStatus::Ready,
        })
        .collect())
}

fn extract_json_object(content: &str) -> Option<String> {
    let without_think = content
        .rsplit_once("</think>")
        .map(|(_, tail)| tail)
        .unwrap_or(content);
    if let (Some(start), Some(end)) = (without_think.find('{'), without_think.rfind('}'))
        && end >= start
    {
        let candidate = without_think[start..=end].to_owned();
        if serde_json::from_str::<serde_json::Value>(&candidate).is_ok() {
            return Some(candidate);
        }
    }

    // Some MiniMax reasoning models begin the final JSON immediately before </think>
    // and continue it after the tag. Recover only the last explicit translation envelope,
    // strip reasoning boundary control characters, then rely on strict cue-ID validation.
    let start = content.rfind(r#"{"translations""#)?;
    let end = content.rfind('}')?;
    if end < start {
        return None;
    }
    let candidate = content[start..=end]
        .replace("<think>", "")
        .replace("</think>", "")
        .chars()
        .filter(|character| !matches!(character, '\n' | '\r' | '\t'))
        .collect::<String>();
    serde_json::from_str::<serde_json::Value>(&candidate)
        .is_ok()
        .then_some(candidate)
}

fn is_retryable(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

async fn retry_delay(attempt: usize) {
    tokio::time::sleep(std::time::Duration::from_millis(
        300_u64.saturating_mul(1_u64 << attempt.min(3)),
    ))
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> LlmTranslationRequest {
        LlmTranslationRequest {
            provider_id: "openai".into(),
            model: "test-model".into(),
            api_key: "secret".into(),
            source_language: Some("en".into()),
            target_language: "TR".into(),
            segments: vec![
                segment("a", 100, 900, "Hello"),
                segment("b", 1_000, 1_900, "World"),
            ],
            previous_context: vec!["Earlier dialogue".into()],
        }
    }

    fn segment(id: &str, start_ms: u64, end_ms: u64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            id: id.into(),
            start_ms,
            end_ms,
            text: text.into(),
            detected_language: Some("en".into()),
            language_confidence: None,
            is_final: true,
        }
    }

    #[test]
    fn maps_json_by_id_and_preserves_timing() {
        let result = map_llm_translations(
            &request(),
            r#"{"translations":[{"id":"b","text":"Dünya"},{"id":"a","text":"Merhaba"}]}"#,
        )
        .unwrap();
        assert_eq!(result[0].translated_text.as_deref(), Some("Merhaba"));
        assert_eq!(result[0].start_ms, 100);
        assert_eq!(result[1].translated_text.as_deref(), Some("Dünya"));
        assert_eq!(result[1].end_ms, 1_900);
    }

    #[test]
    fn rejects_missing_duplicate_and_unknown_ids() {
        assert!(
            map_llm_translations(&request(), r#"{"translations":[{"id":"a","text":"A"}]}"#)
                .is_err()
        );
        assert!(
            map_llm_translations(
                &request(),
                r#"{"translations":[{"id":"a","text":"A"},{"id":"a","text":"B"}]}"#
            )
            .is_err()
        );
        assert!(
            map_llm_translations(
                &request(),
                r#"{"translations":[{"id":"a","text":"A"},{"id":"x","text":"X"}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn extracts_json_after_minimax_thinking_block() {
        let content =
            "<think>hidden reasoning</think>\n{\"translations\":[{\"id\":\"a\",\"text\":\"A\"}]}";
        assert_eq!(
            extract_json_object(content).as_deref(),
            Some(r#"{"translations":[{"id":"a","text":"A"}]}"#)
        );
    }

    #[test]
    fn repairs_minimax_json_split_across_thinking_boundary() {
        let content = "<think>analysis {\"translations\":[{\"id\":\"cue-</think>\n1\",\"text\":\"Merhaba\"}]}";
        assert_eq!(
            extract_json_object(content).as_deref(),
            Some(r#"{"translations":[{"id":"cue-1","text":"Merhaba"}]}"#)
        );
    }
}
