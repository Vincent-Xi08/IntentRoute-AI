using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Animation;

namespace ProxyManager.Standalone;

/// <summary>
/// 对话框进出场动效：200ms 缩放 0.97→1 + 淡入，缓动 EaseOut。
/// 借鉴 Motion base-dialog 的入场参数，映射为 WPF Storyboard。
/// 注意：Window 本身不允许设置 RenderTransform（CoerceRenderTransform 拒绝），
/// 因此动画作用于窗口的内容根元素。
/// </summary>
internal static class DialogEntrance
{
    public static void Play(Window window)
    {
        ArgumentNullException.ThrowIfNull(window);
        if (window.Content is not FrameworkElement content) return;

        var scale = new ScaleTransform(0.97, 0.97, content.ActualWidth / 2, content.ActualHeight / 2);
        content.RenderTransform = scale;
        content.Opacity = 0;

        var ease = new CubicEase { EasingMode = EasingMode.EaseOut };
        var storyboard = new Storyboard();

        Add(storyboard, content, ScaleTransform.ScaleXProperty, 0.97, 1, ease);
        Add(storyboard, content, ScaleTransform.ScaleYProperty, 0.97, 1, ease);

        var fade = new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(200)) { EasingFunction = ease };
        Storyboard.SetTarget(fade, content);
        Storyboard.SetTargetProperty(fade, new PropertyPath(nameof(UIElement.Opacity)));
        storyboard.Children.Add(fade);

        storyboard.Begin();
    }

    private static void Add(
        Storyboard storyboard,
        FrameworkElement content,
        DependencyProperty property,
        double from,
        double to,
        EasingFunctionBase ease)
    {
        var animation = new DoubleAnimation(from, to, TimeSpan.FromMilliseconds(200)) { EasingFunction = ease };
        Storyboard.SetTarget(animation, content);
        Storyboard.SetTargetProperty(animation,
            new PropertyPath($"(UIElement.RenderTransform).(ScaleTransform.{property.Name})"));
        storyboard.Children.Add(animation);
    }
}
