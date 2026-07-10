# LiveCaption Blueprint

> 从零开始的产品与技术蓝图。最后更新：2026-07-10。

## 1. 产品定义

这是一个 Windows 轻量级配信辅助工具。它复用 Windows Live Captions 进行本机语音转录，使用用户选择的远程 LLM 翻译服务进行翻译、术语校准和场景化表达辅助。

它解决三类彼此相关、但边界明确的问题：

1. 听懂配信：实时显示主播原文与译文。
2. 看懂屏幕：翻译用户在任意应用中选中的文字。
3. 自然表达：根据当前配信上下文，帮助用户写弹幕、YouTube 评论或配信感想。

该软件不是 ASR 引擎、不是本地大模型宿主，也不自动向任何直播平台发送内容。

## 2. 产品原则

- **轻量优先**：语音识别直接使用 Windows Live Captions；不常驻本地 LLM，不处理音频流。
- **实时但不躁动**：宁可让未完成片段稳定地稍后出现，也不让旧译文反复覆盖新画面。
- **原文可追溯**：永远保留 Windows ASR 原文；校准结果和译文不能覆盖原始证据。
- **记录不丢失**：显示可以丢弃过期预览，档案不能因 API 慢或程序异常而漏写。
- **上下文受控**：LLM 只接收有限、与当前场景有关的上下文，绝不把数小时字幕持续塞进同一个对话。
- **人始终在环**：高置信术语可以自动使用；低置信校准、自动提取的特殊词必须可检查、可撤销。

## 3. 正式功能边界

### 3.1 实时字幕

这是一个开关。开启后软件启动或连接 Windows Live Captions，并同时完成：

- 持续读取 Windows ASR 原文。
- 将连续文本切成适合阅读的短语义片段。
- 结合本场上下文和特殊词，以一次 LLM 请求完成保守校准与翻译。
- 在透明 Overlay 中显示原文和译文。
- 把片段持续写入本场会话存储。
- 维护可供“场景翻译”使用的近期上下文。

实时字幕关闭后，不再读取或翻译新字幕；已经保存的会话仍可导出、查看和离线复核。

### 3.2 划词翻译

这是独立开关。默认规则为“非中文翻译为中文”，用户可修改源语言、目标语言和是否使用当前配信语境。

交互建议：托盘菜单负责启用/禁用；真正触发使用全局快捷键。快捷键优先从 UI Automation 读取当前选区；若目标应用不支持，则尝试临时复制到剪贴板、读取文本并恢复原剪贴板内容。

注意：任意应用的选区无法在 Windows 上百分之百可靠取得。浏览器 Canvas、游戏画面、受保护窗口等不应承诺支持；OCR 是后续可选能力，不属于第一版。

### 3.3 场景翻译

这是按需打开的输入窗，不翻译主播正在说的话，而是翻译用户想说的话。

典型用途：

- 配信中发送弹幕。
- 编写 YouTube 评论。
- 配信结束后写一段感想。
- 用当前游戏、角色、人名和事件背景润色自己的表达。

当实时字幕处于开启状态并已有记录时，场景翻译自动构建一个小型“场景包”；用户可以在窗口中关闭该选项或查看它使用了哪些背景。

建议字段：

- 输入内容。
- 源语言、目标语言。
- 用途：弹幕 / 评论 / 感想 / 自由翻译。
- 语气：自然随意 / 简短 / 礼貌。
- 是否使用当前配信上下文。
- 结果的复制按钮与重新生成按钮。

默认行为：中文输入时，目标语言跟随当前配信语言；没有活跃配信时由用户选择。

## 4. 核心架构

```text
Windows Live Captions
        |
        v
UI Automation 捕获与文本差分
        |
        v
语义片段切分器 -----> 原始事件持久化队列
        |
        v
特殊词候选与轻量校准
        |
        v
实时 LLM 请求（校准 + 翻译，一次请求）
        |                         |
        v                         v
Overlay 显示队列              会话存储（SQLite）
                                  |
                                  +--> SRT / VTT / CSV 导出
                                  +--> 场景包
                                  +--> 会后高级 LLM 复核

划词翻译 ------------------------> 通用翻译请求
场景翻译（用户输入 + 场景包） ----> 场景化翻译请求
```

建议技术栈：

- `.NET 8` + `C#`：与参考项目一致，适合 Windows UI Automation、托盘和 Overlay。
- `WPF`：主窗口、设置页、透明 Overlay、场景翻译弹窗。
- `System.Windows.Automation`：读取 Windows Live Captions 的字幕控件。
- `Microsoft.Data.Sqlite`：低内存、事务性会话记录和词典。
- `HttpClient`：OpenAI-compatible LLM API 适配。
- Windows DPAPI：加密保存 API Key。

参考项目 `SakiRinn/LiveCaptions-Translator` 已验证了“Windows Live Captions + UI Automation + Overlay”的可行性，也已有翻译历史与 CSV 导出的实践。它可以作为捕获和 UI 交互的参考；本项目的队列、存储和上下文核心应从头模块化设计，而不直接沿用其“最新翻译完成后取消更旧任务”的历史处理方式。

参考：<https://github.com/SakiRinn/LiveCaptions-Translator>

### 4.1 第一版工程组织

第一版采用单一 Windows 桌面解决方案，但把未来会变化的能力从第一天起隔离开：

```text
src/
  LiveCaption.App/        WPF 主窗口、托盘、Overlay、设置与场景翻译弹窗
  LiveCaption.Core/       领域模型、接口、切分策略、任务编排、Prompt 构造
  LiveCaption.Windows/    Live Captions UI Automation、热键、选区、剪贴板
  LiveCaption.Llm/        OpenAI-compatible 请求、供应商预设、重试和限流
  LiveCaption.Storage/    设置、DPAPI 密钥、JSONL / SQLite 会话与特殊词存储
tests/
  LiveCaption.Core.Tests/ 纯逻辑单元测试
```

建议的依赖与职责：

| 层 | 首选实现 | 作用 |
| --- | --- | --- |
| 桌面 UI | `.NET 10` + `WPF` + MVVM | 透明 Overlay、托盘、设置页和轻量弹窗。 |
| 应用装配 | `Microsoft.Extensions.DependencyInjection` / Logging | 创建服务、日志和生命周期，不让窗口直接管理业务逻辑。 |
| Windows 集成 | `System.Windows.Automation`、Win32 `SendInput`、剪贴板 API | 读取 Live Captions、全局热键与划词文本。 |
| LLM | `HttpClient` + `System.Text.Json` | 直接实现 OpenAI-compatible HTTP 协议，控制依赖和供应商扩展。 |
| 并发 | `System.Threading.Channels`、`CancellationToken` | 使用有界队列，防止网络慢时内存无限增长。 |
| 设置与密钥 | JSON + Windows DPAPI | 常规设置可读，API Key 只以当前 Windows 用户可解密的形式保存。 |
| 会话记录 | 第一版 JSONL；后续 `Microsoft.Data.Sqlite` | 先可靠追加写入，再增加查询、导出和词典关系。 |

选择 WPF 而不选择 Electron、Tauri 或 WinUI 3 的原因是：本项目只服务 Windows，且核心需求是 UI Automation、透明置顶窗口、托盘、热键和低内存常驻；WPF 在这些场景中成熟、依赖少，并能直接复用 .NET 的 Windows API。

### 4.2 第一版实现顺序

第一版不按页面开发，而按可验证的垂直链路开发：

```text
1. 设置页 + DeepSeek 测试请求
2. 划词文本 -> LLM -> 结果浮窗
3. Windows Live Captions -> 文本 -> 基础切分 -> LLM -> 简单 Overlay
4. 托盘、暂停、错误状态、JSONL 追加日志
5. 真实配信连续运行测试与性能修正
```

其中划词翻译应先支持“选中后按快捷键翻译”。“选择文字后显示按钮”复用同一选区和翻译服务，但依赖全局鼠标监听、去抖和选区可用性判断，作为第一版后半段的可选交互模式，不阻塞主路径。

实时字幕第一版只做按片段直译：不发送长上下文、不自动校准、不建立特殊词。切分器以策略对象实现，因此后续可以替换为更适合日语配信的策略，而不影响 UI 或 LLM 客户端。

### 4.3 扩展性边界

第一版的关键不是预先实现所有功能，而是确保未来功能有明确插槽。

```text
ILiveCaptionSource       Windows Live Captions 只是第一种字幕来源
ITextSelectionSource     UI Automation / 剪贴板只是第一种划词来源
ISegmenter               基础断句、配信断句和语言专用断句可并存
ITranslator              DeepSeek 只是第一个 OpenAI-compatible Provider
IContextProvider         第一版返回空上下文，后续返回本场场景包
ITermMatcher             第一版不启用，后续提供特殊词候选与读音匹配
ISessionStore            第一版追加 JSONL，后续可无感迁移至 SQLite
IPostSessionProcessor    第一版不注册，后续接入会后 LLM 复核
```

每次翻译都使用同一个稳定的请求模型，而不是让 UI 直接拼 Prompt：

```text
TranslationRequest
  - Mode: Selection / LiveCaption / Scene
  - SourceText
  - SourceLanguage / TargetLanguage
  - ContextPack
  - TermCandidates
  - Style: Plain / Danmaku / Comment / Reflection
  - ProviderOptions
```

第一版的 `ContextPack` 和 `TermCandidates` 可以为空；第二、三阶段只是在构造请求时填充它们。因此场景翻译、特殊词、ASR 校准不会迫使翻译接口或 UI 推翻重写。

内部流转使用领域事件，而不是窗口之间互相调用：

```text
CaptionObserved
  -> SegmentCommitted
  -> TranslationRequested
  -> TranslationCompleted / TranslationFailed
  -> SessionRecordAppended
  -> (后续) TermSuggestionCreated
```

Overlay 只订阅显示所需事件；存储只订阅已确认片段；后续的会后复核只读取已结束会话。这样慢的 LLM、文件写入或新功能不会拖住字幕捕获。

### 4.4 版本兼容与可维护性

- 配置文件包含 `schema_version`；设置结构变化时写迁移器，避免升级后用户 API 配置失效。
- Prompt 以命名模板和版本号保存，例如 `live-caption-v1`、`selection-v1`；日志记录使用过的模板版本，便于复盘翻译质量变化。
- 所有 LLM 返回都先解析为内部 `TranslationResult`；供应商的字段、流式格式和错误码不得泄漏到 UI 层。
- 所有后台任务都有 `CancellationToken`、超时和有界队列；窗口关闭、暂停会话和切换设置时可以安全停止。
- Live Captions 控件定位、选区读取和剪贴板回退都封装在 Windows 层，并写出可人工验证的诊断日志，因为这些是最易受 Windows 更新影响的边界。

## 5. 实时字幕管线

### 5.1 捕获与片段切分

Windows Live Captions 输出的是不断修订的整段文本，不是带可靠时间戳的句子。捕获层高频读取文本并计算差分，切分层根据下列信号决定何时提交片段：

- 句末标点：`. ? ! 。？！`。
- 文本稳定：约 700-1200ms 未变化。
- 最大时长：同一活跃片段达到约 2.5-3 秒。
- 最大长度：超过按语言配置的字符/词数上限。
- 口语边界：日语等语言可配置常见语气词和停顿词作为弱信号。

切分目标是“可理解的语义片段”，而不是语法完整句。配信中的半句、改口和口癖是常态。

### 5.2 Preview 与 Commit

- `preview`：正在变化的片段的临时译文，仅用于 Overlay；可以被更近的新结果替换。
- `commit`：由标点、稳定时间或强制切分确认的片段；进入档案、场景包和后续导出。

显示队列与存档队列必须分离：

- 显示队列允许取消过期 preview，确保画面跟得上当前配信。
- 存档队列按事务保存每个 commit，不允许因网络延迟丢失。
- 每个请求和片段带 `segment_id`、`revision`、`sequence_id`；旧请求晚返回时不得覆盖更新版本。

### 5.3 延迟与降级

- 运行时可配置请求超时，初始建议 1.5-2 秒。
- 超时的 preview 直接失效，不阻塞后续片段。
- commit 翻译失败时，先写入原文与错误状态；网络恢复后可补翻。
- 队列积压时优先保障捕获和落盘，再保障最新显示，不让内存随配信时间增长。

## 6. LLM 与翻译服务

### 6.1 供应商抽象

第一版以 OpenAI-compatible Chat Completions 抽象为主，配置项包括：

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

`extra_body` 是必要能力：不同服务商关闭思考、JSON 输出、缓存和限流的字段不同，不能为每家服务商硬编码一个 UI。

DeepSeek V4 Flash 适合作为默认候选：官方 API 支持非思考模式，且可以通过 `thinking.type = disabled` 显式关闭默认开启的思考模式。实时字幕不使用推理模式；会后复核和特殊词提取可以使用更高质量模型。

参考：

- <https://api-docs.deepseek.com/zh-cn/guides/thinking_mode>
- <https://api-docs.deepseek.com/quick_start/pricing>

### 6.2 一次请求完成校准与翻译

实时路径不应为“校准请求一次 + 翻译请求一次”。对于 commit 片段，应在一次低 token 请求中要求模型返回结构化结果：

```json
{
  "corrected_source": "ホロナイトやります",
  "translation": "我要玩《空洞骑士》。",
  "applied_term_ids": ["term_hollow_knight"],
  "corrections": [
    {
      "from": "ほろないと",
      "to": "ホロナイト",
      "confidence": 0.91
    }
  ]
}
```

系统提示必须限制模型：只修正有上下文、术语或近音依据的错误；不得补写主播没有说的内容；只输出规定的 JSON。

对于 preview，可选择只做快速翻译，不做复杂校准；对于 commit 才启用完整校准。

### 6.3 成本、隐私与密钥

- API Key 使用 Windows DPAPI 加密，不以明文写入配置或日志。
- 设置页显示本场请求数、估算 token、估算费用和预算上限。
- 网络断开时继续本地记录原文；恢复后由用户选择是否补翻。
- 明确告知用户：音频由 Windows 在本机识别；转录文本、特殊词和发送给场景翻译的背景会传给用户选择的远程 API。

## 7. 特殊词与 ASR 校准

### 7.1 特殊词模型

用户所说的“词、内容、场景”应扩展为可实际校准的结构：

```text
id
原词（canonical_text）
别名（aliases）
读音/假名/罗马音（readings）
固定译法（preferred_translation）
场景（streamer / 团体 / 游戏 / 当前企划）
优先级
范围（本场 / 全局）
来源（手动 / 会后提取）
是否启用
```

例子：

```text
原词：白上フブキ
别名：ふぶき、フブキ
读音：しらかみふぶき
固定译法：白上吹雪
场景：Hololive
范围：全局
```

### 7.2 校准策略

“把日语汉字转为假名再猜发音”不能作为无约束的通用纠错器。正确顺序是：

1. Unicode、标点、全半角规范化。
2. 特殊词的精确匹配、别名匹配和近音候选匹配。
3. 只有高置信候选才自动作为 LLM 的校准提示。
4. LLM 结合最近上下文、候选术语和原文进行保守判断。
5. 永远保留 `raw_asr`；低置信候选只记录建议，不静默修改。

第一版不必引入大型日语形态分析词典。用户可为重要特殊词手工填写读音和别名；日语读音转换、罗马音相似度和自动候选扩展可在第二阶段引入。

### 7.3 会后复核与词典积累

会话结束后，用户可启动高级 LLM 任务，分批处理已保存片段：

- 找出可能的 ASR 错误和更自然的译文。
- 提取高频人名、游戏名、角色名、技能名和固定梗。
- 生成“建议加入特殊词”的列表。

会后任务只能生成建议；用户审核后才写入全局词典。这样不会让一次误识别污染以后所有配信。

## 8. 上下文与场景包

LLM 绝不能持有数小时的连续聊天上下文。实时字幕和场景翻译都使用临时构造的场景包：

```text
当前配信语言与目标语言
+ 最近 6-12 条 committed 的“校准原文 + 译文”
+ 当前高优先级特殊词
+ 可选：本场短摘要（会后或低频更新）
+ 当前请求
```

场景翻译示例：用户输入“刚才那里真的太好笑了”，当前场景包显示主播刚打完某个 Boss 且多次提到角色名。模型应按“观众正在看日语配信、要发简短自然评论”的用途与语气翻译，而不是把中文逐字替换。

场景包规则：

- 只使用 committed、已校准的字幕。
- 默认限制 token 数，优先保留最近内容和高优先级特殊词。
- 绑定当前会话；会话结束或切换配信后不自动混用旧背景。
- 场景翻译窗口向用户展示“已使用本场上下文”，并允许一键关闭。

## 9. 持久化与导出

### 9.1 会话生命周期

实时字幕开始时立即创建会话，以开始时间命名：

```text
sessions/
  2026-07-10_20-35-42.sqlite
  2026-07-10_20-35-42.jsonl
  exports/
    2026-07-10_20-35-42.srt
    2026-07-10_20-35-42.vtt
    2026-07-10_20-35-42.csv
```

SQLite 是主数据源，JSONL 是可恢复的追加事件日志；两者都以小事务持续写入，不在内存积累整场配信。

### 9.2 片段记录

```json
{
  "segment_id": "seg_000143",
  "start_ms": 3725000,
  "end_ms": 3727600,
  "raw_asr": "ほろないとやります",
  "corrected_source": "ホロナイトやります",
  "translation": "我要玩《空洞骑士》。",
  "state": "committed",
  "translator": "deepseek-v4-flash",
  "latency_ms": 840,
  "applied_term_ids": ["term_hollow_knight"]
}
```

运行中不直接把可变片段当作最终 SRT 写入。会话结束时，或用户主动导出时，根据 committed 片段生成标准 SRT、VTT 和 CSV；这样可以正确处理 ASR 修订和片段结束时间。

## 10. UI 与交互

### 常驻后台

- 默认最小化到系统托盘。
- 托盘菜单：实时字幕开关、划词翻译开关、打开场景翻译、暂停翻译、打开 Overlay、设置、结束当前会话、导出。
- 主窗口显示当前会话状态、近期记录、错误状态与费用信息。

### Overlay

- 无边框、半透明、可置顶、可拖动。
- 原文和译文分层显示；可配置字体、颜色、行数与透明度。
- preview 与 commit 显示风格不同，但不反复大幅跳动。

### 场景翻译窗口

- 轻量弹窗，不自动发布到平台。
- 明确显示当前用途、目标语言与是否使用本场上下文。
- 输出后可复制、修改输入再生成，或保存为会话备注。

## 11. 第一版范围（MVP）

第一版必须能在一场真实配信中稳定运行：

1. 连接 Windows Live Captions 并读取文本。
2. 实时字幕开关：原文 + DeepSeek V4 Flash 非思考翻译 + Overlay。
3. 标点、稳定时间、最大时长/长度组成的切片器。
4. `preview` / `commit`、超时、sequence id 和显示/存档双队列。
5. SQLite + JSONL 持续记录；结束会话后导出 SRT、VTT、CSV。
6. 手工特殊词：原词、别名、读音、固定译法、场景、范围。
7. 场景翻译窗口，使用最近 committed 字幕和特殊词。
8. 划词翻译的 UI Automation 选区读取与剪贴板回退。
9. API Key 加密、网络错误提示、请求超时、基础费用统计。

第一版明确不做：

- 自建 ASR、音频捕获或本地 LLM。
- OCR 选区翻译。
- 自动向 YouTube 或其他平台发送消息。
- 无约束的自动 ASR 改写。
- 自动把会后 LLM 提取的特殊词直接写入全局词典。

## 12. 后续阶段

### 第二阶段：质量与校准

- 日语读音转换、罗马音近似和更完善的术语候选召回。
- 会后高级 LLM 复核与特殊词建议。
- 手工“此处识别/翻译错误”反馈，立即写入本场词典。
- 更多翻译服务 profile、预算和缓存策略。

### 第三阶段：知识沉淀

- 会话搜索、标签、收藏和片段备注。
- Markdown、Obsidian、Logseq、Notion 导出。
- 配信摘要、重点片段和术语统计。
- 用户可选择的离线批量复译。

## 13. 验收标准

在一场 3 小时配信中，第一版应满足：

- 不运行本地 ASR 或本地 LLM，内存占用随时长不线性增长。
- API 短暂失败不会中断原文捕获和会话记录。
- 已 commit 的片段不会被旧异步结果覆盖或丢失。
- 会话结束后可获得时间正确的 SRT/VTT/CSV。
- 用户手工添加的特殊词能影响后续实时字幕和场景翻译。
- 场景翻译只使用当前会话的有限、可见且可关闭的上下文。
- 划词翻译失败时不会破坏用户剪贴板内容。

## 14. 实施起点

开始开发时，先完成一个没有复杂 UI 的垂直切片：

```text
连接 Windows Live Captions
-> 读取并切分字幕
-> 调用 DeepSeek V4 Flash（非思考）
-> 控制台/简单窗口显示
-> SQLite 持续写入
-> 导出 SRT
```

验证该链路在真实配信中稳定后，再加入 Overlay、特殊词、场景翻译和划词翻译。这样能先验证最难的实时链路，再把体验层逐步铺上去。
