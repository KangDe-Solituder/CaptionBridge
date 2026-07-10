using System.Windows;

namespace LiveCaption.App;

public partial class OverlayWindow : Window
{
    public OverlayWindow()
    {
        InitializeComponent();
        Left = 260;
        Top = 760;
        MouseLeftButtonDown += (_, eventArgs) => DragMove();
    }

    public void Update(string source, string translation)
    {
        SourceText.Text = source;
        TranslationText.Text = translation;
    }

    public void ApplySettings(LiveCaption.Core.OverlaySettings settings)
    {
        Opacity = settings.Opacity;
        Topmost = settings.AlwaysOnTop;
        SourceText.FontSize = Math.Max(12, settings.FontSize - 5);
        TranslationText.FontSize = settings.FontSize;
    }
}
