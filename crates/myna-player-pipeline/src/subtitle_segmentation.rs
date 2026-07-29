use myna_player_core::TranscriptSegment;

const UTTERANCE_GAP_MS: u64 = 850;
const SOFT_GAP_MS: u64 = 320;
const MAX_CUE_DURATION_MS: u64 = 5_400;
const MAX_CUE_CHARS: usize = 52;
const MIN_CUE_DURATION_MS: u64 = 850;
const CUE_TAIL_MS: u64 = 120;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TimedWord {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

pub(crate) fn subtitle_segments_from_words(
    words: Vec<TimedWord>,
    id_prefix: &str,
    detected_language: Option<String>,
    language_confidence: Option<f32>,
) -> Vec<TranscriptSegment> {
    let words = normalize_words(words);
    if words.is_empty() {
        return Vec::new();
    }

    let utterances = split_utterances(&words);
    let mut cue_words = Vec::new();
    for utterance in utterances {
        cue_words.extend(partition_utterance(utterance));
    }

    let mut segments = cue_words
        .into_iter()
        .enumerate()
        .filter_map(|(index, cue)| {
            let first = cue.first()?;
            let last = cue.last()?;
            let text = render_words(&cue);
            (!text.is_empty()).then(|| TranscriptSegment {
                id: format!("{id_prefix}-{index}"),
                start_ms: first.start_ms,
                end_ms: last.end_ms.max(first.start_ms.saturating_add(1)),
                text,
                detected_language: detected_language.clone(),
                language_confidence,
                is_final: true,
            })
        })
        .collect::<Vec<_>>();

    for index in 0..segments.len() {
        let next_start = segments.get(index + 1).map(|next| next.start_ms);
        let desired_end = segments[index].end_ms.saturating_add(CUE_TAIL_MS);
        segments[index].end_ms = next_start
            .map(|start| desired_end.min(start.saturating_sub(30)))
            .unwrap_or(desired_end)
            .max(segments[index].start_ms.saturating_add(1));
    }

    segments
}

fn normalize_words(words: Vec<TimedWord>) -> Vec<TimedWord> {
    words
        .into_iter()
        .filter_map(|mut word| {
            word.text = word.text.trim().to_owned();
            if word.text.is_empty() {
                return None;
            }
            word.end_ms = word.end_ms.max(word.start_ms);
            Some(word)
        })
        .collect()
}

fn split_utterances(words: &[TimedWord]) -> Vec<&[TimedWord]> {
    let mut utterances = Vec::new();
    let mut start = 0;
    for index in 0..words.len() {
        let current = &words[index];
        let hard_stop = is_hard_punctuation(&current.text);
        let long_gap = words
            .get(index + 1)
            .is_some_and(|next| next.start_ms.saturating_sub(current.end_ms) >= UTTERANCE_GAP_MS);
        if hard_stop || long_gap || index + 1 == words.len() {
            utterances.push(&words[start..=index]);
            start = index + 1;
        }
    }
    utterances
}

fn partition_utterance(words: &[TimedWord]) -> Vec<Vec<TimedWord>> {
    if words.is_empty() {
        return Vec::new();
    }
    let count = words.len();
    let mut costs = vec![f64::INFINITY; count + 1];
    let mut next_break = vec![count; count + 1];
    costs[count] = 0.0;

    for start in (0..count).rev() {
        for end in (start + 1)..=count {
            let slice = &words[start..end];
            let chars = render_words(slice).chars().count();
            let duration = slice
                .last()
                .map(|word| word.end_ms)
                .unwrap_or_default()
                .saturating_sub(slice.first().map(|word| word.start_ms).unwrap_or_default());
            if end > start + 1 && (chars > MAX_CUE_CHARS || duration > MAX_CUE_DURATION_MS) {
                break;
            }

            let boundary_cost = cue_cost(words, start, end, chars, duration);
            let total = boundary_cost + costs[end];
            if total < costs[start] {
                costs[start] = total;
                next_break[start] = end;
            }
        }
    }

    let mut cues = Vec::new();
    let mut cursor = 0;
    while cursor < count {
        let mut end = next_break[cursor];
        if end <= cursor || end > count {
            end = (cursor + 1).min(count);
        }
        cues.push(words[cursor..end].to_vec());
        cursor = end;
    }
    cues
}

fn cue_cost(words: &[TimedWord], start: usize, end: usize, chars: usize, duration: u64) -> f64 {
    let target_chars = 32.0;
    let target_duration = 3_000.0;
    let char_cost = ((chars as f64 - target_chars) / 9.0).powi(2);
    let duration_cost = ((duration as f64 - target_duration) / 1_400.0).powi(2);
    let short_cost = if duration < MIN_CUE_DURATION_MS && end < words.len() {
        18.0
    } else if chars < 10 && end < words.len() {
        10.0
    } else {
        0.0
    };

    let last = &words[end - 1];
    let punctuation_bonus = if is_hard_punctuation(&last.text) {
        -9.0
    } else if is_soft_punctuation(&last.text) {
        -5.0
    } else {
        0.0
    };
    let gap_bonus = words.get(end).map_or(0.0, |next| {
        let gap = next.start_ms.saturating_sub(last.end_ms);
        if gap >= UTTERANCE_GAP_MS {
            -8.0
        } else if gap >= SOFT_GAP_MS {
            -4.0
        } else {
            1.5
        }
    });

    let orphan_penalty = if end < words.len() && words.len() - end <= 2 {
        12.0
    } else {
        0.0
    };
    let first_cue_bias = if start == 0 && end < words.len() && chars < 16 {
        5.0
    } else {
        0.0
    };

    char_cost
        + duration_cost
        + short_cost
        + punctuation_bonus
        + gap_bonus
        + orphan_penalty
        + first_cue_bias
}

fn render_words(words: &[TimedWord]) -> String {
    let mut output = String::new();
    for word in words {
        if output.is_empty()
            || attaches_to_previous(&word.text)
            || attaches_to_next(output.chars().last())
        {
            output.push_str(&word.text);
        } else {
            output.push(' ');
            output.push_str(&word.text);
        }
    }
    output.trim().to_owned()
}

fn attaches_to_previous(value: &str) -> bool {
    matches!(
        value,
        "." | "," | "!" | "?" | ";" | ":" | "%" | ")" | "]" | "}" | "…"
    ) || value.starts_with(['\'', '’'])
}

fn attaches_to_next(previous: Option<char>) -> bool {
    matches!(previous, Some('(' | '[' | '{' | '“' | '‘'))
}

fn is_hard_punctuation(value: &str) -> bool {
    matches!(value, "." | "!" | "?" | "…") || value.ends_with(['.', '!', '?', '…'])
}

fn is_soft_punctuation(value: &str) -> bool {
    matches!(value, "," | ";" | ":") || value.ends_with([',', ';', ':'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start_ms: u64, end_ms: u64) -> TimedWord {
        TimedWord {
            text: text.into(),
            start_ms,
            end_ms,
        }
    }

    #[test]
    fn long_utterance_becomes_multiple_non_overlapping_subtitle_cues() {
        let words = vec![
            word("And", 220, 220),
            word("so", 330, 450),
            word("my", 450, 530),
            word("fellow", 680, 1_170),
            word("Americans", 1_170, 1_980),
            word("ask", 2_010, 3_680),
            word("not", 4_030, 4_280),
            word("what", 4_280, 4_910),
            word("your", 5_040, 5_430),
            word("country", 5_430, 6_340),
            word("can", 6_340, 6_730),
            word("do", 6_730, 6_920),
            word("for", 7_010, 7_140),
            word("you", 7_160, 7_280),
            word(",", 7_280, 7_490),
            word("ask", 8_190, 8_490),
            word("what", 8_630, 8_750),
            word("you", 8_910, 8_980),
            word("can", 8_980, 9_360),
            word("do", 9_360, 9_410),
            word(".", 9_440, 9_470),
        ];

        let cues = subtitle_segments_from_words(words, "window", Some("english".into()), None);
        assert!(cues.len() >= 2, "expected multiple cues: {cues:#?}");
        assert_eq!(cues.first().unwrap().start_ms, 220);
        assert!(cues.last().unwrap().text.ends_with('.'));
        assert!(
            cues.windows(2)
                .all(|pair| pair[0].end_ms < pair[1].start_ms)
        );
        assert!(cues.iter().all(|cue| cue.end_ms > cue.start_ms));
        assert!(
            cues.iter()
                .all(|cue| cue.text.chars().count() <= MAX_CUE_CHARS)
        );
    }

    #[test]
    fn punctuation_is_rendered_without_extra_spaces() {
        let cues = subtitle_segments_from_words(
            vec![
                word("Hello", 0, 400),
                word(",", 400, 420),
                word("world", 500, 900),
                word("!", 900, 920),
            ],
            "window",
            Some("en".into()),
            None,
        );
        assert_eq!(cues[0].text, "Hello, world!");
    }

    #[test]
    fn a_long_pause_starts_a_new_cue_at_the_next_spoken_word() {
        let cues = subtitle_segments_from_words(
            vec![
                word("First", 100, 500),
                word("line", 520, 900),
                word("Second", 2_000, 2_500),
                word("line", 2_520, 2_900),
            ],
            "window",
            Some("en".into()),
            None,
        );
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[1].start_ms, 2_000);
    }
}
