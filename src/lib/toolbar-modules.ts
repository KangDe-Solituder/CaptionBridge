import { Captions, Copy, Languages, ScanText } from "lucide-react";
import type { AppSettings, ToolbarModule } from "./contracts";

export const toolbarModules = {
  selection: { title: "划词翻译", description: "翻译、复制当前选区；没有选区时置灰。" },
  ocr: { title: "截图翻译", description: "框选屏幕文字并翻译，无需选区。" },
  captions: { title: "实时字幕", description: "启动字幕浮窗；运行中不会重复启动。" },
};
export const toolbarButtons = {
  translate: { title: "翻译", icon: Languages },
  copy: { title: "复制", icon: Copy },
  ocr: { title: "识屏", icon: ScanText },
  captions: { title: "字幕", icon: Captions },
};
export type ToolbarButton = keyof typeof toolbarButtons;
export function enabledToolbarButtons(settings: AppSettings): ToolbarButton[] {
  return settings.toolbar_order.filter(module => settings[module].enabled)
    .flatMap((module: ToolbarModule): ToolbarButton[] => module === "selection" ? ["translate", "copy"] : [module]);
}
