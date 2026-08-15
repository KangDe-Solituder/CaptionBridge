import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

export const isTauri = "__TAURI_INTERNALS__" in window;

// Browser dev preview: no Tauri APIs exist, so every window method becomes a
// no-op async fn and the label comes from ?window=main|overlay|selection-toolbar.
function makePreviewWindow(label: string) {
  return new Proxy({ label }, {
    get: (target, key) => (key === "label" ? target.label : () => Promise.resolve(() => undefined)),
  });
}

export const currentWindow = (isTauri
  ? getCurrentWebviewWindow()
  : makePreviewWindow(new URLSearchParams(location.search).get("window") ?? "main")) as ReturnType<typeof getCurrentWebviewWindow>;

export const windowLabel = currentWindow.label;
