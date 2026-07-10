using System.Windows.Interop;

namespace LiveCaption.Windows;

public sealed class GlobalHotkeyService : IDisposable
{
    private const int HotkeyId = 0x4C43;
    private bool _registered;

    public event EventHandler? Pressed;

    public void RegisterDefault()
    {
        if (_registered)
        {
            return;
        }

        if (!NativeMethods.RegisterHotKey(IntPtr.Zero, HotkeyId, NativeMethods.ModAlt, (uint)'Q'))
        {
            throw new InvalidOperationException("无法注册 Alt+Q。该快捷键可能已被其他程序占用。");
        }

        ComponentDispatcher.ThreadFilterMessage += OnThreadFilterMessage;
        _registered = true;
    }

    public void Dispose()
    {
        if (_registered)
        {
            ComponentDispatcher.ThreadFilterMessage -= OnThreadFilterMessage;
            NativeMethods.UnregisterHotKey(IntPtr.Zero, HotkeyId);
            _registered = false;
        }
    }

    private void OnThreadFilterMessage(ref MSG message, ref bool handled)
    {
        if (message.message == NativeMethods.WmHotkey && message.wParam.ToInt32() == HotkeyId)
        {
            handled = true;
            Pressed?.Invoke(this, EventArgs.Empty);
        }
    }
}
