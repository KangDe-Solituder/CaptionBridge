use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    asr_worker::{worker_executable, AsrDependencyProbe, AsrWorker},
    models::{AsrDependencyReport, AsrDependencyStatus, CaptionSourceConfig, VadProfile},
    settings::AppPaths,
};

const WORKER_URL: &str = "https://github.com/KangDe-Solituder/CaptionBridge/releases";
const DRIVER_URL: &str = "https://www.nvidia.com/en-us/drivers/";
const CUDA_URL: &str = "https://developer.nvidia.com/cuda-toolkit-archive";
const CUDNN_URL: &str = "https://developer.nvidia.com/cudnn-downloads";
const CUDA_FILES: &[&str] = &["cublas64_12.dll", "cublasLt64_12.dll"];
const CUDNN_FILES: &[&str] = &["cudnn64_9.dll"];
const MODEL_IDS: &[&str] = &["kotoba-whisper-v2.0-faster", "whisper-large-v3-turbo"];

#[derive(Debug)]
enum DependencyProbeOutcome {
    Lightweight(AsrDependencyProbe),
    ModelDryRun { model_id: String, latency_ms: u64 },
}

pub fn official_url(id: &str) -> Option<&'static str> {
    match id {
        "worker" => Some(WORKER_URL),
        "nvidia_driver" | "gpu_probe" => Some(DRIVER_URL),
        "cuda_12" => Some(CUDA_URL),
        "cudnn_9" => Some(CUDNN_URL),
        _ => None,
    }
}

pub fn runtime_library_dirs() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path));
    }
    if let Some(cuda_path) = std::env::var_os("CUDA_PATH") {
        candidates.push(PathBuf::from(cuda_path).join("bin"));
    }

    let cuda_root = PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA");
    if let Ok(entries) = fs::read_dir(cuda_root) {
        let mut versions = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("bin"))
            .collect::<Vec<_>>();
        versions.sort_by(|left, right| right.cmp(left));
        candidates.extend(versions);
    }

    collect_dirs_containing(
        Path::new(r"C:\Program Files\NVIDIA\CUDNN"),
        "cudnn64_9.dll",
        6,
        &mut candidates,
    );

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(
        manifest
            .join("worker")
            .join(".venv-build")
            .join("Lib")
            .join("site-packages")
            .join("nvidia")
            .join("cudnn")
            .join("bin"),
    );
    candidates.push(
        manifest
            .join("worker")
            .join("dist")
            .join("livecaption-asr-worker")
            .join("_internal"),
    );
    candidates.push(
        manifest
            .join("worker")
            .join("dist")
            .join("livecaption-asr-worker")
            .join("_internal")
            .join("ctranslate2"),
    );
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join("asr-worker").join("_internal"));
            candidates.push(
                parent
                    .join("asr-worker")
                    .join("_internal")
                    .join("ctranslate2"),
            );
        }
    }

    deduplicate_existing(candidates)
}

pub async fn check(
    paths: &AppPaths,
    configured_source: Option<&CaptionSourceConfig>,
) -> AsrDependencyReport {
    let worker_path = worker_executable(paths).ok();
    let driver_path = driver_path();
    let runtime_dirs = runtime_library_dirs();
    let cuda_path = find_complete_dir(&runtime_dirs, CUDA_FILES);
    let cudnn_path = find_complete_dir(&runtime_dirs, CUDNN_FILES);

    let probe_result: Result<DependencyProbeOutcome, String> = if worker_path.is_some() {
        match AsrWorker::spawn(paths).await {
            Ok(mut worker) => {
                let result = match worker.probe_dependencies().await {
                    Ok(probe) => Ok(DependencyProbeOutcome::Lightweight(probe)),
                    Err(error) if is_unsupported_probe(&error) => {
                        match legacy_probe_source(paths, configured_source) {
                            Some(source) => {
                                let model_id = source.model_id().unwrap_or("未知模型").to_string();
                                match worker.load(paths, &source).await {
                                    Ok(()) => worker.dry_run().await.map(|latency_ms| {
                                        DependencyProbeOutcome::ModelDryRun {
                                            model_id,
                                            latency_ms,
                                        }
                                    }),
                                    Err(load_error) => Err(format!(
                                        "旧版 Worker 不支持轻量探测，模型兼容性验证失败：{load_error}"
                                    )),
                                }
                            }
                            None => Err(
                                "旧版 Worker 不支持轻量探测，且没有已安装模型可用于兼容性验证"
                                    .to_string(),
                            ),
                        }
                    }
                    Err(error) => Err(error),
                };
                worker.shutdown().await;
                result
            }
            Err(error) => Err(format!("ASR Worker 启动失败：{error}")),
        }
    } else {
        Err("未找到 ASR Worker，无法执行运行态探测".to_string())
    };
    let cuda_ready = runtime_ready(
        probe_result.as_ref().ok().and_then(runtime_loaded_cuda),
        cuda_path.is_some(),
    );
    let cudnn_ready = runtime_ready(
        probe_result.as_ref().ok().and_then(runtime_loaded_cudnn),
        cudnn_path.is_some(),
    );
    let legacy_validated = matches!(
        &probe_result,
        Ok(DependencyProbeOutcome::ModelDryRun { .. })
    );

    let mut dependencies = vec![
        status(
            "worker",
            "ASR Worker",
            worker_path.is_some(),
            if legacy_validated {
                "识别进程已安装；旧版诊断协议已通过真实模型推理兼容验证".to_string()
            } else if worker_path.is_some() {
                "识别进程已安装".to_string()
            } else {
                "未找到随应用分发的 ASR Worker".to_string()
            },
            worker_path,
            WORKER_URL,
        ),
        status(
            "nvidia_driver",
            "NVIDIA 显卡驱动",
            driver_path.is_some(),
            if driver_path.is_some() {
                "CUDA 驱动接口可用".to_string()
            } else {
                "未找到 nvcuda.dll，请安装适合显卡的 NVIDIA 驱动".to_string()
            },
            driver_path,
            DRIVER_URL,
        ),
        status(
            "cuda_12",
            "CUDA 12 运行库",
            cuda_ready,
            runtime_detail(
                &probe_result,
                true,
                cuda_path.is_some(),
                "Worker 已成功加载 CUDA 12 cuBLAS",
                "已找到 cublas64_12.dll 与 cublasLt64_12.dll",
                "缺少或无法加载 CUDA 12 cuBLAS 运行库",
            ),
            cuda_path,
            CUDA_URL,
        ),
        status(
            "cudnn_9",
            "cuDNN 9 运行库",
            cudnn_ready,
            runtime_detail(
                &probe_result,
                false,
                cudnn_path.is_some(),
                "Worker 已成功加载 cuDNN 9",
                "已找到 cuDNN 9 入口运行库；请用模型测试验证完整推理",
                "缺少或无法加载 cuDNN 9 运行库",
            ),
            cudnn_path,
            CUDNN_URL,
        ),
    ];

    let (probe_ready, probe_detail) = match &probe_result {
        Ok(DependencyProbeOutcome::Lightweight(probe)) if probe.device_count > 0 => (
            true,
            format!(
                "CTranslate2 {} 检测到 {} 个 CUDA 设备；支持 {}",
                probe.ctranslate2_version.as_deref().unwrap_or("未知版本"),
                probe.device_count,
                probe.compute_types.join("、")
            ),
        ),
        Ok(DependencyProbeOutcome::Lightweight(_)) => {
            (false, "CTranslate2 未检测到可用的 CUDA 设备".to_string())
        }
        Ok(DependencyProbeOutcome::ModelDryRun {
            model_id,
            latency_ms,
        }) => (
            true,
            format!(
                "旧版 Worker 不支持轻量探测；已用 {model_id} 完成真实 CUDA dry-run（{latency_ms} ms）"
            ),
        ),
        Err(error) => (false, format!("CTranslate2 加载失败：{error}")),
    };
    dependencies.push(status(
        "gpu_probe",
        "CTranslate2 GPU 探测",
        probe_ready,
        probe_detail,
        None,
        DRIVER_URL,
    ));

    AsrDependencyReport {
        ready: dependencies.iter().all(|item| item.installed),
        dependencies,
    }
}

fn runtime_ready(probed: Option<bool>, found_on_disk: bool) -> bool {
    probed.unwrap_or(found_on_disk)
}

fn runtime_loaded_cuda(outcome: &DependencyProbeOutcome) -> Option<bool> {
    match outcome {
        DependencyProbeOutcome::Lightweight(probe) => probe.cuda_runtime_loaded,
        DependencyProbeOutcome::ModelDryRun { .. } => Some(true),
    }
}

fn runtime_loaded_cudnn(outcome: &DependencyProbeOutcome) -> Option<bool> {
    match outcome {
        DependencyProbeOutcome::Lightweight(probe) => probe.cudnn_runtime_loaded,
        DependencyProbeOutcome::ModelDryRun { .. } => Some(true),
    }
}

fn runtime_detail(
    probe_result: &Result<DependencyProbeOutcome, String>,
    cuda: bool,
    found_on_disk: bool,
    loaded: &str,
    found: &str,
    missing: &str,
) -> String {
    if let Ok(outcome) = probe_result {
        let (state, error) = match outcome {
            DependencyProbeOutcome::Lightweight(probe) if cuda => {
                (probe.cuda_runtime_loaded, probe.cuda_error.as_deref())
            }
            DependencyProbeOutcome::Lightweight(probe) => {
                (probe.cudnn_runtime_loaded, probe.cudnn_error.as_deref())
            }
            DependencyProbeOutcome::ModelDryRun { .. } => {
                return format!("{loaded}（已由真实模型推理验证）")
            }
        };
        if state == Some(true) {
            return loaded.to_string();
        }
        if state == Some(false) {
            return error
                .map(|error| format!("{missing}：{error}"))
                .unwrap_or_else(|| missing.to_string());
        }
    }
    if found_on_disk {
        found.to_string()
    } else {
        missing.to_string()
    }
}

fn is_unsupported_probe(error: &str) -> bool {
    error.contains("unknown_command:probe_dependencies")
}

fn legacy_probe_source(
    paths: &AppPaths,
    configured_source: Option<&CaptionSourceConfig>,
) -> Option<CaptionSourceConfig> {
    if let Some(source @ CaptionSourceConfig::LocalAsr { model_id, .. }) = configured_source {
        if paths.models_dir.join(model_id).join("model.bin").is_file() {
            return Some(source.clone());
        }
    }
    MODEL_IDS.iter().find_map(|model_id| {
        paths
            .models_dir
            .join(model_id)
            .join("model.bin")
            .is_file()
            .then(|| CaptionSourceConfig::LocalAsr {
                model_id: (*model_id).to_string(),
                device: "cuda".to_string(),
                compute_type: "int8_float16".to_string(),
                vad_profile: VadProfile::Normal,
            })
    })
}

fn status(
    id: &str,
    name: &str,
    installed: bool,
    detail: String,
    detected_path: Option<PathBuf>,
    official_url: &str,
) -> AsrDependencyStatus {
    AsrDependencyStatus {
        id: id.to_string(),
        name: name.to_string(),
        installed,
        detail,
        detected_path: detected_path.map(|path| path.to_string_lossy().into_owned()),
        official_url: official_url.to_string(),
    }
}

fn driver_path() -> Option<PathBuf> {
    let system_root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let path = system_root.join("System32").join("nvcuda.dll");
    path.is_file().then_some(path)
}

fn find_complete_dir(dirs: &[PathBuf], files: &[&str]) -> Option<PathBuf> {
    dirs.iter()
        .find(|dir| files.iter().all(|file| dir.join(file).is_file()))
        .cloned()
}

fn collect_dirs_containing(root: &Path, filename: &str, depth: usize, output: &mut Vec<PathBuf>) {
    if depth == 0 || !root.is_dir() {
        return;
    }
    if root.join(filename).is_file() {
        output.push(root.to_path_buf());
        return;
    }
    if let Ok(entries) = fs::read_dir(root) {
        for child in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            if child.is_dir() {
                collect_dirs_containing(&child, filename, depth - 1, output);
            }
        }
    }
}

fn deduplicate_existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| path.is_dir())
        .filter(|path| seen.insert(path.to_string_lossy().to_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whitelists_known_official_links() {
        assert_eq!(official_url("cuda_12"), Some(CUDA_URL));
        assert_eq!(official_url("cudnn_9"), Some(CUDNN_URL));
        assert_eq!(official_url("unknown"), None);
    }

    #[test]
    fn runtime_probe_takes_precedence_over_file_presence() {
        assert!(runtime_ready(Some(true), false));
        assert!(!runtime_ready(Some(false), true));
        assert!(runtime_ready(None, true));
        assert_eq!(CUDNN_FILES, &["cudnn64_9.dll"]);
    }

    #[test]
    fn recognizes_legacy_worker_probe_response() {
        assert!(is_unsupported_probe("unknown_command:probe_dependencies"));
        assert!(!is_unsupported_probe("CUDA driver unavailable"));
    }
}
