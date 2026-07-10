using System.Text.Json;
using LiveCaption.Core;

namespace LiveCaption.Storage;

public sealed class JsonlSessionStore : ISessionStore
{
    private readonly string _sessionDirectory;
    private StreamWriter? _writer;
    private string? _model;

    public JsonlSessionStore(string? rootDirectory = null)
    {
        _sessionDirectory = rootDirectory ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "LiveCaption", "sessions");
    }

    public Task StartAsync(string model, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        Directory.CreateDirectory(_sessionDirectory);
        var name = DateTimeOffset.Now.ToString("yyyy-MM-dd_HH-mm-ss", System.Globalization.CultureInfo.InvariantCulture);
        _writer = new StreamWriter(File.Open(Path.Combine(_sessionDirectory, $"{name}.jsonl"), FileMode.Append, FileAccess.Write, FileShare.Read))
        {
            AutoFlush = true
        };
        _model = model;
        return Task.CompletedTask;
    }

    public Task AppendAsync(CaptionSegment segment, TranslationResult result, CancellationToken cancellationToken) =>
        WriteAsync(new
        {
            at = DateTimeOffset.Now,
            segment_id = segment.SegmentId,
            sequence_id = segment.SequenceId,
            start = segment.StartedAt,
            end = segment.EndedAt,
            source = segment.SourceText,
            translation = result.Text,
            status = result.IsError ? "error" : "ok",
            error = result.ErrorMessage,
            model = _model,
            latency_ms = result.LatencyMilliseconds
        }, cancellationToken);

    public Task AppendFailureAsync(CaptionSegment segment, string failureMessage, CancellationToken cancellationToken) =>
        WriteAsync(new
        {
            at = DateTimeOffset.Now,
            segment_id = segment.SegmentId,
            sequence_id = segment.SequenceId,
            start = segment.StartedAt,
            end = segment.EndedAt,
            source = segment.SourceText,
            status = "error",
            error = failureMessage,
            model = _model
        }, cancellationToken);

    public async Task StopAsync()
    {
        if (_writer is not null)
        {
            await _writer.DisposeAsync().ConfigureAwait(false);
            _writer = null;
        }
    }

    public async ValueTask DisposeAsync() => await StopAsync().ConfigureAwait(false);

    private Task WriteAsync<T>(T value, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return _writer is null
            ? Task.CompletedTask
            : _writer.WriteLineAsync(JsonSerializer.Serialize(value).AsMemory(), cancellationToken);
    }
}
