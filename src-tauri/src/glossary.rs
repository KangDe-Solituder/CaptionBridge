use std::{fs, path::Path};

use crate::{
    models::{Glossary, GlossaryImportResult, GlossaryPair, GlossaryTerm},
    settings::AppPaths,
};

fn glossary_file(paths: &AppPaths) -> std::path::PathBuf {
    paths.data_dir.join("glossary.json")
}

pub fn load(paths: &AppPaths) -> Glossary {
    let file = glossary_file(paths);
    let Ok(raw) = fs::read_to_string(file) else {
        return Glossary::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(paths: &AppPaths, glossary: &Glossary) -> Result<(), String> {
    paths.ensure()?;
    let raw = serde_json::to_string_pretty(glossary).map_err(|error| error.to_string())?;
    fs::write(glossary_file(paths), raw).map_err(|error| error.to_string())
}

pub fn pairs(glossary: &Glossary) -> Vec<GlossaryPair> {
    glossary
        .terms
        .iter()
        .map(|term| GlossaryPair {
            source: term.source.clone(),
            target: term.target.clone(),
        })
        .collect()
}

fn normalize(term: &str) -> String {
    term.trim().to_lowercase()
}

/// Merge an imported file into the stored glossary. Terms are keyed by their
/// normalized source text: matches update the translation and note, new sources
/// are appended. Accepts either a full glossary object or a bare term array.
pub fn import(paths: &AppPaths, file: &Path) -> Result<GlossaryImportResult, String> {
    let raw = fs::read_to_string(file).map_err(|error| error.to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| "术语表文件不是有效的 JSON".to_string())?;
    let imported_terms: Vec<GlossaryTerm> = if value.is_array() {
        serde_json::from_value(value).map_err(|_| "术语表条目格式不正确".to_string())?
    } else {
        let glossary: Glossary =
            serde_json::from_value(value).map_err(|_| "术语表文件格式不正确".to_string())?;
        glossary.terms
    };

    let mut glossary = load(paths);
    let mut added = 0_usize;
    let mut updated = 0_usize;
    for mut term in imported_terms {
        term.source = term.source.trim().to_string();
        term.target = term.target.trim().to_string();
        if term.source.is_empty() || term.target.is_empty() {
            continue;
        }
        if let Some(existing) = glossary
            .terms
            .iter_mut()
            .find(|existing| normalize(&existing.source) == normalize(&term.source))
        {
            existing.target = term.target;
            if !term.note.is_empty() {
                existing.note = term.note;
            }
            updated += 1;
        } else {
            term.id = uuid::Uuid::new_v4().to_string();
            glossary.terms.push(term);
            added += 1;
        }
    }
    save(paths, &glossary)?;
    Ok(GlossaryImportResult {
        glossary,
        added,
        updated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_merges_by_normalized_source() {
        let dir = std::env::temp_dir().join(format!("livecaption-test-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::new(dir.clone());
        paths.ensure().unwrap();
        let mut glossary = Glossary::default();
        glossary.terms.push(GlossaryTerm {
            id: "1".to_string(),
            source: "先生".to_string(),
            target: "老师".to_string(),
            note: "旧译法".to_string(),
        });
        save(&paths, &glossary).unwrap();

        let import_file = dir.join("import.json");
        fs::write(
            &import_file,
            serde_json::json!([
                {"id": "x", "source": "先生 ", "target": "女士们先生们", "note": ""},
                {"id": "y", "source": "新词", "target": "new", "note": "备注"},
                {"id": "z", "source": "", "target": "跳过", "note": ""}
            ])
            .to_string(),
        )
        .unwrap();

        let result = import(&paths, &import_file).unwrap();
        assert_eq!((result.added, result.updated), (1, 1));
        assert_eq!(result.glossary.terms.len(), 2);
        assert_eq!(result.glossary.terms[0].target, "女士们先生们");
        assert_eq!(result.glossary.terms[1].note, "备注");
        let _ = fs::remove_dir_all(dir);
    }
}
