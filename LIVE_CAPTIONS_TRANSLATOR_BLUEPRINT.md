# LiveCaption Product Blueprint

Updated: 2026-07-13.

LiveCaption is a Windows desktop translator built with Tauri v2, React, TypeScript, and Rust. The first product pass is complete: selected-text translation and Windows Live Captions translation are usable end to end, with a compact UI, a persistent overlay, provider configuration, and session export.

## Current product boundary

```text
React / WebView
  main workspace and settings
  selection toolbar and result window
  live-caption overlay
  settings, logs, export controls
        |
        | typed Tauri commands and events
        v
Rust runtime
  settings + Windows Credential Manager secrets
  direct OpenAI-compatible HTTP client
  global selection watcher and clipboard fallback
  Windows Live Captions launcher + UI Automation bridge
  native COM/UIA reader with PowerShell compatibility fallback
  snapshot/status state machine + sentence segmenter
  reconnect supervisor + source health telemetry
  JSONL session writer and tray lifecycle
```

React owns presentation and interaction state. Rust owns secrets, Windows APIs, process lifecycle, HTTP, persistence, and runtime recovery.

## Implemented first-pass behavior

### Selected-text translation

- Customizable shortcut mode and automatic selection mode.
- Automatic mode reacts to drag selection and double-click selection without treating ordinary clicks as selections.
- Floating toolbar is focus-safe, draggable, monitor-aware, and clamped to the active work area.
- Expanded result windows remain open until explicitly closed; dragging does not dismiss them.
- Streaming translation shows the first token early and reports first-token and total latency.

### Realtime captions

- Starts and stops Windows Live Captions through a dedicated runtime controller.
- Reads all candidate `CaptionsTextBlock` UI Automation nodes instead of assuming the first node is current.
- Filters Windows performance/status prompts and pauses on an unchanged prompt snapshot.
- Resumes only when the real caption candidate changes, avoiding stale-prompt deadlocks.
- The source window can be visually hidden using an alpha/click-through layer while remaining UI Automation-readable.
- The overlay separates the translated feed from a bounded two-line live-recognition dock.
- The overlay has close, refresh, expand/collapse, drag-mode, transparency, and caption-color controls.
- Refresh rebuilds the caption reader session without clearing existing exports.

### LLM runtime

- OpenAI-compatible Chat Completions endpoint, model, target language, `extra_body`, timeout, tokens, and temperature.
- API keys are stored through the Windows Credential Manager backend.
- Requests force direct connections with Reqwest `no_proxy()`; system proxy configuration is not changed.
- Total request deadline is capped at five seconds.
- DeepSeek V4 requests explicitly disable thinking unless the user enables it for selection translation.
- SSE streaming is used for early display; live caption output uses short prompts and bounded queues.
- Live captions keep only the newest translation request, canceling superseded in-flight requests; recent committed segments can be supplied as context.
- Runtime status exposes source health, last update, reconnect count, queue depth, and first-token latency.

### Desktop lifecycle

- Main-window close hides to the tray instead of destroying the process.
- Tray menu opens the main workspace, toggles captions, and quits.
- Double-clicking the tray icon restores and focuses the main workspace.
- Tray uses the packaged application icon.
- Overlay and selection windows are transparent auxiliary windows and persist only while their workflow is active.

## Commands

- `get_settings`, `save_settings`, `delete_api_key`
- `test_llm`, `translate_selection`
- `set_caption_running`, `refresh_caption`, `get_runtime_status`
- `save_overlay_position`, `export_caption_session`
- `get_runtime_logs`, `clear_runtime_logs`, `open_logs_dir`

## Events

- `selection:ready`
- `translation:started`, `translation:delta`, `translation:finished`
- `caption:source`, `caption:segment`, `caption:translated`
- `caption:state-changed`, `settings:changed`, `runtime:error`
- `caption:health-changed`

## Current tradeoffs

- The native COM/UIA reader is the primary path; PowerShell remains a compatibility fallback for older Windows builds.
- The Windows status-message filter is heuristic and currently covers the known Chinese, English, and Japanese performance prompts. New localized prompts should be added through tests.
- Direct-only HTTP is optimal for the intended domestic provider path, but users on networks that require a proxy will need an explicit future per-provider proxy option.
- Caption exports are kept in memory for the active session and written to JSONL continuously. The active list should eventually be backed by a streaming session index for very long recordings.

## Phase 2 recommendation

The best second part is **Caption Engine 2.0: reliability first, then translation quality**.

1. Add richer native reader metadata (automation id, bounds, visibility, source revision) and a fake UIA source for deterministic tests.
2. Extend the reconnect supervisor with a maximum restart rate and an explicit `stale` state.
3. Add provider-aware translation policy: glossary, per-provider stream settings, and a speed/context preset.
4. Add a session review workspace after the source path is stable: searchable history, raw-vs-translated comparison, corrections, glossary entries, and export.

This order addresses the current product risk—the source and runtime can silently become stale—before adding larger user-facing features. It also creates the foundation for better translations without coupling UI work to the current PowerShell reader.
