using System.Windows;
using System.Net.Http;
using System.IO;
using LiveCaption.Core;
using LiveCaption.Llm;
using LiveCaption.Storage;
using LiveCaption.Windows;
using Microsoft.Extensions.DependencyInjection;

namespace LiveCaption.App;

[System.Diagnostics.CodeAnalysis.SuppressMessage("Design", "CA1001:Types that own disposable fields should be disposable", Justification = "Application disposes the tray icon in OnExit.")]
public partial class App : System.Windows.Application
{
    private ServiceProvider? _services;
    private System.Windows.Forms.NotifyIcon? _trayIcon;
    private OverlayWindow? _overlay;
    private bool _showingUnhandledError;

    public App()
    {
        EnsureWindowsDirectoryEnvironment();
        CrashReporter.WriteLifecycle("Application instance created.");
        DispatcherUnhandledException += OnDispatcherUnhandledException;
        AppDomain.CurrentDomain.UnhandledException += OnDomainUnhandledException;
        TaskScheduler.UnobservedTaskException += OnUnobservedTaskException;
    }

    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        try
        {
            await StartApplicationAsync();
            CrashReporter.WriteLifecycle("Application startup completed.");
        }
        catch (Exception exception)
        {
            var path = CrashReporter.WriteException(exception, "Application startup failed.", true);
            System.Windows.Forms.MessageBox.Show($"LiveCaption 启动失败。\n\n{exception.Message}\n\n错误日志：{path}", "LiveCaption",
                System.Windows.Forms.MessageBoxButtons.OK, System.Windows.Forms.MessageBoxIcon.Error);
            Shutdown(-1);
        }
    }

    private async Task StartApplicationAsync()
    {
        var services = new ServiceCollection();
        services.AddSingleton<ISettingsStore, FileSettingsStore>();
        services.AddSingleton<ISecretStore, DpapiSecretStore>();
        services.AddSingleton<ISessionStore, JsonlSessionStore>();
        services.AddSingleton<SettingsService>();
        services.AddSingleton<HttpClient>();
        services.AddSingleton<ITranslator, OpenAiCompatibleTranslator>();
        services.AddSingleton<ITextSelectionSource>(provider => new WindowsSelectionSource(
            () => provider.GetRequiredService<SettingsService>().Current.Selection.ClipboardFallbackEnabled));
        services.AddSingleton<ILiveCaptionSource, WindowsLiveCaptionSource>();
        services.AddSingleton<MouseSelectionWatcher>(_ => new MouseSelectionWatcher(Dispatcher));
        services.AddSingleton<GlobalHotkeyService>();
        services.AddSingleton<AppRuntime>();
        services.AddSingleton<MainViewModel>();
        services.AddSingleton<MainWindow>();
        _services = services.BuildServiceProvider();

        var settings = _services.GetRequiredService<SettingsService>();
        await settings.LoadAsync(CancellationToken.None);
        var runtime = _services.GetRequiredService<AppRuntime>();
        _overlay = new OverlayWindow();
        runtime.SelectionReady += OnSelectionReady;
        runtime.CaptionTranslated += OnCaptionTranslated;
        runtime.StartSelectionFeatures();

        var mainWindow = _services.GetRequiredService<MainWindow>();
        mainWindow.Show();
        try
        {
            CreateTrayIcon(mainWindow, runtime);
        }
        catch (Exception exception)
        {
            CrashReporter.WriteException(exception, "Tray icon initialization failed.");
        }
    }

    protected override async void OnExit(ExitEventArgs e)
    {
        try
        {
            if (_services is not null)
            {
                await _services.GetRequiredService<AppRuntime>().StopAsync();
            }

            _trayIcon?.Dispose();
            _services?.Dispose();
            CrashReporter.WriteLifecycle($"Application exited normally with code {e.ApplicationExitCode}.");
        }
        catch (Exception exception)
        {
            CrashReporter.WriteException(exception, "Application shutdown failed.");
        }
        base.OnExit(e);
    }

    private void OnSelectionReady(object? sender, SelectionReadyEventArgs eventArgs)
    {
        Dispatcher.Invoke(() => new SelectionToolbarWindow(eventArgs.Text, eventArgs.Position,
            _services!.GetRequiredService<AppRuntime>()).Show());
    }

    private void OnCaptionTranslated(object? sender, CaptionTranslatedEventArgs eventArgs)
    {
        Dispatcher.Invoke(() =>
        {
            if (_overlay is null)
            {
                return;
            }

            _overlay.ApplySettings(_services!.GetRequiredService<SettingsService>().Current.Overlay);
            _overlay.Update(eventArgs.Segment.SourceText, eventArgs.Result.IsError ? eventArgs.Result.ErrorMessage ?? "翻译失败" : eventArgs.Result.Text);
            if (!_overlay.IsVisible)
            {
                _overlay.Show();
            }
        });
    }

    private void CreateTrayIcon(MainWindow mainWindow, AppRuntime runtime)
    {
        var menu = new System.Windows.Forms.ContextMenuStrip();
        menu.Items.Add("显示主窗口", null, (_, _) => Dispatcher.Invoke(mainWindow.ShowAndActivate));
        menu.Items.Add("实时字幕", null, async (_, _) =>
        {
            try
            {
                await runtime.ToggleLiveAsync();
            }
            catch (Exception exception)
            {
                CrashReporter.WriteException(exception, "Tray live-caption toggle failed.");
            }
        });
        menu.Items.Add("退出", null, (_, _) =>
        {
            mainWindow.RequestExit();
            Shutdown();
        });
        _trayIcon = new System.Windows.Forms.NotifyIcon
        {
            Icon = System.Drawing.SystemIcons.Application,
            Text = "LiveCaption",
            Visible = true,
            ContextMenuStrip = menu
        };
        _trayIcon.DoubleClick += (_, _) => Dispatcher.Invoke(mainWindow.ShowAndActivate);
    }

    private static void EnsureWindowsDirectoryEnvironment()
    {
        var windowsDirectory = Environment.GetEnvironmentVariable("WINDIR");
        if (!string.IsNullOrWhiteSpace(windowsDirectory) && Path.IsPathFullyQualified(windowsDirectory))
        {
            return;
        }

        var systemRoot = Environment.GetEnvironmentVariable("SystemRoot");
        if (!string.IsNullOrWhiteSpace(systemRoot))
        {
            Environment.SetEnvironmentVariable("WINDIR", systemRoot, EnvironmentVariableTarget.Process);
        }
    }

    private void OnDispatcherUnhandledException(object sender, System.Windows.Threading.DispatcherUnhandledExceptionEventArgs eventArgs)
    {
        var path = CrashReporter.WriteException(eventArgs.Exception, "Unhandled WPF dispatcher exception.");
        eventArgs.Handled = true;
        if (_showingUnhandledError)
        {
            return;
        }

        try
        {
            _showingUnhandledError = true;
            System.Windows.Forms.MessageBox.Show($"LiveCaption 遇到了一个错误，但已阻止程序直接退出。\n\n{eventArgs.Exception.Message}\n\n错误日志：{path}",
                "LiveCaption", System.Windows.Forms.MessageBoxButtons.OK, System.Windows.Forms.MessageBoxIcon.Warning);
        }
        finally
        {
            _showingUnhandledError = false;
        }
    }

    private static void OnDomainUnhandledException(object sender, UnhandledExceptionEventArgs eventArgs)
    {
        if (eventArgs.ExceptionObject is Exception exception)
        {
            CrashReporter.WriteException(exception, "Unhandled application-domain exception.", true);
        }
        else
        {
            CrashReporter.WriteLifecycle($"Unhandled non-exception object: {eventArgs.ExceptionObject}");
        }
    }

    private static void OnUnobservedTaskException(object? sender, UnobservedTaskExceptionEventArgs eventArgs)
    {
        CrashReporter.WriteException(eventArgs.Exception, "Unobserved task exception.");
        eventArgs.SetObserved();
    }
}
