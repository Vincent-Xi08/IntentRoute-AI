[CmdletBinding()]
param(
    [string]$SingBoxPath
)

$ErrorActionPreference = 'Stop'
$version = '1.13.19'
$archiveName = "sing-box-$version-windows-amd64.zip"
$downloadUrl = "https://github.com/SagerNet/sing-box/releases/download/v$version/$archiveName"
$expectedSha256 = 'e011a4def2f5e2b143ed54adb2b1a20a6be407806ab4442f3667f1dd817a2c8d'
$environmentVariable = 'INTENTROUTE_TEST_SING_BOX_PATH'
$root = Split-Path -Parent $PSScriptRoot
$tests = Join-Path $root 'ProxyManager.Tests\ProxyManager.Tests.csproj'
$downloadRoot = $null
$previousExecutable = [Environment]::GetEnvironmentVariable($environmentVariable, 'Process')

try {
    if ([string]::IsNullOrWhiteSpace($SingBoxPath)) {
        $downloadRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
            'intentroute-pinned-sing-box-' + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $downloadRoot | Out-Null
        $archivePath = Join-Path $downloadRoot $archiveName
        $expandedPath = Join-Path $downloadRoot 'expanded'

        Write-Host "Downloading official sing-box v$version test dependency..."
        Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath
        $actualSha256 = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actualSha256 -ne $expectedSha256) {
            throw "Pinned sing-box archive SHA-256 mismatch. Expected $expectedSha256 but received $actualSha256."
        }

        Expand-Archive -LiteralPath $archivePath -DestinationPath $expandedPath
        $SingBoxPath = Join-Path $expandedPath "sing-box-$version-windows-amd64\sing-box.exe"
    }

    $resolvedExecutable = (Resolve-Path -LiteralPath $SingBoxPath).Path
    $versionOutput = (& $resolvedExecutable version 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) { throw "Pinned sing-box version probe failed with exit code $LASTEXITCODE." }
    if ($versionOutput -notmatch "(?m)^sing-box version $([Regex]::Escape($version))\r?$") {
        throw "Expected sing-box version $version; the supplied test executable reported a different or unrecognized version."
    }

    [Environment]::SetEnvironmentVariable($environmentVariable, $resolvedExecutable, 'Process')
    dotnet test $tests `
        --configuration Release `
        --no-restore `
        --filter 'Category=RealSingBox' `
        --logger 'trx;LogFileName=sing-box-integration.trx'
    if ($LASTEXITCODE -ne 0) { throw 'Pinned real sing-box integration tests failed.' }

    # Rust core (migration phase 2): the Rust builder output must satisfy the
    # same pinned real `sing-box check`. The generated config contains a fake
    # password by construction; it is written to a temp file and never echoed.
    $repoRoot = Split-Path -Parent $PSScriptRoot
    $rustCli = Join-Path $repoRoot 'rust\target\release\intentroute-cli.exe'
    if (-not (Test-Path -LiteralPath $rustCli)) {
        throw "Rust CLI not found at $rustCli; run ./scripts/test-rust.ps1 first."
    }

    $rustFixtureDirectory = Join-Path ([System.IO.Path]::GetTempPath()) (
        'intentroute-rust-sing-box-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $rustFixtureDirectory | Out-Null
    try {
        $rustFixture = Join-Path $rustFixtureDirectory 'fixture.json'
        @'
{
  "GlobalMode": 1,
  "Rules": [
    {"Id": "r1", "ExeName": "chrome.exe", "Mode": 0, "IsEnabled": true, "Priority": 10,
     "CreatedAt": "2026-09-13 00:00", "TargetHosts": "github.com, *.github.com",
     "TargetIPs": "10.0.0.0/8", "TargetPorts": "443, 1000-2000", "Protocol": "TCP"},
    {"Id": "r2", "ExeName": "game.exe", "Mode": 2, "IsEnabled": true, "Priority": 20,
     "CreatedAt": "2026-09-13 00:01", "Protocol": "UDP"},
    {"Id": "r3", "ExeName": "upd.exe", "Mode": 1, "IsEnabled": false, "Priority": 5,
     "CreatedAt": "2026-09-13 00:02"}
  ],
  "ProxyServers": [
    {"Id": "s1", "Name": "loop", "ProxyType": 0, "Host": "127.0.0.1", "Port": 10808,
     "Username": "u", "Password": "fake-password-not-a-real-secret", "Enabled": true}
  ],
  "ProxyChains": []
}
'@ | Set-Content -LiteralPath $rustFixture -Encoding utf8NoBOM

        $rustGenerated = Join-Path $rustFixtureDirectory 'generated.json'
        & $rustCli build-config $rustFixture --full | Set-Content -LiteralPath $rustGenerated -Encoding utf8NoBOM
        if ($LASTEXITCODE -ne 0) { throw 'Rust builder rejected the representative fixture.' }

        $rustCheck = (& $resolvedExecutable check -c $rustGenerated 2>&1 | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) {
            # Never echo the config itself; sing-box errors may quote rule context.
            throw "Pinned real sing-box rejected the Rust builder output (exit $LASTEXITCODE): $rustCheck"
        }

        # The redacted default output must never carry the fake password.
        $redacted = (& $rustCli build-config $rustFixture) -join "`n"
        if ($redacted -match 'fake-password-not-a-real-secret') {
            throw 'Rust redacted output leaked the fixture password.'
        }
        if ($redacted -notmatch '"\*\*\*"') {
            throw 'Rust redacted output did not mask the outbound password.'
        }

        Write-Host 'Pinned real sing-box accepted the Rust core builder output (redaction verified).'
    }
    finally {
        if (Test-Path -LiteralPath $rustFixtureDirectory) {
            Remove-Item -LiteralPath $rustFixtureDirectory -Recurse -Force
        }
    }

    Write-Host "Pinned real sing-box v$version accepted representative IntentRoute AI builder output."
}
finally {
    [Environment]::SetEnvironmentVariable($environmentVariable, $previousExecutable, 'Process')
    if ($downloadRoot -and (Test-Path -LiteralPath $downloadRoot)) {
        $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        $resolvedDownloadRoot = [System.IO.Path]::GetFullPath($downloadRoot)
        if (-not $resolvedDownloadRoot.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove a pinned-test directory outside the system temp root: $resolvedDownloadRoot"
        }
        Remove-Item -LiteralPath $resolvedDownloadRoot -Recurse -Force
    }
}
