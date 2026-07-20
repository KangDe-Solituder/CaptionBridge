use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::models::{CaptionSegment, CaptionSettings, CaptionSourceKind, CaptionTimeSource};

pub struct BasicSegmenter {
    settings: CaptionSettings,
    committed_prefix: String,
    candidate: String,
    candidate_since: Option<Instant>,
    utterance_since: Option<Instant>,
    revision: u64,
}

impl BasicSegmenter {
    pub fn new(settings: CaptionSettings) -> Self {
        Self {
            settings,
            committed_prefix: String::new(),
            candidate: String::new(),
            candidate_since: None,
            utterance_since: None,
            revision: 0,
        }
    }

    pub fn update(&mut self, text: &str, now: Instant) -> Option<CaptionSegment> {
        let normalized = normalize(text);
        if normalized.is_empty() {
            return None;
        }

        let remaining = remove_committed_overlap(&self.committed_prefix, &normalized);

        if remaining.is_empty() {
            return None;
        }

        if remaining != self.candidate {
            self.candidate = remaining;
            self.candidate_since = Some(now);
            self.utterance_since.get_or_insert(now);
        }

        let since = self.candidate_since.unwrap_or(now);
        let stable =
            now.duration_since(since) >= Duration::from_millis(self.settings.stable_milliseconds);
        let utterance_since = self.utterance_since.unwrap_or(now);
        let expired = now.duration_since(utterance_since)
            >= Duration::from_millis(self.settings.max_duration_milliseconds);
        let punctuated = self
            .candidate
            .chars()
            .last()
            .is_some_and(|char| matches!(char, '.' | '!' | '?' | '。' | '！' | '？'));
        let too_long = self.candidate.chars().count() >= self.settings.max_chars;

        if stable || expired || punctuated || too_long {
            let source_text = self.candidate.clone();
            self.committed_prefix = normalized;
            self.candidate.clear();
            self.candidate_since = None;
            self.utterance_since = None;
            self.revision += 1;
            return Some(CaptionSegment {
                id: Uuid::new_v4().to_string(),
                source_text,
                revision: self.revision,
                committed: true,
                source: CaptionSourceKind::WindowsLiveCaption,
                model_id: None,
                start_ms: 0,
                end_ms: 0,
                final_result: true,
                time_source: CaptionTimeSource::Estimated,
            });
        }

        None
    }
}

fn remove_committed_overlap(committed: &str, current: &str) -> String {
    if let Some(remaining) = current.strip_prefix(committed) {
        return remaining.trim().to_string();
    }
    let committed_chars = committed.chars().collect::<Vec<_>>();
    let current_chars = current.chars().collect::<Vec<_>>();
    let max_overlap = committed_chars.len().min(current_chars.len());
    let overlap = (3..=max_overlap)
        .rev()
        .find(|length| {
            committed_chars[committed_chars.len() - length..] == current_chars[..*length]
        })
        .unwrap_or(0);
    current_chars[overlap..]
        .iter()
        .collect::<String>()
        .trim()
        .to_string()
}

fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_stable_text() {
        let mut segmenter = BasicSegmenter::new(CaptionSettings {
            stable_milliseconds: 100,
            ..CaptionSettings::default()
        });
        let start = Instant::now();
        assert!(segmenter.update("hello world", start).is_none());
        let segment = segmenter
            .update("hello world", start + Duration::from_millis(120))
            .unwrap();
        assert_eq!(segment.source_text, "hello world");
    }

    #[test]
    fn does_not_repeat_committed_prefix() {
        let mut segmenter = BasicSegmenter::new(CaptionSettings {
            stable_milliseconds: 100,
            ..CaptionSettings::default()
        });
        let start = Instant::now();
        segmenter.update("hello", start);
        segmenter.update("hello", start + Duration::from_millis(120));
        segmenter.update("hello world", start + Duration::from_millis(240));
        let segment = segmenter
            .update("hello world", start + Duration::from_millis(360))
            .unwrap();
        assert_eq!(segment.source_text, "world");
    }

    #[test]
    fn commits_continuously_changing_caption_after_max_duration() {
        let mut segmenter = BasicSegmenter::new(CaptionSettings {
            stable_milliseconds: 1_000,
            max_duration_milliseconds: 300,
            ..CaptionSettings::default()
        });
        let start = Instant::now();
        assert!(segmenter.update("hello", start).is_none());
        assert!(segmenter
            .update("hello world", start + Duration::from_millis(150))
            .is_none());
        let segment = segmenter
            .update("hello world again", start + Duration::from_millis(320))
            .unwrap();
        assert_eq!(segment.source_text, "hello world again");
    }

    #[test]
    fn removes_overlap_from_rolling_caption_window() {
        assert_eq!(
            remove_committed_overlap("hello world", "world again"),
            "again"
        );
        assert_eq!(
            remove_committed_overlap("忘れるよ そうだ", "そうだ 次の行"),
            "次の行"
        );
    }
}
