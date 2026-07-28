use serde::{Deserialize, Serialize};
use subahead_core::{
    CueStatus, SubtitleCue, TranslationBatchRequest, TranslationBatchResult,
    TranslationProviderKind,
};

use crate::PipelineError;

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
    let source_language = request
        .source_language
        .as_deref()
        .filter(|language| !language.eq_ignore_ascii_case("auto"))
        .map(|language| language.to_ascii_uppercase());
    let context = if request.previous_context.is_empty() {
        None
    } else {
        Some(request.previous_context.join("\n"))
    };
    let body = DeepLRequest {
        text: request
            .segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect(),
        target_lang: &request.target_language,
        source_lang: source_language,
        context,
    };

    let response = reqwest::Client::new()
        .post(endpoint)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("DeepL-Auth-Key {}", request.api_key.trim()),
        )
        .json(&body)
        .send()
        .await
        .map_err(|error| PipelineError::Provider(format!("DeepL request failed: {error}")))?;
    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|error| PipelineError::Provider(format!("DeepL response failed: {error}")))?;
    if !status.is_success() {
        return Err(PipelineError::Provider(format!(
            "DeepL returned {status}: {}",
            response_text.trim()
        )));
    }

    let translated: DeepLResponse = serde_json::from_str(&response_text)
        .map_err(|error| PipelineError::Provider(format!("invalid DeepL JSON: {error}")))?;
    if translated.translations.len() != request.segments.len() {
        return Err(PipelineError::Provider(format!(
            "DeepL returned {} translations for {} segments",
            translated.translations.len(),
            request.segments.len()
        )));
    }

    let cues = request
        .segments
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
            target_language: Some(request.target_language.clone()),
            status: CueStatus::Ready,
        })
        .collect();

    Ok(TranslationBatchResult {
        cues,
        provider: provider_name(request.provider).into(),
    })
}

fn provider_name(provider: TranslationProviderKind) -> &'static str {
    match provider {
        TranslationProviderKind::None => "none",
        TranslationProviderKind::DeeplFree => "deepl-free",
        TranslationProviderKind::DeeplPro => "deepl-pro",
    }
}
