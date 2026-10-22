use std::{
    os::windows::process::CommandExt,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MIN_SELECTION_DRAG_DISTANCE: i32 = 10;
const MIN_SELECTION_DRAG_DURATION: Duration = Duration::from_millis(80);
const SELECTION_REQUEST_MAX_AGE: Duration = Duration::from_millis(1500);

use arboard::Clipboard;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition};
use windows::Win32::{
    Foundation::POINT,
    System::DataExchange::GetClipboardSequenceNumber,
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
            KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LBUTTON,
            VK_LWIN, VK_MENU, VK_SHIFT,
        },
        WindowsAndMessaging::{GetCursorPos, GetForegroundWindow},
    },
};

use crate::{
    models::{SelectionReadyEvent, SelectionTriggerMode},
    native_selection_reader::{Selection, SelectionReader},
    AppState,
};

const VK_C: VIRTUAL_KEY = VIRTUAL_KEY(0x43);
const VK_L: VIRTUAL_KEY = VIRTUAL_KEY(0x4C);

#[derive(Clone)]
struct ParsedHotkey {
    modifiers: Vec<i32>,
    key: i32,
}

pub fn validate_hotkey(value: &str) -> Result<(), String> {
    parse_hotkey(value).map(|_| ())
}

fn parse_hotkey(value: &str) -> Result<ParsedHotkey, String> {
    let parts: Vec<_> = value.split('+').filter(|v| !v.is_empty()).collect();
    if parts.len() < 2 {
        return Err("快捷键必须包含 Ctrl、Alt、Shift 或 Win 修饰键".into());
    }
    let mut modifiers = Vec::new();
    let mut key = None;
    for part in parts {
        match part {
            "Ctrl" => modifiers.push(VK_CONTROL.0 as i32),
            "Alt" => modifiers.push(VK_MENU.0 as i32),
            "Shift" => modifiers.push(VK_SHIFT.0 as i32),
            "Super" | "Win" => modifiers.push(VK_LWIN.0 as i32),
            value if value.starts_with("Key") && value.len() == 4 => {
                key = Some(value.as_bytes()[3].to_ascii_uppercase() as i32)
            }
            value if value.starts_with("Digit") && value.len() == 6 => {
                key = Some(value.as_bytes()[5] as i32)
            }
            value if value.starts_with('F') => {
                let number = value[1..]
                    .parse::<i32>()
                    .map_err(|_| "不支持这个快捷键".to_string())?;
                if !(1..=12).contains(&number) {
                    return Err("仅支持 F1 到 F12".into());
                }
                key = Some(0x70 + number - 1);
            }
            _ => return Err("当前只支持字母、数字和 F1-F12 组合键".into()),
        }
    }
    if modifiers.is_empty() {
        return Err("快捷键必须包含修饰键".into());
    }
    let key = key.ok_or_else(|| "快捷键缺少主按键".to_string())?;
    if modifiers.contains(&(VK_LWIN.0 as i32))
        && modifiers.contains(&(VK_CONTROL.0 as i32))
        && key == VK_L.0 as i32
    {
        return Err("Ctrl+Win+L 是 Windows Live Captions 系统快捷键，请换一个组合".into());
    }
    Ok(ParsedHotkey { modifiers, key })
}

pub fn start_selection_hotkey_watcher(app: AppHandle, state: AppState) {
    let generation = Arc::new(AtomicU64::new(0));
    let requests = start_selection_reader(app.clone(), state.clone(), Arc::clone(&generation));
    thread::spawn(move || {
        let mut was_down = false;
        let mut mouse_was_down = false;
        let mut was_automatic = false;
        let mut source_window = unsafe { GetForegroundWindow() }.0 as isize;
        let mut mouse_started: Option<(POINT, Instant)> = None;
        loop {
            thread::sleep(Duration::from_millis(24));
            let foreground = unsafe { GetForegroundWindow() }.0 as isize;
            let mouse_down = key_down(VK_LBUTTON.0 as i32);
            if foreground != source_window || (mouse_down && !mouse_was_down) {
                generation.fetch_add(1, Ordering::Relaxed);
            }
            if foreground != source_window {
                mouse_started = None;
                source_window = foreground;
            }
            mouse_was_down = mouse_down;
            let app_window_focused = ["main", "selection-toolbar", "overlay"]
                .iter()
                .any(|label| app_window_blocks_selection(&app, label));
            if app_window_focused {
                generation.fetch_add(1, Ordering::Relaxed);
                was_down = false;
                mouse_started = None;
                continue;
            }
            let (selection_enabled, trigger_mode, hotkey_value, clipboard_fallback) =
                match state.settings.read() {
                    Ok(v) => (
                        v.selection.enabled,
                        v.selection.trigger_mode.clone(),
                        v.selection.hotkey.clone(),
                        v.selection.clipboard_fallback_enabled,
                    ),
                    Err(_) => continue,
                };
            if !selection_enabled {
                generation.fetch_add(1, Ordering::Relaxed);
                was_down = false;
                mouse_started = None;
                continue;
            }
            let automatic = trigger_mode == SelectionTriggerMode::Automatic;
            if automatic != was_automatic {
                generation.fetch_add(1, Ordering::Relaxed);
                mouse_started = None;
                was_down = false;
                was_automatic = automatic;
            }
            if key_down(VK_ESCAPE.0 as i32) {
                generation.fetch_add(1, Ordering::Relaxed);
                mouse_started = None;
                continue;
            }
            if automatic {
                if mouse_down && mouse_started.is_none() {
                    mouse_started = cursor_position().map(|point| (point, Instant::now()));
                } else if !mouse_down {
                    if let Some((start, started)) = mouse_started.take() {
                        if let Some(end) = cursor_position() {
                            if is_intentional_selection_drag(start, end, started.elapsed()) {
                                queue_selection(
                                    &requests,
                                    &generation,
                                    end,
                                    clipboard_fallback,
                                    false,
                                );
                            }
                        }
                    }
                }
                was_down = false;
                continue;
            }
            let hotkey = match parse_hotkey(&hotkey_value) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let down = key_down(hotkey.key) && hotkey.modifiers.iter().all(|key| key_down(*key));
            if down && !was_down {
                // Do not inject Ctrl+C while the user's modifiers are still held.
                let release_started = Instant::now();
                while release_started.elapsed() < Duration::from_millis(700)
                    && (key_down(hotkey.key) || hotkey.modifiers.iter().any(|key| key_down(*key)))
                {
                    thread::sleep(Duration::from_millis(10));
                }
                if !key_down(hotkey.key) && !hotkey.modifiers.iter().any(|key| key_down(*key)) {
                    if let Some(point) = cursor_position() {
                        queue_selection(&requests, &generation, point, clipboard_fallback, true);
                    }
                }
            }
            was_down = down;
        }
    });
}

fn app_window_blocks_selection(app: &AppHandle, label: &str) -> bool {
    let Some(window) = app.get_webview_window(label) else {
        return false;
    };
    let visible = window.is_visible().unwrap_or(false);
    let minimized = visible && window.is_minimized().unwrap_or(true);
    let focused = visible && !minimized && window.is_focused().unwrap_or(false);
    window_state_blocks_selection(visible, minimized, focused)
}

fn window_state_blocks_selection(visible: bool, minimized: bool, focused: bool) -> bool {
    visible && !minimized && focused
}

fn is_intentional_selection_drag(start: POINT, end: POINT, elapsed: Duration) -> bool {
    let distance = (end.x - start.x).abs() + (end.y - start.y).abs();
    distance >= MIN_SELECTION_DRAG_DISTANCE && elapsed >= MIN_SELECTION_DRAG_DURATION
}

struct SelectionRequest {
    generation: u64,
    source_window: isize,
    point: POINT,
    use_focused_element: bool,
    clipboard_fallback: bool,
    sequence_before: u32,
    started: Instant,
}

// One in-flight read plus one replaceable pending request, even if a provider
// ignores its timeout. Never create a thread per drag or queue stale work.
#[derive(Default)]
struct SelectionRequests {
    latest: Mutex<Option<SelectionRequest>>,
    ready: Condvar,
}

impl SelectionRequests {
    fn submit(&self, request: SelectionRequest) {
        *self
            .latest
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(request);
        self.ready.notify_one();
    }

    fn next(&self) -> SelectionRequest {
        let pending = self
            .latest
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.ready
            .wait_while(pending, |pending| pending.is_none())
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .expect("selection request signalled")
    }
}

fn queue_selection(
    requests: &SelectionRequests,
    generation: &AtomicU64,
    point: POINT,
    clipboard_fallback: bool,
    use_focused_element: bool,
) {
    requests.submit(SelectionRequest {
        generation: generation.fetch_add(1, Ordering::Relaxed) + 1,
        source_window: unsafe { GetForegroundWindow() }.0 as isize,
        point,
        use_focused_element,
        clipboard_fallback,
        sequence_before: unsafe { GetClipboardSequenceNumber() },
        started: Instant::now(),
    });
}

fn start_selection_reader(
    app: AppHandle,
    state: AppState,
    generation: Arc<AtomicU64>,
) -> Arc<SelectionRequests> {
    let requests = Arc::new(SelectionRequests::default());
    let worker_requests = Arc::clone(&requests);
    thread::spawn(move || {
        // Construct and drop all UIA/COM objects on this long-lived MTA thread.
        let reader = SelectionReader::new();
        loop {
            let request = worker_requests.next();
            let is_current = || {
                selection_request_is_current(
                    request.generation,
                    generation.load(Ordering::Relaxed),
                    request.source_window,
                    unsafe { GetForegroundWindow() }.0 as isize,
                    request.started.elapsed(),
                ) && !key_down(VK_LBUTTON.0 as i32)
                    && !key_down(VK_ESCAPE.0 as i32)
            };
            let result = capture_selected_text(
                request.clipboard_fallback,
                &is_current,
                || match &reader {
                    Ok(reader) => reader.read(
                        (!request.use_focused_element).then_some(request.point),
                        &is_current,
                    ),
                    Err(error) => Err(error.clone()),
                },
                || capture_compatible_selected_text(request.sequence_before, &is_current),
            );
            if is_current() {
                state.log(
                    "debug",
                    format!("选区读取耗时 {}ms", request.started.elapsed().as_millis()),
                );
                emit_selection_result(&app, &state, request.point, result);
            }
        }
    });
    requests
}

fn selection_request_is_current(
    requested_generation: u64,
    current_generation: u64,
    source_window: isize,
    foreground: isize,
    elapsed: Duration,
) -> bool {
    requested_generation == current_generation
        && source_window == foreground
        && elapsed < SELECTION_REQUEST_MAX_AGE
}

fn emit_selection_result(
    app: &AppHandle,
    state: &AppState,
    point: POINT,
    result: Result<String, String>,
) {
    match result.and_then(nonempty_selection) {
        Ok(text) => {
            let event = SelectionReadyEvent {
                text,
                x: point.x,
                y: point.y,
            };
            let _ = app.emit("selection:ready", &event);
            if let Err(error) = show_toolbar_at_point(app, point) {
                state.log("warn", format!("划词工具条显示失败：{error}"));
            }
        }
        Err(error) => {
            state.log("debug", error);
        }
    }
}

fn nonempty_selection(text: String) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Err("当前应用没有可读取的选中文本".to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn capture_selected_text(
    clipboard_fallback: bool,
    is_current: &impl Fn() -> bool,
    read_native: impl FnOnce() -> Result<Selection, String>,
    copy_fallback: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    if !is_current() {
        return Err("选区读取已取消".to_string());
    }
    let selection = read_native()?;
    if !is_current() {
        return Err("选区读取已取消".to_string());
    }
    match selection {
        Selection::Text(text) => nonempty_selection(text),
        Selection::Empty => Err("当前应用没有可读取的选中文本".to_string()),
        Selection::EmbeddedObjects | Selection::Unsupported if clipboard_fallback => {
            let recovered = copy_fallback()?;
            if !is_current() {
                return Err("选区读取已取消".to_string());
            }
            let recovered = nonempty_selection(recovered)?;
            // Never send known-incomplete text to the toolbar or translation.
            // Removing/guessing a placeholder would silently lose the emoji.
            if recovered.contains('\u{fffc}') {
                Err("源应用复制的选区仍含对象占位符，无法取得完整文字".to_string())
            } else {
                Ok(recovered)
            }
        }
        Selection::EmbeddedObjects => {
            Err("选区包含 emoji 或嵌入对象，需要启用兼容复制以读取完整文字".to_string())
        }
        Selection::Unsupported => Err("当前应用不支持读取文本选区".to_string()),
    }
}

fn capture_compatible_selected_text(
    sequence_before: u32,
    is_current: &impl Fn() -> bool,
) -> Result<String, String> {
    // A real Ctrl+C always wins. Keep observing long enough for the target
    // application to publish its clipboard content before considering a
    // compatibility copy operation.
    if let Some(text) = changed_clipboard_text(sequence_before)? {
        return Ok(text);
    }
    let grace_started = Instant::now();
    let mut user_copy_detected = false;
    while grace_started.elapsed() < Duration::from_millis(320) {
        if !is_current() {
            return Err("选区读取已取消".to_string());
        }
        if let Some(text) = changed_clipboard_text(sequence_before)? {
            return Ok(text);
        }
        if key_down(VK_CONTROL.0 as i32) || key_down(VK_C.0 as i32) {
            user_copy_detected = true;
        }
        thread::sleep(Duration::from_millis(8));
    }

    if user_copy_detected {
        return Err("检测到用户正在复制，本次自动划词不注入 Ctrl+C".to_string());
    }
    if !is_current() {
        return Err("选区读取已取消".to_string());
    }
    capture_selected_text_via_copy(is_current)
}

fn changed_clipboard_text(sequence_before: u32) -> Result<Option<String>, String> {
    if unsafe { GetClipboardSequenceNumber() } == sequence_before {
        return Ok(None);
    }
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    let text = clipboard.get_text().map_err(|error| error.to_string())?;
    let trimmed = text.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

pub fn start_toolbar_dismiss_watcher(app: AppHandle, state: AppState) {
    thread::spawn(move || {
        let mut mouse_was_down = false;
        let mut escape_was_down = false;
        loop {
            thread::sleep(Duration::from_millis(30));
            let Some(window) = app.get_webview_window("selection-toolbar") else {
                continue;
            };
            if !window.is_visible().unwrap_or(false) {
                mouse_was_down = false;
                continue;
            }

            let escape_down = key_down(VK_ESCAPE.0 as i32);
            if escape_down && !escape_was_down {
                let compact = window
                    .outer_size()
                    .ok()
                    .map(|size| size.width as f64 / window.scale_factor().unwrap_or(1.0) <= 320.0)
                    .unwrap_or(false);
                if compact {
                    if let Err(error) = hide_selection_toolbar(&app) {
                        state.log("warn", format!("划词工具条隐藏失败：{error}"));
                    }
                }
            }
            escape_was_down = escape_down;

            let mouse_down = key_down(VK_LBUTTON.0 as i32);
            if mouse_down && !mouse_was_down {
                if let (Some(cursor), Ok(position), Ok(size)) = (
                    cursor_position(),
                    window.outer_position(),
                    window.outer_size(),
                ) {
                    let inside = cursor.x >= position.x
                        && cursor.y >= position.y
                        && cursor.x < position.x + size.width as i32
                        && cursor.y < position.y + size.height as i32;
                    let logical_width = size.width as f64 / window.scale_factor().unwrap_or(1.0);
                    if !inside && logical_width <= 320.0 {
                        if let Err(error) = hide_selection_toolbar(&app) {
                            state.log("warn", format!("划词工具条隐藏失败：{error}"));
                        }
                    }
                }
            }
            mouse_was_down = mouse_down;
        }
    });
}

fn capture_selected_text_via_copy(is_current: &impl Fn() -> bool) -> Result<String, String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    let previous = clipboard.get_text().ok();
    let sequence = unsafe { GetClipboardSequenceNumber() };
    if !is_current()
        || [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_C]
            .iter()
            .any(|key| key_down(key.0 as i32))
    {
        return Err("选区已失效或用户正在操作键盘，本次不注入 Ctrl+C".to_string());
    }
    send_copy_shortcut();

    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(260) {
        if unsafe { GetClipboardSequenceNumber() } != sequence {
            break;
        }
        thread::sleep(Duration::from_millis(12));
    }
    if unsafe { GetClipboardSequenceNumber() } == sequence {
        return Err("当前没有可复制的选中文本".into());
    }

    let selected = clipboard.get_text().map_err(|error| error.to_string())?;
    if let Some(previous) = previous {
        if previous != selected {
            let _ = clipboard.set_text(previous);
        }
    }
    let trimmed = selected.trim().to_string();
    if trimmed.is_empty() {
        Err("没有读取到选中文本".into())
    } else {
        Ok(trimmed)
    }
}

fn show_toolbar_at_point(app: &AppHandle, point: POINT) -> Result<(), String> {
    let window = app
        .get_webview_window("selection-toolbar")
        .ok_or_else(|| "工具条窗口不存在".to_string())?;
    window
        .set_focusable(false)
        .map_err(|error| format!("无法恢复不可聚焦状态：{error}"))?;
    window
        .set_size(LogicalSize::new(246.0, 44.0))
        .map_err(|error| format!("无法恢复工具条尺寸：{error}"))?;
    let size = window.outer_size().unwrap_or(tauri::PhysicalSize {
        width: 246,
        height: 44,
    });
    let monitor = window.available_monitors().ok().and_then(|monitors| {
        monitors.into_iter().find(|monitor| {
            let work_area = monitor.work_area();
            point.x >= work_area.position.x
                && point.x < work_area.position.x + work_area.size.width as i32
                && point.y >= work_area.position.y
                && point.y < work_area.position.y + work_area.size.height as i32
        })
    });
    let (left, top, right, bottom) = monitor
        .map(|monitor| {
            let work_area = monitor.work_area();
            (
                work_area.position.x,
                work_area.position.y,
                work_area.position.x + work_area.size.width as i32,
                work_area.position.y + work_area.size.height as i32,
            )
        })
        .unwrap_or((0, 0, 1920, 1080));
    let max_x = (right - size.width as i32).max(left);
    let max_y = (bottom - size.height as i32).max(top);
    let preferred_x = point.x.saturating_add(12);
    let preferred_y = point.y.saturating_sub(size.height as i32 + 12);
    let x = preferred_x.clamp(left, max_x);
    let y = if preferred_y < top {
        point.y.saturating_add(18).clamp(top, max_y)
    } else {
        preferred_y.clamp(top, max_y)
    };
    window
        .set_position(PhysicalPosition { x, y })
        .map_err(|error| format!("无法移动工具条：{error}"))?;
    window
        .show()
        .map_err(|error| format!("无法显示工具条：{error}"))?;
    Ok(())
}

pub fn hide_selection_toolbar(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("selection-toolbar")
        .ok_or_else(|| "工具条窗口不存在".to_string())?;
    let focus_error = window.set_focusable(false).err();
    let hide_error = window.hide().err();
    match (focus_error, hide_error) {
        (None, None) => Ok(()),
        (Some(focus), None) => Err(format!("无法恢复不可聚焦状态：{focus}")),
        (None, Some(hide)) => Err(format!("无法隐藏工具条：{hide}")),
        (Some(focus), Some(hide)) => Err(format!(
            "无法恢复不可聚焦状态：{focus}；无法隐藏工具条：{hide}"
        )),
    }
}

pub fn launch_live_captions() -> Result<(), String> {
    if live_captions_ready() {
        return Ok(());
    }
    recover_stale_live_captions();
    thread::sleep(Duration::from_millis(250));
    if Command::new("LiveCaptions.exe").spawn().is_ok() {
        return Ok(());
    }
    let inputs = [
        keyboard_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_LWIN, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_L, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_L, KEYEVENTF_KEYUP),
        keyboard_input(VK_LWIN, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
    Ok(())
}

pub fn recover_stale_live_captions() {
    if live_captions_process_exists() && !live_captions_ready() {
        let _ = powershell_command()
            .args([
                "-NoProfile",
                "-Command",
                "Get-Process LiveCaptions -ErrorAction SilentlyContinue | Stop-Process -Force",
            ])
            .output();
    }
}

pub fn live_captions_ready() -> bool {
    crate::native_caption_reader::is_ready()
        || run_uia_script(LIVE_CAPTIONS_READY_SCRIPT)
            .map(|text| text == "READY")
            .unwrap_or(false)
}

pub struct LiveCaptionSnapshot {
    pub signature: String,
    pub caption: Option<String>,
    pub has_status_message: bool,
}

pub fn read_live_caption_snapshot() -> Result<LiveCaptionSnapshot, String> {
    // Prefer the in-process UI Automation reader.  PowerShell remains a
    // compatibility fallback for older Windows builds where the generated
    // UIA proxy is not available to the app process.
    let text = crate::native_caption_reader::read_text()
        .or_else(|_| run_uia_script(LIVE_CAPTIONS_TEXT_SCRIPT))?;
    if text.trim().is_empty() && !live_captions_ready() {
        return Err("Windows Live Captions 字幕节点暂时不可用".to_string());
    }
    if text.contains('\u{FFFD}') {
        return Err("Windows Live Captions 返回了无效编码，正在等待字幕源恢复".to_string());
    }
    Ok(parse_live_caption_snapshot(&text))
}

fn parse_live_caption_snapshot(text: &str) -> LiveCaptionSnapshot {
    let candidates = text
        .split('\u{001e}')
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .collect::<Vec<_>>();
    let has_status_message = candidates
        .iter()
        .any(|candidate| is_live_caption_status_message(candidate));
    let caption = candidates
        .iter()
        .rev()
        .find_map(|candidate| clean_live_caption_candidate(candidate));
    LiveCaptionSnapshot {
        signature: text.to_string(),
        caption,
        has_status_message,
    }
}

fn clean_live_caption_candidate(text: &str) -> Option<String> {
    let without_known_status = text
        .replace("请尝试关闭一些应用程序以提高性能。", "")
        .replace("请尝试关闭一些应用程序以提高性能", "")
        .replace("请尝试关闭一些应用以提高性能。", "")
        .replace("请尝试关闭一些应用以提高性能", "");
    let cleaned = without_known_status.trim();
    (!cleaned.is_empty() && !is_live_caption_status_message(cleaned)).then(|| cleaned.to_string())
}

fn is_live_caption_status_message(text: &str) -> bool {
    let compact = text.split_whitespace().collect::<String>();
    let lower = compact.to_lowercase();
    (compact.contains("提高性能") && compact.contains("关闭") && compact.contains("应用"))
        || (lower.contains("performance")
            && lower.contains("app")
            && (lower.contains("closing") || lower.contains("close")))
        || (compact.contains("パフォーマンス")
            && compact.contains("アプリ")
            && compact.contains("閉じ"))
}

fn live_captions_process_exists() -> bool {
    powershell_command()
        .args(["-NoProfile", "-Command", "if (Get-Process LiveCaptions -ErrorAction SilentlyContinue) { exit 0 } else { exit 1 }"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

const UIA_PREFIX: &str = r#"
Add-Type -AssemblyName UIAutomationClient
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class LiveCaptionUiaNative {
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
  public static IntPtr FindWindow(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint candidate; GetWindowThreadProcessId(h, out candidate); if (candidate == pid) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
}
"@
$p = Get-Process LiveCaptions -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { exit 2 }
$handle = [LiveCaptionUiaNative]::FindWindow([uint32]$p.Id)
if ($handle -eq [IntPtr]::Zero) { exit 3 }
$window = [System.Windows.Automation.AutomationElement]::FromHandle($handle)
$byId = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, "CaptionsTextBlock")
$captionElements = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byId)
$element = $captionElements | Select-Object -Last 1
if (-not $element) {
  $byType = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Text)
  $items = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byType)
  $element = $items | Sort-Object { $_.Current.Name.Length } -Descending | Select-Object -First 1
}
"#;
const LIVE_CAPTIONS_READY_SCRIPT: &str = r#"if ($element) { Write-Output "READY" }"#;
const LIVE_CAPTIONS_TEXT_SCRIPT: &str = r#"
$values = @()
if ($captionElements) {
  foreach ($candidate in $captionElements) {
    if ($candidate.Current.Name -and $candidate.Current.AutomationId -ne "ReadyToCaptionTextBlock") {
      $values += $candidate.Current.Name
    }
  }
}
if ($values.Count -eq 0 -and $element -and $element.Current.Name -and $element.Current.AutomationId -ne "ReadyToCaptionTextBlock") {
  $values += $element.Current.Name
}
if ($values.Count -gt 0) {
  Write-Output ([string]::Join([char]0x1e, $values))
}
"#;

fn run_uia_script(body: &str) -> Result<String, String> {
    run_powershell(&format!("{UIA_PREFIX}\n{body}"))
}
fn run_powershell(script: &str) -> Result<String, String> {
    let wrapped = format!(
        "$utf8 = [System.Text.UTF8Encoding]::new($false)\n[Console]::OutputEncoding = $utf8\n$OutputEncoding = $utf8\n{script}"
    );
    let output = powershell_command()
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &wrapped,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Ok(String::new());
    }
    Ok(decode_powershell_output(&output.stdout).trim().to_string())
}

fn powershell_command() -> Command {
    let mut command = Command::new("powershell");
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn decode_powershell_output(bytes: &[u8]) -> String {
    let utf16 = bytes.starts_with(&[0xFF, 0xFE])
        || (!bytes.is_empty() && bytes.iter().filter(|byte| **byte == 0).count() > bytes.len() / 8);
    if utf16 {
        let start = usize::from(bytes.starts_with(&[0xFF, 0xFE])) * 2;
        let units = bytes[start..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn cursor_position() -> Option<POINT> {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point).ok()? };
    Some(point)
}
fn key_down(key: i32) -> bool {
    (unsafe { GetAsyncKeyState(key) }) < 0
}
fn send_copy_shortcut() {
    let inputs = [
        keyboard_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_C, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_C, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}
fn keyboard_input(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        capture_selected_text, decode_powershell_output, is_intentional_selection_drag,
        nonempty_selection, parse_live_caption_snapshot, run_powershell,
        selection_request_is_current, validate_hotkey, window_state_blocks_selection,
        SelectionRequest, SelectionRequests, SELECTION_REQUEST_MAX_AGE,
    };
    use crate::native_selection_reader::Selection;
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
    use windows::Win32::Foundation::POINT;

    #[test]
    fn selection_toolbar_requires_nonblank_text() {
        for text in ["", " ", "\r\n\t", "\u{00a0}\u{3000}"] {
            assert!(nonempty_selection(text.to_string()).is_err());
        }
        assert_eq!(
            nonempty_selection(" \n选中的文字\n第二行\t ".to_string()).unwrap(),
            "选中的文字\n第二行"
        );
    }

    #[test]
    fn cancelled_selection_does_not_read_the_clipboard_or_inject_copy() {
        assert_eq!(
            capture_selected_text(
                true,
                &|| false,
                || panic!("cancelled request must not read UIA"),
                || panic!("cancelled request must not copy"),
            ),
            Err("选区读取已取消".to_string())
        );
    }

    #[test]
    fn native_selection_bypasses_clipboard_grace_period() {
        assert_eq!(
            capture_selected_text(
                true,
                &|| true,
                || Ok(Selection::Text(" 选中的文字 ".into())),
                || panic!("native text must bypass the entire clipboard path"),
            ),
            Ok("选中的文字".into())
        );
    }

    #[test]
    fn readable_native_emoji_keep_the_fast_path() {
        let text = "YouTube 评论 🎉👍🏽👨‍👩‍👧‍👦🇨🇳❤️1️⃣";
        assert_eq!(
            capture_selected_text(
                true,
                &|| true,
                || Ok(Selection::Text(text.into())),
                || panic!("readable emoji must not trigger clipboard access"),
            ),
            Ok(text.into())
        );
    }

    #[test]
    fn embedded_emoji_use_original_clipboard_text() {
        for original in ["🎉👍🏽👨‍👩‍👧‍👦🇨🇳", "YouTube 评论 ❤️\n下一行 1️⃣🙂"]
        {
            assert_eq!(
                capture_selected_text(
                    true,
                    &|| true,
                    || Ok(Selection::EmbeddedObjects),
                    || Ok(original.into()),
                ),
                Ok(original.into())
            );
        }
        assert!(capture_selected_text(
            false,
            &|| true,
            || Ok(Selection::EmbeddedObjects),
            || panic!("disabled clipboard recovery must be respected"),
        )
        .is_err());
    }

    #[test]
    fn failed_or_incomplete_emoji_recovery_never_reaches_the_toolbar() {
        for recovered in [
            Err("source app did not copy".into()),
            Ok(" \r\n".into()),
            Ok("\u{fffc}\u{fffc}".into()),
            Ok("评论 🙂\u{fffc}".into()),
        ] {
            assert!(capture_selected_text(
                true,
                &|| true,
                || Ok(Selection::EmbeddedObjects),
                || recovered,
            )
            .is_err());
        }
    }

    #[test]
    fn cancelling_during_emoji_recovery_discards_the_result() {
        let current = Cell::new(true);
        assert_eq!(
            capture_selected_text(
                true,
                &|| current.get(),
                || Ok(Selection::EmbeddedObjects),
                || {
                    current.set(false);
                    Ok("🙂".into())
                },
            ),
            Err("选区读取已取消".into())
        );
    }

    #[test]
    fn empty_or_failed_native_read_never_copies_a_whole_line() {
        for result in [
            Ok(Selection::Empty),
            Ok(Selection::Text(" \n\t".into())),
            Err("provider timed out".to_string()),
        ] {
            assert!(capture_selected_text(
                true,
                &|| true,
                || result,
                || panic!("empty selection or provider failure must not copy"),
            )
            .is_err());
        }
    }

    #[test]
    fn unsupported_app_uses_copy_only_when_enabled() {
        assert_eq!(
            capture_selected_text(
                true,
                &|| true,
                || Ok(Selection::Unsupported),
                || Ok("兼容选区".into()),
            ),
            Ok("兼容选区".into())
        );
        assert!(capture_selected_text(
            false,
            &|| true,
            || Ok(Selection::Unsupported),
            || panic!("disabled fallback must not copy"),
        )
        .is_err());
    }

    #[test]
    fn cancellation_during_native_read_discards_text_and_prevents_fallback() {
        for result in [
            Selection::Text("过期文字".into()),
            Selection::Unsupported,
            Selection::EmbeddedObjects,
        ] {
            let current = Cell::new(true);
            assert_eq!(
                capture_selected_text(
                    true,
                    &|| current.get(),
                    || {
                        current.set(false);
                        Ok(result)
                    },
                    || panic!("stale request must not inject copy"),
                ),
                Err("选区读取已取消".into())
            );
        }
    }

    #[test]
    fn pending_selection_is_replaced_by_the_latest_drag() {
        let requests = SelectionRequests::default();
        for generation in 1..=100 {
            requests.submit(SelectionRequest {
                generation,
                source_window: 1,
                point: POINT {
                    x: generation as i32,
                    y: 0,
                },
                use_focused_element: false,
                clipboard_fallback: false,
                sequence_before: 0,
                started: Instant::now(),
            });
        }
        let latest = requests.next();
        assert_eq!(latest.generation, 100);
        assert_eq!(latest.point.x, 100);
        assert!(requests.latest.lock().unwrap().is_none());
    }

    #[test]
    fn stale_selection_or_changed_foreground_never_shows_toolbar() {
        assert!(selection_request_is_current(1, 1, 10, 10, Duration::ZERO));
        assert!(!selection_request_is_current(1, 2, 10, 10, Duration::ZERO));
        assert!(!selection_request_is_current(1, 1, 10, 20, Duration::ZERO));
        assert!(!selection_request_is_current(
            1,
            1,
            10,
            10,
            SELECTION_REQUEST_MAX_AGE
        ));
    }

    #[test]
    fn automatic_selection_requires_a_deliberate_drag() {
        let start = POINT { x: 100, y: 100 };
        assert!(!is_intentional_selection_drag(
            start,
            start,
            Duration::from_millis(200)
        ));
        assert!(!is_intentional_selection_drag(
            start,
            POINT { x: 104, y: 103 },
            Duration::from_millis(200)
        ));
        assert!(!is_intentional_selection_drag(
            start,
            POINT { x: 130, y: 100 },
            Duration::from_millis(40)
        ));
        assert!(is_intentional_selection_drag(
            start,
            POINT { x: 130, y: 100 },
            Duration::from_millis(120)
        ));
    }

    #[test]
    fn hidden_app_windows_never_block_selection() {
        assert!(!window_state_blocks_selection(false, false, false));
        assert!(!window_state_blocks_selection(false, false, true));
        assert!(!window_state_blocks_selection(true, true, true));
        assert!(!window_state_blocks_selection(true, false, false));
        assert!(window_state_blocks_selection(true, false, true));
    }

    #[test]
    fn requires_a_modifier_and_reserves_live_caption_shortcut() {
        assert!(validate_hotkey("KeyA").is_err());
        assert!(validate_hotkey("Ctrl+Super+KeyL").is_err());
        assert!(validate_hotkey("Alt+KeyQ").is_ok());
        assert!(validate_hotkey("Ctrl+Shift+F8").is_ok());
    }

    #[test]
    fn decodes_japanese_powershell_output() {
        let text = "忘れるよ\r\n";
        assert_eq!(decode_powershell_output(text.as_bytes()), text);
        let utf16 = text
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(decode_powershell_output(&utf16), text);
    }

    #[test]
    fn ignores_stale_performance_warning_and_selects_resumed_caption() {
        let warning = "请尝试关闭一些应用程序以提高性能。";
        let paused = parse_live_caption_snapshot(warning);
        assert!(paused.has_status_message);
        assert_eq!(paused.caption, None);
        let resumed = parse_live_caption_snapshot(&format!(
            "古い字幕\u{001e}{warning}\u{001e}新しい字幕です"
        ));
        assert!(resumed.has_status_message);
        assert_eq!(resumed.caption, Some("新しい字幕です".to_string()));
    }

    #[test]
    fn extracts_caption_when_warning_and_new_text_share_one_node() {
        let snapshot =
            parse_live_caption_snapshot("请尝试关闭一些应用程序以提高性能。 新しい字幕です");
        assert!(snapshot.has_status_message);
        assert_eq!(snapshot.caption, Some("新しい字幕です".to_string()));
    }

    #[test]
    fn captures_real_powershell_japanese_output() {
        assert_eq!(
            run_powershell("Write-Output '忘れるよ'").unwrap(),
            "忘れるよ"
        );
    }
}
