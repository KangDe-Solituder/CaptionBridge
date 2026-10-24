#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod asr_dependencies;
mod asr_worker;
mod captions;
mod glossary;
mod gpu_runtime;
mod llm;
mod llm_profiles;
mod model_manager;
mod models;
mod native_caption_reader;
mod native_selection_reader;
mod network;
mod ocr;
mod secrets;
mod segmenter;
mod sessions;
mod settings;
mod toolbar;
mod windows_integration;

use std::{
    collections::HashSet,
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
    AppSettings, AsmrChannelState, AsrDependencyReport, AudioChannelMode, CaptionExportEntry,
    CaptionRuntimeState, CaptionSourceConfig, CaptionSourceHealth, CorrectionProgressEvent,
    Glossary, GlossaryImportResult, ModelInfo, RuntimeLogEntry, RuntimeStatus, SessionPageView,
    SessionSummary, SettingsSaveRequest, SettingsView, TranslationMode, TranslationRequest,
    TranslationResult, VadProfile,
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
    pub ocr: Arc<ocr::Service>,
    pub toolbar: Arc<toolbar::Service>,
    pub paths: AppPaths,
    pub settings: Arc<RwLock<AppSettings>>,
    pub caption: Arc<Mutex<Option<CaptionController>>>,
    pub caption_state: Arc<RwLock<CaptionRuntimeState>>,
    pub current_session: Arc<Mutex<Option<Arc<sessions::JsonlSession>>>>,
    pub correction_tasks: Arc<Mutex<HashSet<String>>>,
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
    pub asmr_channel_state: Arc<RwLock<AsmrChannelState>>,
    pub asmr_reset_generation: Arc<AtomicU64>,
    pub model_downloads: model_manager::DownloadRegistry,
    pub gpu_runtime_downloads: gpu_runtime::DownloadRegistry,
    pub idle_asr_worker: Arc<Mutex<Option<(String, asr_worker::AsrWorker)>>>,
    pub idle_worker_generation: Arc<AtomicU64>,
}

impl AppState {
    fn new(paths: AppPaths, settings: AppSettings) -> Self {
        Self {
            ocr: Arc::new(ocr::Service::default()),
            toolbar: Arc::new(toolbar::Service::default()),
            paths,
            settings: Arc::new(RwLock::new(settings)),
            caption: Arc::new(Mutex::new(None)),
            caption_state: Arc::new(RwLock::new(CaptionRuntimeState::Stopped)),
            current_session: Arc::new(Mutex::new(None)),
            correction_tasks: Arc::new(Mutex::new(HashSet::new())),
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
            asmr_channel_state: Arc::new(RwLock::new(AsmrChannelState::Inactive)),
            asmr_reset_generation: Arc::new(AtomicU64::new(0)),
            model_downloads: model_manager::DownloadRegistry::default(),
            gpu_runtime_downloads: gpu_runtime::DownloadRegistry::default(),
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

    pub fn set_asmr_channel_state(&self, app: &AppHandle, next: AsmrChannelState) {
        let changed = self
            .asmr_channel_state
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
            let _ = app.emit("caption:asmr-channel-changed", &next);
        }
    }
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<SettingsView, String> {
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
async fn set_theme(
    theme: models::ThemeMode,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut current = state.settings.write().map_err(|e| e.to_string())?;
    let mut next = current.clone();
    next.visual.theme = theme.clone();
    settings::save(&state.paths, &next)?;
    *current = next;
    // A theme-only event preserves unsaved form edits and active translations.
    let _ = app.emit("settings:theme-changed", &theme);
    Ok(())
}

#[tauri::command]
async fn save_settings(
    mut request: SettingsSaveRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsView, String> {
    let view = {
        let mut current = state.settings.write().map_err(|e| e.to_string())?;
        request.settings.llm = current.llm.clone();
        request.settings.llm_profiles = current.llm_profiles.clone();
        settings::normalize(&mut request.settings);
        network::validate_download_settings(&request.settings.downloads)?;
        if !request.settings.llm.extra_body_json.trim().is_empty() {
            let value: serde_json::Value =
                serde_json::from_str(&request.settings.llm.extra_body_json)
                    .map_err(|e| e.to_string())?;
            if !value.is_object() {
                return Err("Extra Body JSON 必须是对象".into());
            }
        }
        windows_integration::validate_hotkey(&request.settings.selection.hotkey)?;

        state
            .selection_hotkey_registered
            .store(request.settings.selection.enabled, Ordering::Relaxed);

        settings::save(&state.paths, &request.settings)?;
        *current = request.settings.clone();
        state.toolbar.apply_bindings(&current.ocr);
        state.log("info", "设置已保存");
        let view = SettingsView {
            settings: request.settings,
            api_key_configured: state.api_key_configured.load(Ordering::Relaxed),
            model_directory: state.paths.models_dir.to_string_lossy().into_owned(),
        };
        let _ = app.emit("settings:changed", &view);
        view
    };
    if state.toolbar.mode() != toolbar::Mode::Hidden && !state.ocr.capturing.load(Ordering::SeqCst)
    {
        toolbar::dismiss(&app, &state)?;
    }
    let stop_captions = !view.settings.captions.enabled
        && !matches!(
            *state.caption_state.read().map_err(|e| e.to_string())?,
            CaptionRuntimeState::Stopped
        );
    if stop_captions {
        set_caption_running_inner(app, state.inner().clone(), false).await?;
    }
    Ok(view)
}

#[tauri::command]
async fn hide_selection_toolbar(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    toolbar::dismiss(&app, &state).map_err(|error| {
        state.log("warn", format!("划词工具条隐藏失败：{error}"));
        error
    })
}

#[tauri::command]
async fn test_download_proxy(proxy_url: String) -> Result<String, String> {
    network::test_download_proxy(&proxy_url).await
}

#[tauri::command]
async fn test_llm(state: State<'_, AppState>) -> Result<TranslationResult, String> {
    let (settings, key) = translation_context(&state)?;
    llm::translate(
        settings.clone(),
        key,
        TranslationRequest {
            mode: TranslationMode::Selection,
            source_text: "Hello, LiveCaption.".into(),
            source_language: "auto".into(),
            target_language: settings.llm.target_language.clone(),
            context: Vec::new(),
            glossary: Vec::new(),
        },
    )
    .await
}

fn translation_context(state: &AppState) -> Result<(AppSettings, String), String> {
    // Keep the endpoint/model snapshot and credential lookup under the same lock.
    // Editing a profile must never pair its old endpoint with its new API key.
    let settings = state.settings.read().map_err(|e| e.to_string())?;
    let key = secrets::translation_key(&settings)?;
    Ok((settings.clone(), key))
}

fn translation_glossary(state: &AppState) -> Vec<models::GlossaryPair> {
    let glossary = glossary::load(&state.paths);
    if glossary.use_in_translation {
        glossary::pairs(&glossary)
    } else {
        Vec::new()
    }
}

#[tauri::command]
async fn translate_selection(
    mut request: TranslationRequest,
    stream_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TranslationResult, String> {
    let (settings, key) = translation_context(&state)?;
    if request.glossary.is_empty() {
        request.glossary = translation_glossary(&state);
    }
    let _ = app.emit("translation:started", &request);
    let delta_app = app.clone();
    let result = llm::translate_with_delta(settings, key, request, move |text| {
        if let Some(id) = &stream_id {
            let _ = delta_app.emit_to(
                "selection-toolbar",
                "toolbar:translation-delta",
                serde_json::json!({"stream_id": id, "text": text}),
            );
        } else {
            let _ = delta_app.emit("translation:delta", text);
        }
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
        if !state
            .settings
            .read()
            .map_err(|e| e.to_string())?
            .captions
            .enabled
        {
            return Err("请先在设置 → 功能 → 工具栏中启用实时字幕".into());
        }
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
        secrets::translation_key(&*state.settings.read().map_err(|e| e.to_string())?)?;
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
async fn start_captions_from_toolbar(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    toolbar::dismiss(&app, &state)?;
    // Launch captions in place; the session owns the overlay window.
    // Do not navigate or activate the main window from this quick action.
    set_caption_running_inner(app, state.inner().clone(), true).await
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
        asmr_channel_state: state
            .asmr_channel_state
            .read()
            .map_err(|e| e.to_string())?
            .clone(),
    })
}

#[tauri::command]
fn reset_asr_channel_routing(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let active = state
        .active_source
        .read()
        .map_err(|error| error.to_string())?
        .clone();
    if !matches!(
        active,
        Some(CaptionSourceConfig::LocalAsr {
            vad_profile: VadProfile::Asmr,
            channel_mode: AudioChannelMode::Auto,
            ..
        })
    ) {
        return Err("仅能在运行中的 ASMR 智能锁定模式下重新检测声道".to_string());
    }
    state.asmr_reset_generation.fetch_add(1, Ordering::Relaxed);
    state.set_asmr_channel_state(&app, AsmrChannelState::Searching);
    state.log("info", "已请求重新检测 ASMR 人声声道");
    Ok(())
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
    model_manager::manifest(&model_id)?;
    let (mirror, download_settings) = {
        let settings = state.settings.read().map_err(|e| e.to_string())?;
        (
            settings.captions.model_mirror_url.clone(),
            settings.downloads.clone(),
        )
    };
    network::validate_download_settings(&download_settings)?;
    let cancel = registry.begin(&model_id).await?;
    let download_state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let label = model_id.clone();
        if let Err(error) = model_manager::download_model(
            app,
            paths,
            registry,
            model_id,
            mirror,
            download_settings,
            cancel,
        )
        .await
        {
            download_state.log("warn", format!("模型 {label} 下载未完成：{error}"));
        }
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
async fn download_asr_gpu_runtime(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let paths = state.paths.clone();
    let downloads = state.gpu_runtime_downloads.clone();
    let (source, download_settings) = {
        let settings = state.settings.read().map_err(|error| error.to_string())?;
        (settings.captions.source.clone(), settings.downloads.clone())
    };
    let report = asr_dependencies::check(&paths, Some(&source), &downloads).await;
    let component_ids = report
        .gpu_runtime
        .components
        .iter()
        .filter(|component| component.status != "available")
        .map(|component| component.id.clone())
        .collect::<Vec<_>>();
    if component_ids.is_empty() {
        return Err("当前环境没有需要下载的 GPU 运行库组件".to_string());
    }
    tauri::async_runtime::spawn(async move {
        let _ =
            gpu_runtime::download(app, paths, downloads, component_ids, download_settings).await;
    });
    Ok(())
}

#[tauri::command]
async fn cancel_asr_gpu_runtime(state: State<'_, AppState>) -> Result<(), String> {
    if state
        .gpu_runtime_downloads
        .cancel(gpu_runtime::RUNTIME_ID)
        .await
    {
        Ok(())
    } else {
        Err("GPU 运行库当前未在下载".to_string())
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
    if model_id == ocr::MODEL_ID {
        return ocr::test(&state).await;
    }
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
    let (vad_profile, channel_mode, channel_switch_sensitivity, suppress_non_speech_segments) = {
        let settings = state.settings.read().map_err(|e| e.to_string())?;
        (
            settings.captions.audio_mode,
            settings.captions.source.channel_mode().unwrap_or_default(),
            settings
                .captions
                .source
                .channel_switch_sensitivity()
                .unwrap_or_default(),
            settings
                .captions
                .source
                .suppress_non_speech_segments()
                .unwrap_or(true),
        )
    };
    let source = CaptionSourceConfig::LocalAsr {
        model_id: model_id.clone(),
        device: "cuda".to_string(),
        compute_type: "int8_float16".to_string(),
        vad_profile,
        channel_mode,
        channel_switch_sensitivity,
        suppress_non_speech_segments,
    };
    let mut worker = asr_worker::AsrWorker::spawn(&state.paths).await?;
    let probe_result = worker.probe_dependencies().await;
    let result = async {
        worker
            .load(&state.paths, &source)
            .await
            .map_err(|error| format!("模型加载失败：{error}"))?;
        worker
            .dry_run()
            .await
            .map_err(|error| format!("GPU 推理测试失败：{error}"))
    }
    .await
    .map_err(|error| asr_model_test_error(&probe_result, &error));
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

fn asr_model_test_error(
    probe_result: &Result<asr_worker::AsrDependencyProbe, String>,
    inference_error: &str,
) -> String {
    let probe_detail = match probe_result {
        Ok(probe) => probe
            .cudnn_error
            .as_deref()
            .or(probe.cuda_error.as_deref())
            .map(|error| format!("；依赖探测详情：{error}"))
            .unwrap_or_default(),
        Err(error) => format!("；依赖轻量探测也失败：{error}"),
    };
    format!(
        "ASR 测试失败：{inference_error}{probe_detail}。请打开“设置 → 模型与资源 → 检查依赖”查看详情"
    )
}

#[tauri::command]
async fn delete_model(model_id: String, state: State<'_, AppState>) -> Result<(), String> {
    if model_id == ocr::MODEL_ID {
        state.ocr.delete_model(&state).await?;
        state.model_downloads.clear_failure(&model_id).await;
        return Ok(());
    }
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
    model_manager::delete_model(&state.paths, &model_id)?;
    state.model_downloads.clear_failure(&model_id).await;
    Ok(())
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

fn export_entries(records: &[sessions::SessionRecord]) -> Vec<CaptionExportEntry> {
    let Some(first) = records.first() else {
        return Vec::new();
    };
    let started = first.timestamp;
    records
        .iter()
        .enumerate()
        .map(|(i, record)| {
            let start_ms = (record.timestamp - started).num_milliseconds().max(0) as u64;
            let next_start = records
                .get(i + 1)
                .map(|next| (next.timestamp - started).num_milliseconds().max(0) as u64);
            let end_ms = next_start.unwrap_or(start_ms + 2_500).max(start_ms + 500);
            let corrected = record.corrected.as_ref();
            CaptionExportEntry {
                index: i + 1,
                start_ms,
                end_ms,
                source_text: corrected
                    .map(|value| value.source_text.clone())
                    .unwrap_or_else(|| record.segment.source_text.clone()),
                translated_text: corrected
                    .map(|value| value.translated_text.clone())
                    .unwrap_or_else(|| record.result.translated_text.clone()),
            }
        })
        .collect()
}

#[tauri::command]
async fn export_caption_session(
    path: String,
    format: String,
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let records = match session_id {
        Some(id) => {
            sessions::read_records(&sessions::session_path(&state.paths.sessions_dir, &id)?)
        }
        None => {
            let current = state.current_session.lock().await.clone();
            match current {
                Some(session) => sessions::read_records(session.path()),
                None => return Err("当前没有可导出的字幕".into()),
            }
        }
    };
    if records.is_empty() {
        return Err("当前没有可导出的字幕".into());
    }
    let entries = export_entries(&records);
    let body = match format.as_str() {
        "srt" => entries
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
            entries
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
        "json" => serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?,
        _ => entries
            .iter()
            .map(|r| format!("{}\n{}", r.source_text, r.translated_text))
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    fs::write(Path::new(&path), body).map_err(|e| e.to_string())?;
    state.log("info", format!("字幕已导出：{path}"));
    Ok(())
}

#[tauri::command]
async fn list_caption_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    let current_id = state
        .current_session
        .lock()
        .await
        .as_ref()
        .map(|session| session.id());
    Ok(sessions::summarize(
        &state.paths.sessions_dir,
        current_id.as_deref(),
    ))
}

#[tauri::command]
async fn read_caption_session(
    id: String,
    page: usize,
    page_size: usize,
    state: State<'_, AppState>,
) -> Result<SessionPageView, String> {
    let path = sessions::session_path(&state.paths.sessions_dir, &id)?;
    let records = sessions::read_records(&path);
    Ok(sessions::paginate(&records, page, page_size))
}

#[tauri::command]
async fn delete_caption_session(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let running = matches!(
        *state.caption_state.read().map_err(|e| e.to_string())?,
        CaptionRuntimeState::Starting
            | CaptionRuntimeState::Running
            | CaptionRuntimeState::LoadingModel
            | CaptionRuntimeState::Switching
    );
    let current_id = state
        .current_session
        .lock()
        .await
        .as_ref()
        .map(|session| session.id());
    let path = sessions::session_path(&state.paths.sessions_dir, &id)?;
    if current_id.as_deref() == Some(id.as_str()) {
        if running {
            return Err("实时字幕运行中，请先停止再删除本次记录".to_string());
        }
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        *state.current_session.lock().await = None;
        let _ = app.emit("caption:session-reset", ());
    } else {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    let _ = app.emit("caption:sessions-changed", ());
    state.log("info", format!("字幕记录已删除：{id}"));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_correction(
    app: AppHandle,
    state: AppState,
    id: String,
    path: std::path::PathBuf,
    settings: AppSettings,
    api_key: String,
    glossary_pairs: Vec<models::GlossaryPair>,
) -> Result<(), String> {
    let mut records = sessions::read_records(&path);
    let correctable = records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record.result.error.is_none() && !record.result.translated_text.trim().is_empty()
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let total = correctable.len();
    if total == 0 {
        return Err("记录中没有可纠错的内容".to_string());
    }
    let _ = app.emit(
        "correction:progress",
        CorrectionProgressEvent {
            id: id.clone(),
            completed: 0,
            total,
            error: None,
        },
    );
    const BATCH_SIZE: usize = 20;
    let mut completed = 0_usize;
    for chunk in correctable.chunks(BATCH_SIZE) {
        let inputs = chunk
            .iter()
            .map(|&index| llm::CorrectionInput {
                index,
                source: records[index].segment.source_text.clone(),
                translation: records[index].result.translated_text.clone(),
            })
            .collect::<Vec<_>>();
        let outputs = llm::correct_batch(&settings, &api_key, &inputs, &glossary_pairs).await?;
        let corrected_count = outputs.len();
        for output in outputs {
            if let Some(record) = records.get_mut(output.index) {
                record.corrected = Some(sessions::SessionCorrection {
                    source_text: output.source,
                    translated_text: output.translation,
                    at: chrono::Local::now(),
                    glossary_used: !glossary_pairs.is_empty(),
                });
            }
        }
        completed += corrected_count;
        let _ = app.emit(
            "correction:progress",
            CorrectionProgressEvent {
                id: id.clone(),
                completed,
                total,
                error: None,
            },
        );
    }
    sessions::rewrite_records(&path, &records)?;
    state.log("info", format!("字幕记录纠错完成：{id}（{total} 条）"));
    let _ = app.emit("correction:finished", &id);
    let _ = app.emit("caption:sessions-changed", ());
    Ok(())
}

#[tauri::command]
async fn new_caption_session(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let current = state.current_session.lock().await;
        if let Some(session) = current.as_ref() {
            if sessions::read_records(session.path()).is_empty() {
                return Err("当前记录还没有转录内容，无需新建".to_string());
            }
        }
    }
    let created = Arc::new(sessions::JsonlSession::start(&state.paths.sessions_dir)?);
    *state.current_session.lock().await = Some(created);
    let _ = app.emit("caption:sessions-changed", ());
    state.log("info", "已新建字幕记录");
    Ok(())
}

#[tauri::command]
async fn correct_caption_session(
    id: String,
    use_glossary: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let path = sessions::session_path(&state.paths.sessions_dir, &id)?;
    let running = matches!(
        *state.caption_state.read().map_err(|e| e.to_string())?,
        CaptionRuntimeState::Starting
            | CaptionRuntimeState::Running
            | CaptionRuntimeState::LoadingModel
            | CaptionRuntimeState::Switching
    );
    let current_id = state
        .current_session
        .lock()
        .await
        .as_ref()
        .map(|session| session.id());
    if running && current_id.as_deref() == Some(id.as_str()) {
        return Err("实时字幕运行中，请先停止再纠错".to_string());
    }
    {
        let mut tasks = state.correction_tasks.lock().await;
        if !tasks.insert(id.clone()) {
            return Err("该记录正在纠错中".to_string());
        }
    }
    let (settings, api_key) = match translation_context(&state) {
        Ok(context) => context,
        Err(error) => {
            state.correction_tasks.lock().await.remove(&id);
            return Err(error);
        }
    };
    let glossary_pairs = if use_glossary {
        glossary::pairs(&glossary::load(&state.paths))
    } else {
        Vec::new()
    };
    let task_state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_correction(
            app.clone(),
            task_state.clone(),
            id.clone(),
            path,
            settings,
            api_key,
            glossary_pairs,
        )
        .await
        {
            task_state.log("error", format!("字幕纠错失败：{error}"));
            let _ = app.emit(
                "correction:progress",
                CorrectionProgressEvent {
                    id: id.clone(),
                    completed: 0,
                    total: 0,
                    error: Some(error),
                },
            );
        }
        task_state.correction_tasks.lock().await.remove(&id);
    });
    Ok(())
}

#[tauri::command]
fn get_glossary(state: State<'_, AppState>) -> Result<Glossary, String> {
    Ok(glossary::load(&state.paths))
}

#[tauri::command]
fn save_glossary(
    glossary: Glossary,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    glossary::save(&state.paths, &glossary)?;
    let _ = app.emit("glossary:changed", &glossary);
    state.log("info", "术语表已保存");
    Ok(())
}

#[tauri::command]
fn export_glossary(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let glossary = glossary::load(&state.paths);
    let raw = serde_json::to_string_pretty(&glossary).map_err(|e| e.to_string())?;
    fs::write(Path::new(&path), raw).map_err(|e| e.to_string())?;
    state.log("info", format!("术语表已导出：{path}"));
    Ok(())
}

#[tauri::command]
fn import_glossary(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<GlossaryImportResult, String> {
    let result = glossary::import(&state.paths, Path::new(&path))?;
    let _ = app.emit("glossary:changed", &result.glossary);
    state.log(
        "info",
        format!(
            "术语表导入完成：新增 {} 条，更新 {} 条",
            result.added, result.updated
        ),
    );
    Ok(result)
}

#[tauri::command]
fn open_sessions_dir(state: State<'_, AppState>) -> Result<(), String> {
    state.paths.ensure()?;
    Command::new("explorer")
        .arg(&state.paths.sessions_dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
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

#[tauri::command]
async fn check_asr_dependencies(state: State<'_, AppState>) -> Result<AsrDependencyReport, String> {
    let source = state
        .settings
        .read()
        .map_err(|error| error.to_string())?
        .captions
        .source
        .clone();
    Ok(asr_dependencies::check(&state.paths, Some(&source), &state.gpu_runtime_downloads).await)
}

#[tauri::command]
fn open_asr_dependency_url(dependency_id: String) -> Result<(), String> {
    let url = asr_dependencies::official_url(&dependency_id)
        .ok_or_else(|| "未知的 ASR 依赖项".to_string())?;
    Command::new("explorer")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
struct SingleInstanceGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn acquire_single_instance() -> Result<Option<SingleInstanceGuard>, String> {
    use windows::{
        core::w,
        Win32::{
            Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
        },
    };

    unsafe {
        let handle = CreateMutexW(
            None,
            false,
            w!("Local\\com.dimfi.livecaption.single-instance"),
        )
        .map_err(|error| format!("创建单实例锁失败：{error}"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            Ok(None)
        } else {
            Ok(Some(SingleInstanceGuard(handle)))
        }
    }
}

#[cfg(windows)]
fn focus_existing_instance() {
    use windows::{
        core::w,
        Win32::UI::WindowsAndMessaging::{
            FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
        },
    };

    unsafe {
        if let Ok(window) = FindWindowW(None, w!("LiveCaption")) {
            let _ = ShowWindow(window, SW_RESTORE);
            let _ = SetForegroundWindow(window);
        }
    }
}

#[cfg(windows)]
fn main() {
    let _single_instance = match acquire_single_instance() {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            focus_existing_instance();
            return;
        }
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };
    run_app();
}

#[cfg(not(windows))]
fn main() {
    run_app();
}

fn run_app() {
    // Register state on the builder before Tauri creates any configured
    // webviews. Release assets can load quickly enough to invoke commands
    // before `setup` runs, so managing state from `setup` is racy.
    let paths = AppPaths::shared()
        .unwrap_or_else(|error| panic!("failed to resolve LiveCaption data paths: {error}"));
    paths
        .ensure()
        .unwrap_or_else(|error| panic!("failed to create LiveCaption data paths: {error}"));
    let settings = settings::load(&paths)
        .unwrap_or_else(|error| panic!("failed to load LiveCaption settings: {error}"));
    let state = AppState::new(paths, settings);

    tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            ocr::start_ocr_capture,
            toolbar::get_mouse_capabilities,
            toolbar::get_toolbar_snapshot,
            toolbar::expand_toolbar,
            ocr::get_ocr_capture,
            ocr::get_ocr_result,
            ocr::cancel_ocr,
            ocr::recognize_ocr,
            ocr::retry_ocr_translation,
            ocr::open_ocr_resources,
            get_settings,
            set_theme,
            save_settings,
            hide_selection_toolbar,
            llm_profiles::save_llm_profile,
            llm_profiles::activate_llm_profile,
            llm_profiles::remove_llm_profile,
            test_download_proxy,
            test_llm,
            translate_selection,
            set_caption_running,
            start_captions_from_toolbar,
            refresh_caption,
            reset_asr_channel_routing,
            get_runtime_status,
            save_overlay_position,
            export_caption_session,
            list_caption_sessions,
            read_caption_session,
            delete_caption_session,
            new_caption_session,
            correct_caption_session,
            get_glossary,
            save_glossary,
            export_glossary,
            import_glossary,
            open_sessions_dir,
            get_runtime_logs,
            clear_runtime_logs,
            open_logs_dir,
            list_models,
            download_model,
            cancel_model_download,
            download_asr_gpu_runtime,
            cancel_asr_gpu_runtime,
            verify_model,
            test_model,
            delete_model,
            open_models_dir,
            switch_caption_source,
            check_asr_dependencies,
            open_asr_dependency_url
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    // Keep tray/background services alive without a webview event round trip.
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        window
                            .state::<AppState>()
                            .log("error", format!("关闭主窗口失败: {error}"));
                    }
                }
            }
            if matches!(window.label(), "ocr-capture" | "ocr-result") {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let app = window.app_handle().clone();
                    tauri::async_runtime::spawn(async move {
                        let state = app.state::<AppState>();
                        let _ = ocr::cancel_ocr(app.clone(), state).await;
                    });
                }
            }
        })
        .setup(|app| {
            let state = app.state::<AppState>().inner().clone();
            let settings = state
                .settings
                .read()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
                .clone();

            // A packaged process can take noticeably longer to access Windows
            // Credential Manager. Probe once off the event loop so first paint,
            // close handling, and tray commands remain responsive.
            let credential_app = app.handle().clone();
            let credential_state = state.clone();
            let needs_legacy_probe =
                settings.llm_profiles.is_empty() && settings.llm.profile_id.is_none();
            std::thread::spawn(move || {
                let legacy_has_key = needs_legacy_probe && secrets::has_api_key();
                if let Ok(mut current) = credential_state.settings.write() {
                    llm_profiles::migrate_legacy(&mut current, legacy_has_key);
                    if let Err(error) = settings::save(&credential_state.paths, &current) {
                        credential_state.log("warn", format!("翻译配置迁移保存失败：{error}"));
                    }
                    let configured = current.llm_profiles.iter().any(|p| {
                        Some(&p.id) == current.llm.profile_id.as_ref() && p.api_key_configured
                    });
                    credential_state
                        .api_key_configured
                        .store(configured, Ordering::Relaxed);
                    let _ = credential_app.emit(
                        "llm:profiles-changed",
                        SettingsView {
                            settings: current.clone(),
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
            windows_integration::start_toolbar_dismiss_watcher(app.handle().clone(), state.clone());
            toolbar::start(app.handle().clone(), state.clone());
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
    let screenshot = MenuItem::with_id(app, "screenshot", "截图翻译", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &screenshot, &toggle, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main-tray");
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.tooltip("LiveCaption")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "screenshot" => {
                let app = app.clone();
                let state = app.state::<AppState>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = ocr::start(app.clone(), state).await {
                        show_main_window(&app);
                        let _ = app.emit_to("main", "ocr:error", error);
                    }
                });
            }
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
                state.toolbar.shutdown();
                tauri::async_runtime::spawn(async move {
                    let _ =
                        set_caption_running_inner(app_handle.clone(), state.clone(), false).await;
                    shutdown_idle_asr_worker(&state, "退出应用").await;
                    state.ocr.shutdown().await;
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
