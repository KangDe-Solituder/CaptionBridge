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
    models::{ModelInfo, ModelProgressEvent, ModelStatus},
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
        id: "kotoba-whisper-v2.0-faster",
        display_name: "Kotoba Whisper v2.0 Faster",
        repository: "kotoba-tech/kotoba-whisper-v2.0-faster",
        revision: "f44edd35eaeb2274e85ac7b31fb2c6f59ff1c4bc",
        license: "MIT",
        recommended: true,
        files: KOTOBA_FILES,
    },
    ModelManifest {
        id: "whisper-large-v3-turbo",
        display_name: "Whisper large-v3-turbo",
        repository: "dropbox-dash/faster-whisper-large-v3-turbo",
        revision: "0a363e9161cbc7ed1431c9597a8ceaf0c4f78fcf",
        license: "MIT",
        recommended: false,
        files: TURBO_FILES,
    },
];

#[derive(Clone, Default)]
pub struct DownloadRegistry {
    inner: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl DownloadRegistry {
    pub async fn begin(&self, id: &str) -> Result<Arc<AtomicBool>, String> {
        let mut active = self.inner.lock().await;
        if active.contains_key(id) {
            return Err("该模型已在下载中".to_string());
        }
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

    pub async fn finish(&self, id: &str) {
        self.inner.lock().await.remove(id);
    }

    pub async fn is_active(&self, id: &str) -> bool {
        self.inner.lock().await.contains_key(id)
    }
}

pub fn manifest(id: &str) -> Result<&'static ModelManifest, String> {
    MANIFESTS
        .iter()
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
    for item in MANIFESTS {
        let dir = paths.models_dir.join(item.id);
        let size = installed_size(&dir, item);
        let downloading = downloads.is_active(item.id).await;
        let status = if downloading {
            ModelStatus::Downloading
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
            id: item.id.to_string(),
            display_name: item.display_name.to_string(),
            repository: item.repository.to_string(),
            revision: item.revision.to_string(),
            license: item.license.to_string(),
            expected_size_bytes: expected_size(item),
            installed_size_bytes: size,
            status,
            downloaded_bytes: partial_size(paths, item),
            error: None,
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
) -> Result<(), String> {
    let item = *manifest(&id)?;
    let cancel = registry.begin(&id).await?;
    let result = download_model_inner(&app, &paths, &item, &mirror, &cancel).await;
    registry.finish(&id).await;
    if let Err(error) = &result {
        let status = if cancel.load(Ordering::Relaxed) {
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
    }
    result
}

async fn download_model_inner(
    app: &AppHandle,
    paths: &AppPaths,
    item: &ModelManifest,
    mirror: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    paths.ensure()?;
    let download_dir = paths.downloads_dir.join(item.id);
    let model_dir = paths.models_dir.join(item.id);
    fs::create_dir_all(&download_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
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
        for url in [official, mirror_url] {
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
            return Err(last_error);
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
    emit_progress(app, item, ModelStatus::Available, expected_size(item), None);
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
    let existing = fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(0)
        .min(expected);
    let mut request = client.get(url);
    if existing > 0 {
        request = request.header(RANGE, format!("bytes={existing}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("连接模型源失败：{e}"))?;
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

    #[test]
    fn manifests_are_pinned_and_complete() {
        for model in MANIFESTS {
            assert_eq!(model.revision.len(), 40);
            assert!(model.files.iter().any(|f| f.name == "model.bin"));
            assert!(model
                .files
                .iter()
                .all(|f| f.sha256.len() == 64 && f.size > 0));
        }
    }
}
