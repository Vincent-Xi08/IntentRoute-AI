using System.Windows;
using System.Windows.Media;
using Strings = ProxyManager.Standalone.Localization.Strings;

namespace ProxyManager.Standalone;

public enum DarkDialogIcon { Info, Question, Warning, Error }

public enum DarkDialogButtons { Ok, OkCancel, YesNo }

/// <summary>
/// 深色主题弹窗：与系统 MessageBox 语义对齐（MessageBoxResult 返回值），
/// 但视觉完全走应用设计令牌——Soft 芯片图标 + Primary/Secondary 按钮 + 进场动效。
/// Esc 触发取消/否，Enter 触发确认。启动阶段（主窗口不存在）的致命错误仍走系统 MessageBox。
/// </summary>
public partial class DarkDialogWindow : Window
{
    private DarkDialogButtons _buttons;

    public MessageBoxResult Result { get; private set; } = MessageBoxResult.Cancel;

    private DarkDialogWindow()
    {
        InitializeComponent();
    }

    public static MessageBoxResult Show(
        Window? owner,
        string message,
        string title,
        DarkDialogIcon icon = DarkDialogIcon.Info,
        DarkDialogButtons buttons = DarkDialogButtons.Ok)
    {
        var dialog = new DarkDialogWindow
        {
            _buttons = buttons,
            Title = title
        };
        dialog.MessageText.Text = message;
        dialog.ApplyIcon(icon);
        dialog.ApplyButtons(buttons);

        if (owner != null) dialog.Owner = owner;
        dialog.ShowDialog();
        return dialog.Result;
    }

    private void ApplyIcon(DarkDialogIcon icon)
    {
        // 圆/三角芯片 + Soft 底，与页头图标芯片同一视觉语言。
        var (geometry, stroke, chip) = icon switch
        {
            DarkDialogIcon.Question =>
                ("M12,2 a10,10 0 1,0 0.001,0 M9.6,9.4 a2.4,2.6 0 1,1 3.4,2.4 c-0.8,0.5 -1,1 -1,1.9 M12,16.6 v0.02",
                 "{AccentBrush}", "{AccentSoftBrush}"),
            DarkDialogIcon.Warning =>
                ("M12,3.5 L21.5,20 L2.5,20 Z M12,10 v4.4 M12,17.2 v0.02",
                 "{WarningBrush}", "{WarningSoftBrush}"),
            DarkDialogIcon.Error =>
                ("M12,2 a10,10 0 1,0 0.001,0 M9,9 L15,15 M15,9 L9,15",
                 "{ErrorBrush}", "{ErrorSoftBrush}"),
            _ => ("M12,2 a10,10 0 1,0 0.001,0 M12,11 v5 M12,7.6 v0.02",
                 "{AccentBrush}", "{AccentSoftBrush}")
        };

        IconPath.Data = Geometry.Parse(geometry);
        IconPath.Stroke = (Brush)FindResource(TrimBraces(stroke));
        IconChip.Background = (Brush)FindResource(TrimBraces(chip));
        IconChip.BorderBrush = (Brush)FindResource(TrimBraces(chip));
        IconChip.BorderThickness = new Thickness(1);
    }

    private static string TrimBraces(string key) => key.Trim('{', '}');

    private void ApplyButtons(DarkDialogButtons buttons)
    {
        switch (buttons)
        {
            case DarkDialogButtons.YesNo:
                ConfirmButton.Content = Strings.CommonYes;
                CancelButton.Visibility = Visibility.Visible;
                CancelButton.Content = Strings.CommonNo;
                break;
            case DarkDialogButtons.OkCancel:
                ConfirmButton.Content = Strings.CommonOk;
                CancelButton.Visibility = Visibility.Visible;
                CancelButton.Content = Strings.CommonCancel;
                break;
            default:
                ConfirmButton.Content = Strings.CommonOk;
                CancelButton.Visibility = Visibility.Collapsed;
                break;
        }
    }

    private void Confirm_Click(object sender, RoutedEventArgs e)
    {
        Result = _buttons switch
        {
            DarkDialogButtons.YesNo => MessageBoxResult.Yes,
            _ => MessageBoxResult.OK
        };
        DialogResult = true;
    }

    private void Cancel_Click(object sender, RoutedEventArgs e)
    {
        Result = _buttons == DarkDialogButtons.YesNo ? MessageBoxResult.No : MessageBoxResult.Cancel;
        DialogResult = false;
    }

    private void Window_Loaded(object sender, RoutedEventArgs e)
    {
        DarkTitleBar.Apply(this);
        DialogEntrance.Play(this);
    }
}
