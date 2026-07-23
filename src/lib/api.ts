import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, AsrDependencyReport, CaptionSourceConfig, ModelInfo, RuntimeLogEntry, RuntimeStatus, SettingsView, TranslationRequest, TranslationResult } from "./contracts";

export const getSettings = () => invoke<SettingsView>("get_settings");
export const saveSettings = (settings: AppSettings, apiKey?: string) =>
  invoke<SettingsView>("save_settings", { request: { settings, api_key: apiKey || null } });
export const deleteApiKey = () => invoke<void>("delete_api_key");
export const testLlm = (apiKey?: string) => invoke<TranslationResult>("test_llm", { apiKey: apiKey || null });
export const translateSelection = (request: TranslationRequest) => invoke<TranslationResult>("translate_selection", { request });
export const setCaptionRunning = (running: boolean) => invoke<void>("set_caption_running", { running });
export const refreshCaption = () => invoke<void>("refresh_caption");
export const resetAsrChannelRouting = () => invoke<void>("reset_asr_channel_routing");
export const getRuntimeStatus = () => invoke<RuntimeStatus>("get_runtime_status");
export const saveOverlayPosition = (x: number, y: number) => invoke<void>("save_overlay_position", { x, y });
export const exportCaptionSession = (path: string, format: string) => invoke<void>("export_caption_session", { path, format });
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
