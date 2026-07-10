using System.Diagnostics;
using System.Windows.Automation;
using LiveCaption.Core;

namespace LiveCaption.Windows;

public sealed class WindowsLiveCaptionSource : ILiveCaptionSource
{
    private CancellationTokenSource? _loopCancellation;
    private Task? _loopTask;
    private AutomationElement? _captionElement;

    public event EventHandler<CaptionTextObserved>? TextObserved;

    public bool IsRunning => _loopCancellation is not null;

    public async Task StartAsync(bool keepSystemCaptionVisible, CancellationToken cancellationToken)
    {
        if (IsRunning)
        {
            return;
        }

        TryLaunchLiveCaptions();
        _captionElement = await WaitForCaptionElementAsync(cancellationToken);
        if (_captionElement is null)
        {
            throw new InvalidOperationException("未找到 Windows Live Captions。请确认系统支持该功能，并尝试按 Win + Ctrl + L 启动它。");
        }

        if (!keepSystemCaptionVisible)
        {
            var process = FindLiveCaptionsProcess();
            if (process is not null && process.MainWindowHandle != IntPtr.Zero)
            {
                NativeMethods.ShowWindow(process.MainWindowHandle, NativeMethods.SwHide);
            }
        }

        _loopCancellation = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        _loopTask = PollAsync(_loopCancellation.Token);
    }

    public async Task StopAsync()
    {
        if (_loopCancellation is null)
        {
            return;
        }

        _loopCancellation.Cancel();
        if (_loopTask is not null)
        {
            try
            {
                await _loopTask.ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
            }
        }

        _loopCancellation.Dispose();
        _loopCancellation = null;
        _loopTask = null;
        _captionElement = null;
    }

    public async ValueTask DisposeAsync() => await StopAsync().ConfigureAwait(false);

    private async Task PollAsync(CancellationToken cancellationToken)
    {
        while (!cancellationToken.IsCancellationRequested)
        {
            try
            {
                var text = _captionElement?.Current.Name;
                if (!string.IsNullOrWhiteSpace(text))
                {
                    TextObserved?.Invoke(this, new CaptionTextObserved(DateTimeOffset.Now, text));
                }
            }
            catch (ElementNotAvailableException)
            {
                _captionElement = await WaitForCaptionElementAsync(cancellationToken).ConfigureAwait(false);
            }

            await Task.Delay(50, cancellationToken).ConfigureAwait(false);
        }
    }

    private static void TryLaunchLiveCaptions()
    {
        if (FindLiveCaptionsProcess() is not null)
        {
            return;
        }

        try
        {
            Process.Start(new ProcessStartInfo("LiveCaptions.exe") { UseShellExecute = true });
        }
        catch
        {
            // The caller receives a useful instruction if the element cannot be found.
        }
    }

    private static async Task<AutomationElement?> WaitForCaptionElementAsync(CancellationToken cancellationToken)
    {
        for (var attempt = 0; attempt < 80; attempt++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var element = FindCaptionElement();
            if (element is not null)
            {
                return element;
            }

            await Task.Delay(250, cancellationToken).ConfigureAwait(false);
        }

        return null;
    }

    private static AutomationElement? FindCaptionElement()
    {
        var process = FindLiveCaptionsProcess();
        if (process is null || process.MainWindowHandle == IntPtr.Zero)
        {
            return null;
        }

        var window = AutomationElement.FromHandle(process.MainWindowHandle);
        var byId = window.FindFirst(TreeScope.Descendants,
            new PropertyCondition(AutomationElement.AutomationIdProperty, "CaptionsTextBlock"));
        if (byId is not null)
        {
            return byId;
        }

        return window.FindAll(TreeScope.Descendants,
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Text))
            .Cast<AutomationElement>()
            .OrderByDescending(element => element.Current.Name?.Length ?? 0)
            .FirstOrDefault();
    }

    private static Process? FindLiveCaptionsProcess() =>
        Process.GetProcessesByName("LiveCaptions").FirstOrDefault();
}
