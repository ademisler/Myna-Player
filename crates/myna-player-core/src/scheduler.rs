use crate::{LookAheadPlan, LookAheadRequest, ProcessingPriority, ProcessingWindow};

const DEFAULT_TARGET_BUFFER_MS: u64 = 90_000;
const DEFAULT_URGENT_BUFFER_MS: u64 = 20_000;
const DEFAULT_CHUNK_DURATION_MS: u64 = 30_000;
const MAX_WINDOWS_PER_PLAN: usize = 4;

pub fn plan_full_transcription(
    media_duration_ms: u64,
    chunk_duration_ms: u64,
) -> Vec<ProcessingWindow> {
    if media_duration_ms == 0 {
        return Vec::new();
    }

    let chunk_duration_ms = chunk_duration_ms.clamp(1, 120_000);
    let mut cursor = 0;
    let mut windows = Vec::new();

    while cursor < media_duration_ms {
        let end_ms = cursor
            .saturating_add(chunk_duration_ms)
            .min(media_duration_ms);
        if end_ms <= cursor {
            break;
        }

        windows.push(ProcessingWindow {
            start_ms: cursor,
            end_ms,
            priority: ProcessingPriority::Background,
        });
        cursor = end_ms;
    }

    windows
}

pub fn plan_lookahead(request: &LookAheadRequest) -> LookAheadPlan {
    let target_buffer_ms = request
        .target_buffer_ms
        .unwrap_or(DEFAULT_TARGET_BUFFER_MS)
        .max(1_000);
    let urgent_buffer_ms = request
        .urgent_buffer_ms
        .unwrap_or(DEFAULT_URGENT_BUFFER_MS)
        .min(target_buffer_ms);
    let chunk_duration_ms = request
        .chunk_duration_ms
        .unwrap_or(DEFAULT_CHUNK_DURATION_MS)
        .clamp(5_000, target_buffer_ms);

    let ready_until_ms = request
        .ready_until_ms
        .max(request.playback_position_ms)
        .min(request.media_duration_ms);
    let current_buffer_ms = ready_until_ms.saturating_sub(request.playback_position_ms);
    let desired_until_ms = request
        .playback_position_ms
        .saturating_add(target_buffer_ms)
        .min(request.media_duration_ms);

    let mut cursor = ready_until_ms;
    let mut windows = Vec::new();

    while cursor < desired_until_ms && windows.len() < MAX_WINDOWS_PER_PLAN {
        let end_ms = cursor
            .saturating_add(chunk_duration_ms)
            .min(desired_until_ms)
            .min(request.media_duration_ms);

        if end_ms <= cursor {
            break;
        }

        let distance_from_playback = cursor.saturating_sub(request.playback_position_ms);
        let priority = if distance_from_playback < urgent_buffer_ms {
            ProcessingPriority::Urgent
        } else if distance_from_playback < target_buffer_ms {
            ProcessingPriority::Normal
        } else {
            ProcessingPriority::Background
        };

        windows.push(ProcessingWindow {
            start_ms: cursor,
            end_ms,
            priority,
        });
        cursor = end_ms;
    }

    LookAheadPlan {
        current_buffer_ms,
        target_buffer_ms,
        windows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_empty_buffer_from_playback_position() {
        let plan = plan_lookahead(&LookAheadRequest {
            playback_position_ms: 60_000,
            ready_until_ms: 0,
            media_duration_ms: 600_000,
            target_buffer_ms: Some(90_000),
            urgent_buffer_ms: Some(20_000),
            chunk_duration_ms: Some(30_000),
        });

        assert_eq!(plan.current_buffer_ms, 0);
        assert_eq!(plan.windows.len(), 3);
        assert_eq!(plan.windows[0].start_ms, 60_000);
        assert_eq!(plan.windows[0].priority, ProcessingPriority::Urgent);
        assert_eq!(plan.windows[2].end_ms, 150_000);
    }

    #[test]
    fn does_not_schedule_past_media_end() {
        let plan = plan_lookahead(&LookAheadRequest {
            playback_position_ms: 95_000,
            ready_until_ms: 95_000,
            media_duration_ms: 100_000,
            target_buffer_ms: None,
            urgent_buffer_ms: None,
            chunk_duration_ms: None,
        });

        assert_eq!(plan.windows.len(), 1);
        assert_eq!(plan.windows[0].end_ms, 100_000);
    }

    #[test]
    fn schedules_nothing_when_target_is_ready() {
        let plan = plan_lookahead(&LookAheadRequest {
            playback_position_ms: 10_000,
            ready_until_ms: 120_000,
            media_duration_ms: 600_000,
            target_buffer_ms: Some(90_000),
            urgent_buffer_ms: None,
            chunk_duration_ms: None,
        });

        assert!(plan.windows.is_empty());
        assert_eq!(plan.current_buffer_ms, 110_000);
    }

    #[test]
    fn full_transcription_covers_a_short_end_of_file_window() {
        let windows = plan_full_transcription(60_050, 30_000);

        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].start_ms, 0);
        assert_eq!(windows[0].end_ms, 30_000);
        assert_eq!(windows[1].start_ms, 30_000);
        assert_eq!(windows[1].end_ms, 60_000);
        assert_eq!(windows[2].start_ms, 60_000);
        assert_eq!(windows[2].end_ms, 60_050);
        assert!(
            windows
                .windows(2)
                .all(|pair| pair[0].end_ms == pair[1].start_ms)
        );
    }

    #[test]
    fn full_transcription_stops_exactly_at_media_end() {
        let windows = plan_full_transcription(60_000, 30_000);

        assert_eq!(windows.len(), 2);
        assert_eq!(windows.last().map(|window| window.end_ms), Some(60_000));
    }

    #[test]
    fn full_transcription_has_no_work_for_empty_media() {
        assert!(plan_full_transcription(0, 30_000).is_empty());
    }
}
