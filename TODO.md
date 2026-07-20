# LiveCaption TODO

Updated: 2026-07-19.

## Completed first product pass

- [x] Migrate the WPF prototype to Tauri v2 + React + TypeScript + Rust.
- [x] Add main workspace, settings, selection toolbar/result window, and live-caption overlay.
- [x] Add OpenAI-compatible endpoint, model, target language, `extra_body`, timeout, tokens, and temperature settings.
- [x] Store API keys through the Windows Credential Manager backend.
- [x] Add shortcut and automatic selection triggers, clipboard fallback, copy, regenerate, and persistent result windows.
- [x] Clamp selection windows to the active monitor work area.
- [x] Add configurable caption overlay drag modes: Alt, anywhere, and top handle.
- [x] Add overlay close, refresh, expand/collapse, transparent background, caption color, and font controls.
- [x] Add Windows Live Captions launch, UI Automation reading, sentence segmentation, translation, and JSONL sessions.
- [x] Add status-prompt filtering and snapshot-based pause/resume recovery.
- [x] Add SSE streaming, first-token latency, five-second total deadline, direct no-proxy requests, and DeepSeek V4 thinking control.
- [x] Add bounded UI histories/queues, debounced position persistence, and duplicate source-event suppression.
- [x] Add tray icon, open/toggle/quit menu, double-click restore, and close-to-tray main-window behavior.
- [x] Add strict Rust tests, Unicode preview tests, snapshot recovery tests, frontend production builds, and Clippy checks.
- [x] Keep dev, raw release, and installed builds on one stable settings path without silent default fallback.
- [x] Build release binaries as Windows GUI applications with hidden PowerShell helper processes.
- [x] Limit only realtime captions to five seconds; allow selected-text translation to complete without a total deadline.
- [x] Show the automatic-selection toolbar immediately, capture text in the background, and let the user's own Ctrl+C operation win.

## Phase 2 — Caption Engine 2.0 (recommended)

### Native source reader

- [x] Replace the PowerShell UI Automation bridge with a native `windows` crate COM/UIA reader (PowerShell compatibility fallback retained).
- [ ] Add typed candidate metadata: automation id, bounding rectangle, visibility, source revision, and timestamp.
- [ ] Add tests for multiple candidate nodes, stale nodes, localized status prompts, and source disappearance.

### Reconnect and health

- [x] Add a reader heartbeat and `last_caption_update` timestamp to runtime status.
- [x] Add automatic reconnect/restart after process exit, UIA failure, or repeated reader failures.
- [ ] Add a maximum restart rate and explicit stale-source detection to avoid restart loops.
- [x] Show source health, last update, reconnect count, queue depth, and first-token latency in the main workspace.
- [x] Make the overlay refresh button invoke the same supervised recovery path.

### Translation quality and latency

- [ ] Add a provider capability profile for streaming, thinking control, max output, and endpoint quirks.
- [x] Add optional context from the last 0–4 committed source segments.
- [ ] Add a glossary with preferred translations and term-level overrides.
- [x] Add cancellation for superseded live-caption requests.
- [x] Record request start, first token, completion, timeout, and queue metrics.
- [ ] Add a configurable live-caption translation policy for speed vs context.

### Verification

- [ ] Add frontend component tests for settings, overlay controls, and close-to-tray behavior.
- [ ] Add a fake UIA source and fake SSE provider for deterministic integration tests.
- [ ] Add a long-session soak test covering memory, queue size, reconnects, and export integrity.
- [ ] Add a packaged Windows smoke test for tray icon, multi-monitor positioning, hidden source mode, and refresh.

See [QUALITY_ACCEPTANCE.md](QUALITY_ACCEPTANCE.md) for the release gates and manual Windows test matrix.

## Phase 3 — Session workspace

- [ ] Searchable session history with timestamps and source/provider metadata.
- [ ] Raw-vs-translated comparison, corrections, favorites, notes, and Markdown export.
- [ ] Glossary management with aliases, readings, scope, and priority.
- [ ] Import/export settings and portable diagnostics bundle.

## Later product tracks

- [ ] Scene translation using recent committed captions as optional context.
- [ ] OCR region translation.
- [ ] ASR correction suggestions that preserve the raw caption text.
- [ ] Multiple overlay profiles for language pairs, games, meetings, and videos.
