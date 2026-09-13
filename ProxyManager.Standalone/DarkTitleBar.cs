using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;

namespace ProxyManager.Standalone;

/// <summary>
/// 让 WindowStyle="SingleBorderWindow" 的对话框获得深色系统标题栏，
/// 避免深色内容外圈包一圈白色系统框头。DWMWA_USE_IMMERSIVE_DARK_MODE
/// 在 Win10 20H1+ 为属性 20，更早版本为 19；任一成功即生效。
/// 仅视觉属性，无任何安全语义。
/// </summary>
internal static class DarkTitleBar
{
    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

    public static void Apply(Window window)
    {
        var hwnd = new WindowInteropHelper(window).Handle;
        if (hwnd == IntPtr.Zero) return;
        var dark = 1;
        if (DwmSetWindowAttribute(hwnd, 20, ref dark, sizeof(int)) != 0)
            DwmSetWindowAttribute(hwnd, 19, ref dark, sizeof(int));
    }
}
