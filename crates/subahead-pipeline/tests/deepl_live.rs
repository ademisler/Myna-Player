use subahead_core::{TranscriptSegment, TranslationBatchRequest, TranslationProviderKind};
use subahead_pipeline::translate_with_deepl;

#[tokio::test]
#[ignore = "requires a live DeepL API key in DEEPL_AUTH_KEY"]
async fn translates_a_timed_segment_with_deepl_free() {
    let api_key = std::env::var("DEEPL_AUTH_KEY").expect("DEEPL_AUTH_KEY must be set");
    let request = TranslationBatchRequest {
        segments: vec![TranscriptSegment {
            id: "smoke-1".into(),
            start_ms: 0,
            end_ms: 10_500,
            text: "And so my fellow Americans, ask not what your country can do for you; ask what you can do for your country.".into(),
            detected_language: Some("EN".into()),
            language_confidence: None,
            is_final: true,
        }],
        source_language: Some("EN".into()),
        target_language: "TR".into(),
        provider: TranslationProviderKind::DeeplFree,
        api_key,
        previous_context: Vec::new(),
    };

    let result = translate_with_deepl(&request)
        .await
        .expect("DeepL translation should succeed");
    assert_eq!(result.provider, "deepl-free");
    assert_eq!(result.cues.len(), 1);
    let cue = &result.cues[0];
    assert_eq!(cue.start_ms, 0);
    assert_eq!(cue.end_ms, 10_500);
    assert_eq!(cue.target_language.as_deref(), Some("TR"));
    assert!(
        cue.translated_text
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
    );
    println!(
        "translated_text={}",
        cue.translated_text.as_deref().unwrap_or_default()
    );
}
