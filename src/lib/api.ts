import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, AsrDependencyReport, CaptionSourceConfig, Glossary, GlossaryImportResult, ModelInfo, RuntimeLogEntry, RuntimeStatus, SessionPageView, SessionSummary, SettingsView, TranslationRequest, TranslationResult } from "./contracts";

export const getSettings = () => invoke<SettingsView>("get_settings");
export const saveSettings = (settings: AppSettings) =>
  invoke<SettingsView>("save_settings", { request: { settings } });
export const hideSelectionToolbar = () => invoke<void>("hide_selection_toolbar");
export const testDownloadProxy = (proxyUrl: string) => invoke<string>("test_download_proxy", { proxyUrl });
export const testLlm = () => invoke<TranslationResult>("test_llm");
export const saveLlmProfile = (input: { id?: string; name: string; config: import("./contracts").LlmProviderSettings; api_key?: string; clear_api_key: boolean }) => invoke<SettingsView>("save_llm_profile", { input });
export const activateLlmProfile = (id: string) => invoke<SettingsView>("activate_llm_profile", { id });
export const removeLlmProfile = (id: string) => invoke<SettingsView>("remove_llm_profile", { id });
export const translateSelection = (request: TranslationRequest) => invoke<TranslationResult>("translate_selection", { request });
export const setCaptionRunning = (running: boolean) => invoke<void>("set_caption_running", { running });
export const refreshCaption = () => invoke<void>("refresh_caption");
export const resetAsrChannelRouting = () => invoke<void>("reset_asr_channel_routing");
export const getRuntimeStatus = () => invoke<RuntimeStatus>("get_runtime_status");
export const saveOverlayPosition = (x: number, y: number) => invoke<void>("save_overlay_position", { x, y });
export const exportCaptionSession = (path: string, format: string, sessionId?: string) =>
  invoke<void>("export_caption_session", { path, format, sessionId: sessionId ?? null });
export const listCaptionSessions = () => invoke<SessionSummary[]>("list_caption_sessions");
export const readCaptionSession = (id: string, page: number, pageSize = 20) =>
  invoke<SessionPageView>("read_caption_session", { id, page, pageSize });
export const deleteCaptionSession = (id: string) => invoke<void>("delete_caption_session", { id });
export const newCaptionSession = () => invoke<void>("new_caption_session");
export const correctCaptionSession = (id: string, useGlossary: boolean) =>
  invoke<void>("correct_caption_session", { id, useGlossary });
export const openSessionsDir = () => invoke<void>("open_sessions_dir");
export const getGlossary = () => invoke<Glossary>("get_glossary");
export const saveGlossary = (glossary: Glossary) => invoke<void>("save_glossary", { glossary });
export const exportGlossary = (path: string) => invoke<void>("export_glossary", { path });
export const importGlossary = (path: string) => invoke<GlossaryImportResult>("import_glossary", { path });
export const getRuntimeLogs = () => invoke<RuntimeLogEntry[]>("get_runtime_logs");
export const clearRuntimeLogs = () => invoke<void>("clear_runtime_logs");
export const openLogsDir = () => invoke<void>("open_logs_dir");
export const listModels = () => invoke<ModelInfo[]>("list_models");
export const downloadModel = (modelId: string) => invoke<void>("download_model", { modelId });
export const cancelModelDownload = (modelId: string) => invoke<void>("cancel_model_download", { modelId });
export const verifyModel = (modelId: string) => invoke<void>("verify_model", { modelId });
export const testModel = (modelId: string) => invoke<number>("test_model", { modelId });
export const deleteModel = (modelId: string) => invoke<void>("delete_model", { modelId });
export const openModelsDir = () => invoke<void>("open_models_dir");
export const switchCaptionSource = (source: CaptionSourceConfig) => invoke<void>("switch_caption_source", { source });
export const checkAsrDependencies = () => invoke<AsrDependencyReport>("check_asr_dependencies");
export const openAsrDependencyUrl = (dependencyId: string) => invoke<void>("open_asr_dependency_url", { dependencyId });
export const downloadAsrGpuRuntime = () => invoke<void>("download_asr_gpu_runtime");
export const cancelAsrGpuRuntime = () => invoke<void>("cancel_asr_gpu_runtime");
