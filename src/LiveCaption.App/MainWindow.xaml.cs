using System.Windows;

namespace LiveCaption.App;

public partial class MainWindow : Window
{
    private readonly MainViewModel _viewModel;

    public MainWindow(MainViewModel viewModel)
    {
        InitializeComponent();
        _viewModel = viewModel;
        DataContext = _viewModel;
        Loaded += async (_, _) => await _viewModel.InitializeAsync();
    }

    private void ApiKeyBox_OnPasswordChanged(object sender, RoutedEventArgs eventArgs) => _viewModel.ApiKey = ApiKeyBox.Password;
    private async void Save_OnClick(object sender, RoutedEventArgs eventArgs) => await _viewModel.SaveAsync();
    private async void Test_OnClick(object sender, RoutedEventArgs eventArgs) => await _viewModel.TestAsync();
    private async void ToggleLive_OnClick(object sender, RoutedEventArgs eventArgs) => await _viewModel.ToggleLiveAsync();

    public void ShowAndActivate()
    {
        Show();
        WindowState = WindowState.Normal;
        Activate();
    }
}
