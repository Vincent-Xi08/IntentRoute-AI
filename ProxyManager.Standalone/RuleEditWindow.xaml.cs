using System.Windows;
using System.Windows.Controls;
using Strings = ProxyManager.Standalone.Localization.Strings;

namespace ProxyManager.Standalone;

// v0.13 规则条件编辑器：只编辑域名 / IP / 端口 / 协议 / 备注，
// 不允许修改 ExeName / Mode / Priority（这些仍由右键菜单其余入口管理）。
public partial class RuleEditWindow : Window
{
    // ComboBox 顺序与 ProtocolValues 一一对应；"" 表示不限
    private static readonly string[] ProtocolValues = ["", "TCP", "UDP", "Both"];

    public string Hosts { get; private set; } = "";
    public string Ips { get; private set; } = "";
    public string Ports { get; private set; } = "";
    public string Protocol { get; private set; } = "";
    public string Note { get; private set; } = "";

    public RuleEditWindow(ProxyRule rule)
    {
        InitializeComponent();
        ArgumentNullException.ThrowIfNull(rule);

        ProcessNameText.Text = rule.ExeName ?? "";
        HostsBox.Text = rule.TargetHosts ?? "";
        IpsBox.Text = rule.TargetIPs ?? "";
        PortsBox.Text = rule.TargetPorts ?? "";
        NoteBox.Text = rule.Note ?? "";

        ProtocolBox.Items.Add(Strings.RuleEditProtocolAny);
        ProtocolBox.Items.Add("TCP");
        ProtocolBox.Items.Add("UDP");
        ProtocolBox.Items.Add("Both");
        var storedProtocol = (rule.Protocol ?? "").Trim().ToUpperInvariant();
        var protocolIndex = Array.IndexOf(ProtocolValues, storedProtocol);
        ProtocolBox.SelectedIndex = protocolIndex >= 0 ? protocolIndex : 0;

        // 加载即验一次（空值通过），保证初始按钮状态正确
        Revalidate();
    }

    private void Window_Loaded(object sender, RoutedEventArgs e)
    {
        DarkTitleBar.Apply(this);
        DialogEntrance.Play(this);
    }

    private void Field_Changed(object sender, TextChangedEventArgs e) => Revalidate();

    private void ProtocolBox_SelectionChanged(object sender, SelectionChangedEventArgs e) => Revalidate();

    private void Revalidate()
    {
        var errors = RuleConstraintValidator.Explain(HostsBox.Text, IpsBox.Text, PortsBox.Text);
        ErrorText.Text = string.Join(Environment.NewLine, errors);
        SaveButton.IsEnabled = errors.Count == 0;
    }

    private void Save_Click(object sender, RoutedEventArgs e)
    {
        Hosts = HostsBox.Text;
        Ips = IpsBox.Text;
        Ports = PortsBox.Text;
        Protocol = ProtocolValues[ProtocolBox.SelectedIndex >= 0 ? ProtocolBox.SelectedIndex : 0];
        Note = NoteBox.Text;
        DialogResult = true;
    }

    private void Cancel_Click(object sender, RoutedEventArgs e) => DialogResult = false;
}
