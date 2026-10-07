use myna_player_core::{TranscriptSegment, TranslationBatchRequest, TranslationProviderKind};
use myna_player_pipeline::translate_with_deepl;

#[tokio::test]
#[ignore = "requires a live DeepL API key in DEEPL_AUTH_KEY"]
async fn translates_timed_cues_as_one_contextual_batch() {
    let api_key = std::env::var("DEEPL_AUTH_KEY").expect("DEEPL_AUTH_KEY must be set");
    let segments = vec![
        timed_segment("smoke-1", 220, 2_000, "And so my fellow Americans"),
        timed_segment(
            "smoke-2",
            2_010,
            7_490,
            "ask not what your country can do for you,",
        ),
        timed_segment("smoke-3", 8_190, 9_590, "ask what you can do."),
    ];
    let request = TranslationBatchRequest {
        segments: segments.clone(),
        source_language: Some("english".into()),
        target_language: "TR".into(),
        provider: TranslationProviderKind::DeeplFree,
        api_key,
        previous_context: vec!["A public speech is being delivered.".into()],
    };

    let result = translate_with_deepl(&request)
        .await
        .expect("DeepL translation should succeed");
    assert_eq!(result.provider, "deepl-free");
    assert_eq!(result.cues.len(), segments.len());
    for (cue, source) in result.cues.iter().zip(segments) {
        assert_eq!(cue.id, source.id);
        assert_eq!(cue.start_ms, source.start_ms);
        assert_eq!(cue.end_ms, source.end_ms);
        assert_eq!(cue.target_language.as_deref(), Some("TR"));
        assert!(
            cue.translated_text
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
        );
        println!(
            "{}-{} {}",
            cue.start_ms,
            cue.end_ms,
            cue.translated_text.as_deref().unwrap_or_default()
        );
    }
}

fn timed_segment(id: &str, start_ms: u64, end_ms: u64, text: &str) -> TranscriptSegment {
    TranscriptSegment {
        id: id.into(),
        start_ms,
        end_ms,
        text: text.into(),
        detected_language: Some("english".into()),
        language_confidence: None,
        is_final: true,
    }
}
