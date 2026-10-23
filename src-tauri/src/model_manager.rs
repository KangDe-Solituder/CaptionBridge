use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures_util::StreamExt;
use reqwest::{header::RANGE, StatusCode};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::{
    models::{DownloadSettings, ModelInfo, ModelKind, ModelProgressEvent, ModelStatus},
    network,
    settings::AppPaths,
};

#[derive(Clone, Copy)]
pub struct ManifestFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

#[derive(Clone, Copy)]
pub struct ModelManifest {
    pub kind: ModelKind,
    pub id: &'static str,
    pub display_name: &'static str,
    pub repository: &'static str,
    pub revision: &'static str,
    pub license: &'static str,
    pub recommended: bool,
    pub files: &'static [ManifestFile],
}

const KOTOBA_FILES: &[ManifestFile] = &[
    ManifestFile {
        name: "config.json",
        size: 2_394,
        sha256: "a9306624f5ec14270a014b647e5c316b6e03a662c369758d1b90697a7b0655b9",
    },
    ManifestFile {
        name: "model.bin",
        size: 1_512_927_867,
        sha256: "60d2bc2e33de9d43f2745be09caefe1161acab670f6796d4a750d8d848382b36",
    },
    ManifestFile {
        name: "preprocessor_config.json",
        size: 340,
        sha256: "7ccc62c6f2765af1f3b46c00c9b5894426835a05021c8b9c01eecb6dfb542711",
    },
    ManifestFile {
        name: "tokenizer.json",
        size: 2_481_381,
        sha256: "f70c9740a90657b489cf05b0fa0605c1d497db542f11a70a8cc80a025c94c7d8",
    },
    ManifestFile {
        name: "vocabulary.json",
        size: 1_068_114,
        sha256: "c69260f2ab26d659b7c398f9a2b2b48ed0df16c3b47d7326782fd9cba71690c1",
    },
];

const TURBO_FILES: &[ManifestFile] = &[
    ManifestFile {
        name: "config.json",
        size: 2_263,
        sha256: "b0253ea6c0d3bea6b1e19e91a02acfd3b53f4467362efcb5a3e6b16c9b3a9b7e",
    },
    ManifestFile {
        name: "model.bin",
        size: 1_617_884_929,
        sha256: "e76620f83d5f5b69efd3d87e3dc180c1bd21df9fbebacfd4335e5e1efcc018da",
    },
    ManifestFile {
        name: "preprocessor_config.json",
        size: 340,
        sha256: "7ccc62c6f2765af1f3b46c00c9b5894426835a05021c8b9c01eecb6dfb542711",
    },
    ManifestFile {
        name: "tokenizer.json",
        size: 2_710_337,
        sha256: "297b13372ac43916285644fb9687add3cc62ee2a1adb60da3dc25cc94c1871fd",
    },
    ManifestFile {
        name: "vocabulary.json",
        size: 1_068_114,
        sha256: "c69260f2ab26d659b7c398f9a2b2b48ed0df16c3b47d7326782fd9cba71690c1",
    },
];

pub const MANIFESTS: &[ModelManifest] = &[
    ModelManifest {
        kind: ModelKind::Asr,
        id: "kotoba-whisper-v2.0-faster",
        display_name: "Kotoba Whisper v2.0 Faster",
        repository: "kotoba-tech/kotoba-whisper-v2.0-faster",
        revision: "f44edd35eaeb2274e85ac7b31fb2c6f59ff1c4bc",
        license: "MIT",
        recommended: true,
        files: KOTOBA_FILES,
    },
    ModelManifest {
        kind: ModelKind::Asr,
        id: "whisper-large-v3-turbo",
        display_name: "Whisper large-v3-turbo",
        repository: "dropbox-dash/faster-whisper-large-v3-turbo",
        revision: "0a363e9161cbc7ed1431c9597a8ceaf0c4f78fcf",
        license: "MIT",
        recommended: false,
        files: TURBO_FILES,
    },
];

pub const OCR_MANIFEST: ModelManifest = ModelManifest {
    kind: ModelKind::Ocr,
    id: crate::ocr::MODEL_ID,
    display_name: "PP-OCRv5 Mobile · 截图识别",
    repository: "RapidAI/RapidOCR",
    revision: "v3.10.0",
    license: "Apache-2.0",
    recommended: true,
    files: &[
        ManifestFile {
            name: "det.onnx",
            size: 4_819_576,
            sha256: "4d97c44a20d30a81aad087d6a396b08f786c4635742afc391f6621f5c6ae78ae",
        },
        ManifestFile {
            name: "rec.onnx",
            size: 16_631_306,
            sha256: "5825fc7ebf84ae7a412be049820b4d86d77620f204a041697b0494669b1742c5",
        },
        ManifestFile {
            name: "cls.onnx",
            size: 1_018_508,
            sha256: "54379ae5174d026780215fc748a7f31910dee36818e63d49e17dc598ecc82df7",
        },
    ],
};
fn ocr_model_url(name: &str) -> String {
    let path = match name {
        "det.onnx" => "det/ch_PP-OCRv5_det_mobile.onnx",
        "rec.onnx" => "rec/ch_PP-OCRv5_rec_mobile.onnx",
        _ => "cls/ch_PP-LCNet_x0_25_textline_ori_cls_mobile.onnx",
    };
    format!(
        "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.10.0/onnx/PP-OCRv5/{path}"
    )
}

#[derive(Clone, Default)]
pub struct DownloadRegistry {
    inner: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    failures: Arc<Mutex<HashMap<String, String>>>,
}

impl DownloadRegistry {
    pub async fn begin(&self, id: &str) -> Result<Arc<AtomicBool>, String> {
        let mut active = self.inner.lock().await;
        if active.contains_key(id) {
            return Err("该模型已在下载中".to_string());
        }
        self.failures.lock().await.remove(id);
        let flag = Arc::new(AtomicBool::new(false));
        active.insert(id.to_string(), flag.clone());
        Ok(flag)
    }

    pub async fn cancel(&self, id: &str) -> bool {
        let active = self.inner.lock().await;
        if let Some(flag) = active.get(id) {
            flag.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub async fn finish(&self, id: &str, error: Option<String>) {
        if let Some(error) = error {
            self.failures.lock().await.insert(id.to_string(), error);
        } else {
            self.failures.lock().await.remove(id);
        }
        self.inner.lock().await.remove(id);
    }

    pub async fn failure(&self, id: &str) -> Option<String> {
        self.failures.lock().await.get(id).cloned()
    }

    pub async fn clear_failure(&self, id: &str) {
        self.failures.lock().await.remove(id);
    }

    pub async fn is_active(&self, id: &str) -> bool {
        self.inner.lock().await.contains_key(id)
    }
}

pub fn manifest(id: &str) -> Result<&'static ModelManifest, String> {
    MANIFESTS
        .iter()
        .chain(std::iter::once(&OCR_MANIFEST))
        .find(|m| m.id == id)
        .ok_or_else(|| format!("未知模型：{id}"))
}

fn expected_size(item: &ModelManifest) -> u64 {
    item.files.iter().map(|f| f.size).sum()
}

fn installed_size(dir: &Path, item: &ModelManifest) -> u64 {
    item.files
        .iter()
        .filter_map(|f| fs::metadata(dir.join(f.name)).ok())
        .map(|m| m.len())
        .sum()
}

pub async fn list_models(paths: &AppPaths, downloads: &DownloadRegistry) -> Vec<ModelInfo> {
    let mut list = Vec::new();
    for item in MANIFESTS.iter().chain(std::iter::once(&OCR_MANIFEST)) {
        let dir = paths.models_dir.join(item.id);
        let size = installed_size(&dir, item);
        let downloading = downloads.is_active(item.id).await;
        let error = downloads.failure(item.id).await;
        let status = if downloading {
            ModelStatus::Downloading
        } else if error.is_some() {
            ModelStatus::Failed
        } else if size == 0 {
            ModelStatus::NotInstalled
        } else if item
            .files
            .iter()
            .all(|f| fs::metadata(dir.join(f.name)).is_ok_and(|m| m.len() == f.size))
        {
            ModelStatus::Available
        } else {
            ModelStatus::Corrupt
        };
        list.push(ModelInfo {
            kind: item.kind,
            id: item.id.to_string(),
            display_name: item.display_name.to_string(),
            repository: item.repository.to_string(),
            revision: item.revision.to_string(),
            license: item.license.to_string(),
            expected_size_bytes: expected_size(item),
            installed_size_bytes: size,
            status,
            downloaded_bytes: partial_size(paths, item),
            error,
            recommended: item.recommended,
        });
    }
    list
}

fn partial_size(paths: &AppPaths, item: &ModelManifest) -> u64 {
    item.files
        .iter()
        .filter_map(|file| {
            fs::metadata(
                paths
                    .downloads_dir
                    .join(item.id)
                    .join(format!("{}.part", file.name)),
            )
            .ok()
        })
        .map(|m| m.len())
        .sum()
}

fn emit_progress(
    app: &AppHandle,
    item: &ModelManifest,
    status: ModelStatus,
    downloaded: u64,
    error: Option<String>,
) {
    let _ = app.emit(
        "model:progress",
        ModelProgressEvent {
            model_id: item.id.to_string(),
            status,
            downloaded_bytes: downloaded,
            total_bytes: expected_size(item),
            error,
        },
    );
}

pub async fn download_model(
    app: AppHandle,
    paths: AppPaths,
    registry: DownloadRegistry,
    id: String,
    mirror: String,
    download_settings: DownloadSettings,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let item = *manifest(&id)?;
    let result =
        download_model_inner(&app, &paths, &item, &mirror, &download_settings, &cancel).await;
    let cancelled = cancel.load(Ordering::Relaxed);
    registry
        .finish(
            &id,
            if cancelled {
                None
            } else {
                result.as_ref().err().cloned()
            },
        )
        .await;
    if let Err(error) = &result {
        let status = if cancelled {
            ModelStatus::NotInstalled
        } else {
            ModelStatus::Failed
        };
        emit_progress(
            &app,
            &item,
            status,
            partial_size(&paths, &item),
            Some(error.clone()),
        );
    } else {
        // Publish completion only after clearing the active download, so a UI
        // refresh cannot overwrite Available with a stale Downloading state.
        emit_progress(
            &app,
            &item,
            ModelStatus::Available,
            expected_size(&item),
            None,
        );
    }
    result
}

async fn download_model_inner(
    app: &AppHandle,
    paths: &AppPaths,
    item: &ModelManifest,
    mirror: &str,
    download_settings: &DownloadSettings,
    cancel: &AtomicBool,
) -> Result<(), String> {
    paths.ensure()?;
    let download_dir = paths.downloads_dir.join(item.id);
    let model_dir = paths.models_dir.join(item.id);
    fs::create_dir_all(&download_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;
    let client = network::download_client(download_settings, std::time::Duration::from_secs(10))?;
    let mut total_done = partial_size(paths, item);
    emit_progress(app, item, ModelStatus::Downloading, total_done, None);

    for file in item.files {
        if cancel.load(Ordering::Relaxed) {
            return Err("下载已取消，可稍后继续".to_string());
        }
        let part = download_dir.join(format!("{}.part", file.name));
        if verify_file(&part, file).unwrap_or(false) {
            total_done = total_done.max(partial_size(paths, item));
            continue;
        }
        let official = format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            item.repository, item.revision, file.name
        );
        let mirror_url = format!(
            "{}/{}/resolve/{}/{}",
            mirror.trim_end_matches('/'),
            item.repository,
            item.revision,
            file.name
        );
        let mut last_error = String::new();
        let urls = if item.kind == ModelKind::Ocr {
            vec![ocr_model_url(file.name)]
        } else {
            vec![official, mirror_url]
        };
        for url in urls {
            match download_file(&client, &url, &part, file.size, cancel, |current| {
                let before: u64 = item
                    .files
                    .iter()
                    .take_while(|f| f.name != file.name)
                    .map(|f| {
                        let p = model_dir.join(f.name);
                        if p.exists() {
                            f.size
                        } else {
                            fs::metadata(download_dir.join(format!("{}.part", f.name)))
                                .map(|m| m.len())
                                .unwrap_or(0)
                        }
                    })
                    .sum();
                emit_progress(app, item, ModelStatus::Downloading, before + current, None);
            })
            .await
            {
                Ok(()) => {
                    last_error.clear();
                    break;
                }
                Err(error) => last_error = error,
            }
        }
        if !last_error.is_empty() {
            return Err(format!("{} 下载失败：{last_error}", file.name));
        }
        total_done = partial_size(paths, item);
    }

    emit_progress(app, item, ModelStatus::Verifying, total_done, None);
    for file in item.files {
        let part = download_dir.join(format!("{}.part", file.name));
        if !verify_file(&part, file)? {
            return Err(format!(
                "{} 的 SHA-256 校验失败；已保留 .part 文件",
                file.name
            ));
        }
    }
    for file in item.files {
        let part = download_dir.join(format!("{}.part", file.name));
        let target = model_dir.join(file.name);
        if target.exists() {
            fs::remove_file(&target).map_err(|e| e.to_string())?;
        }
        fs::rename(&part, &target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn download_file<F: Fn(u64)>(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    expected: u64,
    cancel: &AtomicBool,
    progress: F,
) -> Result<(), String> {
    let existing = resumable_size(path, expected)?;
    let mut request = client.get(url);
    if existing > 0 {
        request = request.header(RANGE, format!("bytes={existing}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("连接模型源失败：{e:#}"))?;
    let append = existing > 0 && response.status() == StatusCode::PARTIAL_CONTENT;
    if !response.status().is_success() {
        return Err(format!("模型源返回 HTTP {}", response.status()));
    }
    let mut output = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut current = if append { existing } else { 0 };
    let mut stream = response.bytes_stream();
    loop {
        let next = tokio::time::timeout(std::time::Duration::from_secs(30), stream.next())
            .await
            .map_err(|_| "模型源 30 秒未返回数据".to_string())?;
        let Some(chunk) = next else {
            break;
        };
        if cancel.load(Ordering::Relaxed) {
            return Err("下载已取消，可稍后继续".to_string());
        }
        let chunk = chunk.map_err(|e| e.to_string())?;
        output.write_all(&chunk).map_err(|e| e.to_string())?;
        current += chunk.len() as u64;
        progress(current);
    }
    output.flush().map_err(|e| e.to_string())?;
    if current != expected {
        return Err(format!("文件大小不符：需要 {expected}，实际 {current}"));
    }
    Ok(())
}

fn resumable_size(path: &Path, expected: u64) -> Result<u64, String> {
    let size = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if size >= expected && size > 0 {
        File::create(path).map_err(|error| error.to_string())?;
        Ok(0)
    } else {
        Ok(size)
    }
}

pub fn verify_model(paths: &AppPaths, id: &str) -> Result<(), String> {
    let item = manifest(id)?;
    let dir = paths.models_dir.join(item.id);
    for file in item.files {
        let path = dir.join(file.name);
        if !verify_file(&path, file)? {
            return Err(format!("{} 校验失败", file.name));
        }
    }
    Ok(())
}

fn verify_file(path: &Path, expected: &ManifestFile) -> Result<bool, String> {
    if fs::metadata(path).map(|m| m.len()).unwrap_or(0) != expected.size {
        return Ok(false);
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()) == expected.sha256)
}

pub fn delete_model(paths: &AppPaths, id: &str) -> Result<(), String> {
    let item = manifest(id)?;
    let dir = paths.models_dir.join(item.id);
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_download_survives_refresh_and_clears_on_retry() {
        let registry = DownloadRegistry::default();
        let paths = AppPaths::new(std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()));
        let id = OCR_MANIFEST.id;
        registry.begin(id).await.unwrap();
        assert!(registry.begin(id).await.is_err());
        registry.finish(id, Some("HTTP 403 Forbidden".into())).await;
        let models = list_models(&paths, &registry).await;
        let ocr = models.iter().find(|model| model.id == id).unwrap();
        assert!(matches!(ocr.status, ModelStatus::Failed));
        assert_eq!(ocr.error.as_deref(), Some("HTTP 403 Forbidden"));
        registry.begin(id).await.unwrap();
        assert!(registry.failure(id).await.is_none());
        registry.finish(id, None).await;
        assert!(!registry.is_active(id).await);
        assert!(registry.failure(id).await.is_none());
    }

    #[tokio::test]
    #[ignore = "downloads pinned OCR models from the network"]
    async fn ocr_download_sources_match_manifest() {
        let dir =
            std::env::temp_dir().join(format!("livecaption-ocr-download-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let settings = crate::settings::load(&AppPaths::shared().unwrap()).unwrap();
        eprintln!(
            "OCR download probe: proxy enabled = {}",
            settings.downloads.proxy_enabled
        );
        let client =
            network::download_client(&settings.downloads, std::time::Duration::from_secs(10))
                .unwrap();
        for file in OCR_MANIFEST.files {
            let path = dir.join(file.name);
            let result = download_file(
                &client,
                &ocr_model_url(file.name),
                &path,
                file.size,
                &AtomicBool::new(false),
                |_| {},
            )
            .await;
            assert!(result.is_ok(), "{}: {:?}", file.name, result);
            assert!(
                verify_file(&path, file).unwrap(),
                "{} hash mismatch",
                file.name
            );
            eprintln!("{}: size and SHA-256 verified", file.name);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn manifests_are_pinned_and_complete() {
        for model in MANIFESTS {
            assert_eq!(model.kind, ModelKind::Asr);
            assert_eq!(model.revision.len(), 40);
            assert!(model.files.iter().any(|f| f.name == "model.bin"));
            assert!(model
                .files
                .iter()
                .all(|f| f.sha256.len() == 64 && f.size > 0));
        }
        let ocr = manifest(crate::ocr::MODEL_ID).unwrap();
        assert_eq!(ocr.kind, ModelKind::Ocr);
        assert_eq!(serde_json::to_value(ocr.kind).unwrap(), "ocr");
        assert_eq!(expected_size(ocr), 22_469_390);
        assert!(ocr.files.iter().all(|f| f.sha256.len() == 64 && f.size > 0));
    }

    #[test]
    fn complete_but_invalid_partial_is_restarted_instead_of_resumed() {
        let dir = std::env::temp_dir().join(format!("model-resume-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let part = dir.join("model.bin.part");
        fs::write(&part, [1_u8; 8]).unwrap();

        assert_eq!(resumable_size(&part, 8).unwrap(), 0);
        assert_eq!(fs::metadata(&part).unwrap().len(), 0);

        let _ = fs::remove_dir_all(dir);
    }
}
