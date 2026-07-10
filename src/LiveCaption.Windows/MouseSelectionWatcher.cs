using System.Windows.Threading;
using System.Runtime.InteropServices;

namespace LiveCaption.Windows;

public sealed class MouseSelectionWatcher : IDisposable
{
    private readonly Dispatcher _dispatcher;
    private readonly NativeMethods.LowLevelMouseProc _callback;
    private IntPtr _hook;
    private NativeMethods.Point? _mouseDownPosition;

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
        if (code >= 0 && wParam.ToInt32() == NativeMethods.WmLButtonDown)
        {
            _mouseDownPosition = Marshal.PtrToStructure<NativeMethods.LowLevelMouseHookData>(lParam).Position;
        }
        else if (code >= 0 && wParam.ToInt32() == NativeMethods.WmLButtonUp)
        {
            var mouseUp = Marshal.PtrToStructure<NativeMethods.LowLevelMouseHookData>(lParam).Position;
            var mouseDown = _mouseDownPosition;
            _mouseDownPosition = null;
            if (mouseDown is not null && IsDragSelection(mouseDown.Value, mouseUp))
            {
                _dispatcher.BeginInvoke(async () =>
                {
                    await Task.Delay(110);
                    SelectionCompleted?.Invoke(this, EventArgs.Empty);
                });
            }
        }

        return NativeMethods.CallNextHookEx(_hook, code, wParam, lParam);
    }

    private static bool IsDragSelection(NativeMethods.Point start, NativeMethods.Point end) =>
        Math.Abs(start.X - end.X) >= 4 || Math.Abs(start.Y - end.Y) >= 4;
}
