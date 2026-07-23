use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

use crate::{
    models::{CaptionSourceConfig, VadParameters},
    settings::AppPaths,
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Debug, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum WorkerCommand<'a> {
    Load {
        model_id: &'a str,
        model_path: &'a Path,
        device: &'a str,
        compute_type: &'a str,
        vad: VadParameters,
    },
    Configure {
        vad: VadParameters,
    },
    DryRun,
    ProbeDependencies,
    Start,
    Stop,
    ResetRouting,
    Shutdown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEvent {
    LoadingProgress {
        progress: f32,
        message: String,
    },
    Ready {
        model_id: String,
        device: String,
    },
    Partial {
        text: String,
        start_ms: u64,
        end_ms: u64,
        latency_ms: u64,
        model_id: String,
    },
    Final {
        text: String,
        start_ms: u64,
        end_ms: u64,
        latency_ms: u64,
        model_id: String,
    },
    Health {
        healthy: bool,
        latency_ms: Option<u64>,
        detail: String,
    },
    DependencyProbe {
        device_count: u32,
        compute_types: Vec<String>,
        #[serde(default)]
        ctranslate2_version: Option<String>,
        #[serde(default)]
        cuda_runtime_loaded: Option<bool>,
        #[serde(default)]
        cudnn_runtime_loaded: Option<bool>,
        #[serde(default)]
        cuda_error: Option<String>,
        #[serde(default)]
        cudnn_error: Option<String>,
    },
    Error {
        code: String,
        message: String,
        recoverable: bool,
    },
    Unloaded,
    Shutdown,
}

#[derive(Debug)]
pub struct AsrDependencyProbe {
    pub device_count: u32,
    pub compute_types: Vec<String>,
    pub ctranslate2_version: Option<String>,
    pub cuda_runtime_loaded: Option<bool>,
    pub cudnn_runtime_loaded: Option<bool>,
    pub cuda_error: Option<String>,
    pub cudnn_error: Option<String>,
}

pub struct AsrWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl AsrWorker {
    pub async fn spawn(paths: &AppPaths) -> Result<Self, String> {
        let executable = worker_executable(paths)?;
        let stderr_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(paths.logs_dir.join("asr-worker.stderr.log"))
            .map_err(|error| format!("无法打开 ASR Worker 日志：{error}"))?;
        let mut command = Command::new(&executable);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr_file))
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8");
        let runtime_dirs = crate::asr_dependencies::runtime_library_dirs();
        if !runtime_dirs.is_empty() {
            let mut path_entries = runtime_dirs;
            if let Some(current_path) = std::env::var_os("PATH") {
                path_entries.extend(std::env::split_paths(&current_path));
            }
            if let Ok(worker_path) = std::env::join_paths(path_entries) {
                command.env("PATH", worker_path);
            }
        }
        command.kill_on_drop(true);
        #[cfg(windows)]
        command.as_std_mut().creation_flags(0x08000000);
        let mut child = command
            .spawn()
            .map_err(|e| format!("无法启动 ASR Worker（{}）：{e}", executable.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "ASR Worker stdin 不可用".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "ASR Worker stdout 不可用".to_string())?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    async fn send(&mut self, command: WorkerCommand<'_>) -> Result<(), String> {
        let mut json = serde_json::to_vec(&command).map_err(|e| e.to_string())?;
        json.push(b'\n');
        self.stdin
            .write_all(&json)
            .await
            .map_err(|e| e.to_string())?;
        self.stdin.flush().await.map_err(|e| e.to_string())
    }

    pub async fn load(
        &mut self,
        paths: &AppPaths,
        source: &CaptionSourceConfig,
    ) -> Result<(), String> {
        let CaptionSourceConfig::LocalAsr {
            model_id,
            device,
            compute_type,
            vad_profile,
            channel_mode,
            channel_switch_sensitivity,
            suppress_non_speech_segments,
        } = source
        else {
            return Err("不是本地 ASR 来源".to_string());
        };
        self.send(WorkerCommand::Load {
            model_id,
            model_path: &paths.models_dir.join(model_id),
            device,
            compute_type,
            vad: vad_profile.parameters(
                *channel_mode,
                *channel_switch_sensitivity,
                *suppress_non_speech_segments,
            ),
        })
        .await?;
        loop {
            match tokio::time::timeout(Duration::from_secs(60), self.next())
                .await
                .map_err(|_| "ASR 模型加载超时".to_string())??
            {
                WorkerEvent::Ready {
                    model_id: ready_id,
                    device: ready_device,
                } if ready_id.as_str() == model_id.as_str()
                    && ready_device.as_str() == device.as_str() =>
                {
                    return Ok(())
                }
                WorkerEvent::Ready {
                    model_id: ready_id,
                    device: ready_device,
                } => {
                    return Err(format!(
                        "Worker 加载了意外模型：{ready_id} ({ready_device})"
                    ))
                }
                WorkerEvent::Error { message, .. } => return Err(message),
                _ => {}
            }
        }
    }

    pub async fn dry_run(&mut self) -> Result<u64, String> {
        self.send(WorkerCommand::DryRun).await?;
        loop {
            match tokio::time::timeout(Duration::from_secs(30), self.next())
                .await
                .map_err(|_| "ASR dry-run 超时".to_string())??
            {
                WorkerEvent::Health {
                    healthy: true,
                    latency_ms,
                    detail,
                } if detail == "dry_run_ok" || detail == "gpu_encoder_dry_run_ok" => {
                    return Ok(latency_ms.unwrap_or(0))
                }
                WorkerEvent::Error { message, .. } => return Err(message),
                _ => {}
            }
        }
    }

    pub async fn probe_dependencies(&mut self) -> Result<AsrDependencyProbe, String> {
        self.send(WorkerCommand::ProbeDependencies).await?;
        loop {
            match tokio::time::timeout(Duration::from_secs(30), self.next())
                .await
                .map_err(|_| "ASR 运行环境检查超时".to_string())??
            {
                WorkerEvent::DependencyProbe {
                    device_count,
                    compute_types,
                    ctranslate2_version,
                    cuda_runtime_loaded,
                    cudnn_runtime_loaded,
                    cuda_error,
                    cudnn_error,
                } => {
                    return Ok(AsrDependencyProbe {
                        device_count,
                        compute_types,
                        ctranslate2_version,
                        cuda_runtime_loaded,
                        cudnn_runtime_loaded,
                        cuda_error,
                        cudnn_error,
                    })
                }
                WorkerEvent::Error { message, .. } => return Err(message),
                _ => {}
            }
        }
    }

    pub async fn start(&mut self) -> Result<(), String> {
        self.send(WorkerCommand::Start).await
    }
    pub async fn stop(&mut self) -> Result<(), String> {
        self.send(WorkerCommand::Stop).await
    }

    pub async fn configure(&mut self, source: &CaptionSourceConfig) -> Result<(), String> {
        let CaptionSourceConfig::LocalAsr {
            vad_profile,
            channel_mode,
            channel_switch_sensitivity,
            suppress_non_speech_segments,
            ..
        } = source
        else {
            return Err("不是本地 ASR 来源".to_string());
        };
        self.send(WorkerCommand::Configure {
            vad: vad_profile.parameters(
                *channel_mode,
                *channel_switch_sensitivity,
                *suppress_non_speech_segments,
            ),
        })
        .await
    }
    pub async fn reset_routing(&mut self) -> Result<(), String> {
        self.send(WorkerCommand::ResetRouting).await
    }

    pub async fn next(&mut self) -> Result<WorkerEvent, String> {
        loop {
            let mut raw = Vec::new();
            let read = self
                .stdout
                .read_until(b'\n', &mut raw)
                .await
                .map_err(|error| format!("读取 ASR Worker 输出失败：{error}"))?;
            if read == 0 {
                return Err("ASR Worker 已退出；请查看 asr-worker.stderr.log".to_string());
            }
            // Native libraries can write diagnostics directly to stdout using
            // the Windows active code page. Ignore those lines and decode only
            // NDJSON protocol messages, which always contain a `type` field.
            if let Some(event) = decode_protocol_line(&raw)? {
                return Ok(event);
            }
        }
    }

    pub async fn shutdown(mut self) {
        let _ = self.send(WorkerCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await;
        let _ = self.child.kill().await;
    }
}

fn decode_protocol_line(raw: &[u8]) -> Result<Option<WorkerEvent>, String> {
    if !raw.windows(6).any(|window| window == b"\"type\"") {
        return Ok(None);
    }
    let start = raw.iter().position(|byte| *byte == b'{').unwrap_or(0);
    let protocol = std::str::from_utf8(&raw[start..]).map_err(|error| {
        format!("ASR Worker 协议编码异常（{error}）；请查看 asr-worker.stderr.log")
    })?;
    serde_json::from_str(protocol.trim_end())
        .map(Some)
        .map_err(|error| format!("无效 ASR Worker 协议：{error}"))
}

pub(crate) fn worker_executable(paths: &AppPaths) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("LIVECAPTION_ASR_WORKER").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
    }
    let installed = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .unwrap_or(Path::new("."))
        .join("asr-worker")
        .join("livecaption-asr-worker.exe");
    if installed.is_file() {
        return Ok(installed);
    }
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("worker")
        .join("dist")
        .join("livecaption-asr-worker")
        .join("livecaption-asr-worker.exe");
    if local.is_file() {
        return Ok(local);
    }
    Err(format!(
        "未找到随应用分发的 ASR Worker。模型目录：{}",
        paths.models_dir.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_non_utf8_native_diagnostics() {
        assert!(decode_protocol_line(b"native log: \x81\x8d\n")
            .unwrap()
            .is_none());
    }

    #[test]
    fn decodes_ascii_escaped_japanese_protocol() {
        let event = decode_protocol_line(
            br#"{"type":"partial","text":"\u3053\u3093\u306b\u3061\u306f","start_ms":0,"end_ms":100,"latency_ms":8,"model_id":"kotoba"}
"#,
        )
        .unwrap()
        .unwrap();
        match event {
            WorkerEvent::Partial { text, .. } => assert_eq!(text, "こんにちは"),
            _ => panic!("unexpected worker event"),
        }
    }
}
