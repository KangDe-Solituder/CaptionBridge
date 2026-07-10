using System.ComponentModel;
using System.Runtime.CompilerServices;
using LiveCaption.Core;

namespace LiveCaption.App;

public sealed class MainViewModel : INotifyPropertyChanged
{
    private readonly SettingsService _settings;
    private readonly AppRuntime _runtime;
    private string _endpoint = string.Empty;
    private string _model = string.Empty;
    private string _apiKey = string.Empty;
    private string _targetLanguage = string.Empty;
    private string _extraBodyJson = string.Empty;
    private int _timeoutMilliseconds;
    private int _maxTokens;
    private double _temperature;
    private bool _selectionToolbarEnabled;
    private bool _clipboardFallbackEnabled;
    private bool _keepSystemCaptionVisible;
    private int _stableMilliseconds;
    private int _maximumDurationMilliseconds;
    private int _maximumLength;
    private double _overlayFontSize;
    private double _overlayOpacity;
    private bool _overlayAlwaysOnTop;
    private readonly string _diagnosticsPath = CrashReporter.LatestLogPath;
    private string _status = "正在加载设置…";
    private string _connectionStatus = "尚未测试连接";
    private bool _isTestingConnection;

    public MainViewModel(SettingsService settings, AppRuntime runtime)
    {
        _settings = settings;
        _runtime = runtime;
        _runtime.StatusChanged += (_, status) =>
        {
            Status = status;
            OnPropertyChanged(nameof(LiveButtonText));
        };
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    public string Endpoint { get => _endpoint; set => Set(ref _endpoint, value); }
    public string Model { get => _model; set => Set(ref _model, value); }
    public string ApiKey { get => _apiKey; set => Set(ref _apiKey, value); }
    public string TargetLanguage { get => _targetLanguage; set => Set(ref _targetLanguage, value); }
    public string ExtraBodyJson { get => _extraBodyJson; set => Set(ref _extraBodyJson, value); }
    public int TimeoutMilliseconds { get => _timeoutMilliseconds; set => Set(ref _timeoutMilliseconds, value); }
    public int MaxTokens { get => _maxTokens; set => Set(ref _maxTokens, value); }
    public double Temperature { get => _temperature; set => Set(ref _temperature, value); }
    public bool SelectionToolbarEnabled { get => _selectionToolbarEnabled; set => Set(ref _selectionToolbarEnabled, value); }
    public bool ClipboardFallbackEnabled { get => _clipboardFallbackEnabled; set => Set(ref _clipboardFallbackEnabled, value); }
    public bool KeepSystemCaptionVisible { get => _keepSystemCaptionVisible; set => Set(ref _keepSystemCaptionVisible, value); }
    public int StableMilliseconds { get => _stableMilliseconds; set => Set(ref _stableMilliseconds, value); }
    public int MaximumDurationMilliseconds { get => _maximumDurationMilliseconds; set => Set(ref _maximumDurationMilliseconds, value); }
    public int MaximumLength { get => _maximumLength; set => Set(ref _maximumLength, value); }
    public double OverlayFontSize { get => _overlayFontSize; set => Set(ref _overlayFontSize, value); }
    public double OverlayOpacity { get => _overlayOpacity; set => Set(ref _overlayOpacity, value); }
    public bool OverlayAlwaysOnTop { get => _overlayAlwaysOnTop; set => Set(ref _overlayAlwaysOnTop, value); }
    public string Status { get => _status; set => Set(ref _status, value); }
    public string ConnectionStatus { get => _connectionStatus; set => Set(ref _connectionStatus, value); }
    public bool IsTestingConnection
    {
        get => _isTestingConnection;
        set
        {
            Set(ref _isTestingConnection, value);
            OnPropertyChanged(nameof(TestButtonText));
            OnPropertyChanged(nameof(CanTestConnection));
        }
    }
    public string TestButtonText => IsTestingConnection ? "测试中…" : "测试连接";
    public bool CanTestConnection => !IsTestingConnection;
    public string DiagnosticsPath => _diagnosticsPath;
    public string LiveButtonText => _runtime.IsLiveRunning ? "停止实时字幕" : "启动实时字幕";

    public Task InitializeAsync()
    {
        Apply(_settings.Current, _settings.ApiKey);
        Status = "就绪。选择文字后会出现翻译工具条。";
        return Task.CompletedTask;
    }

    public async Task<bool> SaveAsync()
    {
        try
        {
            await _settings.SaveAsync(BuildSettings(), ApiKey, CancellationToken.None);
            Status = "设置已保存。";
            return true;
        }
        catch (Exception exception)
        {
            Status = $"保存失败：{exception.Message}";
            return false;
        }
    }

    public async Task TestAsync()
    {
        if (IsTestingConnection)
        {
            return;
        }

        IsTestingConnection = true;
        ConnectionStatus = "正在连接翻译服务…";
        Status = ConnectionStatus;
        try
        {
            if (!await SaveAsync())
            {
                ConnectionStatus = Status;
                return;
            }

            ConnectionStatus = "正在发送测试文本…";
            var result = await _runtime.TranslateSelectionAsync("Hello, LiveCaption.", TranslationMode.Selection);
            ConnectionStatus = result.IsError
                ? $"连接失败：{result.ErrorMessage}"
                : $"连接成功 · {result.LatencyMilliseconds} ms · 返回“{result.Text}”";
            Status = ConnectionStatus;
        }
        catch (Exception exception)
        {
            CrashReporter.WriteException(exception, "Translation connection test failed.");
            ConnectionStatus = $"连接失败：{exception.Message}";
            Status = ConnectionStatus;
        }
        finally
        {
            IsTestingConnection = false;
        }
    }

    public async Task ToggleLiveAsync()
    {
        if (!await SaveAsync())
        {
            return;
        }
        try
        {
            await _runtime.ToggleLiveAsync();
        }
        catch (Exception exception)
        {
            Status = exception.Message;
        }
        finally
        {
            OnPropertyChanged(nameof(LiveButtonText));
        }
    }

    public void OpenDiagnosticsFolder()
    {
        try
        {
            CrashReporter.OpenLogDirectory();
        }
        catch (Exception exception)
        {
            Status = $"无法打开诊断目录：{exception.Message}";
        }
    }

    private AppSettings BuildSettings() => new()
    {
        Translator = new TranslatorSettings
        {
            Endpoint = Endpoint,
            Model = Model,
            TargetLanguage = TargetLanguage,
            ExtraBodyJson = ExtraBodyJson,
            TimeoutMilliseconds = Math.Clamp(TimeoutMilliseconds, 500, 30000),
            MaxTokens = Math.Clamp(MaxTokens, 16, 2048),
            Temperature = Math.Clamp(Temperature, 0, 2)
        },
        Selection = new SelectionSettings
        {
            Enabled = true,
            ShowToolbarOnSelection = SelectionToolbarEnabled,
            ClipboardFallbackEnabled = ClipboardFallbackEnabled
        },
        Captions = new CaptionSettings
        {
            KeepSystemCaptionVisible = KeepSystemCaptionVisible,
            StableMilliseconds = Math.Clamp(StableMilliseconds, 200, 5000),
            MaximumDurationMilliseconds = Math.Clamp(MaximumDurationMilliseconds, 500, 10000),
            MaximumLength = Math.Clamp(MaximumLength, 16, 500)
        },
        Overlay = new OverlaySettings
        {
            FontSize = Math.Clamp(OverlayFontSize, 12, 48),
            Opacity = Math.Clamp(OverlayOpacity, 0.2, 1),
            AlwaysOnTop = OverlayAlwaysOnTop
        }
    };

    private void Apply(AppSettings settings, string apiKey)
    {
        Endpoint = settings.Translator.Endpoint;
        Model = settings.Translator.Model;
        ApiKey = apiKey;
        TargetLanguage = settings.Translator.TargetLanguage;
        ExtraBodyJson = settings.Translator.ExtraBodyJson;
        TimeoutMilliseconds = settings.Translator.TimeoutMilliseconds;
        MaxTokens = settings.Translator.MaxTokens;
        Temperature = settings.Translator.Temperature;
        SelectionToolbarEnabled = settings.Selection.ShowToolbarOnSelection;
        ClipboardFallbackEnabled = settings.Selection.ClipboardFallbackEnabled;
        KeepSystemCaptionVisible = settings.Captions.KeepSystemCaptionVisible;
        StableMilliseconds = settings.Captions.StableMilliseconds;
        MaximumDurationMilliseconds = settings.Captions.MaximumDurationMilliseconds;
        MaximumLength = settings.Captions.MaximumLength;
        OverlayFontSize = settings.Overlay.FontSize;
        OverlayOpacity = settings.Overlay.Opacity;
        OverlayAlwaysOnTop = settings.Overlay.AlwaysOnTop;
    }

    private void Set<T>(ref T field, T value, [CallerMemberName] string? name = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return;
        }

        field = value;
        OnPropertyChanged(name);
    }

    private void OnPropertyChanged([CallerMemberName] string? name = null) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
