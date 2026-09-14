[CmdletBinding()]
param(
    [string]$ExecutablePath,

    [ValidateRange(5, 120)]
    [int]$LaunchTimeoutSeconds = 45,

    [ValidateRange(5, 60)]
    [int]$CloseTimeoutSeconds = 20
)

# Smoke gate for the Rust rules console (migration shell): the release
# binary must create its main window, actually render frames (proven by a
# marker file written after three frames), and accept a normal window-close
# request with a clean zero exit — mirroring the WPF packaged smoke test at
# the depth egui supports (no UIA automation ids to assert).
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

if ([string]::IsNullOrWhiteSpace($ExecutablePath)) {
    $ExecutablePath = Join-Path $root 'rust\target\release\intentroute-gui.exe'
}
$resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path

$markerVariable = 'INTENTROUTE_GUI_SMOKE_MARKER'
$markerPath = Join-Path ([System.IO.Path]::GetTempPath()) (
    'intentroute-gui-smoke-' + [Guid]::NewGuid().ToString('N') + '.txt')
$previousMarkerPath = [Environment]::GetEnvironmentVariable($markerVariable, 'Process')
# Hosted CI runners have no GPU: force the wgpu backend, which falls back to
# the D3D12 WARP software adapter. Machines with a real GPU render identically.
$rendererVariable = 'INTENTROUTE_GUI_RENDERER'
$previousRenderer = [Environment]::GetEnvironmentVariable($rendererVariable, 'Process')

$process = $null
try {
    [Environment]::SetEnvironmentVariable($markerVariable, $markerPath, 'Process')
    [Environment]::SetEnvironmentVariable($rendererVariable, 'wgpu', 'Process')
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $resolvedExecutable
    $startInfo.WorkingDirectory = Split-Path -Parent $resolvedExecutable
    # Shell execute (like the WPF smoke test): the marker variable set on
    # this process propagates naturally, and the child inherits no console
    # handles that could outlive this script and wedge a CI pipe.
    $startInfo.UseShellExecute = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw 'intentroute-gui.exe did not start.'
    }

    # Phase 1 — the main window must appear with its title.
    $launchDeadline = [DateTime]::UtcNow.AddSeconds($LaunchTimeoutSeconds)
    $title = ''
    while ([DateTime]::UtcNow -lt $launchDeadline) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) {
            throw "intentroute-gui.exe exited before creating its main window (exit code $($process.ExitCode))."
        }
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
            # The title can lag the handle by a beat; wait for both.
            $title = $process.MainWindowTitle
            if (-not [string]::IsNullOrWhiteSpace($title)) {
                break
            }
        }
    }
    if ([string]::IsNullOrWhiteSpace($title)) {
        throw "intentroute-gui.exe did not expose a main window title within $LaunchTimeoutSeconds seconds."
    }
    if (-not $title.Contains('IntentRoute AI')) {
        throw "intentroute-gui.exe created an unexpected main-window title: '$title'."
    }

    # Phase 2 — the render loop must actually run (marker after 3 frames).
    $markerDeadline = [DateTime]::UtcNow.AddSeconds($LaunchTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $markerDeadline) {
        if (Test-Path -LiteralPath $markerPath -PathType Leaf) {
            break
        }
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) {
            throw "intentroute-gui.exe exited before rendering three frames (exit code $($process.ExitCode))."
        }
    }
    if (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) {
        throw "intentroute-gui.exe did not write the render marker within $LaunchTimeoutSeconds seconds."
    }
    $marker = (Get-Content -Raw -LiteralPath $markerPath).Trim()
    if ($marker -notmatch 'rendered=') {
        throw "Unexpected render marker content: '$marker'."
    }
    Write-Host "Render marker: $marker"

    # Phase 3 — a normal close request must be accepted with a clean exit.
    if (-not $process.CloseMainWindow()) {
        throw 'intentroute-gui.exe did not accept a normal window-close request.'
    }
    if (-not $process.WaitForExit($CloseTimeoutSeconds * 1000)) {
        throw "intentroute-gui.exe did not close within $CloseTimeoutSeconds seconds."
    }
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) {
        throw "intentroute-gui.exe returned exit code $($process.ExitCode) after a normal close."
    }

    Write-Host 'Rust console smoke test passed: release binary created the expected window, rendered its frames, and closed cleanly.'
}
finally {
    if ($process) {
        try {
            $process.Refresh()
            if (-not $process.HasExited) {
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
                $null = $process.WaitForExit(5000)
            }
        }
        catch {
            # Best-effort cleanup; preserve the original smoke-test failure.
        }
        $process.Dispose()
    }
    Remove-Item -LiteralPath $markerPath -Force -ErrorAction SilentlyContinue
    [Environment]::SetEnvironmentVariable($markerVariable, $previousMarkerPath, 'Process')
    [Environment]::SetEnvironmentVariable($rendererVariable, $previousRenderer, 'Process')
}
