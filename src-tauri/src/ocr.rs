//! Screenshot OCR is isolated from selection/caption events and runs only on demand.
use crate::{
    model_manager,
    models::{TranslationMode, TranslationRequest},
    AppState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::{watch, Mutex},
};

pub const MODEL_ID: &str = "ocr-ppocr-v5-mobile";
const IDLE_SECONDS: u64 = 60;

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Block {
    pub id: u32,
    pub text: String,
    pub bounds: [u32; 4],
    pub confidence: f64,
    #[serde(default)]
    pub translation: String,
    #[serde(default)]
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Default)]
pub struct ResultView {
    pub session: u64,
    pub status: String,
    pub blocks: Vec<Block>,
    pub error: Option<String>,
    pub load_ms: u64,
    pub recognize_ms: u64,
    pub source_language: String,
    pub target_language: String,
}
#[derive(Clone, Serialize)]
pub struct Frame {
    pub id: u64,
    pub image: String,
    pub width: u32,
    pub height: u32,
}
pub struct Service {
    transition: Mutex<()>,
    worker: Mutex<Option<Worker>>,
    idle_generation: AtomicU64,
    pub capturing: AtomicBool,
    generation: AtomicU64,
    cancel: watch::Sender<u64>,
    frame: Mutex<Option<Frame>>,
    hidden: Mutex<Vec<String>>,
    result: RwLock<ResultView>,
}
impl Default for Service {
    fn default() -> Self {
        Self {
            transition: Mutex::new(()),
            worker: Mutex::new(None),
            idle_generation: AtomicU64::new(0),
            capturing: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            cancel: watch::channel(0).0,
            frame: Mutex::new(None),
            hidden: Mutex::new(Vec::new()),
            result: RwLock::new(ResultView::default()),
        }
    }
}
struct Worker {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    verified: bool,
}
impl Worker {
    fn start() -> Result<Self, String> {
        let packaged = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .unwrap()
            .join("ocr-worker/livecaption-ocr-worker.exe");
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ocr-worker/dist/livecaption-ocr-worker/livecaption-ocr-worker.exe");
        let exe = if packaged.exists() { packaged } else { dev };
        if !exe.exists() {
            return Err("OCR 运行组件缺失，请重新安装完整版本".into());
        }
        let mut command = Command::new(exe);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().map_err(|e| format!("OCR 启动失败：{e}"))?;
        Ok(Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            verified: false,
        })
    }
    async fn request(&mut self, request: Value) -> Result<Value, String> {
        let mut bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .await
            .map_err(|e| e.to_string())?;
        self.input.flush().await.map_err(|e| e.to_string())?;
        let mut response = Vec::new();
        loop {
            let data = self.output.fill_buf().await.map_err(|e| e.to_string())?;
            if data.is_empty() {
                return Err("OCR 进程已退出，请重试；若重复发生，请重新安装运行组件".into());
            }
            let end = data.iter().position(|b| *b == b'\n').map(|i| i + 1);
            let count = end.unwrap_or(data.len());
            if response.len() + count > 64 * 1024 * 1024 {
                return Err("OCR 返回数据过大".into());
            }
            response.extend_from_slice(&data[..count]);
            self.output.consume(count);
            if end.is_some() {
                break;
            }
        }
        let response: Value = serde_json::from_slice(&response).map_err(|e| e.to_string())?;
        if response["ok"] != true {
            return Err(response["error"].as_str().unwrap_or("OCR 失败").to_string());
        }
        Ok(response)
    }
}
impl Service {
    fn advance(&self) -> u64 {
        let id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.cancel.send_replace(id);
        id
    }
    fn current(&self, id: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == id
    }
    fn publish(&self, app: &AppHandle) {
        if let Ok(view) = self.result.read() {
            let _ = app.emit_to("ocr-result", "ocr:result", &*view);
        }
    }
    async fn request(self: &Arc<Self>, state: &AppState, request: Value) -> Result<Value, String> {
        let generation = self.idle_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let mut guard = self.worker.lock().await;
        let mut cancel = self.cancel.subscribe();
        if guard.is_none() {
            *guard = Some(Worker::start()?);
        }
        let worker = guard.as_mut().unwrap();
        let result = async {
            if request["command"] != "capture" && !worker.verified {
                let paths = state.paths.clone();
                tokio::task::spawn_blocking(move || model_manager::verify_model(&paths, MODEL_ID)).await.map_err(|e| e.to_string())??;
                worker.verified = true;
            }
            tokio::select! {
                _ = cancel.changed() => Err("OCR 已取消".to_string()),
                result = tokio::time::timeout(Duration::from_secs(90), worker.request(request)) => result.map_err(|_| "OCR 超时，请缩小选区后重试".to_string())?
            }
        }.await;
        if result.is_err() {
            if let Some(mut worker) = guard.take() {
                let _ = worker.child.kill().await;
            }
        }
        drop(guard);
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(IDLE_SECONDS)).await;
            let mut guard = service.worker.lock().await;
            if service.idle_generation.load(Ordering::SeqCst) == generation {
                if let Some(mut worker) = guard.take() {
                    let _ = worker.child.kill().await;
                }
            }
        });
        result
    }
    pub async fn shutdown(&self) {
        self.advance();
        if let Some(mut worker) = self.worker.lock().await.take() {
            let _ = worker.child.kill().await;
        }
    }
    pub async fn delete_model(&self, state: &AppState) -> Result<(), String> {
        if self.capturing.load(Ordering::SeqCst) || state.model_downloads.is_active(MODEL_ID).await
        {
            return Err("OCR 正在使用或下载，请结束后再删除".into());
        }
        let mut guard = self
            .worker
            .try_lock()
            .map_err(|_| "OCR 正在识别，请稍后再删除")?;
        if let Some(mut worker) = guard.take() {
            let _ = worker.child.kill().await;
        }
        model_manager::delete_model(&state.paths, MODEL_ID)
    }
    async fn restore(&self, app: &AppHandle) {
        for label in self.hidden.lock().await.drain(..) {
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.show();
            }
        }
    }
}
pub async fn test(state: &AppState) -> Result<u64, String> {
    let result = state
        .ocr
        .request(
            state,
            json!({"command":"test", "model_path":state.paths.models_dir.join(MODEL_ID)}),
        )
        .await?;
    Ok(result["load_ms"].as_u64().unwrap_or(0) + result["recognize_ms"].as_u64().unwrap_or(0))
}
#[tauri::command]
pub async fn start_ocr_capture(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    start(app, state.inner().clone()).await
}
pub async fn start(app: AppHandle, state: AppState) -> Result<(), String> {
    let _transition = state.ocr.transition.lock().await;
    if !state
        .settings
        .read()
        .map_err(|e| e.to_string())?
        .ocr
        .enabled
    {
        return Err("截图翻译已关闭，请在设置 → 功能中启用".into());
    }
    if state.ocr.capturing.swap(true, Ordering::SeqCst) {
        return Err("正在框选，请先完成或按 Esc 取消".into());
    }
    let id = state.ocr.advance();
    let result = async {
        let manifest = model_manager::manifest(MODEL_ID)?;
        if !manifest.files.iter().all(|f| {
            std::fs::metadata(state.paths.models_dir.join(MODEL_ID).join(f.name))
                .is_ok_and(|m| m.len() == f.size)
        }) {
            return Err("请先在设置 → 资源 → 文字识别 · OCR 下载或修复模型".to_string());
        }
        let main = app.get_webview_window("main").ok_or("主窗口不可用")?;
        let monitors = main.available_monitors().map_err(|e| e.to_string())?;
        let mut point = windows::Win32::Foundation::POINT::default();
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point)
                .map_err(|e| e.to_string())?;
        }
        let monitor = monitors
            .iter()
            .find(|m| {
                let p = m.position();
                let s = m.size();
                point.x >= p.x
                    && point.y >= p.y
                    && (point.x as i64) < p.x as i64 + s.width as i64
                    && (point.y as i64) < p.y as i64 + s.height as i64
            })
            .ok_or("未找到当前屏幕")?;
        for label in ["main", "overlay", "selection-toolbar", "ocr-result"] {
            if let Some(window) = app.get_webview_window(label) {
                if window.is_visible().unwrap_or(false) {
                    if label != "selection-toolbar" && label != "ocr-result" {
                        state.ocr.hidden.lock().await.push(label.to_string());
                    }
                    window.hide().map_err(|e| e.to_string())?;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(160)).await;
        let p = monitor.position();
        let s = monitor.size();
        if let Some(result) = app.get_webview_window("ocr-result") {
            let scale = monitor.scale_factor();
            let _ = result.set_position(tauri::PhysicalPosition::new(
                p.x + ((s.width as f64 - 520.0 * scale) / 2.0).max(0.0) as i32,
                p.y + ((s.height as f64 - 340.0 * scale) / 2.0).max(0.0) as i32,
            ));
        }
        let capture = state
            .ocr
            .request(
                &state,
                json!({"command":"capture", "monitor":[p.x,p.y,s.width,s.height]}),
            )
            .await?;
        if !state.ocr.current(id) {
            return Err("截图已取消".into());
        }
        let frame = Frame {
            id,
            image: capture["image"].as_str().ok_or("截图无效")?.into(),
            width: capture["width"].as_u64().ok_or("截图无效")? as u32,
            height: capture["height"].as_u64().ok_or("截图无效")? as u32,
        };
        *state.ocr.frame.lock().await = Some(frame);
        let window = app
            .get_webview_window("ocr-capture")
            .ok_or("框选窗口不可用")?;
        window.set_position(*p).map_err(|e| e.to_string())?;
        window.set_size(*s).map_err(|e| e.to_string())?;
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        let _ = window.emit("ocr:capture", ());
        Ok(())
    }
    .await;
    if result.is_err() {
        state.ocr.capturing.store(false, Ordering::SeqCst);
        *state.ocr.frame.lock().await = None;
        state.ocr.restore(&app).await;
    }
    result
}
#[tauri::command]
pub async fn get_ocr_capture(state: State<'_, AppState>) -> Result<Option<Frame>, String> {
    Ok(state.ocr.frame.lock().await.clone())
}
#[tauri::command]
pub fn get_ocr_result(state: State<'_, AppState>) -> Result<ResultView, String> {
    state
        .ocr
        .result
        .read()
        .map(|v| v.clone())
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn cancel_ocr(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.ocr.advance();
    let _transition = state.ocr.transition.lock().await;
    *state.ocr.frame.lock().await = None;
    let _ = app.emit_to("ocr-capture", "ocr:clear-capture", ());
    for label in ["ocr-capture", "ocr-result"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
    state.ocr.restore(&app).await;
    state.ocr.capturing.store(false, Ordering::SeqCst);
    Ok(())
}
fn validate_rect(rect: [u32; 4], width: u32, height: u32) -> Result<(), String> {
    let [x, y, w, h] = rect;
    if w < 8
        || h < 8
        || x.checked_add(w).is_none_or(|r| r > width)
        || y.checked_add(h).is_none_or(|b| b > height)
    {
        return Err("框选区域太小或超出屏幕".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn recognize_ocr(
    id: u64,
    rect: [u32; 4],
    source_language: Option<String>,
    target_language: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _transition = state.ocr.transition.lock().await;
    let defaults = state.settings.read().map_err(|e| e.to_string())?.clone();
    let source = source_language
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(defaults.ocr.source_language);
    let target = target_language.filter(|s| !s.trim().is_empty()).unwrap_or({
        if defaults.ocr.target_language.is_empty() {
            defaults.llm.target_language
        } else {
            defaults.ocr.target_language
        }
    });
    if source.len() > 80 || target.len() > 80 {
        return Err("请选择有效语言".into());
    }
    let mut frame_guard = state.ocr.frame.lock().await;
    let frame = frame_guard
        .as_ref()
        .filter(|f| f.id == id)
        .ok_or("截图已过期，请重新框选")?;
    validate_rect(rect, frame.width, frame.height)?;
    let frame = frame_guard.take().unwrap();
    drop(frame_guard);
    if !state.ocr.current(id) {
        return Err("截图已取消".into());
    }
    if let Some(w) = app.get_webview_window("ocr-capture") {
        let _ = w.hide();
    }
    state.ocr.restore(&app).await;
    state.ocr.capturing.store(false, Ordering::SeqCst);
    *state.ocr.result.write().map_err(|e| e.to_string())? = ResultView {
        session: id,
        status: "recognizing".into(),
        source_language: source,
        target_language: target,
        ..Default::default()
    };
    state.ocr.publish(&app);
    let window = app
        .get_webview_window("ocr-result")
        .ok_or("结果窗口不可用")?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        if !state.ocr.current(id) {
            return;
        }
        let result=state.ocr.request(&state,json!({"command":"recognize","image":frame.image,"rect":rect,"model_path":state.paths.models_dir.join(MODEL_ID)})).await;
        if !state.ocr.current(id) {
            return;
        }
        let outcome = result.and_then(|v| {
            let blocks: Vec<Block> =
                serde_json::from_value(v["blocks"].clone()).map_err(|e| e.to_string())?;
            if blocks.len() > 100 {
                return Err("文字块超过 100 个，请缩小截图范围".into());
            }
            let mut view = state.ocr.result.write().map_err(|e| e.to_string())?;
            if view.session != id {
                return Ok(());
            }
            view.blocks = blocks;
            view.load_ms = v["load_ms"].as_u64().unwrap_or(0);
            view.recognize_ms = v["recognize_ms"].as_u64().unwrap_or(0);
            view.status = if view.blocks.is_empty() {
                "empty"
            } else {
                "translating"
            }
            .into();
            Ok(())
        });
        if let Err(error) = outcome {
            if let Ok(mut v) = state.ocr.result.write() {
                if v.session == id {
                    v.status = "error".into();
                    v.error = Some(error);
                }
            }
        }
        state.ocr.publish(&app);
        translate_blocks(app, state, id).await;
    });
    Ok(())
}
async fn translate_blocks(app: AppHandle, state: AppState, id: u64) {
    let mut cancel = state.ocr.cancel.subscribe();
    if !state.ocr.current(id) {
        return;
    }
    let view = match state.ocr.result.read() {
        Ok(v) => v.clone(),
        Err(_) => return,
    };
    if view.status != "translating" {
        return;
    }
    let settings = match state.settings.read() {
        Ok(v) => v.clone(),
        Err(_) => return,
    };
    let key = crate::secrets::get_api_key()
        .and_then(|v| v.ok_or("请先在设置 → 应用 → 翻译服务中保存 API Key".into()));
    let glossary = crate::translation_glossary(&state);
    for block in view.blocks {
        if !state.ocr.current(id) {
            return;
        }
        let result = match &key {
            Err(error) => Err(error.clone()),
            Ok(key) => {
                let request = TranslationRequest {
                    mode: TranslationMode::Screenshot,
                    source_text: block.text.clone(),
                    source_language: view.source_language.clone(),
                    target_language: view.target_language.clone(),
                    context: vec![],
                    glossary: glossary.clone(),
                };
                tokio::select! {
                    _=cancel.changed()=>return,
                    result=crate::llm::translate_with_delta(settings.clone(),key.clone(),request,|delta| {
                        if !state.ocr.current(id){return;}
                        if let Ok(mut v)=state.ocr.result.write() { if v.session == id { if let Some(b)=v.blocks.iter_mut().find(|b|b.id==block.id) {b.translation.push_str(&delta);} } }
                        state.ocr.publish(&app);
                    })=>result
                }
            }
        };
        if !state.ocr.current(id) {
            return;
        }
        if let Ok(mut v) = state.ocr.result.write() {
            if v.session != id {
                return;
            }
            if let Some(b) = v.blocks.iter_mut().find(|b| b.id == block.id) {
                match result {
                    Ok(r) => b.translation = r.translated_text,
                    Err(e) => b.error = Some(e),
                }
            }
        }
        state.ocr.publish(&app);
    }
    if state.ocr.current(id) {
        if let Ok(mut v) = state.ocr.result.write() {
            if v.session == id {
                v.status = "ready".into();
            }
        }
        state.ocr.publish(&app);
    }
}
#[tauri::command]
pub async fn retry_ocr_translation(
    source_language: String,
    target_language: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if target_language.trim().is_empty() || source_language.len() > 80 || target_language.len() > 80
    {
        return Err("请选择有效语言".into());
    }
    let _transition = state.ocr.transition.lock().await;
    if state.ocr.capturing.load(Ordering::SeqCst) {
        return Err("请先完成截图框选".into());
    }
    if state
        .ocr
        .result
        .read()
        .map_err(|e| e.to_string())?
        .blocks
        .is_empty()
    {
        return Err("没有可翻译的文字，请重新截图".into());
    }
    let id = state.ocr.advance();
    {
        let mut view = state.ocr.result.write().map_err(|e| e.to_string())?;
        view.session = id;
        view.status = "translating".into();
        view.source_language = source_language;
        view.target_language = target_language;
        for b in &mut view.blocks {
            b.translation.clear();
            b.error = None;
        }
    }
    state.ocr.publish(&app);
    let state = state.inner().clone();
    tauri::async_runtime::spawn(translate_blocks(app, state, id));
    Ok(())
}
#[tauri::command]
pub fn open_ocr_resources(app: AppHandle) {
    crate::show_main_window(&app);
    let _ = app.emit_to("main", "ocr:resources", ());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires built OCR worker and downloaded models; exercises the actual Rust UTF-8 pipe"]
    async fn worker_loads_models_through_utf8_protocol() {
        let paths = crate::settings::AppPaths::shared().unwrap();
        model_manager::verify_model(&paths, MODEL_ID).unwrap();
        let mut worker = Worker::start().unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(90),
            worker
                .request(json!({"command": "test", "model_path": paths.models_dir.join(MODEL_ID)})),
        )
        .await;
        let _ = worker.child.kill().await;
        let response = result.unwrap().unwrap();
        let blocks = response["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 1, "{response}");
        assert!(blocks[0]["text"].as_str().unwrap().contains("OCR test 123"));
    }

    #[test]
    fn crop_rejects_overflow_and_out_of_bounds() {
        assert!(validate_rect([u32::MAX, 0, 8, 8], 1920, 1080).is_err());
        assert!(validate_rect([1900, 0, 30, 30], 1920, 1080).is_err());
        assert!(validate_rect([0, 0, 7, 8], 1920, 1080).is_err());
        assert!(validate_rect([0, 0, 1920, 1080], 1920, 1080).is_ok());
    }
    #[test]
    fn generation_cancels_previous_session() {
        let s = Service::default();
        let first = s.advance();
        assert!(s.current(first));
        let second = s.advance();
        assert!(!s.current(first));
        assert!(s.current(second));
        assert_eq!(*s.cancel.subscribe().borrow(), second);
    }
}
