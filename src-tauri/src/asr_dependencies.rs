use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    asr_worker::{worker_executable, AsrWorker},
    models::{AsrDependencyReport, AsrDependencyStatus},
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
    "cudnn_engines_tensor_ir64_9.dll",
    "cudnn_ext64_9.dll",
    "cudnn_graph64_9.dll",
    "cudnn_heuristic64_9.dll",
    "cudnn_ops64_9.dll",
];

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
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join("asr-worker").join("_internal"));
        }
    }

    deduplicate_existing(candidates)
}

pub async fn check(paths: &AppPaths) -> AsrDependencyReport {
    let worker_path = worker_executable(paths).ok();
    let driver_path = driver_path();
    let runtime_dirs = runtime_library_dirs();
    let cuda_path = find_complete_dir(&runtime_dirs, CUDA_FILES);
    let cudnn_path = find_complete_dir(&runtime_dirs, CUDNN_FILES);

    let mut dependencies = vec![
        status(
            "worker",
            "ASR Worker",
            worker_path.is_some(),
            if worker_path.is_some() {
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
            cuda_path.is_some(),
            if cuda_path.is_some() {
                "已找到 cublas64_12.dll 与 cublasLt64_12.dll".to_string()
            } else {
                "缺少 CUDA 12 cuBLAS 运行库".to_string()
            },
            cuda_path,
            CUDA_URL,
        ),
        status(
            "cudnn_9",
            "cuDNN 9 运行库",
            cudnn_path.is_some(),
            if cudnn_path.is_some() {
                "cuDNN 9 拆分运行库完整".to_string()
            } else {
                "缺少一个或多个 cuDNN 9 运行库文件".to_string()
            },
            cudnn_path,
            CUDNN_URL,
        ),
    ];

    let static_ready = dependencies.iter().all(|item| item.installed);
    let (probe_ready, probe_detail) = if static_ready {
        match AsrWorker::spawn(paths).await {
            Ok(mut worker) => {
                let result = worker.probe_dependencies().await;
                worker.shutdown().await;
                match result {
                    Ok((count, compute_types)) if count > 0 => (
                        true,
                        format!(
                            "检测到 {count} 个 CUDA 设备；支持 {}",
                            compute_types.join("、")
                        ),
                    ),
                    Ok(_) => (false, "CTranslate2 未检测到可用的 CUDA 设备".to_string()),
                    Err(error) => (false, format!("CTranslate2 加载失败：{error}")),
                }
            }
            Err(error) => (false, format!("ASR Worker 启动失败：{error}")),
        }
    } else {
        (false, "请先补齐上方依赖，再执行 GPU 可用性探测".to_string())
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
}
