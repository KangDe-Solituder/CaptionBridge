using System.Windows;
using System.ComponentModel;

namespace LiveCaption.App;

public partial class MainWindow : Window
{
    private readonly MainViewModel _viewModel;
    private bool _exitRequested;

    public MainWindow(MainViewModel viewModel)
    {
        InitializeComponent();
        _viewModel = viewModel;
        DataContext = _viewModel;
        Loaded += async (_, _) =>
        {
            await _viewModel.InitializeAsync();
            ApiKeyBox.Password = _viewModel.ApiKey;
        };
        Closing += OnClosing;
    }

    private void ApiKeyBox_OnPasswordChanged(object sender, RoutedEventArgs eventArgs) => _viewModel.ApiKey = ApiKeyBox.Password;
    private async void Save_OnClick(object sender, RoutedEventArgs eventArgs) => await _viewModel.SaveAsync();
    private async void Test_OnClick(object sender, RoutedEventArgs eventArgs) => await _viewModel.TestAsync();
    private async void ToggleLive_OnClick(object sender, RoutedEventArgs eventArgs) => await _viewModel.ToggleLiveAsync();
    private void OpenDiagnostics_OnClick(object sender, RoutedEventArgs eventArgs) => _viewModel.OpenDiagnosticsFolder();

    public void ShowAndActivate()
    {
        Show();
        WindowState = WindowState.Normal;
        Activate();
    }

    public void RequestExit()
    {
        _exitRequested = true;
        Close();
    }

    private void OnClosing(object? sender, CancelEventArgs eventArgs)
    {
        if (_exitRequested)
        {
            return;
        }

        eventArgs.Cancel = true;
        Hide();
    }
}
