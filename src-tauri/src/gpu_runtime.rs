use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures_util::StreamExt;
use reqwest::{header::RANGE, StatusCode};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use zip::ZipArchive;

use crate::{
    models::{AsrGpuRuntimeInfo, AsrGpuRuntimeProgressEvent},
    settings::AppPaths,
};

pub const RUNTIME_ID: &str = "cuda12-cudnn9";
const PYPI_API: &str = "https://pypi.org/pypi";
const RUNTIME_FILES: &[&str] = &[
    "cublas64_12.dll",
    "cublasLt64_12.dll",
    "cudnn64_9.dll",
    "cudnn_adv64_9.dll",
    "cudnn_cnn64_9.dll",
    "cudnn_engines_precompiled64_9.dll",
    "cudnn_engines_runtime_compiled64_9.dll",
    "cudnn_graph64_9.dll",
    "cudnn_heuristic64_9.dll",
    "cudnn_ops64_9.dll",
];
struct PackageSpec {
    name: &'static str,
    version: &'static str,
    sha256: &'static str,
}

const PACKAGES: &[PackageSpec] = &[
    PackageSpec {
        name: "nvidia-cublas-cu12",
        version: "12.9.2.10",
        sha256: "623f43027d40d44ceadf0043f002bd25cf353e8f13ce90b9a87057019f560661",
    },
    PackageSpec {
        name: "nvidia-cudnn-cu12",
        version: "9.10.2.21",
        sha256: "c6288de7d63e6cf62988f0923f96dc339cea362decb1bf5b3141883392a7d65e",
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
            return Err("GPU 运行库已在下载中".to_string());
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

#[derive(Debug, Deserialize)]
struct PackageMetadata {
    urls: Vec<PackageFile>,
}

#[derive(Debug, Deserialize)]
struct PackageFile {
    filename: String,
    url: String,
    size: u64,
    digests: PackageDigests,
}

#[derive(Debug, Deserialize)]
struct PackageDigests {
    sha256: String,
}

pub fn runtime_dir(paths: &AppPaths) -> PathBuf {
    paths.data_dir.join("runtimes").join(RUNTIME_ID)
}

fn download_dir(paths: &AppPaths) -> PathBuf {
    paths.data_dir.join("runtimes").join(".downloads")
}

fn staging_dir(paths: &AppPaths) -> PathBuf {
    paths
        .data_dir
        .join("runtimes")
        .join(format!(".{RUNTIME_ID}.partial"))
}

pub fn runtime_available(paths: &AppPaths) -> bool {
    let dir = runtime_dir(paths);
    RUNTIME_FILES.iter().all(|file| dir.join(file).is_file())
}

pub async fn status(
    paths: &AppPaths,
    downloads: &DownloadRegistry,
    external_available: bool,
) -> AsrGpuRuntimeInfo {
    let active = downloads.is_active(RUNTIME_ID).await;
    let status = if active {
        "downloading"
    } else if runtime_available(paths) || external_available {
        "available"
    } else {
        "not_installed"
    };
    AsrGpuRuntimeInfo {
        id: RUNTIME_ID.to_string(),
        status: status.to_string(),
        downloaded_bytes: partial_download_size(paths),
        total_bytes: 0,
        error: None,
    }
}

pub async fn download(
    app: AppHandle,
    paths: AppPaths,
    downloads: DownloadRegistry,
) -> Result<(), String> {
    let cancel = downloads.begin(RUNTIME_ID).await?;
    let result = download_inner(&app, &paths, &cancel).await;
    downloads.finish(RUNTIME_ID).await;
    if let Err(error) = &result {
        emit_progress(&app, "failed", 0, 0, Some(error.clone()));
    }
    result
}

async fn download_inner(
    app: &AppHandle,
    paths: &AppPaths,
    cancel: &AtomicBool,
) -> Result<(), String> {
    paths.ensure()?;
    let downloads = download_dir(paths);
    let staging = staging_dir(paths);
    fs::create_dir_all(&downloads).map_err(|error| error.to_string())?;
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let assets = fetch_assets(&client).await?;
    let total = assets.iter().map(|asset| asset.size).sum::<u64>();
    let mut completed = 0_u64;
    emit_progress(app, "downloading", 0, total, None);

    for asset in assets {
        if cancel.load(Ordering::Relaxed) {
            return Err("GPU 运行库下载已取消，可稍后继续".to_string());
        }
        let part = downloads.join(format!("{}.part", asset.filename));
        let existing = fs::metadata(&part)
            .map(|metadata| metadata.len().min(asset.size))
            .unwrap_or(0);
        if !verify_sha256(&part, &asset.digests.sha256).unwrap_or(false) {
            let base = completed;
            download_file(&client, &asset.url, &part, asset.size, cancel, |current| {
                emit_progress(app, "downloading", base + current, total, None);
            })
            .await?;
        } else if existing != asset.size {
            return Err(format!("{} 的下载文件大小不完整", asset.filename));
        }
        if !verify_sha256(&part, &asset.digests.sha256)? {
            return Err(format!("{} 的 SHA-256 校验失败", asset.filename));
        }
        extract_runtime_files(&part, &staging)?;
        completed += asset.size;
        emit_progress(app, "downloading", completed, total, None);
    }

    let missing = RUNTIME_FILES
        .iter()
        .filter(|file| !staging.join(file).is_file())
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("GPU 运行库包缺少文件：{}", missing.join(", ")));
    }

    let target = runtime_dir(paths);
    if target.exists() {
        fs::remove_dir_all(&target).map_err(|error| format!("无法替换旧 GPU 运行库：{error}"))?;
    }
    fs::rename(&staging, &target).map_err(|error| error.to_string())?;
    emit_progress(app, "available", total, total, None);
    Ok(())
}

async fn fetch_assets(client: &reqwest::Client) -> Result<Vec<PackageFile>, String> {
    let mut assets = Vec::with_capacity(PACKAGES.len());
    for package in PACKAGES {
        let url = format!("{PYPI_API}/{}/{}/json", package.name, package.version);
        let metadata = client
            .get(url)
            .send()
            .await
            .map_err(|error| format!("获取 {} 运行库信息失败：{error}", package.name))?
            .error_for_status()
            .map_err(|error| format!("获取 {} 运行库信息失败：{error}", package.name))?
            .json::<PackageMetadata>()
            .await
            .map_err(|error| format!("解析 {} 运行库信息失败：{error}", package.name))?;
        let asset = metadata
            .urls
            .into_iter()
            .find(|file| file.filename.ends_with("win_amd64.whl"))
            .ok_or_else(|| format!("{} 没有可用的 Windows x64 运行库包", package.name))?;
        if !asset.digests.sha256.eq_ignore_ascii_case(package.sha256) {
            return Err(format!("{} 元数据 SHA-256 与内置版本不符", package.name));
        }
        assets.push(asset);
    }
    Ok(assets)
}

fn extract_runtime_files(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("读取 GPU 运行库包失败：{error}"))?;
    let wanted = RUNTIME_FILES.iter().copied().collect::<HashSet<_>>();
    let mut extracted = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("读取 GPU 运行库文件失败：{error}"))?;
        if entry.is_dir() {
            continue;
        }
        let normalized_name = entry.name().replace('\\', "/");
        let Some(name) = normalized_name.rsplit('/').next() else {
            continue;
        };
        if !wanted.contains(name) {
            continue;
        }
        let target = destination.join(name);
        let mut output = File::create(&target).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
        output.flush().map_err(|error| error.to_string())?;
        extracted.insert(name.to_string());
    }
    if extracted.is_empty() {
        return Err(format!("运行库包 {} 未找到 DLL", archive_path.display()));
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
    let existing = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
        .min(expected);
    let mut request = client.get(url);
    if existing > 0 {
        request = request.header(RANGE, format!("bytes={existing}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("下载 GPU 运行库失败：{error}"))?;
    let append = existing > 0 && response.status() == StatusCode::PARTIAL_CONTENT;
    if !response.status().is_success() {
        return Err(format!("GPU 运行库下载返回 HTTP {}", response.status()));
    }
    let mut output = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(|error| error.to_string())?;
    let mut current = if append { existing } else { 0 };
    let mut stream = response.bytes_stream();
    loop {
        let next = tokio::time::timeout(std::time::Duration::from_secs(45), stream.next())
            .await
            .map_err(|_| "GPU 运行库 45 秒未返回数据".to_string())?;
        let Some(chunk) = next else {
            break;
        };
        if cancel.load(Ordering::Relaxed) {
            return Err("GPU 运行库下载已取消，可稍后继续".to_string());
        }
        let chunk = chunk.map_err(|error| error.to_string())?;
        output
            .write_all(&chunk)
            .map_err(|error| error.to_string())?;
        current += chunk.len() as u64;
        progress(current);
    }
    output.flush().map_err(|error| error.to_string())?;
    if current != expected {
        return Err(format!(
            "GPU 运行库文件大小不符：需要 {expected}，实际 {current}"
        ));
    }
    Ok(())
}

fn partial_download_size(paths: &AppPaths) -> u64 {
    fs::read_dir(download_dir(paths))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "part")
                .then_some(entry.path())
        })
        .filter_map(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .sum()
}

fn verify_sha256(path: &Path, expected: &str) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()) == expected)
}

fn emit_progress(
    app: &AppHandle,
    status: &str,
    downloaded_bytes: u64,
    total_bytes: u64,
    error: Option<String>,
) {
    let _ = app.emit(
        "gpu-runtime:progress",
        AsrGpuRuntimeProgressEvent {
            id: RUNTIME_ID.to_string(),
            status: status.to_string(),
            downloaded_bytes,
            total_bytes,
            error,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_manifest_contains_full_encoder_runtime() {
        assert_eq!(RUNTIME_FILES.len(), 10);
        assert!(RUNTIME_FILES.contains(&"cudnn_ops64_9.dll"));
        assert!(RUNTIME_FILES.contains(&"cublasLt64_12.dll"));
    }
}
