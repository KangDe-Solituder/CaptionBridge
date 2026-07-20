use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use chrono::Local;
use serde::Serialize;

use crate::models::{CaptionSegment, TranslationResult};

#[derive(Debug, Clone)]
pub struct JsonlSession {
    path: PathBuf,
}

#[derive(Debug, Serialize)]
struct SessionRecord<'a> {
    timestamp: chrono::DateTime<Local>,
    segment: &'a CaptionSegment,
    result: &'a TranslationResult,
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

    pub fn append(
        &self,
        segment: &CaptionSegment,
        result: &TranslationResult,
    ) -> Result<(), String> {
        let record = SessionRecord {
            timestamp: Local::now(),
            segment,
            result,
        };
        let raw = serde_json::to_string(&record).map_err(|error| error.to_string())?;
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{raw}").map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn writes_jsonl_record() {
        let root = std::env::temp_dir().join(format!("livecaption-test-{}", uuid::Uuid::new_v4()));
        let session = JsonlSession::start(&root).unwrap();
        let segment = CaptionSegment {
            id: "1".to_string(),
            source_text: "hello".to_string(),
            revision: 1,
            committed: true,
            source: crate::models::CaptionSourceKind::WindowsLiveCaption,
            model_id: None,
            start_ms: 0,
            end_ms: 1,
            final_result: true,
            time_source: crate::models::CaptionTimeSource::Estimated,
        };
        let result = TranslationResult {
            source_text: "hello".to_string(),
            translated_text: "你好".to_string(),
            model: "test".to_string(),
            latency_ms: 1,
            first_token_ms: 1,
            cached: false,
            error: None,
        };
        session.append(&segment, &result).unwrap();
        let entries = fs::read_dir(&root).unwrap().count();
        assert_eq!(entries, 1);
        let _ = fs::remove_dir_all(root);
    }
}
