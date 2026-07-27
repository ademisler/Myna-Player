use crate::{LookAheadPlan, LookAheadRequest, ProcessingPriority, ProcessingWindow};

const DEFAULT_TARGET_BUFFER_MS: u64 = 90_000;
const DEFAULT_URGENT_BUFFER_MS: u64 = 20_000;
const DEFAULT_CHUNK_DURATION_MS: u64 = 30_000;
const MAX_WINDOWS_PER_PLAN: usize = 4;

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
}
