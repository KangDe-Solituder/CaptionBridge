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
    private string _status = "正在加载设置…";

    public MainViewModel(SettingsService settings, AppRuntime runtime)
    {
        _settings = settings;
        _runtime = runtime;
        _runtime.StatusChanged += (_, status) => Status = status;
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
    public string Status { get => _status; set => Set(ref _status, value); }
    public string LiveButtonText => _runtime.IsLiveRunning ? "停止实时字幕" : "启动实时字幕";

    public Task InitializeAsync()
    {
        Apply(_settings.Current, _settings.ApiKey);
        Status = "就绪。选择文字后会出现翻译工具条。";
        return Task.CompletedTask;
    }

    public async Task SaveAsync()
    {
        try
        {
            await _settings.SaveAsync(BuildSettings(), ApiKey, CancellationToken.None);
            Status = "设置已保存。";
        }
        catch (Exception exception)
        {
            Status = $"保存失败：{exception.Message}";
        }
    }

    public async Task TestAsync()
    {
        await SaveAsync();
        Status = "正在测试翻译服务…";
        var result = await _runtime.TranslateSelectionAsync("Hello, LiveCaption.", TranslationMode.Selection);
        Status = result.IsError ? $"测试失败：{result.ErrorMessage}" : $"连接正常：{result.Text} ({result.LatencyMilliseconds} ms)";
    }

    public async Task ToggleLiveAsync()
    {
        await SaveAsync();
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
        Captions = new CaptionSettings { KeepSystemCaptionVisible = KeepSystemCaptionVisible },
        Overlay = _settings.Current.Overlay
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
