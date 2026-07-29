use myna_player_core::{TranscriptSegment, TranslationBatchRequest, TranslationProviderKind};
use myna_player_pipeline::{LlmTranslationRequest, translate_with_deepl, translate_with_llm};

fn segments() -> Vec<TranscriptSegment> {
    [
        ("cue-1", 220, 2_800, "I never said she stole the money."),
        (
            "cue-2",
            3_050,
            5_700,
            "I said she borrowed it without asking.",
        ),
        (
            "cue-3",
            6_100,
            8_900,
            "Come on, don't make a mountain out of a molehill.",
        ),
        ("cue-4", 9_200, 11_500, "We're not out of the woods yet."),
    ]
    .into_iter()
    .map(|(id, start_ms, end_ms, text)| TranscriptSegment {
        id: id.into(),
        start_ms,
        end_ms,
        text: text.into(),
        detected_language: Some("en".into()),
        language_confidence: Some(0.99),
        is_final: true,
    })
    .collect()
}

fn verify_timing_and_ids(cues: &[myna_player_core::SubtitleCue]) {
    let source = segments();
    assert_eq!(cues.len(), source.len());
    for (cue, segment) in cues.iter().zip(source) {
        assert_eq!(cue.id, segment.id);
        assert_eq!(cue.start_ms, segment.start_ms);
        assert_eq!(cue.end_ms, segment.end_ms);
        assert!(
            cue.translated_text
                .as_deref()
                .is_some_and(|text| !text.trim().is_empty())
        );
    }
}

#[tokio::test]
#[ignore = "requires live provider credentials"]
async fn live_minimax_translation_quality() {
    let api_key = std::env::var("MINIMAX_API_KEY").expect("MINIMAX_API_KEY must be set");
    let model = std::env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-M2.7".into());
    let cues = translate_with_llm(&LlmTranslationRequest {
        provider_id: "minimax".into(),
        model,
        api_key,
        source_language: Some("en".into()),
        target_language: "Turkish".into(),
        segments: segments(),
        previous_context: vec!["Two people are arguing about a misunderstanding.".into()],
    })
    .await
    .expect("MiniMax translation should succeed");
    verify_timing_and_ids(&cues);
    for cue in cues {
        println!(
            "{} | {} | {}",
            cue.id,
            cue.source_text,
            cue.translated_text.unwrap()
        );
    }
}

#[tokio::test]
#[ignore = "requires live provider credentials"]
async fn live_gemini_translation_quality() {
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let model = std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-3.5-flash".into());
    let cues = translate_with_llm(&LlmTranslationRequest {
        provider_id: "gemini".into(),
        model,
        api_key,
        source_language: Some("en".into()),
        target_language: "Turkish".into(),
        segments: segments(),
        previous_context: vec!["Two people are arguing about a misunderstanding.".into()],
    })
    .await
    .expect("Gemini translation should succeed");
    verify_timing_and_ids(&cues);
    for cue in cues {
        println!(
            "{} | {} | {}",
            cue.id,
            cue.source_text,
            cue.translated_text.unwrap()
        );
    }
}

#[tokio::test]
#[ignore = "requires live provider credentials"]
async fn live_deepl_translation_quality() {
    let api_key = std::env::var("DEEPL_AUTH_KEY").expect("DEEPL_AUTH_KEY must be set");
    let result = translate_with_deepl(&TranslationBatchRequest {
        api_key,
        provider: TranslationProviderKind::DeeplFree,
        source_language: Some("english".into()),
        target_language: "TR".into(),
        segments: segments(),
        previous_context: vec!["Two people are arguing about a misunderstanding.".into()],
    })
    .await
    .expect("DeepL translation should succeed");
    verify_timing_and_ids(&result.cues);
    for cue in result.cues {
        println!(
            "{} | {} | {}",
            cue.id,
            cue.source_text,
            cue.translated_text.unwrap()
        );
    }
}
