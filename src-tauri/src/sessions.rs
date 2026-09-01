use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::models::{
    CaptionSegment, SessionEntryView, SessionPageView, SessionSummary, TranslationResult,
};

#[derive(Debug, Clone)]
pub struct JsonlSession {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCorrection {
    pub source_text: String,
    pub translated_text: String,
    pub at: chrono::DateTime<Local>,
    pub glossary_used: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub timestamp: chrono::DateTime<Local>,
    pub segment: CaptionSegment,
    pub result: TranslationResult,
    #[serde(default)]
    pub corrected: Option<SessionCorrection>,
}

impl JsonlSession {
    pub fn start(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|error| error.to_string())?;
        let file_name = format!("{}.jsonl", Local::now().format("%Y-%m-%d_%H-%M-%S"));
        let path = root.join(file_name);
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        Ok(Self { path })
    }

    pub fn id(&self) -> String {
        self.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(
        &self,
        segment: &CaptionSegment,
        result: &TranslationResult,
    ) -> Result<(), String> {
        let record = SessionRecord {
            timestamp: Local::now(),
            segment: segment.clone(),
            result: result.clone(),
            corrected: None,
        };
        let raw = serde_json::to_string(&record).map_err(|error| error.to_string())?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{raw}").map_err(|error| error.to_string())
    }
}

/// Resolve a session id (file stem) back to a file inside `root`, refusing
/// anything that could escape the sessions directory.
pub fn session_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if id.is_empty()
        || id
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
    {
        return Err("无效的字幕记录 id".to_string());
    }
    let path = root.join(format!("{id}.jsonl"));
    if !path.exists() {
        return Err("字幕记录不存在".to_string());
    }
    Ok(path)
}

pub fn read_records(path: &Path) -> Vec<SessionRecord> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<SessionRecord>(&line).ok())
        .collect()
}

pub fn rewrite_records(path: &Path, records: &[SessionRecord]) -> Result<(), String> {
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut file = fs::File::create(&tmp).map_err(|error| error.to_string())?;
        for record in records {
            let raw = serde_json::to_string(record).map_err(|error| error.to_string())?;
            writeln!(file, "{raw}").map_err(|error| error.to_string())?;
        }
    }
    fs::rename(&tmp, path).map_err(|error| error.to_string())
}

fn created_at_of(path: &Path, first: Option<&SessionRecord>) -> String {
    if let Some(record) = first {
        return record.timestamp.to_rfc3339();
    }
    // An empty session file: fall back to the timestamp encoded in its name.
    if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(stem, "%Y-%m-%d_%H-%M-%S") {
            return naive
                .and_local_timezone(Local)
                .single()
                .map(|time| time.to_rfc3339())
                .unwrap_or_default();
        }
    }
    String::new()
}

pub fn summarize(root: &Path, current_id: Option<&str>) -> Vec<SessionSummary> {
    let mut summaries = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return summaries;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let id = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut entry_count = 0_usize;
        let mut first_record = None;
        let mut has_correction = false;
        if let Ok(file) = fs::File::open(&path) {
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                entry_count += 1;
                if first_record.is_none() {
                    first_record = serde_json::from_str::<SessionRecord>(&line).ok();
                }
                if !has_correction && line.contains("\"corrected\":{") {
                    has_correction = true;
                }
            }
        }
        summaries.push(SessionSummary {
            is_current: current_id == Some(id.as_str()),
            created_at: created_at_of(&path, first_record.as_ref()),
            first_source: first_record
                .as_ref()
                .map(|record| record.segment.source_text.clone())
                .unwrap_or_default(),
            first_translation: first_record
                .as_ref()
                .map(|record| record.result.translated_text.clone())
                .unwrap_or_default(),
            id,
            entry_count,
            has_correction,
        });
    }
    // File names carry a local timestamp, so name order is chronological.
    summaries.sort_by(|a, b| b.id.cmp(&a.id));
    summaries
}

fn entry_view(index: usize, record: &SessionRecord) -> SessionEntryView {
    SessionEntryView {
        index,
        timestamp: record.timestamp.to_rfc3339(),
        source_text: record.segment.source_text.clone(),
        translated_text: record.result.translated_text.clone(),
        corrected_source_text: record
            .corrected
            .as_ref()
            .map(|corrected| corrected.source_text.clone()),
        corrected_translated_text: record
            .corrected
            .as_ref()
            .map(|corrected| corrected.translated_text.clone()),
        model: record.result.model.clone(),
        latency_ms: record.result.latency_ms,
        error: record.result.error.clone(),
    }
}

/// Paginate chronologically. `page` is 1-based and clamps into range, so
/// callers can pass `usize::MAX` to land on the newest page.
pub fn paginate(records: &[SessionRecord], page: usize, page_size: usize) -> SessionPageView {
    let page_size = page_size.clamp(1, 100);
    let total = records.len();
    let page_count = total.div_ceil(page_size).max(1);
    let page = page.clamp(1, page_count);
    let start = (page - 1) * page_size;
    let entries = records
        .iter()
        .enumerate()
        .skip(start)
        .take(page_size)
        .map(|(offset, record)| entry_view(offset + 1, record))
        .collect();
    SessionPageView {
        entries,
        total,
        page,
        page_count,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn sample_record(text: &str) -> SessionRecord {
        SessionRecord {
            timestamp: Local::now(),
            segment: CaptionSegment {
                id: "1".to_string(),
                source_text: text.to_string(),
                revision: 1,
                committed: true,
                source: crate::models::CaptionSourceKind::WindowsLiveCaption,
                model_id: None,
                start_ms: 0,
                end_ms: 1,
                final_result: true,
                time_source: crate::models::CaptionTimeSource::Estimated,
            },
            result: TranslationResult {
                source_text: text.to_string(),
                translated_text: format!("译:{text}"),
                model: "test".to_string(),
                latency_ms: 1,
                first_token_ms: 1,
                cached: false,
                error: None,
            },
            corrected: None,
        }
    }

    #[test]
    fn writes_jsonl_record() {
        let root = std::env::temp_dir().join(format!("livecaption-test-{}", uuid::Uuid::new_v4()));
        let session = JsonlSession::start(&root).unwrap();
        let segment = sample_record("hello").segment;
        let result = sample_record("hello").result;
        session.append(&segment, &result).unwrap();
        let entries = fs::read_dir(&root).unwrap().count();
        assert_eq!(entries, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_path_escaping_session_ids() {
        let root = Path::new("sessions");
        assert!(session_path(root, "../settings").is_err());
        assert!(session_path(root, "").is_err());
        assert!(session_path(root, "a/b").is_err());
    }

    #[test]
    fn paginates_chronologically_and_clamps_to_last_page() {
        let records = (0..45)
            .map(|i| sample_record(&format!("句{i}")))
            .collect::<Vec<_>>();
        let first = paginate(&records, 1, 20);
        assert_eq!(first.entries.len(), 20);
        assert_eq!(first.page, 1);
        assert_eq!(first.page_count, 3);
        assert_eq!(first.entries[0].index, 1);
        let last = paginate(&records, usize::MAX, 20);
        assert_eq!(last.page, 3);
        assert_eq!(last.entries.len(), 5);
        assert_eq!(last.entries[4].index, 45);
        let empty = paginate(&[], 1, 20);
        assert_eq!(empty.page, 1);
        assert_eq!(empty.page_count, 1);
        assert!(empty.entries.is_empty());
    }

    #[test]
    fn summarizes_sessions_newest_first() {
        let root = std::env::temp_dir().join(format!("livecaption-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        for (name, lines) in [("2026-08-01_10-00-00", 2), ("2026-08-02_10-00-00", 1)] {
            let path = root.join(format!("{name}.jsonl"));
            let mut file = fs::File::create(&path).unwrap();
            for _ in 0..lines {
                writeln!(
                    file,
                    "{}",
                    serde_json::to_string(&sample_record("预览")).unwrap()
                )
                .unwrap();
            }
        }
        let summaries = summarize(&root, Some("2026-08-01_10-00-00"));
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, "2026-08-02_10-00-00");
        assert!(!summaries[0].is_current);
        assert!(summaries[1].is_current);
        assert_eq!(summaries[1].entry_count, 2);
        assert_eq!(summaries[1].first_source, "预览");
        let _ = fs::remove_dir_all(root);
    }
}
