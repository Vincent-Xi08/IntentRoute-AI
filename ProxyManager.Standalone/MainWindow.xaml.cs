using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using Microsoft.Win32;
using Strings = ProxyManager.Standalone.Localization.Strings;

namespace ProxyManager.Standalone;

public partial class MainWindow : Window
{
    private readonly AppService _service;
    private readonly ObservableCollection<ProxyRule> _rules = new();
    private readonly ObservableCollection<RuntimeLogLine> _runtimeLogs = new();
    private readonly ObservableCollection<AiRuleEditLine> _aiDrafts = new();
    private readonly ObservableCollection<PolicyFindingPreviewLine> _policyFindings = new();
    private readonly ObservableCollection<AiPolicyAdviceLine> _policyAdvice = new();
    private readonly ObservableCollection<RouteDecisionTraceLine> _routeDecisionTrace = new();
    private readonly OpenAiRuleProvider _openAiProvider = new();
    private readonly OllamaRuleProvider _ollamaProvider = new();
    private readonly CancellationTokenSource _lifetimeCts = new();
    private readonly SemaphoreSlim _aiProviderGate = new(1, 1);
    private List<ProxyRule> _allRules = new();
    private string _searchFilter = "";
    private readonly List<ProcessRow> _allProcesses = new();
    private string _processSearchFilter = string.Empty;
    private bool _isMaximized = false;
    private CancellationTokenSource? _aiGenerationCts;
    private CancellationTokenSource? _policyAnalysisCts;
    private CancellationTokenSource? _policyExplanationCts;
    private CancellationTokenSource? _runtimeReadinessCts;
    private CancellationTokenSource? _routeSimulationCts;
    private Task _policyAnalysisDrainTask = Task.CompletedTask;
    private Task _routeSimulationDrainTask = Task.CompletedTask;
    private int _policyAnalysisVersion;
    private int _aiModelRefreshVersion;
    private int _policyModelRefreshVersion;
    private int _routeSimulationVersion;
    private AiRuleSuggestion? _currentAiSuggestion;
    private AiRuleValidationResult? _currentAiValidation;
    private System.Windows.Threading.DispatcherTimer? _draftRevalidationTimer;
    private readonly System.Windows.Threading.DispatcherTimer _logSearchDebounce = new() { Interval = TimeSpan.FromMilliseconds(300) };
    private readonly System.Windows.Threading.DispatcherTimer _rulesSearchDebounce = new() { Interval = TimeSpan.FromMilliseconds(300) };
    private readonly System.Windows.Threading.DispatcherTimer _processSearchDebounce = new() { Interval = TimeSpan.FromMilliseconds(300) };
    private ICollectionView? _logsView;
    private string _logSearchText = string.Empty;
    private bool _languageUiLoading;
    private PolicyAnalysisReport? _currentPolicyReport;
    private RouteDecisionReport? _currentRouteDecisionReport;
    private RouteDecisionQuery? _currentRouteDecisionQuery;
    private bool _shutdownStarted;
    private bool _shutdownComplete;

    public MainWindow()
    {
        InitializeComponent();

        _service = new AppService();
        _service.StatusChanged += s => PostToUi(() => StatusDetail.Text = s);
        _service.RuntimeStatusChanged += status => PostToUi(() => UpdateRuntimeStatusUi(status));
        _service.ConfigurationStateChanged += () => PostToUi(UpdateConfigurationUi);
        _service.RuntimeLogReceived += line => PostToUi(() =>
        {
            // 等级在入列时解析一次，供日志行着色与过滤共用同一判定。
            RuntimeLogFilter.TryParseLevel(line, out var logLevel);
            _runtimeLogs.Add(new RuntimeLogLine(DateTime.Now.ToString("HH:mm:ss"), line, logLevel));
            while (_runtimeLogs.Count > 500)
                _runtimeLogs.RemoveAt(0);
            if (LogAutoScrollToggle.IsChecked == true && LogsList.Items.Count > 0)
                LogsList.ScrollIntoView(LogsList.Items[^1]);
        });

        Loaded += MainWindow_Loaded;
        _batchResultTimer.Tick += BatchResultTimer_Tick;
        RulesList.ItemsSource = _rules;
        _logsView = CollectionViewSource.GetDefaultView(_runtimeLogs);
        _logsView.Filter = o => o is RuntimeLogLine l &&
            RuntimeLogFilter.Matches(l.Message, CurrentMinimumLogLevel(), _logSearchText);
        LogsList.ItemsSource = _logsView;
        AiDraftList.ItemsSource = _aiDrafts;
        PolicyFindingsList.ItemsSource = _policyFindings;
        PolicyAiAdviceList.ItemsSource = _policyAdvice;
        RouteDecisionTraceList.ItemsSource = _routeDecisionTrace;
        AiProviderCombo.ItemsSource = new[] { Strings.ProviderOpenAiCloud, Strings.ProviderOllamaLocal };
        AiProviderCombo.SelectedIndex = 0;
        PolicyProviderCombo.ItemsSource = new[] { Strings.ProviderOpenAiCloud, Strings.ProviderOllamaLocal };
        PolicyProviderCombo.SelectedIndex = 0;

        // 日志级别下拉：第 0 项「全部」与 Trace 等效；Trace/Debug/Info 用英文技术名。
        LogLevelFilterCombo.ItemsSource = new[]
        {
            Strings.MonitorFilterAll,
            nameof(RuntimeLogLevel.Trace),
            nameof(RuntimeLogLevel.Debug),
            nameof(RuntimeLogLevel.Info),
            Strings.MonitorLevelWarn,
            Strings.MonitorLevelError
        };
        LogLevelFilterCombo.SelectedIndex = 0;
        _logSearchDebounce.Tick += LogSearchDebounce_Tick;
        _rulesSearchDebounce.Tick += RulesSearchDebounce_Tick;
        _processSearchDebounce.Tick += ProcessSearchDebounce_Tick;

        // 允许标题栏拖动
        MouseLeftButtonDown += (s, e) => { if (e.ChangedButton == MouseButton.Left) DragMove(); };
    }

    private async void MainWindow_Loaded(object sender, RoutedEventArgs e)
    {
        try
        {
            UpdateConfigurationUi();
            LoadRules();
            LoadSettings();
            RefreshProcessList();
            await RefreshRuntimeReadinessAsync();
            _lifetimeCts.Token.ThrowIfCancellationRequested();
            UpdateRuntimeStatusUi(_service.GetRuntimeStatus());
            await RefreshAiModelsAsync(_lifetimeCts.Token);
            await RefreshPolicyModelsAsync(_lifetimeCts.Token);
        }
        catch (OperationCanceledException) when (_lifetimeCts.IsCancellationRequested)
        {
            // Normal window shutdown can overlap asynchronous first-load work.
        }
        catch (ObjectDisposedException) when (_shutdownStarted || _shutdownComplete)
        {
            // A provider/runtime may finish disposal while a queued UI continuation unwinds.
        }
    }

    #region 窗口控制

    private void Minimize_Click(object sender, RoutedEventArgs e) => WindowState = WindowState.Minimized;

    private void Maximize_Click(object sender, RoutedEventArgs e)
    {
        _isMaximized = !_isMaximized;
        WindowState = _isMaximized ? WindowState.Maximized : WindowState.Normal;
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();

    #endregion

    #region 导航

    private void Nav_Click(object sender, RoutedEventArgs e)
    {
        if (sender is RadioButton rb) ShowPage(rb.Name);
    }

    private void ShowPage(string pageName)
    {
        PageRules.Visibility = Visibility.Collapsed;
        PageAiAssistant.Visibility = Visibility.Collapsed;
        PagePolicyIntelligence.Visibility = Visibility.Collapsed;
        PageRouteSimulator.Visibility = Visibility.Collapsed;
        PageMonitor.Visibility = Visibility.Collapsed;
        PageProcess.Visibility = Visibility.Collapsed;
        PageSettings.Visibility = Visibility.Collapsed;
        PageAbout.Visibility = Visibility.Collapsed;

        UIElement? page = pageName switch
        {
            "NavRules" => PageRules,
            "NavAi" => PageAiAssistant,
            "NavPolicy" => PagePolicyIntelligence,
            "NavRouteSimulator" => PageRouteSimulator,
            "NavMonitor" => PageMonitor,
            "NavProcess" => PageProcess,
            "NavSettings" => PageSettings,
            "NavAbout" => PageAbout,
            _ => null
        };

        if (page != null) page.Visibility = Visibility.Visible;
        PageTitle.Text = pageName switch
        {
            "NavRules" => Localization.Strings.PageTitleRules,
            "NavAi" => Localization.Strings.PageTitleAi,
            "NavPolicy" => Localization.Strings.PageTitlePolicy,
            "NavRouteSimulator" => Localization.Strings.PageTitleRouteSimulator,
            "NavMonitor" => Localization.Strings.PageTitleMonitor,
            "NavProcess" => Localization.Strings.PageTitleProcess,
            "NavSettings" => Localization.Strings.PageTitleSettings,
            "NavAbout" => Localization.Strings.PageTitleAbout,
            _ => ""
        };
        PageSubtitle.Text = pageName switch
        {
            "NavRules" => Localization.Strings.PageSubtitleRules,
            "NavAi" => Localization.Strings.PageSubtitleAi,
            "NavPolicy" => Localization.Strings.PageSubtitlePolicy,
            "NavRouteSimulator" => Localization.Strings.PageSubtitleRouteSimulator,
            "NavMonitor" => Localization.Strings.PageSubtitleMonitor,
            "NavProcess" => Localization.Strings.PageSubtitleProcess,
            "NavSettings" => Localization.Strings.PageSubtitleSettings,
            "NavAbout" => Localization.Strings.PageSubtitleAbout,
            _ => ""
        };

        if (pageName == "NavRouteSimulator")
            RefreshRouteDecisionFreshness();

        // 页面内容淡入，保留原有 Visibility 切换与 UIA/键盘契约。
        if (page is FrameworkElement element)
        {
            element.Opacity = 0;
            var fade = new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(160))
            {
                EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut }
            };
            element.BeginAnimation(UIElement.OpacityProperty, fade);
        }
    }

    #endregion

    #region AI 规则助手

    private IAiRuleProvider GetSelectedAiProvider() =>
        AiProviderCombo.SelectedIndex == 1 ? _ollamaProvider : _openAiProvider;

    private async void AiProvider_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!IsLoaded || _aiGenerationCts != null || _shutdownStarted) return;
        await RefreshAiModelsAsync(_lifetimeCts.Token);
    }

    private async void RefreshAiModels_Click(object sender, RoutedEventArgs e)
    {
        if (_aiGenerationCts != null || _shutdownStarted) return;
        await RefreshAiModelsAsync(_lifetimeCts.Token);
    }

    private async Task RefreshAiModelsAsync(CancellationToken cancellationToken = default)
    {
        var refreshVersion = ++_aiModelRefreshVersion;
        var provider = GetSelectedAiProvider();
        AiModelCombo.ItemsSource = null;
        AiModelCombo.IsEnabled = false;
        AiGenerateButton.IsEnabled = false;
        AiAcceptButton.IsEnabled = false;
        ResetAiDraft();

        AiPrivacyText.Text = provider.Kind == AiProviderKind.OpenAI
            ? Strings.AiPrivacyOpenAiMsg
            : Strings.AiPrivacyOllamaMsg;

        try
        {
            var models = await RunAiProviderOperationAsync(
                token => provider.ListModelsAsync(token),
                cancellationToken);
            if (refreshVersion != _aiModelRefreshVersion || !ReferenceEquals(provider, GetSelectedAiProvider()))
                return;
            AiModelCombo.ItemsSource = models;
            if (models.Count > 0)
            {
                AiModelCombo.SelectedIndex = 0;
                AiStatusText.Text = provider.Kind == AiProviderKind.OpenAI && !provider.IsAvailable
                    ? Strings.AiMsgModelsNoKey
                    : string.Format(Strings.AiMsgModelsLoadedFormat, models.Count);
                AiGenerateButton.IsEnabled = true;
            }
            else
            {
                AiStatusText.Text = Strings.AiMsgOllamaNoModels;
            }
        }
        catch (AiProviderException ex)
        {
            if (refreshVersion == _aiModelRefreshVersion)
                AiStatusText.Text = ex.Message;
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            // Model discovery cancellation is expected during shutdown or a newer request.
        }
        finally
        {
            if (refreshVersion == _aiModelRefreshVersion && !cancellationToken.IsCancellationRequested)
                AiModelCombo.IsEnabled = true;
        }
    }

    private async void GenerateAi_Click(object sender, RoutedEventArgs e)
    {
        if (_aiGenerationCts != null) return;
        var intent = AiIntentBox.Text.Trim();
        var model = AiModelCombo.SelectedItem as string;
        if (string.IsNullOrWhiteSpace(intent) || string.IsNullOrWhiteSpace(model))
        {
            AiStatusText.Text = Strings.AiMsgEnterIntentAndModel;
            return;
        }

        ResetAiDraft();
        var cts = new CancellationTokenSource();
        using var requestCts = CancellationTokenSource.CreateLinkedTokenSource(
            cts.Token,
            _lifetimeCts.Token);
        _aiGenerationCts = cts;
        AiProviderCombo.IsEnabled = false;
        AiModelCombo.IsEnabled = false;
        AiIntentBox.IsEnabled = false;
        AiGenerateButton.IsEnabled = false;
        AiCancelButton.IsEnabled = true;
        AiStatusText.Text = Strings.AiMsgGenerating;

        try
        {
            var provider = GetSelectedAiProvider();
            var suggestion = await RunAiProviderOperationAsync(
                token => provider.GenerateDraftAsync(new AiRuleRequest(intent, model), token),
                requestCts.Token);
            var validation = AiRuleDraftValidator.Validate(suggestion, _service.Config);
            _currentAiSuggestion = suggestion;
            _currentAiValidation = validation;

            foreach (var draft in suggestion.Rules)
            {
                var line = AiRuleEditLine.FromDraft(draft);
                line.PropertyChanged += (_, _) => ScheduleDraftRevalidation();
                _aiDrafts.Add(line);
            }
            AiDraftCount.Text = string.Format(Localization.Strings.CountRulesFormat, _aiDrafts.Count);

            if (validation.Success)
            {
                var warnings = suggestion.Warnings.Count == 0
                    ? Strings.AiMsgNoWarnings
                    : Strings.AiMsgWarningsPrefix + string.Join(Strings.JoinSeparator, suggestion.Warnings);
                AiStatusText.Text = string.Format(Strings.AiMsgValidationPassedFormat, suggestion.Summary, warnings);
                AiAcceptButton.IsEnabled = true;
            }
            else
            {
                AiStatusText.Text = Strings.AiMsgValidationFailedPrefix + string.Join(Strings.JoinSeparator, validation.Errors);
            }
        }
        catch (OperationCanceledException)
        {
            AiStatusText.Text = Strings.AiMsgCancelled;
        }
        catch (AiProviderException ex)
        {
            AiStatusText.Text = ex.Message;
        }
        catch
        {
            AiStatusText.Text = Strings.AiMsgFailed;
        }
        finally
        {
            if (ReferenceEquals(_aiGenerationCts, cts))
            {
                _aiGenerationCts = null;
                cts.Dispose();
            }
            if (!_shutdownStarted)
            {
                AiProviderCombo.IsEnabled = true;
                AiModelCombo.IsEnabled = true;
                AiIntentBox.IsEnabled = true;
                AiCancelButton.IsEnabled = false;
                AiGenerateButton.IsEnabled = AiModelCombo.SelectedItem is string;
            }
        }
    }

    private async Task<T> RunAiProviderOperationAsync<T>(
        Func<CancellationToken, Task<T>> operation,
        CancellationToken cancellationToken)
    {
        await _aiProviderGate.WaitAsync(cancellationToken);
        try
        {
            return await operation(cancellationToken);
        }
        finally
        {
            _aiProviderGate.Release();
        }
    }

    private void CancelAi_Click(object sender, RoutedEventArgs e) => _aiGenerationCts?.Cancel();

    private void AcceptAiRules_Click(object sender, RoutedEventArgs e)
    {
        if (_currentAiSuggestion == null || _currentAiValidation is not { Success: true }) return;
        try
        {
            var revalidated = AiRuleDraftValidator.Validate(_currentAiSuggestion, _service.Config);
            if (!revalidated.Success)
            {
                _currentAiValidation = revalidated;
                AiAcceptButton.IsEnabled = false;
                AiStatusText.Text = Strings.AiMsgRevalidationFailedPrefix + string.Join(Strings.JoinSeparator, revalidated.Errors);
                return;
            }

            _service.AcceptDisabledAiRules(revalidated.Rules);
            LoadRules();
            AiStatusText.Text = string.Format(Strings.AiMsgAddedFormat, revalidated.Rules.Count);
            AiAcceptButton.IsEnabled = false;
            _currentAiSuggestion = null;
            _currentAiValidation = null;
        }
        catch
        {
            AiStatusText.Text = Strings.AiMsgSaveFailed;
        }
    }

    private void ResetAiDraft()
    {
        _currentAiSuggestion = null;
        _currentAiValidation = null;
        _draftRevalidationTimer?.Stop();
        _aiDrafts.Clear();
        AiDraftCount.Text = string.Format(Localization.Strings.CountRulesFormat, 0);
        AiAcceptButton.IsEnabled = false;
    }

    private void ScheduleDraftRevalidation()
    {
        if (_currentAiSuggestion == null || _shutdownStarted) return;
        if (_draftRevalidationTimer == null)
        {
            _draftRevalidationTimer = new System.Windows.Threading.DispatcherTimer
            {
                Interval = TimeSpan.FromMilliseconds(300)
            };
            _draftRevalidationTimer.Tick += (_, _) =>
            {
                _draftRevalidationTimer!.Stop();
                RevalidateDraftNow();
            };
        }
        _draftRevalidationTimer.Stop();
        _draftRevalidationTimer.Start();
    }

    private void RevalidateDraftNow()
    {
        if (_currentAiSuggestion == null || _shutdownStarted) return;
        var suggestion = BuildSuggestionFromEdits();
        var validation = AiRuleDraftValidator.Validate(suggestion, _service.Config);
        _currentAiSuggestion = suggestion;
        _currentAiValidation = validation;

        if (validation.Success)
        {
            AiAcceptButton.IsEnabled = true;
            AiStatusText.Text = Strings.AiMsgEditRevalidated;
        }
        else
        {
            AiAcceptButton.IsEnabled = false;
            AiStatusText.Text = Strings.AiMsgEditValidationFailedPrefix + string.Join(Strings.JoinSeparator, validation.Errors);
        }
    }

    private AiRuleSuggestion BuildSuggestionFromEdits() => new()
    {
        Summary = _currentAiSuggestion?.Summary ?? string.Empty,
        Warnings = _currentAiSuggestion?.Warnings ?? [],
        Rules = _aiDrafts.Select(line => line.ToDraft()).ToList()
    };

    #endregion

    #region AI 策略体检

    private IAiPolicyExplainer GetSelectedPolicyExplainer() =>
        PolicyProviderCombo.SelectedIndex == 1 ? _ollamaProvider : _openAiProvider;

    private async void PolicyProvider_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!IsLoaded || _policyExplanationCts != null || _shutdownStarted) return;
        await RefreshPolicyModelsAsync(_lifetimeCts.Token);
    }

    private async void RefreshPolicyModels_Click(object sender, RoutedEventArgs e)
    {
        if (_policyExplanationCts != null || _shutdownStarted) return;
        await RefreshPolicyModelsAsync(_lifetimeCts.Token);
    }

    private async Task RefreshPolicyModelsAsync(CancellationToken cancellationToken = default)
    {
        var refreshVersion = ++_policyModelRefreshVersion;
        var provider = GetSelectedPolicyExplainer();
        PolicyModelCombo.ItemsSource = null;
        PolicyModelCombo.IsEnabled = false;
        PolicyExplainButton.IsEnabled = false;
        ResetPolicyExplanation();

        try
        {
            var models = await RunAiProviderOperationAsync(
                token => provider.ListModelsAsync(token),
                cancellationToken);
            if (refreshVersion != _policyModelRefreshVersion ||
                !ReferenceEquals(provider, GetSelectedPolicyExplainer()))
            {
                return;
            }

            PolicyModelCombo.ItemsSource = models;
            if (models.Count > 0)
            {
                PolicyModelCombo.SelectedIndex = 0;
                PolicyStatusText.Text = provider.Kind == AiProviderKind.OpenAI && !provider.IsAvailable
                    ? Strings.PolicyMsgReadyNoKey
                    : Strings.PolicyMsgLocalOnly;
                PolicyExplainButton.IsEnabled =
                    PolicyFindingsList.SelectedItems.Count is > 0 and <= PolicyDisclosure.MaxFindings;
            }
            else
            {
                PolicyStatusText.Text = Strings.PolicyMsgOllamaNoModels;
            }
        }
        catch (AiProviderException ex)
        {
            if (refreshVersion == _policyModelRefreshVersion)
                PolicyStatusText.Text = Strings.PolicyMsgModelUnavailablePrefix + ex.Message;
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            // Shutdown or a newer model refresh canceled this request.
        }
        finally
        {
            if (refreshVersion == _policyModelRefreshVersion && !cancellationToken.IsCancellationRequested)
                PolicyModelCombo.IsEnabled = true;
        }
    }

    private async void AnalyzePolicy_Click(object sender, RoutedEventArgs e)
    {
        await RefreshPolicyAnalysisAsync();
    }

    private async void ExplainPolicy_Click(object sender, RoutedEventArgs e)
    {
        if (_policyExplanationCts != null || _currentPolicyReport == null) return;
        var report = _currentPolicyReport;
        bool matchesBeforePreview;
        try
        {
            matchesBeforePreview = await PolicyReportMatchesCurrentAsync(report, _lifetimeCts.Token);
        }
        catch (OperationCanceledException) when (_lifetimeCts.IsCancellationRequested)
        {
            return;
        }
        if (!matchesBeforePreview)
        {
            await RefreshPolicyAnalysisAsync();
            PolicyStatusText.Text = Strings.PolicyMsgChangedPreviewBlocked;
            return;
        }

        var model = PolicyModelCombo.SelectedItem as string;
        var selectedCodes = PolicyFindingsList.SelectedItems
            .OfType<PolicyFindingPreviewLine>()
            .Select(finding => finding.Code)
            .Distinct(StringComparer.Ordinal)
            .ToList();
        if (string.IsNullOrWhiteSpace(model) || selectedCodes.Count is < 1 or > PolicyDisclosure.MaxFindings)
        {
            PolicyStatusText.Text = Strings.PolicyMsgSelectFindings;
            return;
        }

        var disclosure = PolicyIntelligence.ToDisclosure(report, selectedCodes);
        var provider = GetSelectedPolicyExplainer();
        var preview = AiPolicyContract.CreateInput(disclosure);
        var providerNotice = provider.Kind == AiProviderKind.OpenAI
            ? Strings.PolicyConfirmOpenAi
            : Strings.PolicyConfirmOllama;
        var confirmed = DarkDialogWindow.Show(
            this,
            providerNotice + "\n\n" + Strings.PolicyConfirmJsonHeader + "\n" + preview +
            "\n\n" + Strings.PolicyConfirmExclusionNote,
            Strings.PolicyConfirmTitle,
            DarkDialogIcon.Question,
            DarkDialogButtons.YesNo);
        if (confirmed != MessageBoxResult.Yes)
        {
            PolicyStatusText.Text = Strings.PolicyMsgSendCancelled;
            return;
        }

        bool matchesAfterConfirmation;
        try
        {
            matchesAfterConfirmation = await PolicyReportMatchesCurrentAsync(report, _lifetimeCts.Token);
        }
        catch (OperationCanceledException) when (_lifetimeCts.IsCancellationRequested)
        {
            return;
        }
        if (!matchesAfterConfirmation)
        {
            await RefreshPolicyAnalysisAsync();
            PolicyStatusText.Text = Strings.PolicyMsgChangedDuringConfirm;
            return;
        }

        var cts = new CancellationTokenSource();
        using var requestCts = CancellationTokenSource.CreateLinkedTokenSource(cts.Token, _lifetimeCts.Token);
        _policyExplanationCts = cts;
        PolicyProviderCombo.IsEnabled = false;
        PolicyModelCombo.IsEnabled = false;
        PolicyExplainButton.IsEnabled = false;
        PolicyCancelButton.IsEnabled = true;
        ResetPolicyExplanation();
        PolicyStatusText.Text = disclosure.OmittedFindingCount == 0
            ? Strings.PolicyMsgSending
            : string.Format(Strings.PolicyMsgSendingTopFormat, disclosure.Findings.Count, disclosure.OmittedFindingCount);

        try
        {
            var explanation = await RunAiProviderOperationAsync(
                token => provider.ExplainPolicyAsync(
                    new AiPolicyExplainRequest(model, disclosure),
                    token),
                requestCts.Token);

            if (!await PolicyReportMatchesCurrentAsync(report, requestCts.Token))
            {
                await RefreshPolicyAnalysisAsync();
                PolicyStatusText.Text = Strings.PolicyMsgStaleResponse;
                return;
            }

            PolicyAiSummaryText.Text = explanation.Summary;
            foreach (var priority in explanation.Priorities)
                _policyAdvice.Add(AiPolicyAdviceLine.FromPriority(priority));
            var caveats = explanation.Caveats.Count == 0
                ? Strings.PolicyMsgNoCaveats
                : Strings.PolicyMsgCaveatsPrefix + string.Join(Strings.JoinSeparator, explanation.Caveats);
            PolicyStatusText.Text = string.Format(Strings.PolicyMsgExplanationDoneFormat, explanation.Priorities.Count, caveats);
        }
        catch (OperationCanceledException)
        {
            PolicyStatusText.Text = Strings.PolicyMsgExplanationCancelled;
        }
        catch (AiProviderException ex)
        {
            PolicyStatusText.Text = ex.Message + Strings.PolicyMsgStillValidSuffix;
        }
        catch
        {
            PolicyStatusText.Text = Strings.PolicyMsgExplanationFailed;
        }
        finally
        {
            if (ReferenceEquals(_policyExplanationCts, cts))
            {
                _policyExplanationCts = null;
                cts.Dispose();
            }
            if (!_shutdownStarted)
            {
                PolicyProviderCombo.IsEnabled = true;
                PolicyModelCombo.IsEnabled = true;
                PolicyCancelButton.IsEnabled = false;
                PolicyExplainButton.IsEnabled =
                    PolicyModelCombo.SelectedItem is string &&
                    PolicyFindingsList.SelectedItems.Count is > 0 and <= PolicyDisclosure.MaxFindings;
            }
        }
    }

    private void CancelPolicyExplanation_Click(object sender, RoutedEventArgs e) =>
        _policyExplanationCts?.Cancel();

    private void PolicyFindingSelection_Changed(object sender, SelectionChangedEventArgs e)
    {
        PolicyLocateButton.IsEnabled =
            PolicyFindingsList.SelectedItem is PolicyFindingPreviewLine { PrimaryRuleId: not null };
        PolicyExplainButton.IsEnabled =
            _policyExplanationCts == null &&
            PolicyModelCombo.SelectedItem is string &&
            PolicyFindingsList.SelectedItems.Count is > 0 and <= PolicyDisclosure.MaxFindings;
    }

    private void LocatePolicyRule_Click(object sender, RoutedEventArgs e)
    {
        if (PolicyFindingsList.SelectedItem is not PolicyFindingPreviewLine { PrimaryRuleId: not null } selected)
            return;

        NavRules.IsChecked = true;
        SearchBox.Text = string.Empty;
        ShowPage("NavRules");
        LoadRules();
        var rule = _rules.FirstOrDefault(item => item.Id == selected.PrimaryRuleId);
        if (rule == null) return;
        RulesList.SelectedItem = rule;
        RulesList.ScrollIntoView(rule);
        RulesList.Focus();
    }

    private Task RefreshPolicyAnalysisAsync()
    {
        var previous = _policyAnalysisCts;
        previous?.Cancel();
        var version = ++_policyAnalysisVersion;

        if (!_service.IsConfigurationWritable)
        {
            _policyAnalysisCts = null;
            ShowProtectedPolicyState();
            return Task.CompletedTask;
        }

        var snapshot = _service.Config;
        var cts = CancellationTokenSource.CreateLinkedTokenSource(_lifetimeCts.Token);
        _policyAnalysisCts = cts;
        BeginPolicyAnalysisUi();
        var currentTask = RunPolicyAnalysisAsync(snapshot, version, cts);
        _policyAnalysisDrainTask = Task.WhenAll(_policyAnalysisDrainTask, currentTask);
        return currentTask;
    }

    private async Task RunPolicyAnalysisAsync(
        AppConfig snapshot,
        int version,
        CancellationTokenSource cts)
    {
        try
        {
            var latest = await Task.Run(
                () => PolicyIntelligence.Analyze(snapshot, cts.Token),
                cts.Token);
            if (cts.IsCancellationRequested || version != _policyAnalysisVersion || _shutdownStarted)
                return;

            _currentPolicyReport = latest;
            _policyFindings.Clear();
            foreach (var finding in latest.Findings)
                _policyFindings.Add(PolicyFindingPreviewLine.FromFinding(finding));
            PolicyActiveCount.Text = latest.ActiveRuleCount.ToString();
            PolicyCriticalCount.Text = latest.CriticalCount.ToString();
            PolicyWarningCount.Text = latest.WarningCount.ToString();
            PolicyDisabledCount.Text = latest.DisabledRuleCount.ToString();
            PolicyFindingCount.Text = string.Format(Localization.Strings.CountFindingsFormat, latest.Findings.Count);
            PolicyEmptyText.Text = Strings.PolicyEmpty;
            PolicyEmptyText.Visibility = latest.Findings.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
            PolicyStatusText.Text = !latest.IsComplete
                ? string.Format(Strings.PolicyMsgBudgetFormat, latest.OmittedFindingCount)
                : latest.Findings.Count == 0
                    ? Strings.PolicyMsgDoneNoIssues
                    : string.Format(Strings.PolicyMsgDoneFormat, latest.Findings.Count);
        }
        catch (OperationCanceledException) when (cts.IsCancellationRequested)
        {
            // A newer snapshot or window shutdown superseded this bounded local scan.
        }
        catch (Exception ex)
        {
            if (version == _policyAnalysisVersion && !_shutdownStarted)
            {
                _currentPolicyReport = null;
                _policyFindings.Clear();
                PolicyFindingCount.Text = string.Format(Localization.Strings.CountFindingsFormat, 0);
                PolicyEmptyText.Text = Strings.PolicyEmptyFailed;
                PolicyEmptyText.Visibility = Visibility.Visible;
                PolicyStatusText.Text = Strings.PolicyMsgFailedPrefix + SingBoxRuntime.RedactSecrets(ex.Message);
            }
        }
        finally
        {
            if (ReferenceEquals(_policyAnalysisCts, cts))
                _policyAnalysisCts = null;
            cts.Dispose();
        }
    }

    private void BeginPolicyAnalysisUi()
    {
        _policyExplanationCts?.Cancel();
        _currentPolicyReport = null;
        _policyFindings.Clear();
        ResetPolicyExplanation();
        PolicyActiveCount.Text = "…";
        PolicyCriticalCount.Text = "…";
        PolicyWarningCount.Text = "…";
        PolicyDisabledCount.Text = "…";
        PolicyFindingCount.Text = Localization.Strings.AnalyzingText;
        PolicyEmptyText.Text = Strings.PolicyEmptyRunning;
        PolicyEmptyText.Visibility = Visibility.Visible;
        PolicyLocateButton.IsEnabled = false;
        PolicyExplainButton.IsEnabled = false;
        PolicyStatusText.Text = Strings.PolicyMsgAnalyzing;
    }

    private void ShowProtectedPolicyState()
    {
        _policyExplanationCts?.Cancel();
        _currentPolicyReport = null;
        _policyFindings.Clear();
        ResetPolicyExplanation();
        PolicyActiveCount.Text = "0";
        PolicyCriticalCount.Text = "0";
        PolicyWarningCount.Text = "0";
        PolicyDisabledCount.Text = "0";
        PolicyFindingCount.Text = string.Format(Localization.Strings.CountFindingsFormat, 0);
        PolicyEmptyText.Text = Strings.PolicyEmptyProtected;
        PolicyEmptyText.Visibility = Visibility.Visible;
        PolicyLocateButton.IsEnabled = false;
        PolicyExplainButton.IsEnabled = false;
        PolicyStatusText.Text = Strings.PolicyMsgProtected;
    }

    private void ResetPolicyExplanation()
    {
        _policyAdvice.Clear();
        PolicyAiSummaryText.Text = Strings.PolicyAiSummaryNotYet;
    }

    private async Task<bool> PolicyReportMatchesCurrentAsync(
        PolicyAnalysisReport report,
        CancellationToken cancellationToken)
    {
        var snapshot = _service.Config;
        return await Task.Run(
            () => PolicyIntelligence.MatchesSnapshot(report, snapshot, cancellationToken),
            cancellationToken);
    }

    #endregion

    #region AI 路由推演

    private async void SimulateRoute_Click(object sender, RoutedEventArgs e)
    {
        if (_routeSimulationCts != null || _shutdownStarted) return;
        if (!_service.IsConfigurationWritable)
        {
            ShowProtectedRouteSimulationState();
            return;
        }

        var query = CreateRouteDecisionQuery();
        var snapshot = _service.Config;
        var version = ++_routeSimulationVersion;
        var cts = CancellationTokenSource.CreateLinkedTokenSource(_lifetimeCts.Token);
        _routeSimulationCts = cts;
        BeginRouteSimulationUi();
        var currentTask = RunRouteSimulationAsync(snapshot, query, version, cts);
        _routeSimulationDrainTask = Task.WhenAll(_routeSimulationDrainTask, currentTask);
        await currentTask;
    }

    private async Task RunRouteSimulationAsync(
        AppConfig snapshot,
        RouteDecisionQuery query,
        int version,
        CancellationTokenSource cts)
    {
        try
        {
            var report = await Task.Run(
                () => PolicyIntelligence.SimulateRoute(snapshot, query, cts.Token),
                cts.Token);
            if (cts.IsCancellationRequested || version != _routeSimulationVersion || _shutdownStarted)
                return;

            if (!report.IsSnapshotBound)
            {
                _currentRouteDecisionReport = null;
                _currentRouteDecisionQuery = null;
                ShowRouteDecisionReport(report);
                return;
            }

            var stillCurrent = await Task.Run(
                () => PolicyIntelligence.MatchesRouteSnapshot(report, _service.Config, query, cts.Token),
                cts.Token);
            if (!stillCurrent)
            {
                InvalidateRouteDecision(Strings.RouteMsgConfigChanged);
                return;
            }

            _currentRouteDecisionReport = report;
            _currentRouteDecisionQuery = query;
            ShowRouteDecisionReport(report);
        }
        catch (OperationCanceledException) when (cts.IsCancellationRequested)
        {
            if (version == _routeSimulationVersion && !_shutdownStarted)
                RouteDecisionStatusText.Text = Strings.RouteMsgCancelled;
        }
        catch (Exception ex)
        {
            if (version == _routeSimulationVersion && !_shutdownStarted)
            {
                InvalidateRouteDecision(Strings.RouteMsgFailed, markStale: false);
                RouteDecisionReasonText.Text = SingBoxRuntime.RedactSecrets(ex.Message);
            }
        }
        finally
        {
            if (ReferenceEquals(_routeSimulationCts, cts))
            {
                _routeSimulationCts = null;
                cts.Dispose();
                if (!_shutdownStarted)
                {
                    RouteSimulateButton.IsEnabled = _service.IsConfigurationWritable;
                    RouteDecisionCancelButton.IsEnabled = false;
                }
            }
        }
    }

    private void BeginRouteSimulationUi()
    {
        _currentRouteDecisionReport = null;
        _currentRouteDecisionQuery = null;
        _routeDecisionTrace.Clear();
        RouteDecisionTraceEmptyText.Visibility = Visibility.Visible;
        RouteDecisionTraceCountText.Text = string.Format(Localization.Strings.CountTraceFormat, 0);
        RouteDecisionBadgeText.Text = Strings.RouteBadgeSimulating;
        RouteDecisionActionText.Text = "…";
        RouteDecisionSourceText.Text = Strings.RouteSrcSimulating;
        RouteDecisionReasonText.Text = Strings.RouteReasonPending;
        RouteDecisionSnapshotText.Text = Strings.RouteSnapshotBinding;
        RouteDecisionStatusText.Text = Strings.RouteStatusSimulating;
        RouteDecisionLocateButton.IsEnabled = false;
        RouteSimulateButton.IsEnabled = false;
        RouteDecisionCancelButton.IsEnabled = true;
    }

    private void ShowRouteDecisionReport(RouteDecisionReport report)
    {
        _routeDecisionTrace.Clear();
        foreach (var step in report.Trace)
            _routeDecisionTrace.Add(RouteDecisionTraceLine.FromStep(step));
        RouteDecisionTraceCountText.Text = string.Format(Localization.Strings.CountTraceFormat, report.Trace.Count);
        RouteDecisionTraceEmptyText.Visibility = report.Trace.Count == 0
            ? Visibility.Visible
            : Visibility.Collapsed;

        RouteDecisionBadgeText.Text = report.Kind switch
        {
            RouteDecisionKind.MatchedRule => Localization.Strings.RouteBadgeMatched,
            RouteDecisionKind.GlobalFallback => Localization.Strings.RouteBadgeFallback,
            RouteDecisionKind.Indeterminate => Localization.Strings.TraceVerdictIndeterminate,
            RouteDecisionKind.InvalidQuery => Localization.Strings.RouteBadgeInvalidQuery,
            RouteDecisionKind.InvalidPolicy => Localization.Strings.RouteBadgeInvalidPolicy,
            _ => Localization.Strings.RouteBadgeNoConclusion
        };
        RouteDecisionActionText.Text = report.Action switch
        {
            ProxyMode.Proxy => Localization.Strings.ModeProxyText,
            ProxyMode.Direct => Localization.Strings.ModeDirectText,
            ProxyMode.Block => Localization.Strings.ModeBlockText,
            _ => Localization.Strings.RouteActionUnproven
        };
        RouteDecisionSourceText.Text = report.Kind switch
        {
            RouteDecisionKind.MatchedRule =>
                string.Format(Strings.RouteSrcMatchedFormat, report.MatchedEvaluationOrder, report.MatchedRuleDisplayName),
            RouteDecisionKind.GlobalFallback => Strings.RouteSrcFallback,
            RouteDecisionKind.Indeterminate => string.Format(Strings.RouteSrcIndeterminateFormat, report.EvaluatedRuleCount),
            RouteDecisionKind.InvalidQuery => Strings.RouteSrcInvalidQuery,
            RouteDecisionKind.InvalidPolicy => Strings.RouteSrcInvalidPolicy,
            _ => Strings.RouteSrcNone
        };
        RouteDecisionReasonText.Text = GetRouteDecisionReasonText(report.Reason) +
            (string.IsNullOrWhiteSpace(report.Error)
                ? string.Empty
                : " " + SingBoxRuntime.RedactSecrets(report.Error));
        RouteDecisionSnapshotText.Text = string.IsNullOrWhiteSpace(report.Fingerprint)
            ? Strings.RouteSnapNoFingerprint
            : string.Format(Strings.RouteSnapFingerprintFormat, report.Fingerprint[..12]);
        RouteDecisionLocateButton.IsEnabled = report.Kind == RouteDecisionKind.MatchedRule &&
            !string.IsNullOrWhiteSpace(report.MatchedRuleId);
        RouteDecisionStatusText.Text = report.Kind switch
        {
            RouteDecisionKind.MatchedRule or RouteDecisionKind.GlobalFallback =>
                Strings.RouteStatusProvenDone,
            RouteDecisionKind.Indeterminate =>
                Strings.RouteStatusIndeterminateDone,
            RouteDecisionKind.InvalidQuery =>
                Strings.RouteStatusInvalidQuery,
            RouteDecisionKind.InvalidPolicy =>
                Strings.RouteStatusInvalidPolicy,
            _ => Strings.RouteStatusNoDecision
        };
    }

    private RouteDecisionQuery CreateRouteDecisionQuery()
    {
        _ = int.TryParse(RoutePortBox.Text, out var port);
        return new RouteDecisionQuery(
            RouteProcessBox.Text,
            RouteDestinationKindCombo.SelectedIndex == 1 ? RouteDestinationKind.Ip : RouteDestinationKind.Domain,
            RouteDestinationBox.Text,
            port,
            RouteTransportCombo.SelectedIndex == 1 ? RouteTransport.Udp : RouteTransport.Tcp);
    }

    private void RouteDestinationKind_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (RouteDestinationLabel != null)
            RouteDestinationLabel.Text = RouteDestinationKindCombo.SelectedIndex == 1 ? Strings.RouteDestinationLabelIp : Strings.RouteDestinationLabelDomain;
        RouteQuery_Changed(sender, e);
    }

    private void RouteQuery_Changed(object sender, RoutedEventArgs e)
    {
        if (_currentRouteDecisionReport == null && _routeSimulationCts == null) return;
        _routeSimulationCts?.Cancel();
        _routeSimulationVersion++;
        InvalidateRouteDecision(Strings.RouteMsgInputChanged, cancelActive: false);
    }

    private void CancelRouteSimulation_Click(object sender, RoutedEventArgs e) =>
        _routeSimulationCts?.Cancel();

    private void LocateRouteDecisionRule_Click(object sender, RoutedEventArgs e)
    {
        RefreshRouteDecisionFreshness();
        var ruleId = _currentRouteDecisionReport?.MatchedRuleId;
        if (string.IsNullOrWhiteSpace(ruleId)) return;

        NavRules.IsChecked = true;
        SearchBox.Text = string.Empty;
        ShowPage("NavRules");
        LoadRules();
        var rule = _rules.FirstOrDefault(item => item.Id == ruleId);
        if (rule == null) return;
        RulesList.SelectedItem = rule;
        RulesList.ScrollIntoView(rule);
        RulesList.Focus();
    }

    private void RefreshRouteDecisionFreshness()
    {
        if (_currentRouteDecisionReport == null || _currentRouteDecisionQuery == null) return;
        try
        {
            if (!PolicyIntelligence.MatchesRouteSnapshot(
                    _currentRouteDecisionReport,
                    _service.Config,
                    _currentRouteDecisionQuery,
                    _lifetimeCts.Token))
            {
                InvalidateRouteDecision(Strings.RouteMsgRulesChanged);
            }
        }
        catch (OperationCanceledException) when (_lifetimeCts.IsCancellationRequested)
        {
            // Window shutdown superseded the freshness check.
        }
    }

    private void InvalidateRouteDecision(
        string status,
        bool markStale = true,
        bool cancelActive = true)
    {
        if (cancelActive) _routeSimulationCts?.Cancel();
        _currentRouteDecisionReport = null;
        _currentRouteDecisionQuery = null;
        _routeDecisionTrace.Clear();
        if (RouteDecisionTraceEmptyText == null) return;
        RouteDecisionTraceEmptyText.Visibility = Visibility.Visible;
        RouteDecisionTraceCountText.Text = string.Format(Localization.Strings.CountTraceFormat, 0);
        RouteDecisionBadgeText.Text = markStale ? Strings.RouteBadgeStale : Strings.RouteBadgeFailed;
        RouteDecisionActionText.Text = "—";
        RouteDecisionSourceText.Text = Strings.RouteSrcHidden;
        RouteDecisionReasonText.Text = status;
        RouteDecisionSnapshotText.Text = Strings.RouteSnapUnbound;
        RouteDecisionLocateButton.IsEnabled = false;
        RouteDecisionStatusText.Text = status;
    }

    private void ShowProtectedRouteSimulationState()
    {
        _routeSimulationVersion++;
        InvalidateRouteDecision(
            Strings.RouteMsgProtected,
            markStale: false);
        RouteDecisionBadgeText.Text = Strings.RouteBadgeProtected;
        RouteSimulateButton.IsEnabled = false;
        RouteDecisionCancelButton.IsEnabled = false;
    }

    private static string GetRouteDecisionReasonText(RouteDecisionReason reason) => reason switch
    {
        RouteDecisionReason.Matched => Strings.RouteReasonMatched,
        RouteDecisionReason.ProcessMismatch => Strings.RouteReasonProcessMismatch,
        RouteDecisionReason.TransportMismatch => Strings.RouteReasonTransportMismatch,
        RouteDecisionReason.PortMismatch => Strings.RouteReasonPortMismatch,
        RouteDecisionReason.DestinationMismatch => Strings.RouteReasonDestinationMismatch,
        RouteDecisionReason.ResolvedIpRequired => Strings.RouteReasonResolvedIpRequired,
        RouteDecisionReason.DomainContextRequired => Strings.RouteReasonDomainContextRequired,
        RouteDecisionReason.EvaluationBudgetExceeded => Strings.RouteReasonBudgetExceeded,
        RouteDecisionReason.InvalidQuery => Strings.RouteReasonInvalidQuery,
        RouteDecisionReason.InvalidPolicy => Strings.RouteReasonInvalidPolicy,
        _ => Strings.RouteReasonUnknown
    };

    #endregion

    #region 规则管理

    private void LoadRules()
    {
        _allRules = PolicyRuntimeOrder.All(_service.Config.Rules).ToList();
        ApplyFilter();
        UpdateStats();
        if (IsLoaded)
            InvalidateRouteDecision(Strings.RouteMsgRulesRefreshed);
        _ = RefreshPolicyAnalysisAsync();
    }

    private void ApplyFilter()
    {
        var filtered = string.IsNullOrWhiteSpace(_searchFilter)
            ? _allRules
            : _allRules.Where(r =>
                (r.ExeName ?? string.Empty).Contains(_searchFilter, StringComparison.OrdinalIgnoreCase) ||
                (r.ExePath ?? string.Empty).Contains(_searchFilter, StringComparison.OrdinalIgnoreCase))
                .ToList();

        int index = 1;
        foreach (var rule in filtered)
            rule.Index = index++;

        _rules.Clear();
        foreach (var rule in filtered)
            _rules.Add(rule);

        EmptyState.Visibility = _rules.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void UpdateStats()
    {
        var proxyCount = _allRules.Count(r => r.Mode == ProxyMode.Proxy && r.IsEnabled);
        var directCount = _allRules.Count(r => r.Mode == ProxyMode.Direct && r.IsEnabled);
        var blockCount = _allRules.Count(r => r.Mode == ProxyMode.Block && r.IsEnabled);

            ProxyCount.Text = string.Format(Localization.Strings.CountProxyFormat, proxyCount);
            DirectCount.Text = string.Format(Localization.Strings.CountDirectFormat, directCount);
            BlockCount.Text = string.Format(Localization.Strings.CountBlockFormat, blockCount);
    }

    private void Search_Changed(object sender, TextChangedEventArgs e)
    {
        // 与日志搜索同一 300ms 防抖：避免每敲一个字符就重建一次规则视图。
        _rulesSearchDebounce.Stop();
        _rulesSearchDebounce.Start();
    }

    private void RulesSearchDebounce_Tick(object? sender, EventArgs e)
    {
        _rulesSearchDebounce.Stop();
        _searchFilter = SearchBox.Text;
        ApplyFilter();
    }

    private void AddRule_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = Strings.DialogPickAppTitle,
            Filter = Strings.DialogExeFilter,
            Multiselect = true
        };

        if (dialog.ShowDialog() == true)
        {
            foreach (var file in dialog.FileNames)
                _service.AddRule(file);
            LoadRules();
        }
    }

    private void Clear_Click(object sender, RoutedEventArgs e)
    {
        if (DarkDialogWindow.Show(this, Strings.RulesClearConfirm, Strings.DialogConfirmTitle,
            DarkDialogIcon.Question, DarkDialogButtons.YesNo) == MessageBoxResult.Yes)
        {
            _service.ClearRules();
            LoadRules();
        }
    }

    private ProxyRule? GetSelectedRule() => RulesList.SelectedItem as ProxyRule;

    private void ToggleRule_Click(object sender, RoutedEventArgs e)
    {
        var rule = GetSelectedRule();
        if (rule != null) { _service.ToggleRule(rule.Id); LoadRules(); }
    }

    private void SetProxy_Click(object sender, RoutedEventArgs e)
    {
        var rule = GetSelectedRule();
        if (rule != null) { _service.UpdateRuleMode(rule.Id, ProxyMode.Proxy); LoadRules(); }
    }

    private void SetDirect_Click(object sender, RoutedEventArgs e)
    {
        var rule = GetSelectedRule();
        if (rule != null) { _service.UpdateRuleMode(rule.Id, ProxyMode.Direct); LoadRules(); }
    }

    private void SetBlock_Click(object sender, RoutedEventArgs e)
    {
        var rule = GetSelectedRule();
        if (rule != null) { _service.UpdateRuleMode(rule.Id, ProxyMode.Block); LoadRules(); }
    }

    private void EditRuleConstraints_Click(object sender, RoutedEventArgs e)
    {
        var rule = GetSelectedRule();
        if (rule == null) return;
        var dialog = new RuleEditWindow(rule) { Owner = this };
        if (dialog.ShowDialog() != true) return;
        try
        {
            _service.UpdateRuleConstraints(rule.Id, dialog.Hosts, dialog.Ips, dialog.Ports, dialog.Protocol, dialog.Note);
            LoadRules();
        }
        catch (Exception ex)
        {
            DarkDialogWindow.Show(this, SingBoxRuntime.RedactSecrets(ex.Message), Strings.DialogErrorTitle,
                DarkDialogIcon.Warning);
        }
    }

    private void MoveUp_Click(object sender, RoutedEventArgs e)
    {
        var rule = GetSelectedRule();
        if (rule != null)
        {
            _service.MoveRule(rule.Id, -1);
            LoadRules();
        }
    }

    private void MoveDown_Click(object sender, RoutedEventArgs e)
    {
        var rule = GetSelectedRule();
        if (rule != null)
        {
            _service.MoveRule(rule.Id, 1);
            LoadRules();
        }
    }

    private void DeleteRule_Click(object sender, RoutedEventArgs e)
    {
        var rule = GetSelectedRule();
        if (rule != null)
        {
            if (DarkDialogWindow.Show(this, string.Format(Strings.RulesDeleteConfirmFormat, rule.ExeName), Strings.DialogConfirmTitle,
                DarkDialogIcon.Question, DarkDialogButtons.YesNo) == MessageBoxResult.Yes)
            {
                _service.RemoveRule(rule.Id);
                LoadRules();
            }
        }
    }

    private void RulesList_SelectionChanged(object sender, SelectionChangedEventArgs e) => UpdateBatchRuleButtons();

    private void UpdateBatchRuleButtons()
    {
        var hasSelection = RulesList.SelectedItems.Count > 0;
        BatchEnableButton.IsEnabled = hasSelection;
        BatchDisableButton.IsEnabled = hasSelection;
        BatchDeleteButton.IsEnabled = hasSelection;
        BatchProxyButton.IsEnabled = hasSelection;
        BatchDirectButton.IsEnabled = hasSelection;
        BatchBlockButton.IsEnabled = hasSelection;
    }

    private List<ProxyRule> GetSelectedRules() => RulesList.SelectedItems.OfType<ProxyRule>().ToList();

    // 批量结果反馈走工具栏内的专用文本：不与异步运行状态共用 StatusDetail，
    // 避免运行时 apply 消息把操作结果立刻覆盖；显示数秒后自动淡出。
    // Tick 只在构造时订阅一次：Stop 后不再触发，避免每次显示重复挂接。
    private readonly System.Windows.Threading.DispatcherTimer _batchResultTimer =
        new() { Interval = TimeSpan.FromSeconds(4) };

    private void ShowBatchFeedback(string message)
    {
        // 清掉上一次可能仍在进行的淡出动画，再恢复不透明显示。
        BatchResultText.BeginAnimation(UIElement.OpacityProperty, null);
        BatchResultText.Text = message;
        BatchResultText.Opacity = 1;
        BatchResultText.Visibility = Visibility.Visible;
        _batchResultTimer.Stop();
        _batchResultTimer.Start();
    }

    private void BatchResultTimer_Tick(object? sender, EventArgs e)
    {
        _batchResultTimer.Stop();
        var fade = new DoubleAnimation(1, 0, TimeSpan.FromMilliseconds(400));
        fade.Completed += (_, _) => BatchResultText.Visibility = Visibility.Collapsed;
        BatchResultText.BeginAnimation(UIElement.OpacityProperty, fade);
    }

    private void BatchEnable_Click(object sender, RoutedEventArgs e)
    {
        var rules = GetSelectedRules();
        if (rules.Count == 0) return;
        var updated = _service.SetRulesEnabled(rules.Select(rule => rule.Id).ToList(), true);
        if (updated > 0)
            ShowBatchFeedback(string.Format(Strings.BatchOperationDoneFormat, Strings.RulesBatchEnable, updated));
        LoadRules();
        UpdateBatchRuleButtons();
    }

    private void BatchDisable_Click(object sender, RoutedEventArgs e)
    {
        var rules = GetSelectedRules();
        if (rules.Count == 0) return;
        var updated = _service.SetRulesEnabled(rules.Select(rule => rule.Id).ToList(), false);
        if (updated > 0)
            ShowBatchFeedback(string.Format(Strings.BatchOperationDoneFormat, Strings.RulesBatchDisable, updated));
        LoadRules();
        UpdateBatchRuleButtons();
    }

    private void BatchDelete_Click(object sender, RoutedEventArgs e) => DeleteSelectedRulesWithConfirmation();

    private void DeleteSelectedRulesWithConfirmation()
    {
        var rules = GetSelectedRules();
        if (rules.Count == 0) return;
        if (DarkDialogWindow.Show(this, string.Format(Strings.RulesBatchDeleteConfirmFormat, rules.Count), Strings.DialogConfirmTitle,
            DarkDialogIcon.Question, DarkDialogButtons.YesNo) != MessageBoxResult.Yes) return;
        var removed = _service.RemoveRules(rules.Select(rule => rule.Id).ToList());
        if (removed > 0)
            ShowBatchFeedback(string.Format(Strings.BatchOperationDoneFormat, Strings.RulesBatchDelete, removed));
        LoadRules();
        UpdateBatchRuleButtons();
    }

    // 键盘交互：Delete 删除选中规则（走批量删除同一确认流程）。
    private void RulesList_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Delete) return;
        DeleteSelectedRulesWithConfirmation();
        e.Handled = true;
    }

    // 双击规则行打开条件编辑器，与右键菜单第一项同一入口。
    private void RulesList_DoubleClick(object sender, MouseButtonEventArgs e)
    {
        if (GetSelectedRule() != null)
            EditRuleConstraints_Click(sender, e);
    }

    // Ctrl+F 在规则页聚焦搜索框；Esc 在搜索框内清空并交还列表焦点。
    private void Window_PreviewKeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key == Key.F && Keyboard.Modifiers == ModifierKeys.Control &&
            PageRules.Visibility == Visibility.Visible && PageRules.Opacity > 0.99)
        {
            SearchBox.Focus();
            SearchBox.SelectAll();
            e.Handled = true;
        }
    }

    private void SearchBox_KeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Escape) return;
        if (SearchBox.Text.Length > 0)
        {
            SearchBox.Text = string.Empty;
        }
        else
        {
            RulesList.Focus();
        }
        e.Handled = true;
    }

    // 批量设置模式：Tag 承载目标模式（Proxy/Direct/Block），复用 v0.12 的 SetRulesMode 原子事务。
    private void BatchMode_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not Button { Tag: string modeName } || !Enum.TryParse(modeName, out ProxyMode mode))
            return;
        var rules = GetSelectedRules();
        if (rules.Count == 0) return;
        var updated = _service.SetRulesMode(rules.Select(rule => rule.Id).ToList(), mode);
        if (updated > 0)
            ShowBatchFeedback(string.Format(Strings.BatchOperationDoneFormat, mode switch
            {
                ProxyMode.Proxy => Strings.RulesBatchProxy,
                ProxyMode.Direct => Strings.RulesBatchDirect,
                ProxyMode.Block => Strings.RulesBatchBlock,
                _ => mode.ToString()
            }, updated));
        LoadRules();
        UpdateBatchRuleButtons();
    }

    private void Import_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = Strings.DialogImportTitle,
            Filter = Strings.DialogJsonFilter
        };

        if (dialog.ShowDialog() == true)
        {
            try
            {
                var json = AppConfigStore.ReadStrictUtf8(dialog.FileName);
                var import = Newtonsoft.Json.JsonConvert.DeserializeObject<ImportData>(json);
                if (import?.Rules == null)
                    throw new InvalidDataException(Strings.ImportPreviewMissingRules);

                var preview = _service.PreviewRuleImport(import.Rules);
                var previewWindow = new RuleImportPreviewWindow(preview) { Owner = this };
                if (previewWindow.ShowDialog() != true)
                    return;

                var added = _service.ImportRules(import.Rules);
                LoadRules();
                DarkDialogWindow.Show(this,
                    string.Format(Strings.ImportDoneFormat, added, preview.SkipCount),
                    Strings.DialogSuccessTitle, DarkDialogIcon.Info);
            }
            catch (Exception ex)
            {
                DarkDialogWindow.Show(this, string.Format(Strings.ImportFailedFormat, ex.Message), Strings.DialogErrorTitle,
                    DarkDialogIcon.Error);
            }
        }
    }

    private void Export_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new SaveFileDialog
        {
            Title = Strings.DialogExportTitle,
            Filter = Strings.DialogJsonFilter,
            FileName = $"rules_{DateTime.Now:yyyyMMdd}.json"
        };

        if (dialog.ShowDialog() == true)
        {
            try
            {
                var export = new
                {
                    Version = "0.3.0",
                    ExportTime = DateTime.Now,
                    Rules = _service.Config.Rules
                };
                var json = Newtonsoft.Json.JsonConvert.SerializeObject(export, Newtonsoft.Json.Formatting.Indented);
                File.WriteAllText(dialog.FileName, json);
                DarkDialogWindow.Show(this, string.Format(Strings.ExportDoneFormat, _service.Config.Rules.Count), Strings.DialogSuccessTitle,
                    DarkDialogIcon.Info);
            }
            catch (Exception ex)
            {
                DarkDialogWindow.Show(this, string.Format(Strings.ExportFailedFormat, ex.Message), Strings.DialogErrorTitle,
                    DarkDialogIcon.Error);
            }
        }
    }

    private class ImportData
    {
        public List<ProxyRule>? Rules { get; set; }
    }

    #endregion

    #region 全局模式

    private void Mode_Changed(object sender, RoutedEventArgs e)
    {
        _service.SetGlobalMode(ModeProxy.IsChecked == true ? GlobalMode.ProxyAll : GlobalMode.DirectAll);
        if (IsLoaded)
            InvalidateRouteDecision(Strings.RouteMsgGlobalModeChanged);
        _ = RefreshPolicyAnalysisAsync();
    }

    #endregion

    #region 流量监控

    private void ClearLogs_Click(object sender, RoutedEventArgs e)
    {
        _service.ClearLogs();
        _runtimeLogs.Clear();
    }

    private RuntimeLogLevel CurrentMinimumLogLevel() => LogLevelFilterCombo.SelectedIndex switch
    {
        2 => RuntimeLogLevel.Debug,
        3 => RuntimeLogLevel.Info,
        4 => RuntimeLogLevel.Warn,
        5 => RuntimeLogLevel.Error,
        _ => RuntimeLogLevel.Trace // 第 0 项「全部」与 Trace 等效（最低档）
    };

    private void LogSearch_Changed(object sender, TextChangedEventArgs e)
    {
        // 300ms 防抖：停止旧计时后重新起表，避免每敲一个字符就刷新一次视图。
        _logSearchDebounce.Stop();
        _logSearchDebounce.Start();
    }

    private void LogSearchDebounce_Tick(object? sender, EventArgs e)
    {
        _logSearchDebounce.Stop();
        _logSearchText = LogSearchBox.Text;
        _logsView?.Refresh();
    }

    private void LogLevelFilter_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (_logsView == null) return;
        _logsView.Refresh();
    }

    private void ExportLogs_Click(object sender, RoutedEventArgs e)
    {
        var lines = (_logsView?.OfType<RuntimeLogLine>() ?? Enumerable.Empty<RuntimeLogLine>())
            .Select(l => new RuntimeLogLineSnapshot(l.Time, l.Message))
            .ToList();
        if (lines.Count == 0)
        {
            DarkDialogWindow.Show(this, Strings.MonitorExportEmpty, Strings.DialogErrorTitle,
                DarkDialogIcon.Info);
            return;
        }

        var dialog = new SaveFileDialog
        {
            Title = Strings.DialogExportTitle,
            Filter = "Text files (*.txt)|*.txt",
            FileName = $"intentsroute-logs-{DateTime.Now:yyyyMMdd-HHmmss}.txt"
        };
        if (dialog.ShowDialog(this) != true) return;

        try
        {
            File.WriteAllText(
                dialog.FileName,
                RuntimeLogFilter.BuildExportText(lines),
                new System.Text.UTF8Encoding(false));
            DarkDialogWindow.Show(this,
                string.Format(Strings.MonitorExportDoneFormat, lines.Count, dialog.FileName),
                Strings.DialogSuccessTitle, DarkDialogIcon.Info);
        }
        catch (Exception ex)
        {
            DarkDialogWindow.Show(this,
                string.Format(Strings.MonitorExportFailedFormat, SingBoxRuntime.RedactSecrets(ex.Message)),
                Strings.DialogErrorTitle,
                DarkDialogIcon.Warning);
        }
    }

    #endregion

    #region 进程列表

    private void RefreshProcess_Click(object sender, RoutedEventArgs e)
    {
        RefreshProcessList();
    }

    private void RefreshProcessList()
    {
        var processes = ProcessMonitor.GetRunningProcesses();
        var rules = PolicyRuntimeOrder.Enabled(_service.Config.Rules)
            .Select(item => item.Rule)
            .ToList();
        _allProcesses.Clear();
        _allProcesses.AddRange(processes
            .OrderBy(p => p.Value)
            .Select(p => new ProcessRow(
                p.Key,
                p.Value,
                ProcessMonitor.TryGetProcessPath(p.Key, out var path) ? path : "",
                GetConfiguredCandidate(rules, p.Value))));
        ApplyProcessFilter();
    }

    private void ApplyProcessFilter()
    {
        IEnumerable<ProcessRow> view = _allProcesses;
        if (!string.IsNullOrWhiteSpace(_processSearchFilter))
        {
            view = _allProcesses.Where(r =>
                r.Name.Contains(_processSearchFilter, StringComparison.OrdinalIgnoreCase) ||
                r.Pid.ToString().Contains(_processSearchFilter, StringComparison.OrdinalIgnoreCase));
        }
        var filtered = view.ToList();

        ProcessList.ItemsSource = filtered;
        ProcessCount.Text = string.IsNullOrWhiteSpace(_processSearchFilter)
            ? string.Format(Localization.Strings.CountProcessesFormat, _allProcesses.Count)
            : string.Format(Localization.Strings.CountProcessesFilteredFormat, filtered.Count, _allProcesses.Count);
    }

    private void ProcessSearch_Changed(object sender, TextChangedEventArgs e)
    {
        // 进程快照近千行：每键全量重设 ItemsSource 会明显卡顿，与日志搜索共用 300ms 防抖。
        _processSearchDebounce.Stop();
        _processSearchDebounce.Start();
    }

    private void ProcessSearchDebounce_Tick(object? sender, EventArgs e)
    {
        _processSearchDebounce.Stop();
        _processSearchFilter = ProcessSearchBox.Text;
        ApplyProcessFilter();
    }

    private void ProcessList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        ProcessAddRuleButton.IsEnabled = ProcessList.SelectedItem is ProcessRow;
    }

    private void AddRuleFromProcess_Click(object sender, RoutedEventArgs e)
    {
        if (ProcessList.SelectedItem is not ProcessRow row) return;
        try
        {
            var rule = _service.AddRuleByName(row.Name, row.Path);
            if (rule == null)
            {
                DarkDialogWindow.Show(this, Strings.ProcessRuleExists, Strings.DialogErrorTitle,
                    DarkDialogIcon.Info);
                return;
            }

            NavRules.IsChecked = true;
            SearchBox.Text = string.Empty;
            ShowPage("NavRules");
            LoadRules();
            var added = _rules.FirstOrDefault(item => item.Id == rule.Id);
            if (added == null) return;
            RulesList.SelectedItem = added;
            RulesList.ScrollIntoView(added);
            RulesList.Focus();
        }
        catch (Exception ex)
        {
            DarkDialogWindow.Show(this, SingBoxRuntime.RedactSecrets(ex.Message), Strings.DialogErrorTitle,
                DarkDialogIcon.Warning);
        }
    }

    #endregion

    #region 设置

    private void LoadSettings()
    {
        var config = _service.Config;
        var proxy = _service.GetPrimaryProxy();
        ProxyHost.Text = proxy?.Host ?? config.SocksHost;
        ProxyPort.Text = (proxy?.Port ?? config.SocksPort).ToString();
        ProxyUsername.Text = proxy?.Username ?? string.Empty;
        ProxyPassword.Password = proxy?.Password ?? string.Empty;
        ProxyTypeCombo.SelectedIndex = (proxy?.ProxyType ?? ProxyType.Socks5) switch
        {
            ProxyType.Http => 1,
            ProxyType.Https => 2,
            _ => 0
        };
        RuntimePathBox.Text = config.SingBoxExecutablePath;
        ModeProxy.IsChecked = config.GlobalMode == GlobalMode.ProxyAll;
        ModeDirect.IsChecked = config.GlobalMode == GlobalMode.DirectAll;

        _languageUiLoading = true;
        try
        {
            LanguageCombo.SelectedIndex = UiPreferences.GetLanguage() switch
            {
                UiPreferences.English => 1,
                UiPreferences.FollowSystem => 2,
                _ => 0
            };
        }
        finally
        {
            _languageUiLoading = false;
        }
    }

    private void Language_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (_languageUiLoading || _shutdownStarted) return;
        if (LanguageCombo.SelectedItem is not ComboBoxItem item || item.Tag is not string value) return;
        UiPreferences.SetLanguage(value);
        LanguageSavedHint.Text = ProxyManager.Standalone.Localization.Strings.SettingsLanguageRestartHint;
    }

    private void SaveProxy_Click(object sender, RoutedEventArgs e)
    {
        if (!int.TryParse(ProxyPort.Text, out var port))
        {
            DarkDialogWindow.Show(this, Strings.ProxyInvalidPort, Strings.DialogErrorTitle, DarkDialogIcon.Warning);
            return;
        }

        var proxyType = GetSelectedProxyType();
        try
        {
            _service.UpdatePrimaryProxy(
                proxyType,
                ProxyHost.Text,
                port,
                ProxyUsername.Text,
                ProxyPassword.Password);
            InvalidateRouteDecision(Strings.RouteMsgProxyChanged);
            _ = RefreshPolicyAnalysisAsync();
            ProxyHost.Text = LocalProxyEndpoint.NormalizeOrThrow(ProxyHost.Text, port);
            StatusDetail.Text = string.Format(Strings.ProxySavedFormat, proxyType, ProxyHost.Text, port);
            ProxyTestStatus.Text = Strings.ProxySavedNote;
        }
        catch (Exception ex) when (ex is ArgumentException or InvalidOperationException)
        {
            DarkDialogWindow.Show(this, ex.Message, Strings.ProxySaveFailTitle, DarkDialogIcon.Warning);
        }
    }

    private async void TestProxy_Click(object sender, RoutedEventArgs e)
    {
        if (!int.TryParse(ProxyPort.Text, out var port))
        {
            ProxyTestStatus.Text = Strings.ProxyTestInvalidPort;
            return;
        }

        if (!LocalProxyEndpoint.TryNormalize(ProxyHost.Text, port, out var normalizedHost, out var error))
        {
            ProxyTestStatus.Text = error;
            return;
        }

        TestProxyButton.IsEnabled = false;
        ProxyTestStatus.Text = string.Format(Strings.ProxyTestCheckingFormat, normalizedHost, port);
        try
        {
            var connected = await _service.TestLocalProxyAsync(normalizedHost, port);
            ProxyTestStatus.Text = connected
                ? Strings.ProxyTestOk
                : Strings.ProxyTestFail;
        }
        finally
        {
            TestProxyButton.IsEnabled = _service.IsConfigurationWritable;
        }
    }

    private async void BrowseSingBox_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = Strings.DialogPickRuntimeTitle,
            Filter = Strings.DialogRuntimeFilter
        };
        if (dialog.ShowDialog() != true) return;

        try
        {
            _service.SetSingBoxExecutablePath(dialog.FileName);
            RuntimePathBox.Text = Path.GetFullPath(dialog.FileName);
            await RefreshRuntimeReadinessAsync();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or InvalidOperationException)
        {
            DarkDialogWindow.Show(this, ex.Message, Strings.RuntimeUseFailTitle, DarkDialogIcon.Warning);
        }
    }

    private async void RefreshRuntime_Click(object sender, RoutedEventArgs e) =>
        await RefreshRuntimeReadinessAsync();

    private async void ClearSingBoxPath_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            _service.ClearSingBoxExecutablePath();
            RuntimePathBox.Text = string.Empty;
            await RefreshRuntimeReadinessAsync();
        }
        catch (InvalidOperationException ex)
        {
            DarkDialogWindow.Show(this, ex.Message, Strings.RuntimePathFailTitle, DarkDialogIcon.Warning);
        }
    }

    private async void AiHealthCheck_Click(object sender, RoutedEventArgs e)
    {
        if (_shutdownStarted) return;
        var button = (Button)sender;
        button.IsEnabled = false;
        AiHealthText.Text = Strings.SettingsRuntimeChecking;
        try
        {
            var provider = GetSelectedAiProvider();
            var model = AiModelCombo.SelectedItem as string;
            var check = await AiProviderDiagnostics.CheckAsync(provider, model, _lifetimeCts.Token);
            var providerLabel = check.Kind == AiProviderKind.OpenAI ? "OpenAI" : "Ollama";
            var stateLabel = check.State switch
            {
                AiProviderHealthState.Ready => Strings.HealthStateReady,
                AiProviderHealthState.NotConfigured => Strings.HealthStateNotConfigured,
                AiProviderHealthState.Misconfigured => Strings.HealthStateMisconfigured,
                AiProviderHealthState.Unreachable => Strings.HealthStateUnreachable,
                _ => Strings.HealthStateUnknown
            };
            AiHealthText.Text = $"{providerLabel}：{stateLabel}\n{string.Join("\n", check.Details)}";
        }
        catch (AiProviderException ex)
        {
            AiHealthText.Text = ex.Message;
        }
        catch (OperationCanceledException) when (_lifetimeCts.IsCancellationRequested)
        {
            // Shutdown during diagnostics keeps the last visible state.
        }
        catch (Exception ex)
        {
            AiHealthText.Text = string.Format(Strings.HealthFailedFormat, SingBoxRuntime.RedactSecrets(ex.Message));
        }
        finally
        {
            if (!_shutdownStarted) button.IsEnabled = true;
        }
    }

    private async Task RefreshRuntimeReadinessAsync()
    {
        if (!_service.IsConfigurationWritable)
        {
            RuntimeReadinessText.Text = Strings.RuntimeReadyProtected;
            RuntimeVersionText.Text = Localization.Strings.VersionPrefix + Localization.Strings.VersionCheckBlocked;
            return;
        }

        RefreshRuntimeButton.IsEnabled = false;
        RuntimeReadinessText.Text = Strings.RuntimeReadyReading;
        _runtimeReadinessCts?.Cancel();
        _runtimeReadinessCts?.Dispose();
        var cts = new CancellationTokenSource();
        _runtimeReadinessCts = cts;
        try
        {
            var readiness = await _service.ProbeRuntimeReadinessAsync(cts.Token);
            RuntimePathBox.Text = readiness.ExecutablePath ?? _service.Config.SingBoxExecutablePath;
            RuntimeVersionText.Text = Localization.Strings.VersionPrefix +
                (readiness.Version ?? Localization.Strings.VersionUnrecognized);
            RuntimeReadinessText.Text = readiness.IsReady
                ? Strings.RuntimeReadyOk
                : readiness.Error ?? Strings.RuntimeReadyNotReady;
            if (!_service.GetRuntimeStatus().IsRunning)
            {
                StatusDetail.Text = readiness.IsReady
                    ? Strings.RuntimeReadyWaiting
                    : readiness.Error ?? Strings.RuntimeReadyNotReady;
            }
        }
        catch (OperationCanceledException)
        {
            RuntimeReadinessText.Text = Strings.RuntimeReadyCancelled;
        }
        finally
        {
            if (ReferenceEquals(_runtimeReadinessCts, cts))
            {
                _runtimeReadinessCts = null;
                cts.Dispose();
            }
            RefreshRuntimeButton.IsEnabled = _service.IsConfigurationWritable;
        }
    }

    private ProxyType GetSelectedProxyType() => ProxyTypeCombo.SelectedIndex switch
    {
        1 => ProxyType.Http,
        2 => ProxyType.Https,
        _ => ProxyType.Socks5
    };

    private void UpdateConfigurationUi()
    {
        var writable = _service.IsConfigurationWritable;
        ConfigRecoveryBanner.Visibility = writable ? Visibility.Collapsed : Visibility.Visible;
        PageRules.IsEnabled = writable;
        PageAiAssistant.IsEnabled = writable;
        PagePolicyIntelligence.IsEnabled = writable;
        PageRouteSimulator.IsEnabled = writable;
        RuntimeSettingsCard.IsEnabled = writable;
        ProxySettingsCard.IsEnabled = writable;
        ModeDirect.IsEnabled = writable;
        ModeProxy.IsEnabled = writable;

        if (!writable)
        {
            ShowProtectedRouteSimulationState();
            var backup = _service.ConfigurationRecoveryBackupPath;
            var recoveryCopyAvailable = !string.IsNullOrWhiteSpace(backup) && File.Exists(backup);
            ResetConfigButton.IsEnabled = recoveryCopyAvailable;
            RecoverConfigButton.IsEnabled = recoveryCopyAvailable;
            ResetConfigButton.ToolTip = ResetConfigButton.IsEnabled
                ? Strings.RecoveryResetReady
                : Strings.RecoveryResetUnavailable;
            RecoverConfigButton.ToolTip = RecoverConfigButton.IsEnabled
                ? Strings.RecoveryImportReady
                : Strings.RecoveryImportUnavailable;
            var reason = string.IsNullOrWhiteSpace(_service.ConfigurationError)
                ? string.Empty
                : Strings.RecoveryReasonPrefix + _service.ConfigurationError + " ";
            ConfigRecoveryText.Text = string.IsNullOrWhiteSpace(backup)
                ? reason + Strings.RecoveryNoCopy
                : reason + string.Format(Strings.RecoveryWithCopyFormat, backup);
            StatusDetail.Text = Strings.RecoveryActiveStatus;
        }
        else
        {
            ResetConfigButton.IsEnabled = false;
            RecoverConfigButton.IsEnabled = false;
            RouteSimulateButton.IsEnabled = _routeSimulationCts == null;
        }
    }

    private void OpenConfigFolder_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var startInfo = new ProcessStartInfo
            {
                FileName = "explorer.exe",
                UseShellExecute = false,
                CreateNoWindow = true
            };
            startInfo.ArgumentList.Add(_service.ConfigDirectory);
            using var process = new Process { StartInfo = startInfo };
            process.Start();
        }
        catch
        {
            DarkDialogWindow.Show(this, _service.ConfigDirectory, Strings.DialogConfigDirTitle, DarkDialogIcon.Info);
        }
    }

    private async void RecoverConfig_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = Strings.DialogRecoverTitle,
            Filter = Strings.DialogJsonFilter
        };
        if (dialog.ShowDialog() != true) return;

        try
        {
            _service.RecoverConfigurationFromFile(dialog.FileName);
            UpdateConfigurationUi();
            LoadSettings();
            LoadRules();
            await RefreshRuntimeReadinessAsync();
        }
        catch (Exception ex) when (ex is IOException or InvalidDataException or UnauthorizedAccessException or ArgumentException or InvalidOperationException or Newtonsoft.Json.JsonException or AppConfigProtectionException)
        {
            DarkDialogWindow.Show(this, string.Format(Strings.RecoveryImportFailBodyFormat, ex.Message),
                Strings.RecoveryImportFailTitle, DarkDialogIcon.Warning);
        }
    }

    private async void ResetConfig_Click(object sender, RoutedEventArgs e)
    {
        var answer = DarkDialogWindow.Show(this,
            Strings.RecoveryResetConfirmBody,
            Strings.RecoveryResetConfirmTitle,
            DarkDialogIcon.Warning,
            DarkDialogButtons.YesNo);
        if (answer != MessageBoxResult.Yes) return;

        try
        {
            _service.ResetUnusableConfiguration();
            UpdateConfigurationUi();
            LoadSettings();
            LoadRules();
            await RefreshRuntimeReadinessAsync();
        }
        catch (Exception ex)
        {
            DarkDialogWindow.Show(this, ex.Message, Strings.RecoveryResetFailTitle, DarkDialogIcon.Error);
        }
    }

    private void UpdateRuntimeStatusUi(SingBoxRuntimeStatus status)
    {
        var brushKey = status.State switch
        {
            SingBoxRuntimeState.Running when status.IsRunning => "SuccessBrush",
            SingBoxRuntimeState.RunningStale when status.IsRunning => "WarningBrush",
            SingBoxRuntimeState.Probing or SingBoxRuntimeState.Starting or SingBoxRuntimeState.Checking => "WarningBrush",
            SingBoxRuntimeState.Failed => "DangerBrush",
            _ => "TextMutedBrush"
        };
        SetStatusBrush(brushKey);
        if (!string.IsNullOrWhiteSpace(status.ExecutablePath))
            RuntimePathBox.Text = status.ExecutablePath;
        if (!string.IsNullOrWhiteSpace(status.Version))
            RuntimeVersionText.Text = Localization.Strings.VersionPrefix + status.Version;
    }

    private void SetStatusBrush(string resourceKey)
    {
        if (FindResource(resourceKey) is Brush brush)
            StatusDot.Fill = brush;
    }

    #endregion

    #region 拖放

    private void Window_DragOver(object sender, DragEventArgs e)
    {
        if (e.Data.GetDataPresent(DataFormats.FileDrop))
        {
            var files = (string[])e.Data.GetData(DataFormats.FileDrop);
            e.Effects = files.Any(f => f.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
                ? DragDropEffects.Copy : DragDropEffects.None;
        }
        else
        {
            e.Effects = DragDropEffects.None;
        }
        e.Handled = true;
        DropOverlay.Visibility = Visibility.Visible;
    }

    private void Window_DragLeave(object sender, DragEventArgs e)
    {
        DropOverlay.Visibility = Visibility.Collapsed;
    }

    private void Window_Drop(object sender, DragEventArgs e)
    {
        DropOverlay.Visibility = Visibility.Collapsed;
        if (e.Data.GetDataPresent(DataFormats.FileDrop))
        {
            foreach (var file in (string[])e.Data.GetData(DataFormats.FileDrop))
            {
                if (file.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
                {
                    _service.AddRule(file);
                }
            }
            LoadRules();
        }
    }

    #endregion

    protected override async void OnClosing(CancelEventArgs e)
    {
        if (_shutdownComplete)
        {
            base.OnClosing(e);
            return;
        }

        e.Cancel = true;
        base.OnClosing(e);
        if (_shutdownStarted) return;
        _shutdownStarted = true;
        IsEnabled = false;
        StatusDetail.Text = Strings.ShutdownStopping;
        _lifetimeCts.Cancel();
        _draftRevalidationTimer?.Stop();
        _draftRevalidationTimer = null;
        _logSearchDebounce.Stop();
        _rulesSearchDebounce.Stop();
        _processSearchDebounce.Stop();
        _aiModelRefreshVersion++;
        _policyModelRefreshVersion++;
        _policyAnalysisVersion++;
        _routeSimulationVersion++;
        _aiGenerationCts?.Cancel();
        _policyAnalysisCts?.Cancel();
        _policyExplanationCts?.Cancel();
        _routeSimulationCts?.Cancel();
        _runtimeReadinessCts?.Cancel();
        try
        {
            await Task.WhenAll(_policyAnalysisDrainTask, _routeSimulationDrainTask);
        }
        catch (Exception ex)
        {
            Debug.WriteLine("IntentRoute AI policy-analysis shutdown failed: " + SingBoxRuntime.RedactSecrets(ex.Message));
        }
        try
        {
            await _service.DisposeAsync();
        }
        catch (Exception ex)
        {
            Debug.WriteLine("IntentRoute AI runtime shutdown failed: " + SingBoxRuntime.RedactSecrets(ex.Message));
        }

        try
        {
            await _aiProviderGate.WaitAsync();
            try
            {
                _openAiProvider.Dispose();
                _ollamaProvider.Dispose();
            }
            finally
            {
                _aiProviderGate.Release();
            }
        }
        catch (Exception ex)
        {
            Debug.WriteLine("IntentRoute AI provider shutdown failed: " + SingBoxRuntime.RedactSecrets(ex.Message));
        }
        finally
        {
            _aiGenerationCts?.Dispose();
            _policyExplanationCts?.Dispose();
            _routeSimulationCts?.Dispose();
            _runtimeReadinessCts?.Dispose();
            _lifetimeCts.Dispose();
            _aiProviderGate.Dispose();
            _shutdownComplete = true;
            _shutdownStarted = false;
            _ = Dispatcher.BeginInvoke(new Action(Close));
        }
    }

    private void PostToUi(Action action)
    {
        try
        {
            if (Dispatcher.CheckAccess())
                action();
            else
                _ = Dispatcher.BeginInvoke(action);
        }
        catch
        {
            // The window may already be shutting down; runtime cleanup must not wait on UI dispatch.
        }
    }

    private static string GetConfiguredCandidate(IEnumerable<ProxyRule> rules, string processName)
    {
        var rule = rules.FirstOrDefault(r =>
            string.Equals(r.ExeName, "*", StringComparison.Ordinal) ||
            string.Equals(r.ExeName, processName, StringComparison.OrdinalIgnoreCase));
        return rule?.Mode switch
        {
            ProxyMode.Proxy => Strings.ProcCandProxy,
            ProxyMode.Direct => Strings.ProcCandDirect,
            ProxyMode.Block => Strings.ProcCandBlock,
            _ => Strings.ProcCandNone
        };
    }

    private sealed record RuntimeLogLine(string Time, string Message, RuntimeLogLevel Level = RuntimeLogLevel.Info);

    private sealed record ProcessRow(uint Pid, string Name, string Path, string Status);

    private sealed record RouteDecisionTraceLine(
        string Order,
        string RuleName,
        string Evaluation,
        string Reason)
    {
        public static RouteDecisionTraceLine FromStep(RouteDecisionTraceStep step) => new(
            $"#{step.EvaluationOrder}",
            step.DisplayName,
            step.Evaluation switch
            {
                RouteRuleEvaluation.ProvenMatch => Localization.Strings.TraceVerdictMatch,
                RouteRuleEvaluation.ProvenMiss => Localization.Strings.TraceVerdictMiss,
                RouteRuleEvaluation.Indeterminate => Localization.Strings.TraceVerdictIndeterminate,
                _ => Strings.ProcCandUnknown
            },
            GetRouteDecisionReasonText(step.Reason));
    }

    private sealed class AiRuleEditLine : INotifyPropertyChanged
    {
        public static readonly IReadOnlyList<string> ActionOptions = ["Proxy", "Direct", "Block"];
        public static readonly IReadOnlyList<string> ProtocolOptions = ["TCP", "UDP", "Both"];

        private string _processName = string.Empty;
        private string _action = "Direct";
        private string _targetHosts = string.Empty;
        private string _targetIps = string.Empty;
        private string _targetPorts = string.Empty;
        private string _protocol = "Both";
        private string _rationale = string.Empty;

        public string ProcessName { get => _processName; set => Set(ref _processName, value); }
        public string Action { get => _action; set => Set(ref _action, value); }
        public string TargetHosts { get => _targetHosts; set => Set(ref _targetHosts, value); }
        public string TargetIps { get => _targetIps; set => Set(ref _targetIps, value); }
        public string TargetPorts { get => _targetPorts; set => Set(ref _targetPorts, value); }
        public string Protocol { get => _protocol; set => Set(ref _protocol, value); }
        public string Rationale { get => _rationale; set => Set(ref _rationale, value); }
        public IReadOnlyList<string> Actions => ActionOptions;
        public IReadOnlyList<string> Protocols => ProtocolOptions;
        public double Confidence { get; init; }
        public string ConfidenceDisplay => Confidence.ToString("P0");

        public event PropertyChangedEventHandler? PropertyChanged;

        public static AiRuleEditLine FromDraft(AiRuleDraft draft) => new()
        {
            ProcessName = draft.ProcessName,
            Action = draft.Action,
            TargetHosts = draft.TargetHosts,
            TargetIps = draft.TargetIps,
            TargetPorts = draft.TargetPorts,
            Protocol = draft.Protocol,
            Rationale = draft.Rationale,
            Confidence = draft.Confidence
        };

        public AiRuleDraft ToDraft() => new()
        {
            ProcessName = ProcessName,
            TargetHosts = TargetHosts,
            TargetIps = TargetIps,
            TargetPorts = TargetPorts,
            Protocol = Protocol,
            Action = Action,
            Rationale = Rationale,
            Confidence = Confidence
        };

        private void Set<T>(ref T field, T value)
        {
            if (EqualityComparer<T>.Default.Equals(field, value)) return;
            field = value;
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(null));
        }
    }

    private sealed record PolicyFindingPreviewLine(
        string Severity,
        string Code,
        string Title,
        string AffectedRules,
        string Guidance,
        string? PrimaryRuleId)
    {
        public static PolicyFindingPreviewLine FromFinding(PolicyFinding finding)
        {
            var severity = finding.Severity switch
            {
                PolicyFindingSeverity.Critical => Localization.Strings.SeverityCritical,
                PolicyFindingSeverity.Warning => Localization.Strings.SeverityWarning,
                _ => Localization.Strings.SeverityInfo
            };
            var affected = finding.Rules.Count == 0
                ? Strings.PolicyLineGlobalDefault
                : string.Join(" → ", finding.Rules.Select(rule =>
                    rule.EvaluationOrder.HasValue
                        ? $"#{rule.EvaluationOrder} {rule.DisplayName}"
                        : string.Format(Strings.PolicyLineDisabledFormat, rule.DisplayName)));
            return new PolicyFindingPreviewLine(
                severity,
                finding.Code,
                finding.Title,
                affected,
                finding.Detail + " " + finding.Recommendation,
                finding.Rules.FirstOrDefault()?.RuleId);
        }
    }

    private sealed record AiPolicyAdviceLine(
        string Code,
        string Explanation,
        string SafeNextStep,
        string Confidence)
    {
        public static AiPolicyAdviceLine FromPriority(AiPolicyPriority priority) => new(
            priority.FindingCode,
            priority.Explanation,
            priority.SafeNextStep,
            priority.Confidence.ToString("P0"));
    }
}
