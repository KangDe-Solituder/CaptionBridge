use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    asr_worker::{worker_executable, AsrDependencyProbe, AsrWorker},
    gpu_runtime,
    models::{
        AsrDependencyReport, AsrDependencyStatus, AudioChannelMode, CaptionSourceConfig,
        ChannelSwitchSensitivity, VadProfile,
    },
    settings::AppPaths,
};

const WORKER_URL: &str = "https://github.com/KangDe-Solituder/CaptionBridge/releases";
const DRIVER_URL: &str = "https://www.nvidia.com/en-us/drivers/";
const CUDA_URL: &str = "https://developer.nvidia.com/cuda-toolkit-archive";
const CUDNN_URL: &str = "https://developer.nvidia.com/cudnn-downloads";
const CUDA_FILES: &[&str] = &["cublas64_12.dll", "cublasLt64_12.dll"];
const CUDNN_FILES: &[&str] = &[
    "cudnn64_9.dll",
    "cudnn_adv64_9.dll",
    "cudnn_cnn64_9.dll",
    "cudnn_engines_precompiled64_9.dll",
    "cudnn_engines_runtime_compiled64_9.dll",
    "cudnn_graph64_9.dll",
    "cudnn_heuristic64_9.dll",
    "cudnn_ops64_9.dll",
];
const MODEL_IDS: &[&str] = &["kotoba-whisper-v2.0-faster", "whisper-large-v3-turbo"];

#[derive(Debug)]
enum DependencyProbeOutcome {
    Lightweight(AsrDependencyProbe),
    ModelDryRun {
        model_id: String,
        latency_ms: u64,
    },
    Verified {
        probe: AsrDependencyProbe,
        model_id: String,
        latency_ms: u64,
    },
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

pub fn runtime_library_dirs(paths: &AppPaths) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(gpu_runtime::runtime_dir(paths));
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

    #[cfg(debug_assertions)]
    {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let worker_root = manifest.join("worker");
        let build_venv = worker_root.join(".venv-build");
        add_python_runtime_dirs(&mut candidates, &build_venv);
        // Development environments may keep optional GPU wheels outside the
        // bundled Worker. Release builds intentionally never search these
        // source-tree paths, otherwise a packaged Worker can mix DLL versions.
        for variable in ["VIRTUAL_ENV", "CONDA_PREFIX", "PYTHONHOME"] {
            if let Some(root) = std::env::var_os(variable) {
                add_python_runtime_dirs(&mut candidates, Path::new(&root));
            }
        }
        collect_dirs_containing(&worker_root, "cublas64_12.dll", 8, &mut candidates);
        collect_dirs_containing(&worker_root, "cudnn64_9.dll", 8, &mut candidates);
        candidates.push(
            worker_root
                .join("dist")
                .join("livecaption-asr-worker")
                .join("_internal"),
        );
        candidates.push(
            worker_root
                .join("dist")
                .join("livecaption-asr-worker")
                .join("_internal")
                .join("ctranslate2"),
        );
    }
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

#[cfg(debug_assertions)]
fn add_python_runtime_dirs(output: &mut Vec<PathBuf>, environment: &Path) {
    let site_packages = environment.join("Lib").join("site-packages");
    output.push(site_packages.join("nvidia").join("cublas").join("bin"));
    output.push(site_packages.join("nvidia").join("cudnn").join("bin"));
    output.push(site_packages.join("ctranslate2"));
}

pub async fn check(
    paths: &AppPaths,
    configured_source: Option<&CaptionSourceConfig>,
    downloads: &gpu_runtime::DownloadRegistry,
) -> AsrDependencyReport {
    let worker_path = worker_executable(paths).ok();
    let driver_path = driver_path();
    let runtime_dirs = runtime_library_dirs(paths);
    let cuda_path = find_complete_dir(&runtime_dirs, CUDA_FILES);
    let cudnn_path = find_complete_dir(&runtime_dirs, CUDNN_FILES);

    let probe_result: Result<DependencyProbeOutcome, String> = if worker_path.is_some() {
        match AsrWorker::spawn(paths).await {
            Ok(mut worker) => {
                let result = match worker.probe_dependencies().await {
                    Ok(probe) => match legacy_probe_source(paths, configured_source) {
                        Some(source) => {
                            let model_id = source.model_id().unwrap_or("未知模型").to_string();
                            match worker.load(paths, &source).await {
                                Ok(()) => worker.dry_run().await.map(|latency_ms| {
                                    DependencyProbeOutcome::Verified {
                                        probe,
                                        model_id,
                                        latency_ms,
                                    }
                                }),
                                Err(error) => Err(format!("真实 ASR 模型验证失败：{error}")),
                            }
                        }
                        None => Ok(DependencyProbeOutcome::Lightweight(probe)),
                    },
                    Err(probe_error) => match legacy_probe_source(paths, configured_source) {
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
                                    "依赖轻量探测失败：{probe_error}；真实 ASR 模型验证也失败：{load_error}"
                                )),
                            }
                        }
                        None if is_unsupported_probe(&probe_error) => Err(
                            "旧版 Worker 不支持轻量探测，且没有已安装模型可用于真实 ASR 验证"
                                .to_string(),
                        ),
                        None => Err(probe_error),
                    },
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
    let model_validated = matches!(
        &probe_result,
        Ok(DependencyProbeOutcome::ModelDryRun { .. })
            | Ok(DependencyProbeOutcome::Verified { .. })
    );

    let mut dependencies = vec![
        status(
            "worker",
            "ASR Worker",
            worker_path.is_some(),
            if model_validated {
                "识别进程已安装；已通过真实 GPU encoder 推理验证".to_string()
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
        Ok(DependencyProbeOutcome::Verified {
            probe,
            model_id,
            latency_ms,
        }) => (
            true,
            format!(
                "CTranslate2 {} 检测到 {} 个 CUDA 设备；已用 {model_id} 完成真实 GPU encoder 推理（{latency_ms} ms）；支持 {}",
                probe.ctranslate2_version.as_deref().unwrap_or("未知版本"),
                probe.device_count,
                probe.compute_types.join("、")
            ),
        ),
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

    let externally_loaded = probe_result
        .as_ref()
        .ok()
        .map(|outcome| {
            runtime_loaded_cuda(outcome) == Some(true)
                && runtime_loaded_cudnn(outcome) == Some(true)
        })
        .unwrap_or(false);
    let runtime_status = gpu_runtime::status(paths, downloads, externally_loaded).await;

    AsrDependencyReport {
        ready: dependencies.iter().all(|item| item.installed),
        dependencies,
        gpu_runtime: runtime_status,
    }
}

fn runtime_ready(probed: Option<bool>, found_on_disk: bool) -> bool {
    probed.unwrap_or(found_on_disk)
}

fn runtime_loaded_cuda(outcome: &DependencyProbeOutcome) -> Option<bool> {
    match outcome {
        DependencyProbeOutcome::Lightweight(probe) => probe.cuda_runtime_loaded,
        DependencyProbeOutcome::ModelDryRun { .. } | DependencyProbeOutcome::Verified { .. } => {
            Some(true)
        }
    }
}

fn runtime_loaded_cudnn(outcome: &DependencyProbeOutcome) -> Option<bool> {
    match outcome {
        DependencyProbeOutcome::Lightweight(probe) => probe.cudnn_runtime_loaded,
        DependencyProbeOutcome::ModelDryRun { .. } | DependencyProbeOutcome::Verified { .. } => {
            Some(true)
        }
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
            DependencyProbeOutcome::ModelDryRun { .. }
            | DependencyProbeOutcome::Verified { .. } => {
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
                channel_mode: AudioChannelMode::Auto,
                channel_switch_sensitivity: ChannelSwitchSensitivity::Standard,
                suppress_non_speech_segments: true,
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
    #[cfg(not(debug_assertions))]
    fn release_runtime_search_never_uses_source_worker_directories() {
        let paths = AppPaths::new(
            std::env::temp_dir().join(format!("runtime-paths-{}", uuid::Uuid::new_v4())),
        );
        let source_worker = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("worker");
        assert!(runtime_library_dirs(&paths)
            .iter()
            .all(|candidate| !candidate.starts_with(&source_worker)));
    }

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
        assert_eq!(CUDNN_FILES.len(), 8);
        assert!(CUDNN_FILES.contains(&"cudnn_ops64_9.dll"));
        assert!(CUDNN_FILES.contains(&"cudnn_graph64_9.dll"));
    }

    #[test]
    fn successful_model_inference_overrides_lightweight_runtime_flags() {
        let outcome = DependencyProbeOutcome::Verified {
            probe: AsrDependencyProbe {
                device_count: 1,
                compute_types: vec!["int8_float16".to_string()],
                ctranslate2_version: Some("test".to_string()),
                cuda_runtime_loaded: None,
                cudnn_runtime_loaded: Some(false),
                cuda_error: None,
                cudnn_error: Some("legacy probe mismatch".to_string()),
            },
            model_id: "test-model".to_string(),
            latency_ms: 42,
        };
        assert_eq!(runtime_loaded_cuda(&outcome), Some(true));
        assert_eq!(runtime_loaded_cudnn(&outcome), Some(true));
        assert_eq!(
            runtime_detail(&Ok(outcome), false, false, "loaded", "found", "missing"),
            "loaded（已由真实模型推理验证）"
        );
    }

    #[test]
    fn recognizes_legacy_worker_probe_response() {
        assert!(is_unsupported_probe("unknown_command:probe_dependencies"));
        assert!(!is_unsupported_probe("CUDA driver unavailable"));
    }
}
