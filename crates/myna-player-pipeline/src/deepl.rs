use myna_player_core::{
    CueStatus, SubtitleCue, TranslationBatchRequest, TranslationBatchResult,
    TranslationProviderKind,
};
use serde::{Deserialize, Serialize};

use crate::PipelineError;

const MAX_BATCH_SEGMENTS: usize = 40;
const MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Serialize)]
struct DeepLRequest<'a> {
    text: Vec<&'a str>,
    target_lang: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeepLResponse {
    translations: Vec<DeepLTranslation>,
}

#[derive(Debug, Deserialize)]
struct DeepLTranslation {
    detected_source_language: Option<String>,
    text: String,
}

pub async fn translate_with_deepl(
    request: &TranslationBatchRequest,
) -> Result<TranslationBatchResult, PipelineError> {
    if request.api_key.trim().is_empty() {
        return Err(PipelineError::TranslationUnavailable(
            "DeepL API key is empty".into(),
        ));
    }
    if request.segments.is_empty() {
        return Ok(TranslationBatchResult {
            cues: Vec::new(),
            provider: provider_name(request.provider).into(),
        });
    }

    let endpoint = match request.provider {
        TranslationProviderKind::DeeplFree => "https://api-free.deepl.com/v2/translate",
        TranslationProviderKind::DeeplPro => "https://api.deepl.com/v2/translate",
        TranslationProviderKind::None => {
            return Err(PipelineError::TranslationUnavailable(
                "translation provider is disabled".into(),
            ));
        }
    };
    let source_language = normalize_deepl_source_language(request.source_language.as_deref());
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .map_err(|error| PipelineError::Provider(error.to_string()))?;
    let mut cues = Vec::with_capacity(request.segments.len());
    let mut rolling_context = request.previous_context.clone();
    for segments in request.segments.chunks(MAX_BATCH_SEGMENTS) {
        let context = build_translation_context(&rolling_context, segments);
        let body = DeepLRequest {
            text: segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect(),
            target_lang: &request.target_language,
            source_lang: source_language.clone(),
            context,
        };
        let translated =
            request_deepl_batch(&client, endpoint, request.api_key.trim(), &body).await?;
        cues.extend(map_translations(
            segments,
            translated,
            &request.target_language,
        )?);
        rolling_context.extend(segments.iter().map(|segment| segment.text.clone()));
    }

    Ok(TranslationBatchResult {
        cues,
        provider: provider_name(request.provider).into(),
    })
}

fn build_translation_context(
    previous_context: &[String],
    segments: &[myna_player_core::TranscriptSegment],
) -> Option<String> {
    let previous = previous_context
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let current = segments
        .iter()
        .map(|segment| segment.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    if previous.is_empty() && current.is_empty() {
        return None;
    }

    let mut context = String::new();
    if !previous.is_empty() {
        context.push_str(
            "Previous dialogue:
",
        );
        context.push_str(&previous.join(
            "
",
        ));
        context.push_str(
            "

",
        );
    }
    if !current.is_empty() {
        context.push_str("Current continuous dialogue (translate each requested timed cue consistently within this passage):
");
        context.push_str(&current.join(" "));
    }
    const MAX_CONTEXT_CHARS: usize = 8_000;
    if context.chars().count() > MAX_CONTEXT_CHARS {
        context = context
            .chars()
            .rev()
            .take(MAX_CONTEXT_CHARS)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
    }
    Some(context)
}

async fn request_deepl_batch(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    body: &DeepLRequest<'_>,
) -> Result<DeepLResponse, PipelineError> {
    for attempt in 0..MAX_ATTEMPTS {
        let response = client
            .post(endpoint)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("DeepL-Auth-Key {api_key}"),
            )
            .json(body)
            .send()
            .await;
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
                    "DeepL request failed: {error}"
                )));
            }
        };
        let status = response.status();
        let response_text = response
            .text()
            .await
            .map_err(|error| PipelineError::Provider(format!("DeepL response failed: {error}")))?;
        if status.is_success() {
            return serde_json::from_str(&response_text)
                .map_err(|error| PipelineError::Provider(format!("invalid DeepL JSON: {error}")));
        }
        if is_retryable_status(status) && attempt + 1 < MAX_ATTEMPTS {
            retry_delay(attempt).await;
            continue;
        }
        return Err(PipelineError::Provider(format!(
            "DeepL returned {status}: {}",
            response_text.trim()
        )));
    }
    Err(PipelineError::Provider(
        "DeepL retry budget was exhausted".into(),
    ))
}

fn map_translations(
    segments: &[myna_player_core::TranscriptSegment],
    translated: DeepLResponse,
    target_language: &str,
) -> Result<Vec<SubtitleCue>, PipelineError> {
    if translated.translations.len() != segments.len() {
        return Err(PipelineError::Provider(format!(
            "DeepL returned {} translations for {} segments",
            translated.translations.len(),
            segments.len()
        )));
    }
    Ok(segments
        .iter()
        .zip(translated.translations)
        .map(|(segment, translation)| SubtitleCue {
            id: segment.id.clone(),
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            source_text: segment.text.clone(),
            translated_text: Some(translation.text),
            source_language: segment
                .detected_language
                .clone()
                .or(translation.detected_source_language),
            target_language: Some(target_language.to_owned()),
            status: CueStatus::Ready,
        })
        .collect())
}

fn normalize_deepl_source_language(language: Option<&str>) -> Option<String> {
    let normalized = language?.trim().to_ascii_lowercase().replace('_', "-");
    if normalized.is_empty() || matches!(normalized.as_str(), "auto" | "unknown" | "und") {
        return None;
    }

    let code = match normalized.as_str() {
        "arabic" => "AR",
        "bulgarian" => "BG",
        "chinese" | "mandarin" | "zh-cn" | "zh-tw" => "ZH",
        "czech" => "CS",
        "danish" => "DA",
        "dutch" => "NL",
        "english" | "en-gb" | "en-us" => "EN",
        "estonian" => "ET",
        "finnish" => "FI",
        "french" => "FR",
        "german" => "DE",
        "greek" => "EL",
        "hebrew" => "HE",
        "hungarian" => "HU",
        "indonesian" => "ID",
        "italian" => "IT",
        "japanese" => "JA",
        "korean" => "KO",
        "latvian" => "LV",
        "lithuanian" => "LT",
        "norwegian" | "norwegian bokmal" | "norwegian bokmål" => "NB",
        "polish" => "PL",
        "portuguese" | "pt-br" | "pt-pt" => "PT",
        "romanian" => "RO",
        "russian" => "RU",
        "slovak" => "SK",
        "slovenian" => "SL",
        "spanish" => "ES",
        "swedish" => "SV",
        "thai" => "TH",
        "turkish" => "TR",
        "ukrainian" => "UK",
        "vietnamese" => "VI",
        candidate if is_supported_deepl_source_code(candidate) => {
            return Some(candidate.to_ascii_uppercase());
        }
        _ => return None,
    };
    Some(code.into())
}

fn is_supported_deepl_source_code(language: &str) -> bool {
    matches!(
        language,
        "ar" | "bg"
            | "cs"
            | "da"
            | "de"
            | "el"
            | "en"
            | "es"
            | "et"
            | "fi"
            | "fr"
            | "he"
            | "hu"
            | "id"
            | "it"
            | "ja"
            | "ko"
            | "lt"
            | "lv"
            | "nb"
            | "nl"
            | "pl"
            | "pt"
            | "ro"
            | "ru"
            | "sk"
            | "sl"
            | "sv"
            | "th"
            | "tr"
            | "uk"
            | "vi"
            | "zh"
    )
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || matches!(status.as_u16(), 502..=504)
}

async fn retry_delay(attempt: usize) {
    let milliseconds = 250_u64.saturating_mul(1_u64 << attempt.min(3));
    tokio::time::sleep(std::time::Duration::from_millis(milliseconds)).await;
}

fn provider_name(provider: TranslationProviderKind) -> &'static str {
    match provider {
        TranslationProviderKind::None => "none",
        TranslationProviderKind::DeeplFree => "deepl-free",
        TranslationProviderKind::DeeplPro => "deepl-pro",
    }
}

#[cfg(test)]
mod tests {
    use myna_player_core::TranscriptSegment;

    use super::*;

    fn segment(id: usize) -> TranscriptSegment {
        TranscriptSegment {
            id: id.to_string(),
            start_ms: id as u64 * 1_000,
            end_ms: id as u64 * 1_000 + 900,
            text: format!("source {id}"),
            detected_language: Some("en".into()),
            language_confidence: None,
            is_final: true,
        }
    }

    #[tokio::test]
    async fn missing_credential_fails_before_network_access() {
        let error = translate_with_deepl(&TranslationBatchRequest {
            segments: vec![segment(1)],
            source_language: Some("en".into()),
            target_language: "TR".into(),
            provider: TranslationProviderKind::DeeplFree,
            api_key: String::new(),
            previous_context: Vec::new(),
        })
        .await
        .unwrap_err();
        assert!(matches!(error, PipelineError::TranslationUnavailable(_)));
    }

    #[test]
    fn partial_batch_is_rejected_without_source_fallback() {
        let error = map_translations(
            &[segment(1), segment(2)],
            DeepLResponse {
                translations: vec![DeepLTranslation {
                    detected_source_language: Some("EN".into()),
                    text: "çeviri".into(),
                }],
            },
            "TR",
        )
        .unwrap_err();
        assert!(error.to_string().contains("1 translations for 2"));
    }

    #[test]
    fn batching_never_sends_more_than_forty_segments() {
        let segments = (0..95).map(segment).collect::<Vec<_>>();
        assert_eq!(
            segments
                .chunks(MAX_BATCH_SEGMENTS)
                .map(<[_]>::len)
                .collect::<Vec<_>>(),
            vec![40, 40, 15]
        );
    }

    #[test]
    fn batch_context_contains_the_complete_current_dialogue() {
        let segments = vec![segment(1), segment(2), segment(3)];
        let context = build_translation_context(&["earlier line".into()], &segments).unwrap();
        assert!(context.contains("earlier line"));
        assert!(context.contains("source 1 source 2 source 3"));
        assert!(context.contains("continuous dialogue"));
    }

    #[test]
    fn retry_policy_is_limited_to_transient_statuses() {
        assert!(is_retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(reqwest::StatusCode::BAD_GATEWAY));
        assert!(!is_retryable_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!is_retryable_status(reqwest::StatusCode::BAD_REQUEST));
    }

    #[test]
    fn whisper_language_names_are_normalized_for_deepl() {
        assert_eq!(
            normalize_deepl_source_language(Some("english")),
            Some("EN".into())
        );
        assert_eq!(
            normalize_deepl_source_language(Some("Turkish")),
            Some("TR".into())
        );
        assert_eq!(
            normalize_deepl_source_language(Some("en-US")),
            Some("EN".into())
        );
    }

    #[test]
    fn unsupported_source_languages_fall_back_to_deepl_detection() {
        assert_eq!(normalize_deepl_source_language(Some("auto")), None);
        assert_eq!(normalize_deepl_source_language(Some("klingon")), None);
        assert_eq!(normalize_deepl_source_language(Some("xx")), None);
    }
}
