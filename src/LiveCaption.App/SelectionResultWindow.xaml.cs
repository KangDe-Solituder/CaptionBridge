using System.Windows;
using LiveCaption.Core;

namespace LiveCaption.App;

public partial class SelectionResultWindow : Window
{
    private readonly TranslationResult _result;

    public SelectionResultWindow(string title, string source, TranslationResult result)
    {
        InitializeComponent();
        _result = result;
        TitleText.Text = title;
        SourceText.Text = source;
        ResultText.Text = result.IsError ? $"请求失败：{result.ErrorMessage}" : result.Text;
        LatencyText.Text = result.IsError ? "" : $"{result.LatencyMilliseconds} ms · {result.Provider}";
    }

    private void Copy_OnClick(object sender, RoutedEventArgs eventArgs) => System.Windows.Clipboard.SetText(ResultText.Text);
    private void Close_OnClick(object sender, RoutedEventArgs eventArgs) => Close();
}
