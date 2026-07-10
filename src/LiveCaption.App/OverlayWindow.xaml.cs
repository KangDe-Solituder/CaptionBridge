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
}
