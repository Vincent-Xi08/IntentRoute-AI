using System.Windows;
using Strings = ProxyManager.Standalone.Localization.Strings;

namespace ProxyManager.Standalone;

public partial class RuleImportPreviewWindow : Window
{
    public RuleImportPreviewWindow(RuleImportPreview preview)
    {
        InitializeComponent();
        ArgumentNullException.ThrowIfNull(preview);

        Title = Strings.ImportPreviewTitle;
        SummaryText.Text = preview.SummaryText;
        RowsList.ItemsSource = preview.Rows;
        ConfirmButton.IsEnabled = preview.HasAdditions;
        NothingText.Visibility = preview.HasAdditions ? Visibility.Collapsed : Visibility.Visible;
    }

    private void Window_Loaded(object sender, RoutedEventArgs e)
    {
        DarkTitleBar.Apply(this);
        DialogEntrance.Play(this);
    }

    private void Confirm_Click(object sender, RoutedEventArgs e) => DialogResult = true;

    private void Cancel_Click(object sender, RoutedEventArgs e) => DialogResult = false;
}
