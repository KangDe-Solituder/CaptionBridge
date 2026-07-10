using LiveCaption.Core;
using LiveCaption.Llm;
using LiveCaption.Storage;

namespace LiveCaption.Core.Tests;

public sealed class TranslationAndSegmentTests
{
    [Fact]
    public void ApiKeyNormalizationRemovesBearerPrefixAndSmartQuotes()
    {
        var succeeded = OpenAiCompatibleTranslator.TryNormalizeApiKey(
            "  \u201cBearer sk-test_123\u201d  ",
            out var normalized,
            out var error);

        Assert.True(succeeded, error);
        Assert.Equal("sk-test_123", normalized);
    }

    [Fact]
    public void ApiKeyNormalizationRejectsNonAsciiCharacters()
    {
        var succeeded = OpenAiCompatibleTranslator.TryNormalizeApiKey(
            "sk-test\u5bc6\u94a5",
            out _,
            out var error);

        Assert.False(succeeded);
        Assert.Contains("\u65e0\u6548\u5b57\u7b26", error);
    }

    [Fact]
    public void CreatePayloadMergesDeepSeekThinkingSetting()
    {
        var payload = OpenAiCompatibleTranslator.CreatePayload(CreateRequest("{\"thinking\":{\"type\":\"disabled\"}}"));

        Assert.Equal("deepseek-v4-flash", payload["model"]?.GetValue<string>());
        Assert.Equal("disabled", payload["thinking"]?["type"]?.GetValue<string>());
        Assert.Equal(160, payload["max_tokens"]?.GetValue<int>());
    }

    [Fact]
    public void ExtractContentReadsOpenAiCompatibleResponse()
    {
        const string response = "{\"choices\":[{\"message\":{\"content\":\"你好\"}}]}";

        Assert.Equal("你好", OpenAiCompatibleTranslator.ExtractContent(response));
    }

    [Fact]
    public void SegmenterEmitsSentenceAtPunctuation()
    {
        var segmenter = new BasicSegmenter(new CaptionSettings());
        var time = new DateTimeOffset(2026, 7, 11, 12, 0, 0, TimeSpan.FromHours(8));

        var result = segmenter.Push(new CaptionTextObserved(time, "これはテストです。"));

        var segment = Assert.Single(result);
        Assert.Equal("これはテストです。", segment.SourceText);
        Assert.Equal(SegmentState.Commit, segment.State);
    }

    [Fact]
    public void SegmenterEmitsStablePartialText()
    {
        var segmenter = new BasicSegmenter(new CaptionSettings { StableMilliseconds = 1000 });
        var started = new DateTimeOffset(2026, 7, 11, 12, 0, 0, TimeSpan.FromHours(8));

        Assert.Empty(segmenter.Push(new CaptionTextObserved(started, "ちょっと待って")));
        var result = segmenter.Push(new CaptionTextObserved(started.AddMilliseconds(1000), "ちょっと待って"));

        Assert.Equal("ちょっと待って", Assert.Single(result).SourceText);
    }

    [Fact]
    public void SegmenterDoesNotRepeatCommittedPrefix()
    {
        var segmenter = new BasicSegmenter(new CaptionSettings { StableMilliseconds = 1000 });
        var started = new DateTimeOffset(2026, 7, 11, 12, 0, 0, TimeSpan.FromHours(8));

        Assert.Single(segmenter.Push(new CaptionTextObserved(started, "hello.")));
        Assert.Empty(segmenter.Push(new CaptionTextObserved(started.AddMilliseconds(100), "hello. next")));
        var result = segmenter.Push(new CaptionTextObserved(started.AddMilliseconds(1100), "hello. next"));

        Assert.Equal("next", Assert.Single(result).SourceText);
    }

    [Fact]
    public async Task QueueProcessesSegmentsInOrder()
    {
        var queue = new SequentialCaptionTranslator();
        var completed = new List<long>();
        queue.Translated += (_, value) => completed.Add(value.Segment.SegmentId);
        using var cancellation = new CancellationTokenSource();
        var worker = queue.RunAsync((segment, _) => Task.FromResult(new TranslationResult(segment.SourceText, 1, "test")), cancellation.Token);

        await queue.EnqueueAsync(CreateSegment(1), cancellation.Token);
        await queue.EnqueueAsync(CreateSegment(2), cancellation.Token);
        queue.Complete();
        await worker;

        Assert.Equal([1L, 2L], completed);
    }

    [Fact]
    public void SettingsMigrationUpdatesLegacySchemaVersion()
    {
        var migrated = FileSettingsStore.Migrate(new AppSettings { SchemaVersion = 0 });

        Assert.Equal(AppSettings.CurrentSchemaVersion, migrated.SchemaVersion);
    }

    private static TranslationRequest CreateRequest(string extraBody) => new(
        TranslationMode.Selection,
        "Hello",
        "auto",
        "中文",
        ContextPack.Empty,
        [],
        "Plain",
        new ProviderOptions("https://api.deepseek.com", "deepseek-v4-flash", extraBody, 1800, 160, 0.1));

    private static CaptionSegment CreateSegment(long id) => new(id, id, DateTimeOffset.Now, DateTimeOffset.Now, $"segment-{id}", SegmentState.Commit);
}
