#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod asr_worker;
mod captions;
mod llm;
mod model_manager;
mod models;
mod native_caption_reader;
mod secrets;
mod segmenter;
mod sessions;
mod settings;
mod windows_integration;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, RwLock,
    },
};

use captions::CaptionController;
use models::{
    AppSettings, CaptionExportEntry, CaptionRuntimeState, CaptionSourceConfig, CaptionSourceHealth,
    ModelInfo, RuntimeLogEntry, RuntimeStatus, SettingsSaveRequest, SettingsView, TranslationMode,
    TranslationRequest, TranslationResult,
};
use settings::AppPaths;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub paths: AppPaths,
    pub settings: Arc<RwLock<AppSettings>>,
    pub caption: Arc<Mutex<Option<CaptionController>>>,
    pub caption_state: Arc<RwLock<CaptionRuntimeState>>,
    pub caption_exports: Arc<RwLock<Vec<CaptionExportEntry>>>,
    pub selection_hotkey_registered: Arc<AtomicBool>,
    pub last_error: Arc<RwLock<Option<String>>>,
    pub source_health: Arc<RwLock<CaptionSourceHealth>>,
    pub last_caption_update_ms: Arc<AtomicU64>,
    pub reader_restarts: Arc<AtomicU32>,
    pub source_error_count: Arc<AtomicU32>,
    pub translation_queue_depth: Arc<AtomicU32>,
    pub last_translation_latency_ms: Arc<AtomicU64>,
    pub last_translation_first_token_ms: Arc<AtomicU64>,
    pub runtime_logs: Arc<RwLock<Vec<RuntimeLogEntry>>>,
    pub api_key_configured: Arc<AtomicBool>,
    pub active_source: Arc<RwLock<Option<CaptionSourceConfig>>>,
    pub asr_latency_ms: Arc<AtomicU64>,
    pub model_downloads: model_manager::DownloadRegistry,
    pub idle_asr_worker: Arc<Mutex<Option<(String, asr_worker::AsrWorker)>>>,
    pub idle_worker_generation: Arc<AtomicU64>,
}

impl AppState {
    fn new(paths: AppPaths, settings: AppSettings) -> Self {
        Self {
            paths,
            settings: Arc::new(RwLock::new(settings)),
            caption: Arc::new(Mutex::new(None)),
            caption_state: Arc::new(RwLock::new(CaptionRuntimeState::Stopped)),
            caption_exports: Arc::new(RwLock::new(Vec::new())),
            selection_hotkey_registered: Arc::new(AtomicBool::new(false)),
            last_error: Arc::new(RwLock::new(None)),
            source_health: Arc::new(RwLock::new(CaptionSourceHealth::Unknown)),
            last_caption_update_ms: Arc::new(AtomicU64::new(0)),
            reader_restarts: Arc::new(AtomicU32::new(0)),
            source_error_count: Arc::new(AtomicU32::new(0)),
            translation_queue_depth: Arc::new(AtomicU32::new(0)),
            last_translation_latency_ms: Arc::new(AtomicU64::new(0)),
            last_translation_first_token_ms: Arc::new(AtomicU64::new(0)),
            runtime_logs: Arc::new(RwLock::new(Vec::new())),
            api_key_configured: Arc::new(AtomicBool::new(false)),
            active_source: Arc::new(RwLock::new(None)),
            asr_latency_ms: Arc::new(AtomicU64::new(0)),
            model_downloads: model_manager::DownloadRegistry::default(),
            idle_asr_worker: Arc::new(Mutex::new(None)),
            idle_worker_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set_caption_state(&self, app: &AppHandle, next: CaptionRuntimeState) {
        if let Ok(mut state) = self.caption_state.write() {
            *state = next.clone();
        }
        let _ = app.emit("caption:state-changed", next);
    }

    pub fn log(&self, level: &str, message: impl Into<String>) {
        let entry = RuntimeLogEntry {
            timestamp: chrono::Local::now().to_rfc3339(),
            level: level.to_string(),
            message: message.into(),
        };
        if let Ok(mut logs) = self.runtime_logs.write() {
            logs.push(entry.clone());
            if logs.len() > 500 {
                logs.drain(..100);
            }
        }
        let _ = self.paths.ensure();
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.paths.logs_dir.join("runtime.jsonl"))
        {
            if let Ok(line) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{line}");
            }
        }
    }

    pub fn set_source_health(&self, app: &AppHandle, next: CaptionSourceHealth) {
        let changed = self
            .source_health
            .write()
            .map(|mut current| {
                if *current == next {
                    false
                } else {
                    *current = next.clone();
                    true
                }
            })
            .unwrap_or(false);
        if changed {
            let _ = app.emit("caption:health-changed", &next);
        }
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<SettingsView, String> {
    let settings = state
        .settings
        .read()
        .map_err(|error| error.to_string())?
        .clone();
    Ok(SettingsView {
        settings,
        // Credential Manager is an external Windows service. Reading it here
        // would block Tauri's command thread while all three webviews boot.
        api_key_configured: state.api_key_configured.load(Ordering::Relaxed),
        model_directory: state.paths.models_dir.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
fn save_settings(
    mut request: SettingsSaveRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    request.settings.llm.timeout_milliseconds =
        request.settings.llm.timeout_milliseconds.clamp(500, 5_000);
    request.settings.captions.context_segments = request.settings.captions.context_segments.min(4);
    request.settings.schema_version = 6;
    if !request.settings.llm.extra_body_json.trim().is_empty() {
        let value: serde_json::Value = serde_json::from_str(&request.settings.llm.extra_body_json)
            .map_err(|e| e.to_string())?;
        if !value.is_object() {
            return Err("Extra Body JSON 必须是对象".into());
        }
    }
    windows_integration::validate_hotkey(&request.settings.selection.hotkey)?;
    if let Some(key) = request.api_key.as_deref().filter(|v| !v.trim().is_empty()) {
        secrets::set_api_key(key)?;
        if secrets::get_api_key()?.is_none() {
            return Err("API Key 写入凭据管理器后无法回读".into());
        }
        state.api_key_configured.store(true, Ordering::Relaxed);
    }

    state
        .selection_hotkey_registered
        .store(request.settings.selection.enabled, Ordering::Relaxed);

    settings::save(&state.paths, &request.settings)?;
    *state.settings.write().map_err(|e| e.to_string())? = request.settings.clone();
    state.log("info", "设置已保存");
    let view = SettingsView {
        settings: request.settings,
        api_key_configured: state.api_key_configured.load(Ordering::Relaxed),
        model_directory: state.paths.models_dir.to_string_lossy().into_owned(),
    };
    let _ = app.emit("settings:changed", &view);
    Ok(view)
}

#[tauri::command]
fn delete_api_key(state: State<'_, AppState>) -> Result<(), String> {
    secrets::delete_api_key()?;
    state.api_key_configured.store(false, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn test_llm(
    api_key: Option<String>,
    state: State<'_, AppState>,
) -> Result<TranslationResult, String> {
    let settings = state.settings.read().map_err(|e| e.to_string())?.clone();
    let key = match api_key.filter(|v| !v.trim().is_empty()) {
        Some(value) => secrets::normalize_api_key(&value)?,
        None => secrets::get_api_key()?.ok_or_else(|| "请先保存 API Key".to_string())?,
    };
    llm::translate(
        settings.clone(),
        key,
        TranslationRequest {
            mode: TranslationMode::Selection,
            source_text: "Hello, LiveCaption.".into(),
            source_language: "auto".into(),
            target_language: settings.llm.target_language.clone(),
            context: Vec::new(),
        },
    )
    .await
}

#[tauri::command]
async fn translate_selection(
    request: TranslationRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TranslationResult, String> {
    let settings = state.settings.read().map_err(|e| e.to_string())?.clone();
    let key = secrets::get_api_key()?.ok_or_else(|| "请先保存 API Key".to_string())?;
    let _ = app.emit("translation:started", &request);
    let delta_app = app.clone();
    let result = llm::translate_with_delta(settings, key, request, move |text| {
        let _ = delta_app.emit("translation:delta", text);
    })
    .await?;
    let _ = app.emit("translation:finished", &result);
    Ok(result)
}

async fn set_caption_running_inner(
    app: AppHandle,
    state: AppState,
    running: bool,
) -> Result<(), String> {
    if running {
        let mut controller = state.caption.lock().await;
        if controller.is_some() {
            let active = matches!(
                *state.caption_state.read().map_err(|e| e.to_string())?,
                CaptionRuntimeState::Starting
                    | CaptionRuntimeState::Running
                    | CaptionRuntimeState::LoadingModel
                    | CaptionRuntimeState::Switching
            );
            if active {
                return Ok(());
            }
            if let Some(stale) = controller.take() {
                stale.stop.store(true, Ordering::Relaxed);
                stale.join.abort();
            }
        }
        if secrets::get_api_key()?.is_none() {
            return Err("请先保存 API Key".into());
        }
        state
            .caption_exports
            .write()
            .map_err(|e| e.to_string())?
            .clear();
        state.set_caption_state(&app, CaptionRuntimeState::Starting);
        state.log("info", "正在启动实时字幕翻译");
        *controller = Some(captions::spawn_caption_session(app, state.clone()));
    } else {
        state.set_caption_state(&app, CaptionRuntimeState::Stopping);
        if let Some(controller) = state.caption.lock().await.take() {
            controller.stop.store(true, Ordering::Relaxed);
            let mut join = controller.join;
            if tokio::time::timeout(std::time::Duration::from_secs(3), &mut join)
                .await
                .is_err()
            {
                join.abort();
            }
        }
        shutdown_idle_asr_worker(&state, "停止字幕").await;
        if let Some(window) = app.get_webview_window("overlay") {
            let _ = window.hide();
        }
        state.set_source_health(&app, CaptionSourceHealth::Unknown);
        state.set_caption_state(&app, CaptionRuntimeState::Stopped);
        state.log("info", "实时字幕翻译已停止");
    }
    Ok(())
}

async fn shutdown_idle_asr_worker(state: &AppState, reason: &str) {
    state.idle_worker_generation.fetch_add(1, Ordering::Relaxed);
    let idle_worker = { state.idle_asr_worker.lock().await.take() };
    if let Some((model_id, worker)) = idle_worker {
        worker.shutdown().await;
        state.log("info", format!("{reason}，已关闭 ASR Worker：{model_id}"));
    }
}

#[tauri::command]
async fn set_caption_running(
    running: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    set_caption_running_inner(app, state.inner().clone(), running).await
}

#[tauri::command]
async fn refresh_caption(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let state = state.inner().clone();
    let mut controller = state.caption.lock().await;
    if let Some(current) = controller.take() {
        current.stop.store(true, Ordering::Relaxed);
        let mut join = current.join;
        if tokio::time::timeout(std::time::Duration::from_secs(3), &mut join)
            .await
            .is_err()
        {
            join.abort();
        }
    } else {
        return Err("实时字幕尚未启动".into());
    }
    state.set_caption_state(&app, CaptionRuntimeState::Starting);
    state.log("info", "正在刷新实时字幕连接");
    *controller = Some(captions::spawn_caption_session(app, state.clone()));
    Ok(())
}

#[tauri::command]
async fn get_runtime_status(state: State<'_, AppState>) -> Result<RuntimeStatus, String> {
    let selected_source = state
        .settings
        .read()
        .map_err(|e| e.to_string())?
        .captions
        .source
        .clone();
    Ok(RuntimeStatus {
        caption_running: matches!(
            *state.caption_state.read().map_err(|e| e.to_string())?,
            CaptionRuntimeState::Starting
                | CaptionRuntimeState::Running
                | CaptionRuntimeState::LoadingModel
                | CaptionRuntimeState::Switching
        ),
        caption_state: state
            .caption_state
            .read()
            .map_err(|e| e.to_string())?
            .clone(),
        selection_hotkey_registered: state.selection_hotkey_registered.load(Ordering::Relaxed),
        last_error: state.last_error.read().map_err(|e| e.to_string())?.clone(),
        source_health: state
            .source_health
            .read()
            .map_err(|e| e.to_string())?
            .clone(),
        last_caption_update_ms: state.last_caption_update_ms.load(Ordering::Relaxed),
        reader_restarts: state.reader_restarts.load(Ordering::Relaxed),
        source_error_count: state.source_error_count.load(Ordering::Relaxed),
        translation_queue_depth: state.translation_queue_depth.load(Ordering::Relaxed),
        last_translation_latency_ms: state.last_translation_latency_ms.load(Ordering::Relaxed),
        last_translation_first_token_ms: state
            .last_translation_first_token_ms
            .load(Ordering::Relaxed),
        selected_source,
        active_source: state
            .active_source
            .read()
            .map_err(|e| e.to_string())?
            .clone(),
        active_model: state
            .active_source
            .read()
            .map_err(|e| e.to_string())?
            .as_ref()
            .and_then(|source| source.model_id().map(str::to_string)),
        asr_latency_ms: state.asr_latency_ms.load(Ordering::Relaxed),
    })
}

#[tauri::command]
async fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>, String> {
    Ok(model_manager::list_models(&state.paths, &state.model_downloads).await)
}

#[tauri::command]
async fn download_model(
    model_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let paths = state.paths.clone();
    let registry = state.model_downloads.clone();
    let mirror = state
        .settings
        .read()
        .map_err(|e| e.to_string())?
        .captions
        .model_mirror_url
        .clone();
    tauri::async_runtime::spawn(async move {
        let _ = model_manager::download_model(app, paths, registry, model_id, mirror).await;
    });
    Ok(())
}

#[tauri::command]
async fn cancel_model_download(model_id: String, state: State<'_, AppState>) -> Result<(), String> {
    if state.model_downloads.cancel(&model_id).await {
        Ok(())
    } else {
        Err("该模型当前未在下载".to_string())
    }
}

#[tauri::command]
async fn verify_model(
    model_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _ = app.emit(
        "model:progress",
        models::ModelProgressEvent {
            model_id: model_id.clone(),
            status: models::ModelStatus::Verifying,
            downloaded_bytes: 0,
            total_bytes: 0,
            error: None,
        },
    );
    let paths = state.paths.clone();
    let id = model_id.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || model_manager::verify_model(&paths, &id))
            .await
            .map_err(|e| e.to_string())?;
    let status = if result.is_ok() {
        models::ModelStatus::Available
    } else {
        models::ModelStatus::Corrupt
    };
    let _ = app.emit(
        "model:progress",
        models::ModelProgressEvent {
            model_id,
            status,
            downloaded_bytes: 0,
            total_bytes: 0,
            error: result.as_ref().err().cloned(),
        },
    );
    result
}

#[tauri::command]
async fn test_model(
    model_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    let _ = app.emit(
        "model:progress",
        models::ModelProgressEvent {
            model_id: model_id.clone(),
            status: models::ModelStatus::Loading,
            downloaded_bytes: 0,
            total_bytes: 0,
            error: None,
        },
    );
    let verify_paths = state.paths.clone();
    let verify_id = model_id.clone();
    tokio::task::spawn_blocking(move || model_manager::verify_model(&verify_paths, &verify_id))
        .await
        .map_err(|error| error.to_string())??;
    state.idle_worker_generation.fetch_add(1, Ordering::Relaxed);
    let idle_worker = { state.idle_asr_worker.lock().await.take() };
    if let Some((_, worker)) = idle_worker {
        worker.shutdown().await;
    }
    let source = CaptionSourceConfig::LocalAsr {
        model_id: model_id.clone(),
        device: "cuda".to_string(),
        compute_type: "int8_float16".to_string(),
        vad_profile: state
            .settings
            .read()
            .map_err(|e| e.to_string())?
            .captions
            .audio_mode,
    };
    let mut worker = asr_worker::AsrWorker::spawn(&state.paths).await?;
    let result = async {
        worker.load(&state.paths, &source).await?;
        worker.dry_run().await
    }
    .await;
    worker.shutdown().await;
    let (status, error) = match &result {
        Ok(_) => (models::ModelStatus::Available, None),
        Err(error) => (models::ModelStatus::Incompatible, Some(error.clone())),
    };
    let _ = app.emit(
        "model:progress",
        models::ModelProgressEvent {
            model_id,
            status,
            downloaded_bytes: 0,
            total_bytes: 0,
            error,
        },
    );
    result
}

#[tauri::command]
async fn delete_model(model_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let active = state
        .active_source
        .read()
        .map_err(|e| e.to_string())?
        .as_ref()
        .and_then(|source| source.model_id().map(str::to_string));
    if active.as_deref() == Some(&model_id) {
        return Err("不能删除正在使用的模型".to_string());
    }
    state.idle_worker_generation.fetch_add(1, Ordering::Relaxed);
    let idle_worker = { state.idle_asr_worker.lock().await.take() };
    if let Some((idle_id, worker)) = idle_worker {
        worker.shutdown().await;
        state.log("info", format!("删除前已释放空闲模型：{idle_id}"));
    }
    model_manager::delete_model(&state.paths, &model_id)
}

#[tauri::command]
fn open_models_dir(state: State<'_, AppState>) -> Result<(), String> {
    state.paths.ensure()?;
    Command::new("explorer")
        .arg(&state.paths.models_dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

async fn preflight_source(source: &CaptionSourceConfig, state: &AppState) -> Result<(), String> {
    match source {
        CaptionSourceConfig::WindowsLiveCaption => Ok(()),
        CaptionSourceConfig::LocalAsr {
            model_id,
            device,
            compute_type,
            ..
        } => {
            if device != "cuda" || compute_type != "int8_float16" {
                return Err("当前版本仅支持 CUDA int8_float16".to_string());
            }
            model_manager::verify_model(&state.paths, model_id)
        }
    }
}

#[tauri::command]
async fn switch_caption_source(
    source: CaptionSourceConfig,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let state = state.inner().clone();
    let was_running = matches!(
        *state.caption_state.read().map_err(|e| e.to_string())?,
        CaptionRuntimeState::Starting
            | CaptionRuntimeState::Running
            | CaptionRuntimeState::LoadingModel
    );
    if was_running {
        preflight_source(&source, &state).await?;
    }
    if was_running {
        state.set_caption_state(&app, CaptionRuntimeState::Switching);
    }
    if let Some(controller) = state.caption.lock().await.take() {
        controller.stop.store(true, Ordering::Relaxed);
        let mut join = controller.join;
        if tokio::time::timeout(std::time::Duration::from_secs(3), &mut join)
            .await
            .is_err()
        {
            join.abort();
        }
    }
    {
        let mut settings = state.settings.write().map_err(|e| e.to_string())?;
        settings.captions.source = source;
        settings::save(&state.paths, &settings)?;
    }
    state
        .caption_exports
        .write()
        .map_err(|e| e.to_string())?
        .clear();
    let _ = app.emit("caption:session-reset", ());
    if was_running {
        state.set_caption_state(&app, CaptionRuntimeState::Starting);
        *state.caption.lock().await = Some(captions::spawn_caption_session(app, state.clone()));
    } else {
        state.set_caption_state(&app, CaptionRuntimeState::Stopped);
    }
    Ok(())
}

#[tauri::command]
fn save_overlay_position(x: i32, y: i32, state: State<'_, AppState>) -> Result<(), String> {
    let mut current = state.settings.write().map_err(|e| e.to_string())?;
    current.overlay.x = Some(x);
    current.overlay.y = Some(y);
    settings::save(&state.paths, &current)
}

#[tauri::command]
fn export_caption_session(
    path: String,
    format: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let records = state.caption_exports.read().map_err(|e| e.to_string())?;
    if records.is_empty() {
        return Err("当前没有可导出的字幕".into());
    }
    let body = match format.as_str() {
        "srt" => records
            .iter()
            .map(|r| {
                format!(
                    "{}\n{} --> {}\n{}\n{}\n",
                    r.index,
                    srt_time(r.start_ms),
                    srt_time(r.end_ms),
                    r.translated_text,
                    r.source_text
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "vtt" => format!(
            "WEBVTT\n\n{}",
            records
                .iter()
                .map(|r| format!(
                    "{} --> {}\n{}\n{}\n",
                    vtt_time(r.start_ms),
                    vtt_time(r.end_ms),
                    r.translated_text,
                    r.source_text
                ))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        "json" => serde_json::to_string_pretty(&*records).map_err(|e| e.to_string())?,
        _ => records
            .iter()
            .map(|r| format!("{}\n{}", r.source_text, r.translated_text))
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    fs::write(Path::new(&path), body).map_err(|e| e.to_string())?;
    state.log("info", format!("字幕已导出：{path}"));
    Ok(())
}

fn srt_time(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}
fn vtt_time(ms: u64) -> String {
    srt_time(ms).replace(',', ".")
}

#[tauri::command]
fn get_runtime_logs(state: State<'_, AppState>) -> Result<Vec<RuntimeLogEntry>, String> {
    Ok(state
        .runtime_logs
        .read()
        .map_err(|e| e.to_string())?
        .clone())
}

#[tauri::command]
fn clear_runtime_logs(state: State<'_, AppState>) -> Result<(), String> {
    state
        .runtime_logs
        .write()
        .map_err(|e| e.to_string())?
        .clear();
    let file = state.paths.logs_dir.join("runtime.jsonl");
    if file.exists() {
        fs::remove_file(file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn open_logs_dir(state: State<'_, AppState>) -> Result<(), String> {
    state.paths.ensure()?;
    Command::new("explorer")
        .arg(&state.paths.logs_dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            delete_api_key,
            test_llm,
            translate_selection,
            set_caption_running,
            refresh_caption,
            get_runtime_status,
            save_overlay_position,
            export_caption_session,
            get_runtime_logs,
            clear_runtime_logs,
            open_logs_dir,
            list_models,
            download_model,
            cancel_model_download,
            verify_model,
            test_model,
            delete_model,
            open_models_dir,
            switch_caption_source
        ])
        .setup(|app| {
            // Use one explicit data directory for dev, raw release binaries,
            // and installed builds. This avoids environment-dependent path
            // resolution producing separate settings files.
            let paths = AppPaths::shared().map_err(anyhow::Error::msg)?;
            paths.ensure().map_err(anyhow::Error::msg)?;
            let settings = settings::load(&paths).map_err(anyhow::Error::msg)?;
            let state = AppState::new(paths, settings.clone());
            app.manage(state.clone());

            // A packaged process can take noticeably longer to access Windows
            // Credential Manager. Probe once off the event loop so first paint,
            // close handling, and tray commands remain responsive.
            let credential_app = app.handle().clone();
            let credential_state = state.clone();
            std::thread::spawn(move || {
                let configured = secrets::has_api_key();
                credential_state
                    .api_key_configured
                    .store(configured, Ordering::Relaxed);
                if let Ok(settings) = credential_state.settings.read().map(|value| value.clone()) {
                    let _ = credential_app.emit(
                        "settings:changed",
                        SettingsView {
                            settings,
                            api_key_configured: configured,
                            model_directory: credential_state
                                .paths
                                .models_dir
                                .to_string_lossy()
                                .into_owned(),
                        },
                    );
                }
            });
            windows_integration::validate_hotkey(&settings.selection.hotkey)
                .map_err(anyhow::Error::msg)?;
            state
                .selection_hotkey_registered
                .store(settings.selection.enabled, Ordering::Relaxed);
            windows_integration::start_selection_hotkey_watcher(
                app.handle().clone(),
                state.clone(),
            );
            windows_integration::start_toolbar_dismiss_watcher(app.handle().clone());
            state.log("info", "LiveCaption 已启动");
            build_tray(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run LiveCaption");
}

fn build_tray(app: &mut tauri::App) -> Result<(), tauri::Error> {
    let open = MenuItem::with_id(app, "open", "打开 LiveCaption", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle_captions", "切换实时字幕", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &toggle, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main-tray");
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.tooltip("LiveCaption")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "toggle_captions" => {
                let app_handle = app.clone();
                let state = app.state::<AppState>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    let running = matches!(
                        *state.caption_state.read().unwrap(),
                        CaptionRuntimeState::Starting
                            | CaptionRuntimeState::Running
                            | CaptionRuntimeState::LoadingModel
                            | CaptionRuntimeState::Switching
                    );
                    let _ = set_caption_running_inner(app_handle, state, !running).await;
                });
            }
            "quit" => {
                let app_handle = app.clone();
                let state = app.state::<AppState>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    let _ =
                        set_caption_running_inner(app_handle.clone(), state.clone(), false).await;
                    shutdown_idle_asr_worker(&state, "退出应用").await;
                    app_handle.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
