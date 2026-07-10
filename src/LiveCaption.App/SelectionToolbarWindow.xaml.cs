using System.Windows;
using LiveCaption.Core;

namespace LiveCaption.App;

public partial class SelectionToolbarWindow : Window
{
    private readonly string _source;
    private readonly AppRuntime _runtime;

    public SelectionToolbarWindow(string source, System.Drawing.Point position, AppRuntime runtime)
    {
        InitializeComponent();
        _source = source;
        _runtime = runtime;
        Left = position.X - 40;
        Top = position.Y + 18;
        Deactivated += (_, _) => Close();
    }

    private async void Translate_OnClick(object sender, RoutedEventArgs eventArgs) => await TranslateAsync(TranslationMode.Selection, "翻译");
    private async void Explain_OnClick(object sender, RoutedEventArgs eventArgs) => await TranslateAsync(TranslationMode.Explanation, "解释");

    private void Copy_OnClick(object sender, RoutedEventArgs eventArgs)
    {
        System.Windows.Clipboard.SetText(_source);
        Close();
    }

    private async Task TranslateAsync(TranslationMode mode, string title)
    {
        Hide();
        var result = await _runtime.TranslateSelectionAsync(_source, mode);
        var window = new SelectionResultWindow(title, _source, result);
        window.Left = Left;
        window.Top = Top + 58;
        window.Show();
        Close();
    }
}
