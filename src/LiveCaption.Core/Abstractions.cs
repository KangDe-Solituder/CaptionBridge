namespace LiveCaption.Core;

public interface ITranslator
{
    Task<TranslationResult> TranslateAsync(TranslationRequest request, CancellationToken cancellationToken);
}

public interface ITextSelectionSource
{
    Task<string?> TryGetSelectionAsync(CancellationToken cancellationToken);
}

public interface ILiveCaptionSource : IAsyncDisposable
{
    event EventHandler<CaptionTextObserved>? TextObserved;

    bool IsRunning { get; }
    Task StartAsync(bool keepSystemCaptionVisible, CancellationToken cancellationToken);
    Task StopAsync();
}

public interface ISegmenter
{
    IReadOnlyList<CaptionSegment> Push(CaptionTextObserved observed);
    void Reset();
}

public interface ISettingsStore
{
    Task<AppSettings> LoadAsync(CancellationToken cancellationToken);
    Task SaveAsync(AppSettings settings, CancellationToken cancellationToken);
}

public interface ISecretStore
{
    Task<string?> GetAsync(string name, CancellationToken cancellationToken);
    Task SetAsync(string name, string value, CancellationToken cancellationToken);
    Task DeleteAsync(string name, CancellationToken cancellationToken);
}

public interface ISessionStore : IAsyncDisposable
{
    Task StartAsync(string model, CancellationToken cancellationToken);
    Task AppendAsync(CaptionSegment segment, TranslationResult result, CancellationToken cancellationToken);
    Task AppendFailureAsync(CaptionSegment segment, string failureMessage, CancellationToken cancellationToken);
    Task StopAsync();
}
