//! Process-scoped mouse bindings and a toolbar independent of text selection.
use crate::{
    models::{AppSettings, MouseAction},
    native_selection_reader::{Selection, SelectionReader},
    AppState,
};
use serde::Serialize;
use std::{
    mem::size_of,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering},
        mpsc::{channel, Sender},
        Arc, Condvar, Mutex, OnceLock, RwLock,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State};
use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, POINT, RECT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::{
            GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RIDI_DEVICEINFO,
            RID_DEVICE_INFO, RIM_TYPEMOUSE,
        },
        WindowsAndMessaging::*,
    },
};

#[derive(Clone, Default, Serialize, PartialEq, Eq)]
pub struct MouseCapabilities {
    pub detected: bool,
    pub buttons: u32,
    pub hook_ready: bool,
    pub error: Option<String>,
}
#[derive(Clone, Default, Serialize)]
pub struct Snapshot {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub mode: Mode,
    pub revision: u64,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Hidden,
    Docked,
    Transient,
    Expanded,
}
#[derive(Clone, Copy)]
struct ReadRequest {
    revision: u64,
    foreground: isize,
}
#[derive(Default)]
pub struct Service {
    snapshot: Mutex<Snapshot>,
    pub generation: AtomicU64,
    capabilities: RwLock<MouseCapabilities>,
    actions: [AtomicU8; 2],
    stopped: AtomicBool,
    manual_poll: AtomicBool,
    hook_thread: AtomicU32,
    pending: Mutex<Option<ReadRequest>>,
    ready: Condvar,
}
impl Service {
    pub fn apply_bindings(&self, config: &crate::models::OcrSettings) {
        let report = self.capabilities.read().unwrap_or_else(|e| e.into_inner());
        let actions = if self.stopped.load(Ordering::SeqCst) {
            [0, 0]
        } else {
            configured_actions(config, &report)
        };
        for (slot, action) in self.actions.iter().zip(actions) {
            slot.store(action, Ordering::SeqCst);
        }
    }
    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        for action in &self.actions {
            action.store(0, Ordering::SeqCst);
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.ready.notify_all();
        let id = self.hook_thread.load(Ordering::SeqCst);
        if id != 0 {
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }
    fn read_selection(&self) {
        let request = ReadRequest {
            revision: self.generation.load(Ordering::SeqCst),
            foreground: unsafe { GetForegroundWindow() }.0 as isize,
        };
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(request);
        self.ready.notify_one();
    }
    pub fn mode(&self) -> Mode {
        self.snapshot.lock().unwrap_or_else(|e| e.into_inner()).mode
    }
}

pub fn detect_mouse() -> MouseCapabilities {
    let mut report = MouseCapabilities::default();
    unsafe {
        let mut count = 0;
        if GetRawInputDeviceList(None, &mut count, size_of::<RAWINPUTDEVICELIST>() as u32)
            == u32::MAX
        {
            report.error = Some("鼠标检测失败，请稍后重试".into());
            return report;
        }
        if count == 0 {
            return report;
        }
        let mut devices = vec![RAWINPUTDEVICELIST::default(); count as usize];
        let found = GetRawInputDeviceList(
            Some(devices.as_mut_ptr()),
            &mut count,
            size_of::<RAWINPUTDEVICELIST>() as u32,
        );
        if found == u32::MAX {
            report.error = Some("鼠标设备正在变化，请稍后重试".into());
            return report;
        }
        for device in devices
            .iter()
            .take(found as usize)
            .filter(|d| d.dwType == RIM_TYPEMOUSE)
        {
            let mut info = RID_DEVICE_INFO {
                cbSize: size_of::<RID_DEVICE_INFO>() as u32,
                ..Default::default()
            };
            let mut size = info.cbSize;
            if GetRawInputDeviceInfoW(
                device.hDevice,
                RIDI_DEVICEINFO,
                Some((&mut info as *mut RID_DEVICE_INFO).cast()),
                &mut size,
            ) != u32::MAX
            {
                report.detected = true;
                report.buttons = report.buttons.max(info.Anonymous.mouse.dwNumberOfButtons);
            }
        }
    }
    report
}
#[tauri::command]
pub async fn get_mouse_capabilities(
    state: State<'_, AppState>,
) -> Result<MouseCapabilities, String> {
    Ok(state
        .toolbar
        .capabilities
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone())
}
#[tauri::command]
pub async fn get_toolbar_snapshot(state: State<'_, AppState>) -> Result<Snapshot, String> {
    Ok(state
        .toolbar
        .snapshot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Area {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
fn placement(area: Area, size: (i32, i32), cursor: Option<POINT>, gap: i32) -> (i32, i32) {
    let max_x = (area.right - size.0 - gap).max(area.left);
    let max_y = (area.bottom - size.1 - gap).max(area.top);
    if let Some(point) = cursor {
        let x = point.x.saturating_add(gap).clamp(area.left, max_x);
        let above = point.y.saturating_sub(size.1 + gap);
        let y = if above >= area.top {
            above
        } else {
            point.y.saturating_add(gap)
        }
        .clamp(area.top, max_y);
        (x, y)
    } else {
        (max_x, max_y)
    }
}
fn visible_cursor() -> Option<POINT> {
    let mut cursor = CURSORINFO {
        cbSize: size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetCursorInfo(&mut cursor).ok()?;
    }
    (cursor.flags == CURSOR_SHOWING).then_some(cursor.ptScreenPos)
}
fn monitor_for(app: &AppHandle, point: Option<POINT>) -> Result<tauri::Monitor, String> {
    let window = app
        .get_webview_window("selection-toolbar")
        .ok_or("工具栏窗口不可用")?;
    let monitors = window.available_monitors().map_err(|e| e.to_string())?;
    let contains = |m: &tauri::Monitor, p: POINT| {
        let a = m.position();
        let s = m.size();
        p.x >= a.x
            && p.y >= a.y
            && (p.x as i64) < a.x as i64 + s.width as i64
            && (p.y as i64) < a.y as i64 + s.height as i64
    };
    if let Some(point) = point {
        if let Some(m) = monitors.iter().find(|m| contains(m, point)) {
            return Ok(m.clone());
        }
    }
    // A hidden/invalid pointer must not select an arbitrary old monitor. Prefer the foreground window.
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(GetForegroundWindow(), &mut rect) }.is_ok() {
        let center = POINT {
            x: ((rect.left as i64 + rect.right as i64) / 2) as i32,
            y: ((rect.top as i64 + rect.bottom as i64) / 2) as i32,
        };
        if let Some(m) = monitors.iter().find(|m| contains(m, center)) {
            return Ok(m.clone());
        }
    }
    window
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .or_else(|| monitors.into_iter().next())
        .ok_or("当前没有可用屏幕".into())
}
fn compact_width(settings: &AppSettings) -> f64 {
    let buttons = usize::from(settings.selection.enabled) * 2
        + usize::from(settings.ocr.enabled)
        + usize::from(settings.captions.enabled);
    // 78px actions, 1px dividers, two 2px gaps around each divider, 10px frame.
    10.0 + buttons.max(1) as f64 * 78.0 + buttons.saturating_sub(1) as f64 * 5.0
}

fn position(app: &AppHandle, mode: Mode, point: Option<POINT>) -> Result<(i32, i32), String> {
    let window = app
        .get_webview_window("selection-toolbar")
        .ok_or("工具栏窗口不可用")?;
    let monitor = monitor_for(app, point)?;
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let logical = if mode == Mode::Expanded {
        (520.0, 340.0)
    } else {
        let state = app.state::<AppState>();
        let settings = state.settings.read().map_err(|e| e.to_string())?;
        (compact_width(&settings), 44.0)
    };
    let size = (
        (logical.0 * scale).round() as i32,
        (logical.1 * scale).round() as i32,
    );
    let area = Area {
        left: work.position.x,
        top: work.position.y,
        right: work.position.x + work.size.width as i32,
        bottom: work.position.y + work.size.height as i32,
    };
    let valid = point
        .filter(|p| p.x >= area.left && p.x < area.right && p.y >= area.top && p.y < area.bottom);
    let pos = placement(
        area,
        size,
        if mode == Mode::Docked { None } else { valid },
        (12.0 * scale).round() as i32,
    );
    window
        .set_focusable(mode == Mode::Expanded)
        .map_err(|e| e.to_string())?;
    // Place on the destination display before applying logical dimensions (mixed DPI).
    window
        .set_position(PhysicalPosition::new(pos.0, pos.1))
        .map_err(|e| e.to_string())?;
    window
        .set_size(LogicalSize::new(logical.0, logical.1))
        .map_err(|e| e.to_string())?;
    window.set_always_on_top(true).map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    if mode == Mode::Expanded {
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(pos)
}
fn publish(app: &AppHandle, state: &AppState, snapshot: &mut Snapshot) {
    snapshot.revision = state.toolbar.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = app.emit_to("selection-toolbar", "toolbar:state", &*snapshot);
}
pub fn show_selection(
    app: &AppHandle,
    state: &AppState,
    point: POINT,
    text: String,
) -> Result<(), String> {
    let mut snapshot = state.toolbar.snapshot.lock().map_err(|e| e.to_string())?;
    if state.ocr.capturing.load(Ordering::SeqCst) || state.toolbar.stopped.load(Ordering::SeqCst) {
        return Ok(());
    }
    let (x, y) = position(app, Mode::Transient, Some(point))?;
    state.toolbar.manual_poll.store(false, Ordering::SeqCst);
    *snapshot = Snapshot {
        text,
        x,
        y,
        mode: Mode::Transient,
        revision: snapshot.revision,
    };
    publish(app, state, &mut snapshot);
    Ok(())
}
pub fn summon(app: &AppHandle, state: &AppState) -> Result<(), String> {
    {
        let mut snapshot = state.toolbar.snapshot.lock().map_err(|e| e.to_string())?;
        if state.ocr.capturing.load(Ordering::SeqCst)
            || state.toolbar.stopped.load(Ordering::SeqCst)
        {
            return Ok(());
        }
        let (x, y) = position(app, Mode::Transient, visible_cursor())?;
        state.toolbar.manual_poll.store(true, Ordering::SeqCst);
        *snapshot = Snapshot {
            text: String::new(),
            x,
            y,
            mode: Mode::Transient,
            revision: snapshot.revision,
        };
        publish(app, state, &mut snapshot);
    }
    state.toolbar.read_selection();
    Ok(())
}
pub fn dismiss(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let resident = state
        .settings
        .read()
        .map_err(|e| e.to_string())?
        .ocr
        .resident_toolbar;
    let mut snapshot = state.toolbar.snapshot.lock().map_err(|e| e.to_string())?;
    let dock = resident
        && !state.ocr.capturing.load(Ordering::SeqCst)
        && !state.toolbar.stopped.load(Ordering::SeqCst);
    if dock {
        // Keep a dismissal on its current display, even after another app takes focus.
        let point = if snapshot.mode == Mode::Hidden {
            visible_cursor()
        } else {
            Some(POINT {
                x: snapshot.x,
                y: snapshot.y,
            })
        };
        let (x, y) = position(app, Mode::Docked, point)?;
        snapshot.x = x;
        snapshot.y = y;
        snapshot.mode = Mode::Docked;
    } else {
        if let Some(w) = app.get_webview_window("selection-toolbar") {
            w.set_focusable(false).map_err(|e| e.to_string())?;
            w.hide().map_err(|e| e.to_string())?;
        }
        snapshot.mode = Mode::Hidden;
    }
    snapshot.text.clear();
    publish(app, state, &mut snapshot);
    Ok(())
}
#[tauri::command]
pub async fn expand_toolbar(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut snapshot = state.toolbar.snapshot.lock().map_err(|e| e.to_string())?;
    if snapshot.text.is_empty() {
        return Err("当前没有可翻译的选区".into());
    }
    let point = POINT {
        x: snapshot.x,
        y: snapshot.y,
    };
    let (x, y) = position(&app, Mode::Expanded, Some(point))?;
    snapshot.mode = Mode::Expanded;
    snapshot.x = x;
    snapshot.y = y;
    publish(&app, &state, &mut snapshot);
    Ok(())
}
pub fn suspend(app: &AppHandle, state: &AppState) {
    let mut snapshot = state
        .toolbar
        .snapshot
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    snapshot.mode = Mode::Hidden;
    snapshot.text.clear();
    publish(app, state, &mut snapshot);
    if let Some(w) = app.get_webview_window("selection-toolbar") {
        let _ = w.hide();
    }
}
fn own_foreground(app: &AppHandle) -> bool {
    let foreground = unsafe { GetForegroundWindow() };
    app.webview_windows()
        .values()
        .any(|window| window.hwnd().is_ok_and(|h| h.0 == foreground.0))
}

struct HookContext {
    service: Arc<Service>,
    sender: Sender<u8>,
    pressed: [AtomicU8; 2],
}
impl HookContext {
    fn button(&self, index: usize, down: bool) -> bool {
        if self.service.stopped.load(Ordering::Relaxed) {
            return false;
        }
        let mut pressed = self.pressed[index].load(Ordering::Relaxed);
        let (consume, action) = button_event(
            &mut pressed,
            self.service.actions[index].load(Ordering::Relaxed),
            down,
        );
        self.pressed[index].store(pressed, Ordering::Relaxed);
        if let Some(action) = action {
            // This channel never waits for capacity. Real, non-injected clicks
            // are queued even while the UI is busy; no four-click overflow.
            if self.sender.send(action).is_err() {
                self.service.shutdown();
            }
        }
        consume
    }
}
static HOOK: OnceLock<HookContext> = OnceLock::new();
fn action_code(action: MouseAction) -> u8 {
    match action {
        MouseAction::Default => 0,
        MouseAction::Toolbar => 1,
        MouseAction::Capture => 2,
    }
}
fn configured_actions(config: &crate::models::OcrSettings, report: &MouseCapabilities) -> [u8; 2] {
    let requested = [config.mouse_button_1, config.mouse_button_2];
    std::array::from_fn(|index| {
        if config.mouse_shortcuts
            && report.hook_ready
            && report.detected
            && report.buttons >= 4 + index as u32
            && (requested[index] != MouseAction::Capture || config.enabled)
        {
            action_code(requested[index])
        } else {
            0
        }
    })
}
fn install_mouse_hook() -> windows::core::Result<HHOOK> {
    unsafe {
        let module = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), HINSTANCE(module.0), 0)
    }
}
fn renew_mouse_hook(hook: &mut Option<HHOOK>) -> windows::core::Result<()> {
    // Install first, then remove the previous handle on this same message
    // thread. Shared pressed state survives renewal, including mid-click.
    let replacement = install_mouse_hook()?;
    if let Some(previous) = hook.replace(replacement) {
        unsafe {
            let _ = UnhookWindowsHookEx(previous);
        }
    }
    Ok(())
}
// Paired decisions ensure rebinding or disconnecting mid-click never leaks an unmatched up event.
fn button_event(pressed: &mut u8, configured: u8, down: bool) -> (bool, Option<u8>) {
    if down {
        // A new physical down starts a fresh pair even if an up was missed
        // while Windows had silently removed the old hook.
        *pressed = configured;
        (*pressed != 0, None)
    } else {
        let previous = std::mem::take(pressed);
        (previous != 0, (previous != 0).then_some(previous))
    }
}
unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 && (w.0 as u32 == WM_XBUTTONDOWN || w.0 as u32 == WM_XBUTTONUP) {
        if let Some(context) = HOOK.get() {
            let event = &*(l.0 as *const MSLLHOOKSTRUCT);
            let button = (event.mouseData >> 16) as usize;
            if (1..=2).contains(&button)
                && event.flags & 1 == 0
                && !context.service.stopped.load(Ordering::Relaxed)
                && context.button(button - 1, w.0 as u32 == WM_XBUTTONDOWN)
            {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, w, l)
}
pub fn start(app: AppHandle, state: AppState) {
    let (sender, receiver) = channel();
    let service = state.toolbar.clone();
    if HOOK
        .set(HookContext {
            service: service.clone(),
            sender,
            pressed: Default::default(),
        })
        .is_err()
    {
        return;
    }
    let hook_state = state.clone();
    thread::spawn(move || unsafe {
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        service
            .hook_thread
            .store(GetCurrentThreadId(), Ordering::SeqCst);
        *service
            .capabilities
            .write()
            .unwrap_or_else(|e| e.into_inner()) = detect_mouse();
        let mut hook = None;
        let mut refresh = || {
            let result = renew_mouse_hook(&mut hook);
            {
                let mut report = service
                    .capabilities
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                report.hook_ready = result.is_ok();
                report.error = result
                    .err()
                    .map(|e| format!("侧键监听失败，将自动重试：{e}"));
            }
            // Never wait for a settings write on the input hook thread.
            if let Ok(settings) = hook_state.settings.try_read() {
                service.apply_bindings(&settings.ocr);
            }
        };
        refresh();
        // Windows does not report timeout-removal of low-level hooks. Renew
        // periodically without injecting mouse input or dropping a click pair.
        let timer = SetTimer(None, 0, 5_000, None);
        if timer == 0 {
            hook_state.log("error", "无法创建侧键监听恢复计时器，请重启应用");
        }
        while !service.stopped.load(Ordering::SeqCst) && GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if timer != 0 && msg.message == WM_TIMER && msg.wParam.0 == timer {
                refresh();
            } else {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if timer != 0 {
            let _ = KillTimer(None, timer);
        }
        if let Some(hook) = hook {
            let _ = UnhookWindowsHookEx(hook);
        }
        service
            .capabilities
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .hook_ready = false;
        for action in &service.actions {
            action.store(0, Ordering::SeqCst);
        }
    });
    let action_app = app.clone();
    let action_state = state.clone();
    thread::spawn(move || {
        while !action_state.toolbar.stopped.load(Ordering::SeqCst) {
            let action = match receiver.recv_timeout(Duration::from_millis(250)) {
                Ok(action) => action,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if action_state.toolbar.stopped.load(Ordering::SeqCst) {
                break;
            }
            if action_state.ocr.capturing.load(Ordering::SeqCst) {
                continue;
            }
            if action == 1 {
                if let Err(e) = summon(&action_app, &action_state) {
                    action_state.log("warn", e);
                }
            } else if action == 2 {
                let app = action_app.clone();
                let state = action_state.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = crate::ocr::start(app.clone(), state).await {
                        crate::show_main_window(&app);
                        let _ = app.emit_to("main", "ocr:error", error);
                    }
                });
            }
        }
    });
    let reader_app = app.clone();
    let reader_state = state.clone();
    thread::spawn(move || {
        let reader = SelectionReader::new();
        loop {
            let service = &reader_state.toolbar;
            let guard = service.pending.lock().unwrap_or_else(|e| e.into_inner());
            let mut guard = service
                .ready
                .wait_while(guard, |p| {
                    p.is_none() && !service.stopped.load(Ordering::SeqCst)
                })
                .unwrap_or_else(|e| e.into_inner());
            if service.stopped.load(Ordering::SeqCst) {
                break;
            }
            let Some(request) = guard.take() else {
                continue;
            };
            drop(guard);
            let current = || {
                service.generation.load(Ordering::SeqCst) == request.revision
                    && unsafe { GetForegroundWindow() }.0 as isize == request.foreground
                    && !reader_state.ocr.capturing.load(Ordering::SeqCst)
                    && !service.stopped.load(Ordering::SeqCst)
            };
            let text = if current() && !own_foreground(&reader_app) {
                match &reader {
                    Ok(reader) => match reader.read(None, &current) {
                        Ok(Selection::Text(text)) => text.trim().to_string(),
                        _ => String::new(),
                    },
                    Err(_) => String::new(),
                }
            } else {
                String::new()
            };
            let mut snapshot = service.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            if current()
                && matches!(snapshot.mode, Mode::Transient | Mode::Docked)
                && snapshot.text != text
            {
                snapshot.text = text;
                publish(&reader_app, &reader_state, &mut snapshot);
            }
        }
    });
    thread::spawn(move || {
        let mut tick = 0;
        while !state.toolbar.stopped.load(Ordering::SeqCst) {
            if tick % 4 == 0 {
                let mut report = detect_mouse();
                let mut current = state
                    .toolbar
                    .capabilities
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                report.hook_ready = current.hook_ready;
                if !current.hook_ready && current.error.is_some() {
                    report.error = current.error.clone();
                }
                if *current != report {
                    *current = report.clone();
                    let _ = app.emit("mouse:capabilities", report);
                }
            }
            if let Ok(settings) = state.settings.try_read() {
                let config = &settings.ocr;
                state.toolbar.apply_bindings(config);
                let resident = config.resident_toolbar;
                drop(settings);
                if state.ocr.capturing.load(Ordering::SeqCst) {
                    if state.toolbar.mode() != Mode::Hidden {
                        suspend(&app, &state);
                    }
                } else {
                    let mode = state.toolbar.mode();
                    if (resident && mode == Mode::Hidden) || (!resident && mode == Mode::Docked) {
                        if let Err(e) = dismiss(&app, &state) {
                            state.log("warn", e);
                        }
                    }
                    let mode = state.toolbar.mode();
                    if mode == Mode::Docked
                        || (mode == Mode::Transient
                            && state.toolbar.manual_poll.load(Ordering::SeqCst))
                    {
                        state.toolbar.read_selection();
                    }
                }
            }
            tick = (tick + 1) % 4;
            thread::sleep(Duration::from_millis(500));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bound_context() -> (HookContext, std::sync::mpsc::Receiver<u8>) {
        let (sender, receiver) = channel();
        let service = Arc::new(Service::default());
        *service.capabilities.write().unwrap() = MouseCapabilities {
            detected: true,
            buttons: 5,
            hook_ready: true,
            error: None,
        };
        service.apply_bindings(&crate::models::OcrSettings {
            mouse_shortcuts: true,
            mouse_button_1: MouseAction::Toolbar,
            mouse_button_2: MouseAction::Capture,
            ..Default::default()
        });
        (
            HookContext {
                service,
                sender,
                pressed: Default::default(),
            },
            receiver,
        )
    }
    #[test]
    fn rapid_bound_clicks_are_consumed_and_dispatched_without_queue_loss() {
        let (context, receiver) = bound_context();
        for index in 0..128 {
            assert!(context.button(index % 2, true));
            assert!(context.button(index % 2, false));
        }
        assert_eq!(
            receiver.try_iter().collect::<Vec<_>>(),
            (0..128)
                .map(|index| 1 + (index % 2) as u8)
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn saved_binding_applies_immediately_without_changing_an_inflight_pair() {
        let (context, receiver) = bound_context();
        assert!(context.button(0, true));
        context
            .service
            .apply_bindings(&crate::models::OcrSettings::default());
        assert!(context.button(0, false));
        assert_eq!(receiver.try_recv().unwrap(), 1);
        assert!(!context.button(0, true));
        assert!(!context.button(0, false));
        assert!(receiver.try_recv().is_err());
    }
    #[test]
    fn shutdown_immediately_restores_pass_through_and_cannot_be_reenabled() {
        let (context, receiver) = bound_context();
        context.service.shutdown();
        context.service.apply_bindings(&crate::models::OcrSettings {
            mouse_shortcuts: true,
            mouse_button_1: MouseAction::Toolbar,
            ..Default::default()
        });
        assert!(!context.button(0, true));
        assert!(!context.button(0, false));
        assert!(receiver.try_recv().is_err());
    }
    #[test]
    fn a_new_down_recovers_from_an_up_missed_during_hook_removal() {
        let mut pressed = 1;
        assert_eq!(button_event(&mut pressed, 2, true), (true, None));
        assert_eq!(button_event(&mut pressed, 0, false), (true, Some(2)));
    }
    #[test]
    fn compact_size_tracks_enabled_buttons_including_captions() {
        let mut settings = AppSettings::default();
        assert_eq!(compact_width(&settings), 337.0);
        settings.selection.enabled = false;
        assert_eq!(compact_width(&settings), 171.0);
        settings.ocr.enabled = false;
        assert_eq!(compact_width(&settings), 88.0);
    }

    #[test]
    fn placement_respects_negative_monitors_and_taskbar() {
        let a = Area {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        assert_eq!(placement(a, (246, 44), None, 12), (-258, 984));
        assert_eq!(
            placement(a, (246, 44), Some(POINT { x: -1, y: 1039 }), 12),
            (-258, 983)
        );
        assert_eq!(
            placement(a, (246, 44), Some(POINT { x: -1919, y: 0 }), 12),
            (-1907, 12)
        );
    }
    #[test]
    fn mixed_dpi_and_small_work_areas_stay_bounded() {
        assert_eq!(
            placement(
                Area {
                    left: 0,
                    top: -100,
                    right: 160,
                    bottom: 90
                },
                (246, 44),
                None,
                12
            ),
            (0, 34)
        );
        assert_eq!(
            placement(
                Area {
                    left: 0,
                    top: 0,
                    right: 3840,
                    bottom: 2080
                },
                (492, 88),
                None,
                24
            ),
            (3324, 1968)
        );
    }
    #[test]
    fn defaults_do_not_intercept_and_bound_pairs_survive_reconfiguration() {
        let mut pressed = 0;
        assert_eq!(button_event(&mut pressed, 0, true), (false, None));
        assert_eq!(button_event(&mut pressed, 1, false), (false, None));
        assert_eq!(button_event(&mut pressed, 1, true), (true, None));
        assert_eq!(button_event(&mut pressed, 0, false), (true, Some(1)));
        assert_eq!(button_event(&mut pressed, 0, false), (false, None));
    }
    #[test]
    fn old_settings_leave_all_entry_points_disabled() {
        let config: crate::models::OcrSettings = serde_json::from_str("{}").unwrap();
        assert!(!config.resident_toolbar && !config.mouse_shortcuts);
        assert_eq!(config.mouse_button_1, MouseAction::Default);
        assert_eq!(config.mouse_button_2, MouseAction::Default);
    }
    #[test]
    fn missing_devices_and_disabled_switches_pass_through() {
        let mut config = crate::models::OcrSettings {
            mouse_shortcuts: true,
            mouse_button_1: MouseAction::Toolbar,
            mouse_button_2: MouseAction::Capture,
            ..Default::default()
        };
        let mut report = MouseCapabilities {
            detected: true,
            buttons: 5,
            hook_ready: true,
            error: None,
        };
        assert_eq!(configured_actions(&config, &report), [1, 2]);
        config.enabled = false;
        assert_eq!(configured_actions(&config, &report), [1, 0]);
        config.enabled = true;
        report.buttons = 4;
        assert_eq!(configured_actions(&config, &report), [1, 0]);
        report.buttons = 3;
        assert_eq!(configured_actions(&config, &report), [0, 0]);
        report.buttons = 5;
        report.detected = false;
        assert_eq!(configured_actions(&config, &report), [0, 0]);
        report.detected = true;
        report.hook_ready = false;
        assert_eq!(configured_actions(&config, &report), [0, 0]);
        report.hook_ready = true;
        config.mouse_shortcuts = false;
        assert_eq!(configured_actions(&config, &report), [0, 0]);
    }
    #[test]
    #[ignore = "requires an interactive Windows desktop; installs a pass-through hook then removes it"]
    fn windows_device_and_hook_smoke() {
        let report = detect_mouse();
        assert!(report.error.is_none(), "{:?}", report.error);
        println!(
            "Mouse detected: {}, reported buttons: {}",
            report.detected, report.buttons
        );
        // No HookContext is installed in this test, so all input passes through.
        let mut hook = None;
        renew_mouse_hook(&mut hook).expect("install mouse hook");
        unsafe {
            let timer = SetTimer(None, 0, 10, None);
            assert_ne!(timer, 0, "create renewal timer");
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                if message.message == WM_TIMER && message.wParam.0 == timer {
                    renew_mouse_hook(&mut hook).expect("renew mouse hook without a gap");
                    break;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            KillTimer(None, timer).expect("release renewal timer");
            UnhookWindowsHookEx(hook.unwrap()).expect("release mouse hook");
        }
    }
}
