using System.Text;

namespace LiveCaption.Core;

public sealed class BasicSegmenter : ISegmenter
{
    private readonly CaptionSettings _settings;
    private string _activeText = string.Empty;
    private string _lastObservedText = string.Empty;
    private string _committedPrefix = string.Empty;
    private DateTimeOffset _activeStartedAt;
    private DateTimeOffset _lastChangedAt;
    private long _nextSegmentId;
    private long _nextSequenceId;

    public BasicSegmenter(CaptionSettings settings)
    {
        _settings = settings;
    }

    public IReadOnlyList<CaptionSegment> Push(CaptionTextObserved observed)
    {
        var text = Normalize(observed.Text);
        if (string.IsNullOrWhiteSpace(text))
        {
            return [];
        }

        var emitted = new List<CaptionSegment>();
        if (!string.Equals(text, _lastObservedText, StringComparison.Ordinal))
        {
            _lastObservedText = text;
            var committedOffset = 0;
            if (!string.IsNullOrEmpty(_committedPrefix) && text.StartsWith(_committedPrefix, StringComparison.Ordinal))
            {
                committedOffset = _committedPrefix.Length;
                text = text[committedOffset..].TrimStart();
                committedOffset = _lastObservedText.Length - text.Length;
            }
            else
            {
                _committedPrefix = string.Empty;
            }

            if (string.IsNullOrWhiteSpace(text))
            {
                return emitted;
            }

            if (string.IsNullOrEmpty(_activeText))
            {
                _activeStartedAt = observed.ObservedAt;
            }

            _activeText = text;
            _lastChangedAt = observed.ObservedAt;

            var completeLength = FindLastSentenceEnd(text);
            if (completeLength > 0)
            {
                emitted.Add(CreateSegment(text[..completeLength], observed.ObservedAt));
                _committedPrefix = _lastObservedText[..(committedOffset + completeLength)];
                _activeText = text[completeLength..].Trim();
                _activeStartedAt = observed.ObservedAt;
            }
        }
        else if (!string.IsNullOrWhiteSpace(_activeText))
        {
            var stableFor = observed.ObservedAt - _lastChangedAt;
            var activeFor = observed.ObservedAt - _activeStartedAt;
            if (stableFor.TotalMilliseconds >= _settings.StableMilliseconds ||
                activeFor.TotalMilliseconds >= _settings.MaximumDurationMilliseconds ||
                _activeText.Length >= _settings.MaximumLength)
            {
                emitted.Add(CreateSegment(_activeText, observed.ObservedAt));
                _committedPrefix = _lastObservedText;
                _activeText = string.Empty;
            }
        }

        return emitted;
    }

    public void Reset()
    {
        _activeText = string.Empty;
        _lastObservedText = string.Empty;
        _committedPrefix = string.Empty;
        _activeStartedAt = default;
        _lastChangedAt = default;
    }

    private CaptionSegment CreateSegment(string text, DateTimeOffset endedAt) => new(
        Interlocked.Increment(ref _nextSegmentId),
        Interlocked.Increment(ref _nextSequenceId),
        _activeStartedAt == default ? endedAt : _activeStartedAt,
        endedAt,
        text.Trim(),
        SegmentState.Commit);

    private static int FindLastSentenceEnd(string text)
    {
        for (var index = text.Length - 1; index >= 0; index--)
        {
            if (".?!。？！".Contains(text[index]))
            {
                return index + 1;
            }
        }

        return 0;
    }

    private static string Normalize(string text)
    {
        var builder = new StringBuilder(text.Length);
        var previousWhitespace = false;
        foreach (var character in text.Normalize())
        {
            if (char.IsWhiteSpace(character))
            {
                if (!previousWhitespace)
                {
                    builder.Append(' ');
                }

                previousWhitespace = true;
                continue;
            }

            builder.Append(character);
            previousWhitespace = false;
        }

        return builder.ToString().Trim();
    }
}
