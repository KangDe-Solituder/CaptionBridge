using System.Windows.Threading;

namespace LiveCaption.Windows;

public sealed class MouseSelectionWatcher : IDisposable
{
    private readonly Dispatcher _dispatcher;
    private readonly NativeMethods.LowLevelMouseProc _callback;
    private IntPtr _hook;

    public MouseSelectionWatcher(Dispatcher dispatcher)
    {
        _dispatcher = dispatcher;
        _callback = HookCallback;
    }

    public event EventHandler? SelectionCompleted;

    public void Start()
    {
        if (_hook != IntPtr.Zero)
        {
            return;
        }

        _hook = NativeMethods.SetWindowsHookEx(NativeMethods.WhMouseLl, _callback, NativeMethods.GetModuleHandle(null), 0);
        if (_hook == IntPtr.Zero)
        {
            throw new InvalidOperationException("无法注册全局鼠标监听。");
        }
    }

    public void Dispose()
    {
        if (_hook != IntPtr.Zero)
        {
            NativeMethods.UnhookWindowsHookEx(_hook);
            _hook = IntPtr.Zero;
        }
    }

    private IntPtr HookCallback(int code, IntPtr wParam, IntPtr lParam)
    {
        if (code >= 0 && wParam.ToInt32() == NativeMethods.WmLButtonUp)
        {
            _dispatcher.BeginInvoke(async () =>
            {
                await Task.Delay(110);
                SelectionCompleted?.Invoke(this, EventArgs.Empty);
            });
        }

        return NativeMethods.CallNextHookEx(_hook, code, wParam, lParam);
    }
}
