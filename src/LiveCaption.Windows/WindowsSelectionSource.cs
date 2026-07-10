using System.Windows;
using System.Windows.Automation;
using System.Runtime.InteropServices;
using LiveCaption.Core;

namespace LiveCaption.Windows;

public sealed class WindowsSelectionSource : ITextSelectionSource
{
    private readonly Func<bool> _clipboardFallbackEnabled;

    public WindowsSelectionSource(Func<bool> clipboardFallbackEnabled)
    {
        _clipboardFallbackEnabled = clipboardFallbackEnabled;
    }

    public async Task<string?> TryGetSelectionAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var automationText = TryGetAutomationSelection();
        if (!string.IsNullOrWhiteSpace(automationText))
        {
            return automationText.Trim();
        }

        return _clipboardFallbackEnabled() ? await TryGetClipboardSelectionAsync(cancellationToken) : null;
    }

    private static string? TryGetAutomationSelection()
    {
        try
        {
            var focused = AutomationElement.FocusedElement;
            if (focused is null || !focused.TryGetCurrentPattern(TextPattern.Pattern, out var pattern))
            {
                return null;
            }

            var ranges = ((TextPattern)pattern).GetSelection();
            return ranges.Length == 0 ? null : string.Join(" ", ranges.Select(range => range.GetText(-1)));
        }
        catch (ElementNotAvailableException)
        {
            return null;
        }
        catch (InvalidOperationException)
        {
            return null;
        }
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
            return Clipboard.GetText().Trim();
        }
        catch (COMException)
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
                catch (COMException)
                {
                    // Another application may own the clipboard; leaving the copied text is safer than retrying.
                }
            }
        }
    }
}
