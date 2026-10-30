//! Ask the running instance to restore its own window through Tauri.
//! Native ShowWindow from a second process bypasses Tao's cached visibility:
//! the next hide() can become a no-op because Tao still considers it hidden.
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        System::Threading::{CreateEventW, SetEvent, WaitForSingleObject, INFINITE},
    },
};

const WAKE_EVENT: &str = "Local\\com.dimfi.livecaption.show-main";

pub struct WakeSignal(HANDLE);

impl WakeSignal {
    pub fn new() -> Result<Self, String> {
        Self::named(WAKE_EVENT)
    }

    fn named(name: &str) -> Result<Self, String> {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // Auto-reset preserves a wake sent before setup finishes, and consumes
        // it once. The first process retains a handle for its entire lifetime.
        unsafe { CreateEventW(None, false, false, PCWSTR(name.as_ptr())) }
            .map(Self)
            .map_err(|error| format!("创建窗口唤醒信号失败：{error}"))
    }

    pub fn notify(&self) -> Result<(), String> {
        unsafe { SetEvent(self.0) }.map_err(|error| format!("发送窗口唤醒信号失败：{error}"))
    }

    fn wait(&self, timeout: u32) -> bool {
        unsafe { WaitForSingleObject(self.0, timeout) == WAIT_OBJECT_0 }
    }
}

impl Drop for WakeSignal {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        use tauri::Manager;
        let signal = match WakeSignal::new() {
            Ok(signal) => signal,
            Err(error) => {
                app.state::<crate::AppState>().log("error", error);
                return;
            }
        };
        while signal.wait(INFINITE) {
            // These calls dispatch onto Tauri's event loop and update its state.
            crate::show_main_window(&app);
        }
        app.state::<crate::AppState>()
            .log("error", "窗口唤醒监听意外停止");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_survives_sender_exit_and_is_consumed_once() {
        let name = format!("Local\\livecaption-wake-test-{}", uuid::Uuid::new_v4());
        let owner = WakeSignal::named(&name).unwrap();
        assert!(!owner.wait(0));
        std::thread::spawn({
            let name = name.clone();
            move || WakeSignal::named(&name).unwrap().notify().unwrap()
        })
        .join()
        .unwrap();
        // The listener can start after the second process has already exited.
        let listener = WakeSignal::named(&name).unwrap();
        assert!(listener.wait(0));
        assert!(!listener.wait(0));
        owner.notify().unwrap();
        assert!(listener.wait(0));
        assert!(!listener.wait(0));
    }
}
