using System.Windows;
using LiveCaption.Core;

namespace LiveCaption.Windows;

public sealed class WindowsSelectionSource : ITextSelectionSource
{
    private readonly Func<bool> _clipboardFallbackEnabled;

    public WindowsSelectionSource(Func<bool> clipboardFallbackEnabled)
    {
        _clipboardFallbackEnabled = clipboardFallbackEnabled;
    }

    public Task<string?> TryGetSelectionAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return _clipboardFallbackEnabled()
            ? TryGetClipboardSelectionAsync(cancellationToken)
            : Task.FromResult<string?>(null);
    }

    private static async Task<string?> TryGetClipboardSelectionAsync(CancellationToken cancellationToken)
    {
        IDataObject? original = null;
        uint copiedSequence = 0;
        try
        {
            original = Clipboard.GetDataObject();
            NativeMethods.SendCopyShortcut();
            await Task.Delay(90, cancellationToken);
            if (!Clipboard.ContainsText())
            {
                return null;
            }

            copiedSequence = NativeMethods.GetClipboardSequenceNumber();
            var text = Clipboard.GetText().Trim();
            return string.IsNullOrWhiteSpace(text) ? null : text;
        }
        catch (Exception)
        {
            return null;
        }
        finally
        {
            if (original is not null && copiedSequence != 0 && NativeMethods.GetClipboardSequenceNumber() == copiedSequence)
            {
                try
                {
                    Clipboard.SetDataObject(original, true);
                }
                catch (Exception)
                {
                    // Another application may own the clipboard; do not retry or interrupt the user.
                }
            }
        }
    }
}
