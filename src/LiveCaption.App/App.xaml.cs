using System.Windows;
using System.Net.Http;
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

    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
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
        CreateTrayIcon(mainWindow, runtime);
    }

    protected override async void OnExit(ExitEventArgs e)
    {
        if (_services is not null)
        {
            await _services.GetRequiredService<AppRuntime>().StopAsync();
        }

        _trayIcon?.Dispose();
        _services?.Dispose();
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
        menu.Items.Add("实时字幕", null, async (_, _) => await runtime.ToggleLiveAsync());
        menu.Items.Add("退出", null, (_, _) => Shutdown());
        _trayIcon = new System.Windows.Forms.NotifyIcon
        {
            Icon = System.Drawing.SystemIcons.Application,
            Text = "LiveCaption",
            Visible = true,
            ContextMenuStrip = menu
        };
        _trayIcon.DoubleClick += (_, _) => Dispatcher.Invoke(mainWindow.ShowAndActivate);
    }
}
