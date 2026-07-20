# LiveCaption

一个面向 Windows 的实时字幕翻译与划词翻译桌面工具。

> 命名建议：如果现在重新命名，我会优先考虑 **CaptionBridge（字幕桥）**。这个名字比 LiveCaption 更能覆盖项目目前的完整边界：它既连接 Windows Live Captions 或本地 ASR 与大语言模型，也连接“任意应用中的选中文字”和翻译结果。当前代码、安装包和 bundle identifier 仍使用 `LiveCaption`，因此本文暂不把它当作已经完成的改名。

## 项目简介

LiveCaption 的目标是在不打断当前应用的情况下，提供两条快速翻译路径：

- **划词翻译**：在任意 Windows 应用中选中文字，通过自动选区感知或全局快捷键唤起翻译工具。
- **实时字幕翻译**：读取 Windows Live Captions，或从系统默认输出设备进行本地语音识别，将字幕翻译后显示在始终置顶的悬浮窗口中。

翻译请求使用 OpenAI-compatible Chat Completions 接口，支持流式输出、目标语言、上下文、温度、最大 token 数和 provider-specific 的请求参数。实时字幕会维护有限的翻译队列，优先处理最新片段，并记录源状态、ASR 延迟、队列深度和首 token 延迟。

当前版本：`0.5.0`

目标平台：Windows 11（使用 Windows Live Captions 时建议 22H2 或更高版本）

## 功能

### 划词翻译

- 自动选区模式：完成拖选或双击选词后，在选区附近显示轻量工具条。
- 快捷键模式：使用可配置的全局快捷键唤起工具条，默认快捷键为 `Alt+Q`。
- 支持剪贴板回退方案，兼容无法直接读取选区的应用。
- 翻译结果在独立窗口中显示，可复制、重新生成、折叠或关闭。
- 使用流式响应尽早显示首个 token，同时展示总耗时和首 token 延迟。

### 实时字幕翻译

- **Windows Live Captions 来源**：通过 Windows UI Automation 读取系统字幕，原生 COM/UIA 读取优先，PowerShell 作为兼容性回退。
- **本地 ASR 来源**：捕获 Windows 默认扬声器的 WASAPI loopback 音频，使用 faster-whisper 与 VAD 进行本地识别。
- 可启动、停止和刷新字幕来源；检测到字幕进程退出、UI Automation 失效或连续读取错误时，会尝试恢复连接。
- 过滤 Windows 的性能提示和重复的陈旧字幕，按稳定时间、标点、长度和最长时长切分句段。
- 始终置顶的透明悬浮字幕窗口，支持透明度、字体、颜色、宽度、主题、模糊和动效设置。
- 悬浮窗口支持按住 `Alt` 拖动，也可以切换为任意位置拖动或顶部拖拽手柄模式。
- 关闭主窗口时默认隐藏到系统托盘；双击托盘图标可恢复主窗口。

### 模型、翻译与导出

- 内置两个经过 revision 和 SHA-256 固定的本地 ASR 模型清单，可在设置页下载、校验、测试和删除。
- 支持 OpenAI、DeepSeek 以及其他 OpenAI-compatible 服务；API Key 保存到 Windows Credential Manager，不写入普通设置文件。
- 对 DeepSeek V4 的 thinking 参数进行 provider-aware 处理，可在设置中控制是否启用思考模式。
- 当前会话持续写入 JSONL，并可导出为 `SRT`、`WebVTT`、`TXT` 或 `JSON`。
- 日志页提供运行日志、源健康状态、重连次数、翻译队列和延迟指标。

## 工作方式

```text
任意应用中的选区 ──┐
                   ├─> Rust 运行时 ──> OpenAI-compatible LLM ──> 翻译结果窗口
Windows Live Captions ┤
系统音频 ─> 本地 ASR ┘
                         └─> 实时字幕分段 ──> 始终置顶的 Overlay
                                      └─> JSONL 会话 ──> SRT / VTT / TXT / JSON
```

项目采用 Tauri v2：

- `src/`：React + TypeScript 前端、主工作区、设置页、选区工具条和字幕 Overlay。
- `src-tauri/src/`：Rust 后端，负责 Windows API、UI Automation、进程生命周期、HTTP 请求、密钥、设置、日志和会话持久化。
- `src-tauri/worker/`：本地 ASR worker，负责音频 loopback、VAD 和 faster-whisper 推理。
- `src-tauri/icons/`：Tauri 打包所需的应用图标。

## 开发环境

基础开发需要：

- Windows 11；
- Node.js 与 pnpm；
- Rust stable、MSVC 工具链及 Tauri v2 的 Windows 构建依赖；
- 若要构建或调试本地 ASR worker：Python、可用的 NVIDIA CUDA/cuDNN 运行时。

LLM 翻译需要一个 OpenAI-compatible endpoint 和 API Key。实时使用 Windows Live Captions 时，需要系统已启用对应功能；使用本地 ASR 时则需要下载约 1.5–1.6 GB 的模型文件。

## 安装与运行

在项目根目录执行：

```powershell
pnpm install
pnpm tauri dev
```

仅构建前端：

```powershell
pnpm build
```

构建 Windows 安装包：

```powershell
pnpm tauri build
```

如果需要重新构建本地 ASR worker，先准备 Python 虚拟环境和 CUDA/cuDNN，再执行：

```powershell
cd src-tauri\worker
.\build-worker.ps1 -Python "C:\Path\To\python.exe"
```

worker 的依赖版本记录在 [`src-tauri/worker/requirements.lock.txt`](src-tauri/worker/requirements.lock.txt) 中。开发时如果只使用 Windows Live Captions 来源，可以不加载本地 ASR 模型。

## 质量检查

完整检查会执行前端类型检查与生产构建、Rust 单元测试和 Clippy：

```powershell
pnpm quality
```

也可以单独执行：

```powershell
cd src-tauri
cargo test
cargo check
cargo clippy -- -D warnings
```

Windows 运行态验收清单见 [`QUALITY_ACCEPTANCE.md`](QUALITY_ACCEPTANCE.md)，产品架构和阶段计划见 [`LIVE_CAPTIONS_TRANSLATOR_BLUEPRINT.md`](LIVE_CAPTIONS_TRANSLATOR_BLUEPRINT.md)，待办事项见 [`TODO.md`](TODO.md)。

## 数据与隐私

应用数据默认位于 Tauri 的 app data 目录，当前 Windows bundle identifier 对应的常见路径为：

```text
%LOCALAPPDATA%\com.dimfi.livecaption\
├─ settings.json       # 普通设置
├─ models\             # 本地 ASR 模型与下载中的文件
├─ logs\               # runtime.jsonl 与 ASR worker 日志
└─ sessions\           # 当前及历史字幕会话 JSONL
```

- API Key 使用 Windows Credential Manager 保存，不写入 `settings.json`。
- 选中的文本、字幕片段和翻译结果会发送到用户配置的 LLM endpoint；使用本地 ASR 并不等于翻译过程完全离线。
- 本地 ASR 的音频只用于当前进程内识别；会话文件保存文本、译文和时间信息，不保存原始音频。
- 当前网络请求使用 direct/no-proxy 模式；必须经过系统代理的网络环境暂不适配。

## 当前限制与后续方向

- 当前版本主要面向 Windows，暂不支持 macOS 或 Linux。
- 本地 ASR worker 的识别语言目前在代码中固定为日语，适合日语语音或视频；自动语言识别和更多语言仍需后续接入。
- Windows Live Captions 的状态提示过滤和 UIA 读取仍受 Windows 版本、系统语言及字幕窗口结构影响。
- 当前会话列表主要驻留在内存，同时持续写入 JSONL；长时间录制的可搜索会话工作区尚未完成。
- 计划中的方向包括：更完整的 UIA 候选元数据、重连速率限制、术语表、速度/上下文策略、可搜索会话工作区，以及更完整的集成和长时间运行测试。

## 命名备选

如果未来正式改名，我会按以下优先级考虑：

1. **CaptionBridge / 字幕桥**：最准确地表达“字幕来源到翻译结果”的连接能力，也能容纳划词翻译和未来的 OCR、会议、视频场景。
2. **CaptionFlow**：更偏产品化，强调字幕从捕获、分段、翻译到展示的完整流动。
3. **LingoOverlay**：突出多语言和悬浮窗口，辨识度较好，但对划词翻译的覆盖不如前两个名称。

正式改名时还需要同步修改 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、窗口标题、应用图标元数据、Credential Manager service name、数据目录兼容策略以及构建产物名称。
