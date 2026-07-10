using System.Drawing;
using LiveCaption.Core;
using LiveCaption.Windows;

namespace LiveCaption.App;

public sealed class AppRuntime : IAsyncDisposable
{
    private readonly SettingsService _settings;
    private readonly ITranslator _translator;
    private readonly ITextSelectionSource _selectionSource;
    private readonly ILiveCaptionSource _captionSource;
    private readonly ISessionStore _sessionStore;
    private readonly MouseSelectionWatcher _mouseWatcher;
    private readonly GlobalHotkeyService _hotkeyService;
    private SequentialCaptionTranslator? _captionTranslator;
    private BasicSegmenter? _segmenter;
    private CancellationTokenSource? _liveCancellation;
    private Task? _captionWorker;
    private string? _lastSelection;

    public AppRuntime(SettingsService settings, ITranslator translator, ITextSelectionSource selectionSource,
        ILiveCaptionSource captionSource, ISessionStore sessionStore, MouseSelectionWatcher mouseWatcher,
        GlobalHotkeyService hotkeyService)
    {
        _settings = settings;
        _translator = translator;
        _selectionSource = selectionSource;
        _captionSource = captionSource;
        _sessionStore = sessionStore;
        _mouseWatcher = mouseWatcher;
        _hotkeyService = hotkeyService;
    }

    public event EventHandler<SelectionReadyEventArgs>? SelectionReady;
    public event EventHandler<CaptionTranslatedEventArgs>? CaptionTranslated;
    public event EventHandler<string>? StatusChanged;

    public bool IsLiveRunning => _captionSource.IsRunning;

    public void StartSelectionFeatures()
    {
        _mouseWatcher.SelectionCompleted += OnMouseSelectionCompleted;
        _mouseWatcher.Start();
        _hotkeyService.Pressed += OnHotkeyPressed;
        _hotkeyService.RegisterDefault();
    }

    public async Task<TranslationResult> TranslateSelectionAsync(string source, TranslationMode mode, CancellationToken cancellationToken = default)
    {
        var request = new TranslationRequest(mode, source, "自动检测", _settings.Current.Translator.TargetLanguage,
            ContextPack.Empty, [], "Plain", _settings.ProviderOptions);
        return await _translator.TranslateAsync(request, cancellationToken);
    }

    public async Task ToggleLiveAsync()
    {
        if (IsLiveRunning)
        {
            await StopLiveAsync();
        }
        else
        {
            await StartLiveAsync();
        }
    }

    public async Task StartLiveAsync()
    {
        if (IsLiveRunning)
        {
            return;
        }

        StatusChanged?.Invoke(this, "正在启动 Windows Live Captions…");
        _liveCancellation = new CancellationTokenSource();
        _segmenter = new BasicSegmenter(_settings.Current.Captions);
        _captionTranslator = new SequentialCaptionTranslator();
        _captionTranslator.Translated += OnCaptionTranslated;
        _captionTranslator.Failed += OnCaptionTranslationFailed;
        _captionWorker = _captionTranslator.RunAsync(TranslateCaptionAsync, _liveCancellation.Token);
        _captionSource.TextObserved += OnCaptionObserved;
        await _sessionStore.StartAsync(_settings.Current.Translator.Model, _liveCancellation.Token);
        await _captionSource.StartAsync(_settings.Current.Captions.KeepSystemCaptionVisible, _liveCancellation.Token);
        StatusChanged?.Invoke(this, "实时字幕已启动。");
    }

    public async Task StopLiveAsync()
    {
        _captionSource.TextObserved -= OnCaptionObserved;
        _captionTranslator?.Complete();
        _liveCancellation?.Cancel();
        await _captionSource.StopAsync();
        if (_captionWorker is not null)
        {
            try
            {
                await _captionWorker;
            }
            catch (OperationCanceledException)
            {
            }
        }

        await _sessionStore.StopAsync();
        _liveCancellation?.Dispose();
        _liveCancellation = null;
        _captionWorker = null;
        _captionTranslator = null;
        StatusChanged?.Invoke(this, "实时字幕已停止。");
    }

    public async Task StopAsync()
    {
        await StopLiveAsync();
        _mouseWatcher.Dispose();
        _hotkeyService.Dispose();
        await _captionSource.DisposeAsync();
        await _sessionStore.DisposeAsync();
    }

    public async ValueTask DisposeAsync() => await StopAsync();

    private async void OnMouseSelectionCompleted(object? sender, EventArgs eventArgs)
    {
        if (_settings.Current.Selection.Enabled && _settings.Current.Selection.ShowToolbarOnSelection)
        {
            await RaiseSelectionAsync(false);
        }
    }

    private async void OnHotkeyPressed(object? sender, EventArgs eventArgs) => await RaiseSelectionAsync(true);

    private async Task RaiseSelectionAsync(bool force)
    {
        var text = await _selectionSource.TryGetSelectionAsync(CancellationToken.None);
        if (string.IsNullOrWhiteSpace(text) || (!force && string.Equals(text, _lastSelection, StringComparison.Ordinal)))
        {
            return;
        }

        _lastSelection = text;
        SelectionReady?.Invoke(this, new SelectionReadyEventArgs(text, System.Windows.Forms.Cursor.Position));
    }

    private async void OnCaptionObserved(object? sender, CaptionTextObserved observed)
    {
        if (_segmenter is null || _captionTranslator is null || _liveCancellation is null)
        {
            return;
        }

        foreach (var segment in _segmenter.Push(observed))
        {
            await _captionTranslator.EnqueueAsync(segment, _liveCancellation.Token);
        }
    }

    private Task<TranslationResult> TranslateCaptionAsync(CaptionSegment segment, CancellationToken cancellationToken) =>
        TranslateSelectionAsync(segment.SourceText, TranslationMode.LiveCaption, cancellationToken);

    private async void OnCaptionTranslated(object? sender, (CaptionSegment Segment, TranslationResult Result) result)
    {
        await _sessionStore.AppendAsync(result.Segment, result.Result, CancellationToken.None);
        CaptionTranslated?.Invoke(this, new CaptionTranslatedEventArgs(result.Segment, result.Result));
    }

    private async void OnCaptionTranslationFailed(object? sender, (CaptionSegment Segment, Exception Error) result)
    {
        await _sessionStore.AppendFailureAsync(result.Segment, result.Error.Message, CancellationToken.None);
        CaptionTranslated?.Invoke(this, new CaptionTranslatedEventArgs(result.Segment,
            new TranslationResult(string.Empty, 0, _settings.Current.Translator.Model, true, result.Error.Message)));
    }
}

public sealed class SelectionReadyEventArgs : EventArgs
{
    public SelectionReadyEventArgs(string text, Point position)
    {
        Text = text;
        Position = position;
    }

    public string Text { get; }
    public Point Position { get; }
}

public sealed class CaptionTranslatedEventArgs : EventArgs
{
    public CaptionTranslatedEventArgs(CaptionSegment segment, TranslationResult result)
    {
        Segment = segment;
        Result = result;
    }

    public CaptionSegment Segment { get; }
    public TranslationResult Result { get; }
}
