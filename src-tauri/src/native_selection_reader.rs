//! Read selections on one dedicated MTA thread. COM objects never leave that
//! thread, and provider calls have shorter timeouts than UIA's 20s default.

use std::{marker::PhantomData, rc::Rc};

use windows::{
    core::HRESULT,
    Win32::{
        Foundation::{E_NOINTERFACE, E_POINTER, POINT},
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        UI::Accessibility::{
            CUIAutomation8, IUIAutomation2, IUIAutomationTextPattern, SupportedTextSelection_None,
            UIA_TextPatternId, UIA_E_NOTSUPPORTED,
        },
    },
};

#[derive(Debug, PartialEq)]
pub enum Selection {
    Text(String),
    // UIA can expose inline emoji/images as U+FFFC instead of their text.
    // The source app's copy operation is needed to recover the original.
    EmbeddedObjects,
    Empty,
    Unsupported,
}

struct ComApartment(PhantomData<Rc<()>>);

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub struct SelectionReader {
    // Fields are dropped in declaration order: release interfaces before COM.
    automation: IUIAutomation2,
    _apartment: ComApartment,
}

impl SelectionReader {
    pub fn new() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|error| format!("选区 COM 初始化失败：{error}"))?;
            let apartment = ComApartment(PhantomData);
            let automation: IUIAutomation2 =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                    .map_err(|error| format!("选区 UIA 初始化失败：{error}"))?;
            automation
                .SetConnectionTimeout(250)
                .and_then(|_| automation.SetTransactionTimeout(500))
                .and_then(|_| automation.SetAutoSetFocus(false))
                .map_err(|error| format!("设置选区 UIA 参数失败：{error}"))?;
            Ok(Self {
                automation,
                _apartment: apartment,
            })
        }
    }

    pub fn read(
        &self,
        point: Option<POINT>,
        is_current: &impl Fn() -> bool,
    ) -> Result<Selection, String> {
        self.read_inner(point, is_current)
            .map_err(|error| format!("读取选区失败：{error}"))
    }

    fn read_inner(
        &self,
        point: Option<POINT>,
        is_current: &impl Fn() -> bool,
    ) -> Result<Selection, String> {
        ensure_current(is_current)?;
        // SAFETY: the reader is created, used and dropped on its MTA thread.
        unsafe {
            // Hotkey selection follows keyboard focus even if the pointer is
            // over another app. Automatic drags use their mouse-up position.
            let mut element = match point {
                Some(point) => self.automation.ElementFromPoint(point),
                None => self.automation.GetFocusedElement(),
            }
            .map_err(|error| error.to_string())?;
            let walker = self
                .automation
                .ControlViewWalker()
                .map_err(|error| error.to_string())?;
            for _ in 0..8 {
                ensure_current(is_current)?;
                match element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                    Ok(pattern) => {
                        ensure_current(is_current)?;
                        if pattern
                            .SupportedTextSelection()
                            .map_err(|e| e.to_string())?
                            != SupportedTextSelection_None
                        {
                            ensure_current(is_current)?;
                            let ranges = match pattern.GetSelection() {
                                Ok(ranges) => ranges,
                                Err(error) if null_interface(error.code()) => {
                                    return Ok(Selection::Empty);
                                }
                                Err(error) => return Err(error.to_string()),
                            };
                            let count = ranges.Length().map_err(|e| e.to_string())?;
                            let mut texts = Vec::new();
                            for index in 0..count {
                                ensure_current(is_current)?;
                                let range = ranges.GetElement(index).map_err(|e| e.to_string())?;
                                let text =
                                    range.GetText(-1).map_err(|e| e.to_string())?.to_string();
                                texts.push(text);
                            }
                            ensure_current(is_current)?;
                            // A supported empty selection must not fall back
                            // to copying a whole line or a parent's old selection.
                            return Ok(selection_from_ranges(texts));
                        }
                    }
                    Err(error) if pattern_unavailable(error.code()) => {}
                    Err(error) => return Err(error.to_string()),
                }
                ensure_current(is_current)?;
                element = match walker.GetParentElement(&element) {
                    Ok(parent) => parent,
                    Err(error) if null_interface(error.code()) => break,
                    Err(error) => return Err(error.to_string()),
                };
            }
            Ok(Selection::Unsupported)
        }
    }
}

fn ensure_current(is_current: &impl Fn() -> bool) -> Result<(), String> {
    if is_current() {
        Ok(())
    } else {
        Err("选区读取已取消或超时".to_string())
    }
}

fn pattern_unavailable(code: HRESULT) -> bool {
    code == E_NOINTERFACE || null_interface(code) || code.0 as u32 == UIA_E_NOTSUPPORTED
}

fn null_interface(code: HRESULT) -> bool {
    // windows 0.58 maps a successful COM call returning NULL to Error::empty()
    // (HRESULT 0), rather than E_POINTER. UIA uses NULL for missing patterns,
    // no selection, and the parent of the desktop root.
    code == HRESULT(0) || code == E_POINTER
}

fn selection_from_ranges(texts: Vec<String>) -> Selection {
    let text = texts
        .iter()
        .filter(|text| !text.trim().is_empty())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Selection::Empty
    } else if trimmed.contains('\u{fffc}') {
        Selection::EmbeddedObjects
    } else {
        Selection::Text(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_and_whitespace_ranges_are_empty() {
        assert_eq!(selection_from_ranges(vec![]), Selection::Empty);
        assert_eq!(
            selection_from_ranges(vec!["".into(), "\r\n\t \u{3000}".into()]),
            Selection::Empty
        );
    }

    #[test]
    fn preserves_multiple_selected_ranges_and_unicode() {
        assert_eq!(
            selection_from_ranges(vec![" 日本語🙂".into(), "".into(), "中文\n第二行 ".into()]),
            Selection::Text("日本語🙂\n中文\n第二行".into())
        );
    }

    #[test]
    fn object_replacement_characters_require_original_text_recovery() {
        for text in [
            "\u{fffc}\u{fffc}",
            "YouTube 评论 \u{fffc} 👍🏽",
            "第一行\n\u{fffc}",
        ] {
            assert_eq!(
                selection_from_ranges(vec![text.into()]),
                Selection::EmbeddedObjects
            );
        }
    }

    #[test]
    fn native_emoji_survive_utf16_conversion_without_clipboard_recovery() {
        let original = "🎉👍🏽👨‍👩‍👧‍👦🇨🇳❤️1️⃣ 中文🙂";
        let native = windows::core::BSTR::from(original);
        assert_eq!(
            selection_from_ranges(vec![native.to_string()]),
            Selection::Text(original.into())
        );
    }

    #[test]
    fn provider_failure_is_not_treated_as_unsupported() {
        assert!(pattern_unavailable(E_NOINTERFACE));
        assert!(pattern_unavailable(E_POINTER));
        assert!(!pattern_unavailable(HRESULT(0x800705B4_u32 as i32))); // timeout
        assert!(!pattern_unavailable(HRESULT(0x80070005_u32 as i32))); // access denied
    }

    #[test]
    fn successful_null_com_interface_is_a_missing_pattern() {
        let pattern: windows::core::Result<IUIAutomationTextPattern> =
            unsafe { windows::core::Type::from_abi(std::ptr::null_mut()) };
        let code = pattern.unwrap_err().code();
        assert!(null_interface(code));
        assert!(pattern_unavailable(code));
    }

    #[test]
    fn initializes_native_reader_and_cancels_before_querying_desktop() {
        let reader = SelectionReader::new().expect("native UIA initialization");
        assert!(reader.read(Some(POINT { x: 0, y: 0 }), &|| false).is_err());
        assert!(reader.read(None, &|| false).is_err());
    }
}
