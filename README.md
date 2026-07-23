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

基础开发环境：

- Windows 11，以及系统自带或单独安装的 WebView2 Runtime；
- Node.js 24 LTS；
- pnpm 11.15.1，版本已通过 `package.json` 的 `packageManager` 字段固定；
- Rust stable、MSVC 工具链，以及 Visual Studio Build Tools 的“使用 C++ 的桌面开发”组件；
- 构建本地 ASR worker 时只需要 Python 3.12 x64；运行 GPU 本地 ASR 时，CTranslate2 仍会实际使用 CUDA 12.x 和 cuDNN 9。它们可以来自系统环境，也可以打包进自包含 worker。

本项目已在 Node.js 24.18.0、pnpm 11.15.1、Python 3.12.10、CUDA 12.8 和 cuDNN 9.24 上完成验证。其他 CUDA 12.x/cuDNN 9 组合也可以使用，但应确认运行时能加载 `cublas64_12.dll`、`cublasLt64_12.dll` 和 `cudnn64_9.dll`。CTranslate2 是 faster-whisper 的推理后端，不是可删除的“探测专用依赖”；cuDNN 也会被 GPU 推理实际调用。

LLM 翻译需要一个 OpenAI-compatible endpoint 和 API Key。实时使用 Windows Live Captions 时，需要系统已启用对应功能；使用本地 ASR 时则需要下载约 1.5–1.6 GB 的模型文件。

## 安装与运行

安装 Node.js 后，启用项目固定的 pnpm 版本：

```powershell
corepack enable
corepack install --global pnpm@11.15.1
pnpm --version
```

如果当前 Node.js 没有附带 Corepack，可以改用：

```powershell
npm install --global pnpm@11.15.1
```

在项目根目录安装锁定依赖：

```powershell
pnpm install --frozen-lockfile
```

`pnpm-workspace.yaml` 已允许 `esbuild` 执行安装脚本。若旧检出版本出现 `ERR_PNPM_IGNORED_BUILDS`，请更新仓库后重新安装，或执行 `pnpm approve-builds esbuild`。

启动 Tauri 开发环境：

```powershell
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

默认生成经过验证的 NSIS `setup.exe`。MSI/WiX 不再属于默认目标，避免 WiX 工具异常导致已经成功的 NSIS 构建被整体判定为失败。

### 构建本地 ASR worker

两个本地模型 `Kotoba Whisper v2.0 Faster` 和 `Whisper large-v3-turbo` 都通过同一个 faster-whisper worker 运行，因此依赖相同。Windows Live Captions 来源本身不需要 Python、CUDA 或模型，但当前 Tauri 配置会把 worker 作为应用资源打包，所以执行 `pnpm tauri dev` 或 `pnpm tauri build` 前仍需生成一次 worker；仅运行 `pnpm build` 构建前端则不需要。

worker 的 Python 依赖版本记录在 [`src-tauri/worker/requirements.lock.txt`](src-tauri/worker/requirements.lock.txt) 中，其中也固定了与 CTranslate2 4.6.0 兼容的 `setuptools` 版本。默认构建不会复制 CUDA/cuDNN 系统运行库，worker 当前约为 228 MB；它与虚拟环境、Rust `target` 和模型文件都不会提交到 Git，每台开发机需要单独生成。换句话说，`git pull` 会更新 worker 源码，但不会替换已经存在的本地 exe；worker 源码更新后应重新执行下面的构建命令。

先安装 Python 3.12 x64，然后执行：

```powershell
cd src-tauri\worker

$Python = "C:\Users\<你的用户名>\AppData\Local\Programs\Python\Python312\python.exe"
& $Python -m venv .\.venv-build
.\.venv-build\Scripts\python.exe -m pip install --upgrade pip
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\build-worker.ps1 `
  -Python $Python
```

安装后的应用会从 Worker 自身目录、系统 `PATH`、`CUDA_PATH` 和标准 NVIDIA 安装目录中查找 CUDA 12/cuDNN 9。可以在“设置 → 字幕 → 本地 ASR 运行环境”点击“检查依赖”：新版 Worker 会在进程内实际加载 CUDA/cuDNN DLL，并调用 CTranslate2 进行轻量 GPU 探测；旧版 Worker 不支持该诊断命令时，应用会自动改用已安装模型执行真实 CUDA dry-run。只要实际推理通过就会判定环境可用，不要求为了诊断接口升级 Worker。检查器也不会再把未被当前推理路径使用的可选 cuDNN 拆分 DLL 判为硬性缺失。

如确实需要生成包含 GPU 运行库的自包含 worker，可以显式使用以下参数：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\build-worker.ps1 `
  -Python $Python `
  -BundleGpuRuntime `
  -CudaBin "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8\bin" `
  -CudnnBin "C:\Program Files\NVIDIA\CUDNN\v9.0\bin\12.8"
```

自包含 worker 约为 2 GB，可能再次触发 NSIS 的大数据块映射限制，因此正常安装包不建议使用该选项。

成功后应存在：

```text
src-tauri\worker\dist\livecaption-asr-worker\livecaption-asr-worker.exe
```

`-ExecutionPolicy Bypass` 只对这一次子进程生效，不会修改系统的永久 PowerShell 执行策略。

### 常见安装与启动问题

- **无法识别 `pnpm`**：确认 Node.js 和 pnpm 已安装，重新打开终端后执行 `where.exe node`、`where.exe pnpm` 和 `pnpm --version`。
- **`pnpm.ps1` 被执行策略阻止**：可以直接使用 `pnpm.cmd`；不需要为了运行 pnpm 永久关闭 PowerShell 安全策略。
- **`ERR_PNPM_IGNORED_BUILDS: esbuild`**：确认已检出 `pnpm-workspace.yaml`，然后重新执行 `pnpm install --frozen-lockfile`。
- **`resource path worker\\dist\\livecaption-asr-worker doesn't exist`**：本地 ASR worker 尚未构建；按上面的步骤生成 `livecaption-asr-worker.exe`。
- **旧 Worker 不支持轻量探测**：这是 `git pull` 后仍在使用被 Git 忽略的旧 exe，并不表示推理版本不兼容。检查器会自动用当前已安装模型执行真实 CUDA dry-run；验证成功即可继续使用。只有希望使用更快的轻量诊断时才需要重新执行 `build-worker.ps1`。
- **本地 ASR 提示缺少 CUDA/cuDNN**：在“设置 → 字幕”运行依赖检查。当前 CTranslate2 版本需要 CUDA 12 的 cuBLAS 和 cuDNN 9；不能直接使用只包含 `cublas64_13.dll` 的 CUDA 13 目录。检查器显示的是 Worker 进程内的实际加载结果；模型管理中的“测试”还会执行一次真实的静音推理。
- **NSIS 报 `Internal compiler error #12345: error mmapping datablock`**：通常是把约 2 GB 的 CUDA/cuDNN DLL 一并装入 worker 导致。重新使用不带 `-BundleGpuRuntime` 的默认命令构建 worker，再执行 `pnpm tauri build`。
- **PowerShell 阻止 `build-worker.ps1`**：使用上面进程级的 `powershell.exe -ExecutionPolicy Bypass -File ...` 命令。
- **Vite 报 `EBUSY ... src-tauri\\target\\...\\livecaption.exe`**：当前配置已从 Vite 监听中排除 `src-tauri`。拉取最新代码、结束旧的开发进程后重新执行 `pnpm tauri dev`。

## 多设备开发同步

Git 只同步源代码、锁文件和配置。`node_modules`、`.tools`、Python 虚拟环境、worker 构建产物、Rust `target`、本地模型、日志和会话数据均已忽略，不应提交。

在另一台电脑首次检出后，需要分别安装 Node.js/pnpm、Rust/MSVC；需要构建本地 ASR worker 时还要安装 Python。GPU 推理所需的 CUDA/cuDNN 可以安装在目标系统，也可以用 `-BundleGpuRuntime` 打包进 worker。日常开始工作前建议执行：

```powershell
git pull --rebase
pnpm install --frozen-lockfile
```

完成修改后再执行测试、提交并推送：

```powershell
pnpm build
cargo check --manifest-path src-tauri\Cargo.toml
git add -A
git commit -m "描述本次修改"
git push
```

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
├─ logs\               # runtime.jsonl 与 ASR worker 日志
└─ sessions\           # 当前及历史字幕会话 JSONL
```

本地 ASR 模型默认放在项目根目录的 `Model\`，下载中的临时文件位于 `Model\.downloads\`。这两个目录均不会提交到 Git；如果希望两台电脑共用模型，需要另行复制，或在每台电脑上通过应用重新下载。

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
