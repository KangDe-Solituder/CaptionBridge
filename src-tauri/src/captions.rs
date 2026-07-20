use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use tauri::{async_runtime::JoinHandle, AppHandle, Emitter, Manager, PhysicalPosition};
use tokio::{
    sync::{Mutex, Notify},
    time::Duration,
};

use crate::{
    asr_worker::{AsrWorker, WorkerEvent},
    llm, model_manager,
    models::{
        CaptionExportEntry, CaptionRuntimeState, CaptionSegment, CaptionSourceConfig,
        CaptionSourceHealth, CaptionSourceKind, CaptionTimeSource, CaptionTranslatedEvent,
        TranslationMode, TranslationRequest, TranslationResult,
    },
    secrets,
    segmenter::BasicSegmenter,
    sessions::JsonlSession,
    windows_integration, AppState,
};

pub struct CaptionController {
    pub stop: Arc<AtomicBool>,
    pub join: JoinHandle<()>,
}

#[derive(Clone)]
struct TranslationJob {
    segment: CaptionSegment,
    context: Vec<String>,
}

#[derive(Default)]
struct FinalQueue {
    jobs: Mutex<VecDeque<TranslationJob>>,
    ready: Notify,
    closed: AtomicBool,
}

impl FinalQueue {
    async fn push(&self, mut job: TranslationJob) {
        let mut jobs = self.jobs.lock().await;
        if jobs.len() >= 4 {
            if let Some(last) = jobs.back_mut() {
                let combined = last.segment.source_text.chars().count()
                    + job.segment.source_text.chars().count();
                if combined <= 96 {
                    last.segment.source_text.push_str(&job.segment.source_text);
                    last.segment.end_ms = job.segment.end_ms;
                    return;
                }
            }
            if let Some(previous) = jobs.pop_back() {
                job.segment.source_text = format!(
                    "{} {}",
                    previous.segment.source_text, job.segment.source_text
                );
                job.segment.start_ms = previous.segment.start_ms;
                job.context = previous.context;
            }
        }
        jobs.push_back(job);
        drop(jobs);
        self.ready.notify_one();
    }

    async fn pop(&self) -> Option<TranslationJob> {
        loop {
            if let Some(job) = self.jobs.lock().await.pop_front() {
                return Some(job);
            }
            if self.closed.load(Ordering::Relaxed) {
                return None;
            }
            self.ready.notified().await;
        }
    }

    async fn depth(&self) -> u32 {
        self.jobs.lock().await.len() as u32
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        self.ready.notify_waiters();
    }
}

pub fn spawn_caption_session(app: AppHandle, state: AppState) -> CaptionController {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_task = stop.clone();
    let join = tauri::async_runtime::spawn(async move {
        if let Err(error) = run_session(app.clone(), state.clone(), stop_task).await {
            let _ = app.emit("runtime:error", &error);
            state.log("error", &error);
            if let Ok(mut last) = state.last_error.write() {
                *last = Some(error);
            }
            state.set_source_health(&app, CaptionSourceHealth::Error);
            state.set_caption_state(&app, CaptionRuntimeState::Error);
        }
    });
    CaptionController { stop, join }
}

async fn run_session(app: AppHandle, state: AppState, stop: Arc<AtomicBool>) -> Result<(), String> {
    let settings = state.settings.read().map_err(|e| e.to_string())?.clone();
    let source = settings.captions.source.clone();
    let api_key = secrets::get_api_key()?.ok_or_else(|| "请先保存 API Key".to_string())?;
    let session = Arc::new(JsonlSession::start(&state.paths.sessions_dir)?);
    let queue = Arc::new(FinalQueue::default());
    let accepted_context = Arc::new(Mutex::new(VecDeque::<String>::new()));

    if let Some(window) = app.get_webview_window("overlay") {
        if let (Some(x), Some(y)) = (settings.overlay.x, settings.overlay.y) {
            let _ = window.set_position(PhysicalPosition { x, y });
        }
        let _ = window.show();
    }
    if let Ok(mut active) = state.active_source.write() {
        *active = Some(source.clone());
    }
    let translator = spawn_translator(
        app.clone(),
        state.clone(),
        settings.clone(),
        api_key,
        session,
        queue.clone(),
    );
    state.set_caption_state(&app, CaptionRuntimeState::Running);
    state.log("info", format!("字幕会话已启动：{:?}", source));

    let source_result = match &source {
        CaptionSourceConfig::WindowsLiveCaption => {
            run_windows_source(
                &app,
                &state,
                &settings.captions,
                &stop,
                &queue,
                &accepted_context,
            )
            .await
        }
        CaptionSourceConfig::LocalAsr { .. } => {
            run_local_source(
                &app,
                &state,
                &source,
                settings.captions.context_segments,
                &stop,
                &queue,
                &accepted_context,
            )
            .await
        }
    };
    queue.close();
    let mut translator = translator;
    if tokio::time::timeout(Duration::from_secs(10), &mut translator)
        .await
        .is_err()
    {
        translator.abort();
    }
    if let Ok(mut active) = state.active_source.write() {
        *active = None;
    }
    if source_result.is_ok() {
        state.set_caption_state(&app, CaptionRuntimeState::Stopped);
    }
    source_result
}

fn spawn_translator(
    app: AppHandle,
    state: AppState,
    settings: crate::models::AppSettings,
    api_key: String,
    session: Arc<JsonlSession>,
    queue: Arc<FinalQueue>,
) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let session_started = Instant::now();
        let mut previous_end = 0_u64;
        while let Some(job) = queue.pop().await {
            state
                .translation_queue_depth
                .store(queue.depth().await + 1, Ordering::Relaxed);
            let segment = job.segment;
            let request = TranslationRequest {
                mode: TranslationMode::LiveCaption,
                source_text: segment.source_text.clone(),
                source_language: "ja".to_string(),
                target_language: settings.llm.target_language.clone(),
                context: job.context,
            };
            let result = match llm::translate(settings.clone(), api_key.clone(), request).await {
                Ok(value) => value,
                Err(error) => TranslationResult {
                    source_text: segment.source_text.clone(),
                    translated_text: String::new(),
                    model: settings.llm.model.clone(),
                    latency_ms: 0,
                    first_token_ms: 0,
                    cached: false,
                    error: Some(error),
                },
            };
            state
                .translation_queue_depth
                .store(queue.depth().await, Ordering::Relaxed);
            state.last_translation_latency_ms.store(
                result.latency_ms.min(u64::MAX as u128) as u64,
                Ordering::Relaxed,
            );
            state.last_translation_first_token_ms.store(
                result.first_token_ms.min(u64::MAX as u128) as u64,
                Ordering::Relaxed,
            );
            let _ = session.append(&segment, &result);
            let estimated_end = session_started.elapsed().as_millis() as u64;
            let (start_ms, end_ms) = if segment.time_source == CaptionTimeSource::Model {
                (segment.start_ms, segment.end_ms.max(segment.start_ms + 1))
            } else {
                let end = estimated_end;
                (previous_end.max(end.saturating_sub(2_500)), end)
            };
            previous_end = end_ms;
            if let Ok(mut records) = state.caption_exports.write() {
                let index = records.len() + 1;
                records.push(CaptionExportEntry {
                    index,
                    start_ms,
                    end_ms: end_ms.max(start_ms + 500),
                    source_text: segment.source_text.clone(),
                    translated_text: result.translated_text.clone(),
                });
            }
            let _ = app.emit(
                "caption:translated",
                CaptionTranslatedEvent { segment, result },
            );
        }
    })
}

async fn accept_final(
    app: &AppHandle,
    state: &AppState,
    queue: &FinalQueue,
    context: &Mutex<VecDeque<String>>,
    context_limit: usize,
    segment: CaptionSegment,
) {
    let _ = app.emit("caption:segment", &segment);
    let previous = {
        let mut accepted = context.lock().await;
        let previous = accepted.iter().cloned().collect();
        accepted.push_back(segment.source_text.clone());
        while accepted.len() > context_limit {
            accepted.pop_front();
        }
        previous
    };
    queue
        .push(TranslationJob {
            segment,
            context: previous,
        })
        .await;
    state
        .translation_queue_depth
        .store(queue.depth().await, Ordering::Relaxed);
}

async fn run_windows_source(
    app: &AppHandle,
    state: &AppState,
    settings: &crate::models::CaptionSettings,
    stop: &AtomicBool,
    queue: &FinalQueue,
    context: &Mutex<VecDeque<String>>,
) -> Result<(), String> {
    if settings.auto_launch {
        windows_integration::launch_live_captions()?;
    }
    let mut ready = false;
    for _ in 0..60 {
        if windows_integration::live_captions_ready() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if !ready {
        return Err(
            "未找到 Windows Live Captions 字幕节点，请按 Ctrl+Win+L 检查系统字幕".to_string(),
        );
    }
    let mut segmenter = BasicSegmenter::new(settings.clone());
    let poll = Duration::from_millis(settings.poll_milliseconds);
    let mut last_source_text = String::new();
    let mut last_emitted_source = String::new();
    let mut paused_snapshot = None;
    let mut consecutive_errors = 0_u32;
    let mut last_change = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        match windows_integration::read_live_caption_snapshot() {
            Ok(snapshot) => {
                consecutive_errors = 0;
                state.source_error_count.store(0, Ordering::Relaxed);
                let changed = snapshot
                    .caption
                    .as_deref()
                    .is_some_and(|caption| caption != last_source_text);
                if changed {
                    last_change = Instant::now();
                    state.last_caption_update_ms.store(
                        chrono::Utc::now().timestamp_millis().max(0) as u64,
                        Ordering::Relaxed,
                    );
                    state.set_source_health(app, CaptionSourceHealth::Ready);
                } else if snapshot.has_status_message {
                    state.set_source_health(app, CaptionSourceHealth::StatusPrompt);
                } else if last_change.elapsed() > Duration::from_millis(1_500) {
                    state.set_source_health(app, CaptionSourceHealth::Quiet);
                }
                if let Some(text) =
                    accept_caption_snapshot(snapshot, &last_source_text, &mut paused_snapshot)
                {
                    last_source_text.clone_from(&text);
                    let preview = trailing_chars(&text, settings.max_chars * 2);
                    if preview != last_emitted_source {
                        let _ = app.emit("caption:source", &preview);
                        last_emitted_source = preview;
                    }
                    if let Some(segment) = segmenter.update(&text, Instant::now()) {
                        accept_final(
                            app,
                            state,
                            queue,
                            context,
                            settings.context_segments,
                            segment,
                        )
                        .await;
                    }
                }
            }
            Err(error) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                state
                    .source_error_count
                    .store(consecutive_errors, Ordering::Relaxed);
                state.set_source_health(app, CaptionSourceHealth::Stale);
                if consecutive_errors >= 3 {
                    state.set_source_health(app, CaptionSourceHealth::Reconnecting);
                    state.reader_restarts.fetch_add(1, Ordering::Relaxed);
                    windows_integration::recover_stale_live_captions();
                    if settings.auto_launch {
                        let _ = windows_integration::launch_live_captions();
                    }
                    consecutive_errors = 0;
                } else {
                    state.log("warn", error);
                }
            }
        }
        tokio::time::sleep(poll).await;
    }
    Ok(())
}

async fn run_local_source(
    app: &AppHandle,
    state: &AppState,
    source: &CaptionSourceConfig,
    context_limit: usize,
    stop: &AtomicBool,
    queue: &FinalQueue,
    context: &Mutex<VecDeque<String>>,
) -> Result<(), String> {
    let model_id = source
        .model_id()
        .ok_or_else(|| "本地来源没有模型".to_string())?;
    model_manager::verify_model(&state.paths, model_id)?;
    state.set_caption_state(app, CaptionRuntimeState::LoadingModel);
    state.set_source_health(app, CaptionSourceHealth::Reconnecting);
    state.idle_worker_generation.fetch_add(1, Ordering::Relaxed);
    let idle_worker = { state.idle_asr_worker.lock().await.take() };
    let (mut worker, reused) = match idle_worker {
        Some((idle_id, worker)) if idle_id == model_id => (worker, true),
        Some((idle_id, worker)) => {
            state.log("info", format!("切换模型，释放空闲 Worker：{idle_id}"));
            worker.shutdown().await;
            (AsrWorker::spawn(&state.paths).await?, false)
        }
        None => (AsrWorker::spawn(&state.paths).await?, false),
    };
    if !reused {
        worker.load(&state.paths, source).await?;
    }
    worker.start().await?;
    state.set_caption_state(app, CaptionRuntimeState::Running);
    state.set_source_health(app, CaptionSourceHealth::Ready);
    while !stop.load(Ordering::Relaxed) {
        match tokio::time::timeout(Duration::from_millis(250), worker.next()).await {
            Ok(Ok(WorkerEvent::Partial {
                text,
                start_ms,
                end_ms,
                latency_ms,
                model_id,
            })) => {
                state.asr_latency_ms.store(latency_ms, Ordering::Relaxed);
                let _ = app.emit("caption:source", &text);
                let _ = app.emit(
                    "caption:segment",
                    CaptionSegment {
                        id: format!("partial-{start_ms}"),
                        source_text: text,
                        revision: 0,
                        committed: false,
                        source: CaptionSourceKind::LocalAsr,
                        model_id: Some(model_id),
                        start_ms,
                        end_ms,
                        final_result: false,
                        time_source: CaptionTimeSource::Model,
                    },
                );
            }
            Ok(Ok(WorkerEvent::Final {
                text,
                start_ms,
                end_ms,
                latency_ms,
                model_id,
            })) => {
                state.asr_latency_ms.store(latency_ms, Ordering::Relaxed);
                state.last_caption_update_ms.store(
                    chrono::Utc::now().timestamp_millis().max(0) as u64,
                    Ordering::Relaxed,
                );
                accept_final(
                    app,
                    state,
                    queue,
                    context,
                    context_limit,
                    CaptionSegment {
                        id: uuid::Uuid::new_v4().to_string(),
                        source_text: text,
                        revision: 1,
                        committed: true,
                        source: CaptionSourceKind::LocalAsr,
                        model_id: Some(model_id),
                        start_ms,
                        end_ms,
                        final_result: true,
                        time_source: CaptionTimeSource::Model,
                    },
                )
                .await;
            }
            Ok(Ok(WorkerEvent::Health {
                healthy, detail, ..
            })) => {
                state.set_source_health(
                    app,
                    if healthy {
                        CaptionSourceHealth::Ready
                    } else {
                        CaptionSourceHealth::Error
                    },
                );
                state.log("info", format!("ASR Worker: {detail}"));
            }
            Ok(Ok(WorkerEvent::Error {
                code,
                message,
                recoverable,
            })) => {
                state.source_error_count.fetch_add(1, Ordering::Relaxed);
                if !recoverable {
                    worker.shutdown().await;
                    return Err(format!("{code}: {message}"));
                }
                state.log("warn", format!("{code}: {message}"));
            }
            Ok(Ok(WorkerEvent::LoadingProgress { progress, message })) => {
                state.log(
                    "info",
                    format!("ASR 加载 {:.0}%：{}", progress * 100.0, message),
                );
            }
            Ok(Ok(WorkerEvent::Ready { model_id, device })) => {
                state.log("info", format!("ASR 已就绪：{model_id} ({device})"));
            }
            Ok(Ok(_)) | Err(_) => {}
            Ok(Err(error)) => {
                return Err(error);
            }
        }
    }
    let _ = worker.stop().await;
    let generation = state.idle_worker_generation.fetch_add(1, Ordering::Relaxed) + 1;
    *state.idle_asr_worker.lock().await = Some((model_id.to_string(), worker));
    let idle_state = state.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        if idle_state.idle_worker_generation.load(Ordering::Relaxed) == generation {
            let expired_worker = { idle_state.idle_asr_worker.lock().await.take() };
            if let Some((id, worker)) = expired_worker {
                idle_state.log("info", format!("空闲 60 秒，已释放模型：{id}"));
                worker.shutdown().await;
            }
        }
    });
    Ok(())
}

fn accept_caption_snapshot(
    snapshot: windows_integration::LiveCaptionSnapshot,
    last_source_text: &str,
    paused_snapshot: &mut Option<String>,
) -> Option<String> {
    let candidate_changed = snapshot
        .caption
        .as_deref()
        .is_some_and(|caption| caption != last_source_text);
    if snapshot.has_status_message {
        let snapshot_changed = paused_snapshot.as_deref() != Some(snapshot.signature.as_str());
        if paused_snapshot.is_none() || !snapshot_changed || !candidate_changed {
            *paused_snapshot = Some(snapshot.signature);
            return None;
        }
    } else if paused_snapshot.is_some() && !candidate_changed {
        return None;
    }
    let caption = snapshot.caption?;
    *paused_snapshot = snapshot.has_status_message.then_some(snapshot.signature);
    Some(caption)
}

fn trailing_chars(value: &str, limit: usize) -> String {
    let count = value.chars().count();
    if count <= limit {
        value.to_string()
    } else {
        format!("…{}", value.chars().skip(count - limit).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn full_final_queue_merges_without_reordering() {
        let queue = FinalQueue::default();
        for text in ["一", "二", "三", "四", "五"] {
            queue
                .push(TranslationJob {
                    segment: CaptionSegment {
                        id: text.into(),
                        source_text: text.into(),
                        revision: 1,
                        committed: true,
                        source: CaptionSourceKind::LocalAsr,
                        model_id: None,
                        start_ms: 0,
                        end_ms: 1,
                        final_result: true,
                        time_source: CaptionTimeSource::Model,
                    },
                    context: vec![],
                })
                .await;
        }
        assert_eq!(queue.depth().await, 4);
        let mut output = Vec::new();
        for _ in 0..4 {
            output.push(queue.pop().await.unwrap().segment.source_text);
        }
        assert_eq!(output, vec!["一", "二", "三", "四五"]);
    }

    #[test]
    fn vad_presets_match_product_defaults() {
        let normal = crate::models::VadProfile::Normal.parameters();
        let asmr = crate::models::VadProfile::Asmr.parameters();
        assert_eq!(
            (
                normal.threshold,
                normal.silence_commit_ms,
                normal.max_segment_ms,
                normal.partial_interval_ms
            ),
            (0.5, 500, 8000, 800)
        );
        assert_eq!(
            (
                asmr.threshold,
                asmr.silence_commit_ms,
                asmr.max_segment_ms,
                asmr.partial_interval_ms
            ),
            (0.3, 1000, 12000, 1200)
        );
    }
}
