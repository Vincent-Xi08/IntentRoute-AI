[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

# Rust core gate (migration phase 1): the GitHub runner images ship both the
# MSVC toolchain on PATH and a working LIB environment, so unlike a raw Git
# Bash session no link.exe surgery is needed there.
if ($env:CI -ne 'true') {
    Write-Warning 'Local Git Bash sessions resolve coreutils link.exe ahead of MSVC; if linking fails, run from a developer prompt or set PATH/LIB as in CI.'
}

Push-Location (Join-Path $root 'rust')
try {
    cargo test --workspace --all-targets
    if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
    cargo build --workspace --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
}
finally {
    Pop-Location
}
Write-Host 'Rust core tests and release build passed.'
