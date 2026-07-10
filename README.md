# LiveCaption

一个面向 Windows Live Captions 的轻量实时翻译工具。

当前第一版支持：

- OpenAI-compatible LLM（含 DeepSeek V4 Flash 预设）
- 划词翻译、解释与复制
- Windows Live Captions 的启动、捕获与基础按片翻译
- 透明 Overlay 和 JSONL 会话日志

开发环境：.NET 10 SDK、Windows 11（Live Captions 需要 Windows 11 22H2 或更高版本）。

## 运行与诊断

```powershell
dotnet run --project src\LiveCaption.App
```

应用启动、可恢复错误和未处理异常会写入：

```text
%LocalAppData%\LiveCaption\logs\latest.log
```

主界面“概览”页提供“打开目录”按钮。关闭主窗口只会隐藏到系统托盘；请使用托盘菜单中的“退出”结束进程。
