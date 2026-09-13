[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,

    [ValidateRange(5, 60)]
    [int]$LaunchTimeoutSeconds = 20,

    [ValidateRange(5, 60)]
    [int]$CloseTimeoutSeconds = 20
)

$ErrorActionPreference = 'Stop'
$resolvedOutput = (Resolve-Path -LiteralPath $OutputDirectory).Path
$application = Join-Path $resolvedOutput 'IntentRouteAI.exe'
$expectedTitle = 'IntentRoute AI - Windows 智能分流'
$diagnosticVariable = 'INTENTROUTE_SMOKE_DIAGNOSTIC_PATH'
$diagnosticPath = Join-Path ([System.IO.Path]::GetTempPath()) (
    'intentroute-wpf-smoke-' + [Guid]::NewGuid().ToString('N') + '.txt')
$previousDiagnosticPath = [Environment]::GetEnvironmentVariable($diagnosticVariable, 'Process')

if (-not (Test-Path -LiteralPath $application -PathType Leaf)) {
    throw "Published package is missing IntentRouteAI.exe: $resolvedOutput"
}

$process = $null
try {
    [Environment]::SetEnvironmentVariable($diagnosticVariable, $diagnosticPath, 'Process')
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $application
    $startInfo.WorkingDirectory = $resolvedOutput
    # Honor the packaged requireAdministrator manifest exactly as an end user does.
    # GitHub-hosted Windows runners execute with the required administrative token.
    $startInfo.UseShellExecute = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw 'Published IntentRouteAI.exe did not start.'
    }

    $launchDeadline = [DateTime]::UtcNow.AddSeconds($LaunchTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $launchDeadline) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) {
            throw "Published IntentRouteAI.exe exited before creating its main window (exit code $($process.ExitCode))."
        }
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
            break
        }
    }

    $process.Refresh()
    if ($process.MainWindowHandle -eq [IntPtr]::Zero) {
        throw "Published IntentRouteAI.exe did not create a main window within $LaunchTimeoutSeconds seconds."
    }
    if ($process.MainWindowTitle -ne $expectedTitle) {
        $diagnostic = if (Test-Path -LiteralPath $diagnosticPath -PathType Leaf) {
            Get-Content -Raw -LiteralPath $diagnosticPath
        }
        else {
            'No redacted managed-exception diagnostic was produced.'
        }
        throw "Published IntentRouteAI.exe created an unexpected main-window title: '$($process.MainWindowTitle)'.`n$diagnostic"
    }

    # High-DPI coverage: the main window must opt into Per-Monitor V2 DPI awareness
    # through its embedded application manifest. GetWindowDpiAwarenessContext returns
    # an opaque handle, so equality against the PMv2 context constant must go through
    # AreDpiAwarenessContextsEqual rather than a raw handle comparison.
    Add-Type -Namespace Native -Name Win32 -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")] public static extern IntPtr GetWindowDpiAwarenessContext(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool AreDpiAwarenessContextsEqual(IntPtr valueA, IntPtr valueB);
'@
    $dpiContext = [Native.Win32]::GetWindowDpiAwarenessContext($process.MainWindowHandle)
    $perMonitorV2 = [IntPtr](-4)
    $isPerMonitorV2 = [Native.Win32]::AreDpiAwarenessContextsEqual($dpiContext, $perMonitorV2)
    if (-not $isPerMonitorV2) {
        throw 'Main window DPI awareness is not Per-Monitor V2.'
    }

    # Keyboard-navigation coverage: the main window must expose an assistive-technology
    # name, a usable set of keyboard-focusable controls, Tab traversal that moves focus,
    # and arrow-key movement within the navigation radio group.
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    Add-Type -AssemblyName System.Windows.Forms
    $null = [Native.Win32]::SetForegroundWindow($process.MainWindowHandle)
    Start-Sleep -Milliseconds 500

    $root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    if ([string]::IsNullOrWhiteSpace($root.Current.Name)) {
        throw 'Main window exposes no UI Automation name for assistive technology.'
    }

    $focusableCondition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::IsKeyboardFocusableProperty, $true)
    $focusableCount = $root.FindAll(
        [System.Windows.Automation.TreeScope]::Descendants, $focusableCondition).Count
    # The count depends on first-run state (disabled controls are not keyboard focusable);
    # a fresh machine exposes about 19, so the floor keeps margin below that.
    if ($focusableCount -lt 15) {
        throw "Main window exposes only $focusableCount keyboard-focusable controls; expected at least 15."
    }

    $navIdCondition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::AutomationIdProperty, 'NavRules')
    $navRules = $root.FindFirst(
        [System.Windows.Automation.TreeScope]::Descendants, $navIdCondition)
    if ($null -eq $navRules) {
        throw 'Main window does not expose the NavRules automation element.'
    }
    $navRules.SetFocus()
    Start-Sleep -Milliseconds 200

    [System.Windows.Forms.SendKeys]::SendWait('{TAB}')
    Start-Sleep -Milliseconds 200
    $afterTab = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($null -ne $afterTab -and $afterTab.Current.AutomationId -eq 'NavRules') {
        throw 'Tab key did not move keyboard focus away from the first navigation item.'
    }

    $navRules.SetFocus()
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait('{DOWN}')
    Start-Sleep -Milliseconds 200
    $afterDown = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($null -eq $afterDown -or $afterDown.Current.AutomationId -ne 'NavPolicy') {
        $seenId = if ($null -ne $afterDown) { $afterDown.Current.AutomationId } else { '<none>' }
        throw "Down arrow did not move focus within the navigation group (focused: '$seenId')."
    }

    # Page coverage (v0.14): rules-page batch action buttons must start disabled on a fresh
    # profile, and the monitor/process pages must expose their toolbar controls once
    # selected. Pages toggle through Visibility, so collapsed pages keep their children out
    # of the UIA tree and each page has to be selected before its controls can be found.
    # The app switches pages in the WPF Click handler (Nav_Click). Empirically the nav
    # radio buttons expose only SelectionItemPattern (Select() sets IsChecked without
    # raising Click, so the page never becomes visible), and the spacebar is the only
    # input that raises OnClick on a focused WPF ButtonBase -- so the pages are switched
    # by focusing the radio and sending SPACE, then verified by polling for a control
    # that exists only on the target page.
    function Find-ControlByAutomationId {
        param(
            [System.Windows.Automation.AutomationElement]$Root,
            [string]$AutomationId
        )
        $condition = [System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::AutomationIdProperty, $AutomationId)
        return $Root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
    }

    function Select-NavPage {
        param(
            [System.Windows.Automation.AutomationElement]$Root,
            [string]$AutomationId,
            [Parameter(Mandatory = $true)][string]$VerifyControlId
        )
        $nav = Find-ControlByAutomationId -Root $Root -AutomationId $AutomationId
        if ($null -eq $nav) {
            throw "Main window does not expose the $AutomationId navigation element."
        }
        $nav.SetFocus()
        Start-Sleep -Milliseconds 250
        [System.Windows.Forms.SendKeys]::SendWait(' ')
        # Poll for a control that exists only on the target page; a focused-but-not-activated
        # radio (or any silent Select-path regression) fails loudly here instead of producing
        # confusing not-found errors in the per-page assertions below.
        $verifyDeadline = [DateTime]::UtcNow.AddSeconds(5)
        while ([DateTime]::UtcNow -lt $verifyDeadline) {
            Start-Sleep -Milliseconds 350
            if ($null -ne (Find-ControlByAutomationId -Root $Root -AutomationId $VerifyControlId)) {
                return
            }
        }
        throw "Selecting $AutomationId did not make its page visible (verify control '$VerifyControlId' not found)."
    }

    foreach ($batchButtonId in @('BatchEnableButton', 'BatchDisableButton', 'BatchDeleteButton', 'BatchProxyButton', 'BatchDirectButton', 'BatchBlockButton')) {
        $batchButton = Find-ControlByAutomationId -Root $root -AutomationId $batchButtonId
        if ($null -eq $batchButton) {
            throw "Rules page does not expose the $batchButtonId automation element."
        }
        if ($batchButton.Current.IsEnabled) {
            throw "$batchButtonId must start disabled until a rule row is selected."
        }
    }

    Select-NavPage -Root $root -AutomationId 'NavMonitor' -VerifyControlId 'LogSearchBox'
    foreach ($monitorControlId in @('LogSearchBox', 'LogLevelFilterCombo', 'LogAutoScrollToggle')) {
        $monitorControl = Find-ControlByAutomationId -Root $root -AutomationId $monitorControlId
        if ($null -eq $monitorControl) {
            throw "Monitor page does not expose the $monitorControlId automation element."
        }
    }
    # v0.29: the log empty-state hint must exist — on a fresh profile no log line exists yet.
    if ($null -eq (Find-ControlByAutomationId -Root $root -AutomationId 'LogsEmptyText')) {
        throw 'Monitor page does not expose the LogsEmptyText empty-state element.'
    }

    Select-NavPage -Root $root -AutomationId 'NavProcess' -VerifyControlId 'ProcessSearchBox'
    $processSearchBox = Find-ControlByAutomationId -Root $root -AutomationId 'ProcessSearchBox'
    if ($null -eq $processSearchBox) {
        throw 'Process page does not expose the ProcessSearchBox automation element.'
    }
    $processAddRuleButton = Find-ControlByAutomationId -Root $root -AutomationId 'ProcessAddRuleButton'
    if ($null -eq $processAddRuleButton) {
        throw 'Process page does not expose the ProcessAddRuleButton automation element.'
    }
    if ($processAddRuleButton.Current.IsEnabled) {
        throw 'ProcessAddRuleButton must start disabled until a process row is selected.'
    }

    # Restore the default page so the application is left in a clean state before closing.
    Select-NavPage -Root $root -AutomationId 'NavRules' -VerifyControlId 'BatchEnableButton'

    if (-not $process.CloseMainWindow()) {
        throw 'Published IntentRouteAI.exe did not accept a normal window-close request.'
    }
    if (-not $process.WaitForExit($CloseTimeoutSeconds * 1000)) {
        throw "Published IntentRouteAI.exe did not close within $CloseTimeoutSeconds seconds."
    }
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) {
        $diagnostic = if (Test-Path -LiteralPath $diagnosticPath -PathType Leaf) {
            Get-Content -Raw -LiteralPath $diagnosticPath
        }
        else {
            'No redacted managed-exception diagnostic was produced.'
        }
        throw "Published IntentRouteAI.exe returned exit code $($process.ExitCode) after a normal close.`n$diagnostic"
    }

    Write-Host 'WPF smoke test passed: published single-file app created the expected main window, exposed the rules-page batch buttons and the monitor/process page toolbars, and closed cleanly.'
}
finally {
    if ($process) {
        try {
            if (-not $process.HasExited) {
                $process.Kill($true)
                $null = $process.WaitForExit(5000)
            }
        }
        catch {
            # Best-effort cleanup; preserve the original smoke-test failure.
        }
        $process.Dispose()
    }
    Remove-Item -LiteralPath $diagnosticPath -Force -ErrorAction SilentlyContinue
    [Environment]::SetEnvironmentVariable($diagnosticVariable, $previousDiagnosticPath, 'Process')
}
