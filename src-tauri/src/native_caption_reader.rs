//! Native UI Automation reader for Windows Live Captions.
//!
//! The first implementation used a PowerShell UIA bridge.  That bridge is a
//! useful compatibility fallback, but spawning PowerShell for every polling
//! tick adds jitter and can leave a stale process when Live Captions changes
//! its window.  This module keeps the hot path in-process and only returns the
//! raw UIA candidates; status filtering remains in `windows_integration` so it
//! is shared by both readers.

use std::mem::size_of;

use windows::{
    core::BSTR,
    Win32::{
        Foundation::{CloseHandle, BOOL, HWND, LPARAM},
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_MULTITHREADED,
            },
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_TextControlTypeId,
            },
            WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId},
        },
    },
};

const PROCESS_NAME: &str = "livecaptions.exe";
const CAPTION_AUTOMATION_ID: &str = "CaptionsTextBlock";
const READY_AUTOMATION_ID: &str = "ReadyToCaptionTextBlock";

/// Read the current Live Captions UIA nodes as a record-separator-delimited
/// string.  COM is initialized and torn down on the calling thread because
/// the Tokio worker that polls captions is free to move between threads.
pub fn read_text() -> Result<String, String> {
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
        if initialized.is_err() {
            return Err(format!("COM 初始化失败: {initialized:?}"));
        }
        let result = read_text_inner();
        CoUninitialize();
        result
    }
}

pub fn is_ready() -> bool {
    read_text().is_ok()
}

unsafe fn read_text_inner() -> Result<String, String> {
    let pid = find_process_id()?.ok_or_else(|| "LiveCaptions 进程尚未启动".to_string())?;
    let hwnd = find_window(pid)?.ok_or_else(|| "LiveCaptions 窗口尚未创建".to_string())?;
    let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        .map_err(|error| format!("UI Automation 初始化失败: {error:?}"))?;
    let root = automation
        .ElementFromHandle(hwnd)
        .map_err(|error| format!("读取 Live Captions 窗口失败: {error:?}"))?;
    let condition = automation
        .CreateTrueCondition()
        .map_err(|error| format!("创建 UIA 查询条件失败: {error:?}"))?;
    let elements = root
        .FindAll(TreeScope_Descendants, &condition)
        .map_err(|error| format!("枚举 Live Captions 节点失败: {error:?}"))?;

    let mut caption_nodes = Vec::new();
    let mut text_nodes = Vec::new();
    let length = elements
        .Length()
        .map_err(|error| format!("读取 UIA 节点数量失败: {error:?}"))?;
    for index in 0..length {
        let element = match elements.GetElement(index) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let name = match element.CurrentName() {
            Ok(value) => bstr_to_string(value),
            Err(_) => continue,
        };
        if name.trim().is_empty() {
            continue;
        }
        let automation_id = element
            .CurrentAutomationId()
            .map(bstr_to_string)
            .unwrap_or_default();
        if automation_id == READY_AUTOMATION_ID {
            continue;
        }
        if automation_id == CAPTION_AUTOMATION_ID {
            caption_nodes.push(name);
            continue;
        }
        if element
            .CurrentControlType()
            .is_ok_and(|control_type| control_type == UIA_TextControlTypeId)
        {
            text_nodes.push(name);
        }
    }

    if caption_nodes.is_empty() {
        // Older Windows builds expose the caption text as a generic Text
        // control. Keep all text nodes so the shared parser can identify the
        // status prompt and choose the newest caption.
        caption_nodes = text_nodes;
    }
    Ok(caption_nodes.join("\u{001e}"))
}

unsafe fn find_process_id() -> Result<Option<u32>, String> {
    let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
        .map_err(|error| format!("枚举进程失败: {error:?}"))?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = None;
    if Process32FirstW(snapshot, &mut entry).is_ok() {
        loop {
            let name = String::from_utf16_lossy(&entry.szExeFile)
                .trim_end_matches('\0')
                .to_ascii_lowercase();
            if name == PROCESS_NAME {
                found = Some(entry.th32ProcessID);
                break;
            }
            if Process32NextW(snapshot, &mut entry).is_err() {
                break;
            }
        }
    }
    let _ = CloseHandle(snapshot);
    Ok(found)
}

struct WindowSearch {
    pid: u32,
    hwnd: HWND,
}

unsafe extern "system" fn enum_window(hwnd: HWND, parameter: LPARAM) -> BOOL {
    let search = &mut *(parameter.0 as *mut WindowSearch);
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == search.pid {
        search.hwnd = hwnd;
        BOOL(0)
    } else {
        BOOL(1)
    }
}

unsafe fn find_window(pid: u32) -> Result<Option<HWND>, String> {
    let mut search = WindowSearch {
        pid,
        hwnd: HWND(std::ptr::null_mut()),
    };
    EnumWindows(
        Some(enum_window),
        LPARAM(&mut search as *mut WindowSearch as isize),
    )
    .map_err(|error| format!("枚举窗口失败: {error:?}"))?;
    Ok((!search.hwnd.0.is_null()).then_some(search.hwnd))
}

fn bstr_to_string(value: BSTR) -> String {
    value.to_string()
}
