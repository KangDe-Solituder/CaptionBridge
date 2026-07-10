# LiveCaption Blueprint

> 更新日期：2026-07-11。当前结论：WPF 版本保留为原型，长期产品主线迁移到 `Tauri + React + Rust`。

## 1. 产品定义

LiveCaption 是一个轻量级 Windows 配信辅助工具。它复用 Windows Live Captions 做本地语音转录，通过远程 LLM API 完成实时字幕翻译、划词翻译、解释和基于当前字幕上下文的场景翻译。

它解决三类问题：

1. 听懂配信：显示主播原文与译文。
2. 看懂屏幕：翻译或解释用户在任意应用中选中的文字。
3. 自然表达：根据当前配信上下文，辅助用户写弹幕、YouTube 评论或配信感想。

它不是本地 ASR、不是本地大模型宿主，也不会自动向直播平台发送内容。

## 2. 为什么迁移到 Tauri + React

WPF 原型验证了 Windows Live Captions + UI Automation + LLM 翻译的可行性，但也暴露了明显问题：

- 现代 UI 成本高：统一视觉、动效、半透明、浮窗状态和设置页质感都需要大量 XAML 工作。
- 窗口体验脆弱：划词工具条、结果卡片、Overlay、托盘和主窗口的生命周期容易互相影响。
- 前端表达力不足：想做接近现代 AI 桌面工具的体验，React 生态更合适。
- 调试体验割裂：WPF 的 XAML 运行时错误容易绕过单元测试，视觉问题也不容易快速迭代。

Tauri + React 不能让 Windows 集成自动变简单，但它能把问题拆得更清楚：

- React 负责现代 UI、状态、动效、浮窗内容和设置体验。
- Rust 负责 Windows API、Live Captions、全局快捷键、剪贴板、托盘、文件和 LLM 请求。
- 核心协议和数据模型独立于 UI，后续特殊词、场景包和会后复核可以平滑加入。

因此迁移目标不是“为了换技术栈”，而是让产品体验和工程边界更适合长期维护。

## 3. 产品原则

- 轻量优先：语音识别复用 Windows Live Captions，不常驻本地 LLM，不处理音频流。
- 实时但不跳闪：宁可稍晚显示稳定片段，也不让旧译文反复覆盖新画面。
- 原文可追溯：永远保留 Windows ASR 原文，校准结果和译文不能覆盖原始证据。
- 记录不丢失：显示可以丢弃过期 preview，档案不能因 API 慢或程序异常漏写。
- 上下文受控：LLM 只接收有限、可见、与当前场景相关的上下文。
- 人始终在环：低置信校准、会后提取的特殊词必须可检查、可撤销。
- UI 像产品：所有窗口都必须可关闭、可拖动或有明确停靠逻辑，状态和错误必须可理解。

## 4. 技术架构

```text
React UI
  - Main Window
  - Settings
  - Selection Toolbar
  - Result Card
  - Caption Overlay
  - Scene Translation Window
        |
        | Tauri commands / events
        v
Rust Backend
  - LLM Client
  - Settings / Secrets
  - Global Shortcut
  - Clipboard / Selection
  - Windows Live Captions Launcher
  - UI Automation Caption Reader
  - Session Logger
        |
        v
Core Domain
  - TranslationRequest
  - Segmenter
  - ContextPack
  - TermCandidates
  - Session Records
```

建议工程结构：

```text
src/
  app/                 React 应用入口、路由和全局状态
  components/          通用 UI 控件
  features/
    settings/          设置页
    selection/         划词工具条与结果卡片
    captions/          实时字幕状态与 Overlay UI
    scene/             场景翻译窗口
  lib/                 API client、类型、格式化工具

src-tauri/
  src/
    main.rs
    commands/          Tauri command 边界
    llm/               OpenAI-compatible client
    settings/          配置与 schema migration
    secrets/           Keyring / Windows credential
    windows/           Live Captions、UI Automation、快捷键、剪贴板
    session/           JSONL / 后续 SQLite
    core/              领域模型、切片器、请求结构
```

## 5. 第一版范围

第一版要先做出“翻译 + 划词 + 字幕”的可信闭环，而不是一次性实现所有高级能力。

必须实现：

1. LLM 设置与测试连接。
2. OpenAI-compatible Chat Completions，支持 DeepSeek V4 Flash 预设。
3. `extra_body` JSON，默认支持 `thinking.type = disabled`。
4. API Key 安全保存。
5. 划词后显示工具条：翻译、解释、复制。
6. 全局快捷键触发同一划词翻译流程。
7. 翻译结果卡片：可关闭、可复制、位置合理。
8. 开启实时字幕时主动启动 Windows Live Captions。
9. 读取 Live Captions 文本，按片段提交给 LLM。
10. Overlay 显示一行原文和一行译文。
11. 会话 JSONL 追加日志。
12. 托盘菜单：字幕开关、划词开关、场景翻译入口、设置、退出。

第一版明确不做：

- 自建 ASR 或音频捕获。
- 本地 LLM。
- OCR 选区翻译。
- 自动向平台发送弹幕或评论。
- 完整特殊词系统。
- 会后高级 LLM 整理。
- 复杂 ASR 自动校准。

## 6. UI 设计方向

目标不是把 Windows 做成 macOS，而是吸收现代 Apple 风格中“安静、统一、轻盈、响应自然”的部分。

设计要求：

- 主窗口使用扁平化、低噪声、信息清晰的工具型布局。
- 设置页是从上到下的连续设置列表，用分组和留白组织，而不是复杂抽屉。
- 浮窗必须有明确关闭按钮；结果卡片和 Overlay 支持拖动。
- 工具条使用深色胶囊形态，贴近选区，避免遮挡文本。
- 动效只服务状态变化：出现、关闭、加载、完成、错误。
- 颜色不做单一紫蓝渐变；主色、状态色、中性色要形成完整系统。
- 文本不挤压、不溢出、不被按钮或窗口边界遮挡。

窗口类型：

```text
MainWindow
  常规管理界面：状态、设置、日志入口、快速操作。

SelectionToolbar
  选中文字后出现的小工具条：翻译、解释、复制。

ResultCard
  显示翻译或解释结果，可复制，可关闭，可拖动。

CaptionOverlay
  透明置顶字幕窗口，显示原文和译文。

SceneTranslator
  用户输入自己想说的话，选择用途、语气、目标语言，可使用当前字幕上下文。
```

## 7. LLM 与请求模型

所有翻译都使用统一请求模型，UI 不直接拼 Prompt。

```text
TranslationRequest
  - mode: selection | explain | live_caption | scene
  - source_text
  - source_language
  - target_language
  - style
  - context_pack
  - term_candidates
  - provider_options
```

DeepSeek V4 Flash 默认配置示例：

```json
{
  "base_url": "https://api.deepseek.com",
  "model": "deepseek-v4-flash",
  "timeout_ms": 1800,
  "max_tokens": 160,
  "temperature": 0.1,
  "extra_body": {
    "thinking": { "type": "disabled" }
  }
}
```

`extra_body` 是必要能力，因为不同供应商关闭思考、启用 JSON、缓存或限流的字段不同，不能为每家服务商硬编码一个 UI。

实时字幕路径中，commit 片段可以要求模型一次性返回结构化结果：

```json
{
  "corrected_source": "...",
  "translation": "...",
  "applied_term_ids": [],
  "corrections": []
}
```

第一版可以先只使用 `translation`，但数据结构从第一天保留 `corrected_source`、`applied_term_ids` 和 `corrections`，为后续 ASR 校准和特殊词做准备。

## 8. 划词翻译

默认行为：开启后，用户在常见应用中选中文字，工具条在选区附近出现。点击按钮后才调用远程 API。

功能：

- 翻译：默认非中文翻译为中文。
- 解释：用中文说明词义、读法、句子重点或上下文含义。
- 复制：复制选中文本。
- 快捷键：作为自动工具条失败时的可靠回退。

选区读取策略：

1. 第一版以受控剪贴板回退为主。
2. 读取前记录剪贴板状态，读取后仅在确认用户未更新剪贴板时恢复。
3. UI Automation 选区读取作为增强路径，必须隔离异常，不能导致主进程崩溃。
4. 失败时给出轻量提示，不反复弹错误窗口。

风险边界：

- 游戏画面、Canvas、受保护窗口、密码框不承诺支持。
- 后续可以加入 OCR，但不属于第一版。

## 9. 实时字幕管线

Windows Live Captions 输出的是不断修订的文本，而不是可靠的句子流。因此需要“读取、差分、切片、翻译、显示、记录”管线。

```text
Live Captions UI Automation
  -> normalize text
  -> diff current text
  -> segmenter
  -> translation queue
  -> overlay display
  -> session JSONL
```

基础切片信号：

- 句末标点：`. ? ! 。？！`
- 文本稳定：约 700-1200ms 未变化。
- 最大时长：同一活跃片段约 2.5-3 秒。
- 最大长度：超过语言相关上限则强制提交。
- 口语边界：后续可按日语语气词、停顿词作为弱信号。

`preview` 与 `commit`：

- `preview`：正在变化的临时片段，只用于 Overlay，可被新结果替换。
- `commit`：由标点、稳定时间或强制切分确认的片段，进入日志、上下文和后续导出。

每个片段必须带：

```text
segment_id
sequence_id
revision
started_at
committed_at
raw_asr
corrected_source
translation
status
latency_ms
model
```

旧异步结果晚返回时，不得覆盖更新的 Overlay 或 committed 记录。

## 10. 场景翻译

场景翻译不是翻译主播正在说的话，而是翻译用户想表达的话。

典型用途：

- 配信中发弹幕。
- 写 YouTube 评论。
- 配信结束后写感想。
- 根据当前游戏、角色、人名和事件背景润色表达。

当实时字幕开启且已有 committed 记录时，场景翻译自动构建一个小型 `ContextPack`：

```text
当前配信语言与目标语言
+ 最近 6-12 条 committed 字幕
+ 当前高优先级特殊词
+ 用户输入
```

约束：

- 上下文必须有限。
- UI 要显示“已使用当前字幕上下文”，并允许关闭。
- 不把数小时字幕塞进同一个对话。
- 不跨会话自动混用旧背景。

## 11. 存储与导出

第一版使用 JSONL 追加日志，原因是简单、可靠、易恢复、低内存。

示例：

```json
{
  "segment_id": "seg_000143",
  "sequence_id": 143,
  "start_ms": 3725000,
  "end_ms": 3727600,
  "raw_asr": "...",
  "corrected_source": "...",
  "translation": "...",
  "status": "committed",
  "model": "deepseek-v4-flash",
  "latency_ms": 840,
  "error": null
}
```

后续可迁移到 SQLite，用于历史页、搜索、标签、特殊词关系和导出。迁移时保留同一 `SessionStore` 抽象，避免 UI 重写。

## 12. 特殊词与 ASR 校准

用户提到的“词、内容、场景”应扩展为可计算结构：

```text
id
canonical_text
description
aliases
readings
preferred_translation
scene
scope
priority
source
enabled
```

校准策略必须保守：

1. 永远保留 `raw_asr`。
2. 先做规范化和特殊词候选匹配。
3. 只把高置信候选交给 LLM 判断。
4. LLM 只能修正有上下文、术语或近音证据的错误。
5. 低置信候选只记录建议，不静默改写。

日语汉字转假名、罗马音相似度和近音召回属于第二阶段能力，不放进第一版。

## 13. 迁移策略

不需要立刻删除 WPF 原型。建议迁移分三步：

1. 建立 Tauri 工程，只实现 LLM 设置、测试连接、主窗口和托盘。
2. 实现划词工具条和结果卡片，把 UI 体验先做成可长期使用的水平。
3. 将 WPF 原型中已验证的 Live Captions 读取思路迁移到 Rust 后端，再接入 Overlay 和 JSONL。

WPF 代码的价值：

- 证明 Live Captions 路线可行。
- 保留 UI Automation、剪贴板和 API 请求踩坑记录。
- 作为 Rust 实现的行为参考。

WPF 代码不再作为 UI 质量投入对象，避免继续在不满意的前端基础上修补。

## 14. 验收标准

第一版 Tauri 应满足：

- 主窗口、设置页、工具条、结果卡片、Overlay 视觉统一。
- 所有浮窗位置合理，有关闭按钮，必要时可拖动。
- 有清晰托盘菜单和退出入口。
- DeepSeek 测试连接可用，错误能定位 Endpoint、API Key、模型或网络问题。
- 划词翻译在 Chrome、VS Code、记事本等常见应用中可用。
- 划词失败不会破坏用户剪贴板。
- 开启实时字幕时能启动或连接 Windows Live Captions。
- 字幕原文和译文能显示到 Overlay，并追加写入 JSONL。
- API 失败不影响原文捕获和本地记录。
- 连续运行 1 小时内存不线性增长；发布前目标为 3 小时稳定运行。

## 15. 后续阶段

第二阶段：字幕质量与特殊词。

- 特殊词管理。
- 日语读音、近音候选、ASR 保守校准。
- 会后 LLM 复核。
- SRT / VTT / CSV 导出。
- 请求预算、缓存、失败补翻。

第三阶段：知识沉淀。

- 会话搜索、标签、收藏和备注。
- 特殊词提取建议与审核入库。
- Markdown、Obsidian、Logseq、Notion 导出或联动。
- 配信摘要、重点片段和术语统计。
