using System.Threading.Channels;

namespace LiveCaption.Core;

public sealed class SequentialCaptionTranslator
{
    private readonly Channel<CaptionSegment> _channel = Channel.CreateBounded<CaptionSegment>(new BoundedChannelOptions(64)
    {
        FullMode = BoundedChannelFullMode.Wait,
        SingleReader = true,
        SingleWriter = false
    });

    public event EventHandler<(CaptionSegment Segment, TranslationResult Result)>? Translated;
    public event EventHandler<(CaptionSegment Segment, Exception Error)>? Failed;

    public ValueTask EnqueueAsync(CaptionSegment segment, CancellationToken cancellationToken) =>
        _channel.Writer.WriteAsync(segment, cancellationToken);

    public async Task RunAsync(Func<CaptionSegment, CancellationToken, Task<TranslationResult>> translate, CancellationToken cancellationToken)
    {
        await foreach (var segment in _channel.Reader.ReadAllAsync(cancellationToken))
        {
            try
            {
                var result = await translate(segment, cancellationToken).ConfigureAwait(false);
                Translated?.Invoke(this, (segment, result));
            }
            catch (Exception exception) when (!cancellationToken.IsCancellationRequested)
            {
                Failed?.Invoke(this, (segment, exception));
            }
        }
    }

    public void Complete() => _channel.Writer.TryComplete();
}
