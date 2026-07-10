namespace LiveCaption.Core;

public enum TranslationMode
{
    Selection,
    Explanation,
    LiveCaption,
    Scene
}

public enum SegmentState
{
    Preview,
    Commit,
    Failed
}

public sealed record ContextPack(
    IReadOnlyList<string> RecentSegments,
    IReadOnlyList<TermCandidate> Terms)
{
    public static readonly ContextPack Empty = new([], []);
}

public sealed record TermCandidate(string Id, string Source, string? Reading, string? PreferredTranslation);

public sealed record ProviderOptions(
    string Endpoint,
    string Model,
    string ExtraBodyJson,
    int TimeoutMilliseconds,
    int MaxTokens,
    double Temperature);

public sealed record TranslationRequest(
    TranslationMode Mode,
    string SourceText,
    string SourceLanguage,
    string TargetLanguage,
    ContextPack Context,
    IReadOnlyList<TermCandidate> TermCandidates,
    string Style,
    ProviderOptions ProviderOptions);

public sealed record TranslationResult(
    string Text,
    long LatencyMilliseconds,
    string Provider,
    bool IsError = false,
    string? ErrorMessage = null);

public sealed record CaptionSegment(
    long SegmentId,
    long SequenceId,
    DateTimeOffset StartedAt,
    DateTimeOffset EndedAt,
    string SourceText,
    SegmentState State);

public sealed record CaptionTextObserved(DateTimeOffset ObservedAt, string Text);

public sealed record AppSettings
{
    public const int CurrentSchemaVersion = 1;

    public int SchemaVersion { get; init; } = CurrentSchemaVersion;
    public TranslatorSettings Translator { get; init; } = new();
    public SelectionSettings Selection { get; init; } = new();
    public CaptionSettings Captions { get; init; } = new();
    public OverlaySettings Overlay { get; init; } = new();
}

public sealed record TranslatorSettings
{
    public string Endpoint { get; init; } = "https://api.deepseek.com";
    public string Model { get; init; } = "deepseek-v4-flash";
    public string TargetLanguage { get; init; } = "中文";
    public string ExtraBodyJson { get; init; } = "{\"thinking\":{\"type\":\"disabled\"}}";
    public int TimeoutMilliseconds { get; init; } = 1800;
    public int MaxTokens { get; init; } = 160;
    public double Temperature { get; init; } = 0.1;
}

public sealed record SelectionSettings
{
    public bool Enabled { get; init; } = true;
    public bool ShowToolbarOnSelection { get; init; } = true;
    public bool ClipboardFallbackEnabled { get; init; } = true;
    public string Hotkey { get; init; } = "Alt+Q";
}

public sealed record CaptionSettings
{
    public bool KeepSystemCaptionVisible { get; init; }
    public int StableMilliseconds { get; init; } = 1000;
    public int MaximumDurationMilliseconds { get; init; } = 3000;
    public int MaximumLength { get; init; } = 96;
}

public sealed record OverlaySettings
{
    public double Opacity { get; init; } = 0.9;
    public double FontSize { get; init; } = 22;
    public bool AlwaysOnTop { get; init; } = true;
}
