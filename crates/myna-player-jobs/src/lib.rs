use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, HashSet},
};

use myna_player_core::{ProcessingPriority, ProcessingWindow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledWindow {
    pub generation: u64,
    pub sequence: u64,
    pub window: ProcessingWindow,
}

impl Ord for ScheduledWindow {
    fn cmp(&self, other: &Self) -> Ordering {
        priority_rank(self.window.priority)
            .cmp(&priority_rank(other.window.priority))
            .then_with(|| other.window.start_ms.cmp(&self.window.start_ms))
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for ScheduledWindow {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn priority_rank(priority: ProcessingPriority) -> u8 {
    match priority {
        ProcessingPriority::Urgent => 3,
        ProcessingPriority::Normal => 2,
        ProcessingPriority::Background => 1,
    }
}

#[derive(Debug, Clone)]
pub struct ProcessingQueue {
    generation: u64,
    next_sequence: u64,
    duration_ms: u64,
    chunk_duration_ms: u64,
    lookahead_ms: u64,
    process_full_media: bool,
    pending: BinaryHeap<ScheduledWindow>,
    queued: HashMap<(u64, u64), ProcessingPriority>,
    completed: HashSet<(u64, u64)>,
}

impl ProcessingQueue {
    pub fn new(
        duration_ms: u64,
        chunk_duration_ms: u64,
        lookahead_ms: u64,
        process_full_media: bool,
    ) -> Self {
        Self {
            generation: 0,
            next_sequence: 0,
            duration_ms,
            chunk_duration_ms: chunk_duration_ms.clamp(5_000, 120_000),
            lookahead_ms: lookahead_ms.max(10_000),
            process_full_media,
            pending: BinaryHeap::new(),
            queued: HashMap::new(),
            completed: HashSet::new(),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn total_windows(&self) -> usize {
        if self.duration_ms == 0 {
            0
        } else {
            self.duration_ms.div_ceil(self.chunk_duration_ms) as usize
        }
    }

    pub fn completed_windows(&self) -> usize {
        self.completed.len()
    }

    pub fn restore_completed<I>(&mut self, windows: I)
    where
        I: IntoIterator<Item = (u64, u64)>,
    {
        self.completed.extend(windows);
    }

    pub fn schedule_initial(&mut self, position_ms: u64) {
        self.schedule_around(position_ms, false);
        if self.process_full_media {
            let mut start_ms = 0;
            while start_ms < self.duration_ms {
                let end_ms = start_ms
                    .saturating_add(self.chunk_duration_ms)
                    .min(self.duration_ms);
                self.push(start_ms, end_ms, ProcessingPriority::Background);
                start_ms = end_ms;
            }
        }
    }

    pub fn seek(&mut self, position_ms: u64) {
        self.generation = self.generation.saturating_add(1);
        self.pending.clear();
        self.queued.clear();
        self.schedule_around(position_ms, true);

        if self.process_full_media {
            let mut start_ms = 0;
            while start_ms < self.duration_ms {
                let end_ms = start_ms
                    .saturating_add(self.chunk_duration_ms)
                    .min(self.duration_ms);
                self.push(start_ms, end_ms, ProcessingPriority::Background);
                start_ms = end_ms;
            }
        }
    }

    pub fn promote_lookahead(&mut self, position_ms: u64) {
        self.schedule_around(position_ms, false);
    }

    pub fn pop(&mut self) -> Option<ScheduledWindow> {
        while let Some(job) = self.pending.pop() {
            let key = (job.window.start_ms, job.window.end_ms);
            if self.completed.contains(&key) {
                self.queued.remove(&key);
                continue;
            }
            if self.queued.remove(&key).is_some() {
                return Some(job);
            }
        }
        None
    }

    pub fn mark_completed(&mut self, window: &ProcessingWindow) {
        let key = (window.start_ms, window.end_ms);
        self.queued.remove(&key);
        self.completed.insert(key);
    }

    pub fn requeue(&mut self, job: ScheduledWindow) {
        if job.generation != self.generation {
            return;
        }
        self.push(job.window.start_ms, job.window.end_ms, job.window.priority);
    }

    pub fn ready_until_from(&self, position_ms: u64) -> u64 {
        let mut cursor = canonical_start(position_ms, self.chunk_duration_ms);
        while self.completed.contains(&(
            cursor,
            cursor
                .saturating_add(self.chunk_duration_ms)
                .min(self.duration_ms),
        )) {
            cursor = cursor
                .saturating_add(self.chunk_duration_ms)
                .min(self.duration_ms);
            if cursor >= self.duration_ms {
                break;
            }
        }
        cursor
    }

    fn schedule_around(&mut self, position_ms: u64, after_seek: bool) {
        if self.duration_ms == 0 {
            return;
        }
        let mut start_ms = canonical_start(
            position_ms.min(self.duration_ms.saturating_sub(1)),
            self.chunk_duration_ms,
        );
        let target_ms = position_ms
            .saturating_add(self.lookahead_ms)
            .min(self.duration_ms);
        let mut index = 0_u8;

        while start_ms < target_ms {
            let end_ms = start_ms
                .saturating_add(self.chunk_duration_ms)
                .min(self.duration_ms);
            let priority = if index == 0 || after_seek {
                ProcessingPriority::Urgent
            } else {
                ProcessingPriority::Normal
            };
            self.push(start_ms, end_ms, priority);
            start_ms = end_ms;
            index = index.saturating_add(1);
        }
    }

    fn push(&mut self, start_ms: u64, end_ms: u64, priority: ProcessingPriority) {
        if start_ms >= end_ms || self.completed.contains(&(start_ms, end_ms)) {
            return;
        }
        let key = (start_ms, end_ms);
        if self
            .queued
            .get(&key)
            .is_some_and(|current| priority_rank(*current) >= priority_rank(priority))
        {
            return;
        }

        self.queued.insert(key, priority);
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.pending.push(ScheduledWindow {
            generation: self.generation,
            sequence: self.next_sequence,
            window: ProcessingWindow {
                start_ms,
                end_ms,
                priority,
            },
        });
    }
}

fn canonical_start(position_ms: u64, chunk_duration_ms: u64) -> u64 {
    position_ms / chunk_duration_ms * chunk_duration_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_window_and_lookahead_beat_background_work() {
        let mut queue = ProcessingQueue::new(600_000, 30_000, 90_000, true);
        queue.schedule_initial(0);

        let first = queue.pop().unwrap();
        assert_eq!(first.window.start_ms, 0);
        assert_eq!(first.window.priority, ProcessingPriority::Urgent);

        let second = queue.pop().unwrap();
        assert_eq!(second.window.start_ms, 30_000);
        assert_eq!(second.window.priority, ProcessingPriority::Normal);
    }

    #[test]
    fn seek_invalidates_pending_generation_but_preserves_completed() {
        let mut queue = ProcessingQueue::new(600_000, 30_000, 90_000, true);
        queue.schedule_initial(0);
        let first = queue.pop().unwrap();
        queue.mark_completed(&first.window);
        let old_generation = queue.generation();

        queue.seek(330_000);

        assert!(queue.generation() > old_generation);
        assert_eq!(queue.completed_windows(), 1);
        let next = queue.pop().unwrap();
        assert_eq!(next.window.start_ms, 330_000);
        assert_eq!(next.window.priority, ProcessingPriority::Urgent);
    }

    #[test]
    fn end_of_file_window_is_short_and_exact() {
        let mut queue = ProcessingQueue::new(60_050, 30_000, 90_000, true);
        queue.schedule_initial(59_000);

        let urgent = queue.pop().unwrap();
        assert_eq!(urgent.window.start_ms, 30_000);
        assert_eq!(urgent.window.end_ms, 60_000);

        let tail = queue.pop().unwrap();
        assert_eq!(tail.window.start_ms, 60_000);
        assert_eq!(tail.window.end_ms, 60_050);
    }

    #[test]
    fn restored_windows_are_not_scheduled_again() {
        let mut queue = ProcessingQueue::new(90_000, 30_000, 90_000, true);
        queue.restore_completed([(0, 30_000), (30_000, 60_000)]);
        queue.schedule_initial(0);

        let job = queue.pop().unwrap();
        assert_eq!(job.window.start_ms, 60_000);
        assert_eq!(queue.ready_until_from(0), 60_000);
    }

    #[test]
    fn repeated_seek_keeps_only_latest_urgent_region() {
        let mut queue = ProcessingQueue::new(900_000, 30_000, 90_000, true);
        queue.schedule_initial(0);
        queue.seek(300_000);
        queue.seek(720_000);

        let next = queue.pop().unwrap();
        assert_eq!(next.generation, 2);
        assert_eq!(next.window.start_ms, 720_000);
    }

    #[test]
    fn very_short_media_has_one_exact_window() {
        let mut queue = ProcessingQueue::new(2_400, 30_000, 90_000, true);
        queue.schedule_initial(0);
        let only = queue.pop().unwrap();
        assert_eq!(only.window.start_ms, 0);
        assert_eq!(only.window.end_ms, 2_400);
        queue.mark_completed(&only.window);
        assert!(queue.pop().is_none());
        assert_eq!(queue.ready_until_from(0), 2_400);
    }

    #[test]
    fn promotion_replaces_background_priority_deterministically() {
        let mut queue = ProcessingQueue::new(300_000, 30_000, 30_000, true);
        queue.schedule_initial(0);
        let first = queue.pop().unwrap();
        queue.mark_completed(&first.window);

        queue.promote_lookahead(150_000);
        let promoted = queue.pop().unwrap();
        assert_eq!(promoted.window.start_ms, 150_000);
        assert_eq!(promoted.window.priority, ProcessingPriority::Urgent);
    }

    #[test]
    fn stale_generation_cannot_be_requeued_after_seek() {
        let mut queue = ProcessingQueue::new(300_000, 30_000, 90_000, true);
        queue.schedule_initial(0);
        let stale = queue.pop().unwrap();
        queue.seek(180_000);
        queue.requeue(stale);

        let next = queue.pop().unwrap();
        assert_eq!(next.generation, queue.generation());
        assert_eq!(next.window.start_ms, 180_000);
    }
}
