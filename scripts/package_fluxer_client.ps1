# Builds the Windows release of the Rust GPUI client and stages a portable folder.
#
# Usage (from any directory):
#   powershell -ExecutionPolicy Bypass -File scripts\package_fluxer_client.ps1
#
# Output:
#   fluxer_client\target\fc-release\release\fluxer_client.exe
#   dist\fluxer_client\fluxer_client.exe
#   dist\fluxer_client\README.txt
#
# No signing or installer is performed here; the SHA256 of the staged exe is
# printed for release notes.

[CmdletBinding()]
param(
    [switch]$IncludeVoiceWorker
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$clientDir = Join-Path $repoRoot 'fluxer_client'
$exePath = Join-Path $clientDir 'target\fc-release\release\fluxer_client.exe'
$distDir = Join-Path $repoRoot 'dist\fluxer_client'

$previousTargetDir = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = 'target/fc-release'
Push-Location $clientDir
try {
    & cargo build --release -p fluxer_client_app --bin fluxer_client --jobs 4
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
    if ($null -eq $previousTargetDir) {
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:CARGO_TARGET_DIR = $previousTargetDir
    }
}

if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
    throw "the release executable was not produced: $exePath"
}

New-Item -ItemType Directory -Force -Path $distDir | Out-Null
$distExe = Join-Path $distDir 'fluxer_client.exe'
Copy-Item -LiteralPath $exePath -Destination $distExe -Force

$versionInfo = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($distExe)
$version = $versionInfo.FileVersion
if ([string]::IsNullOrWhiteSpace($version)) {
    $version = '0.1.0'
}

$readme = @"
Fluxer (Rust client) $version

This folder contains a portable Windows build of the Fluxer desktop client.

Run:              fluxer_client.exe
Logs:             %LOCALAPPDATA%\FluxerClient\logs\client.log
Start menu:       %APPDATA%\Microsoft\Windows\Start Menu\Programs\Fluxer Client.lnk
AppUserModelID:   Fluxer.Client.Rust

The client creates the Start menu shortcut on first start if it is missing.
Run fluxer_client.exe from this folder; moving the exe later leaves the shortcut
pointing at the old location.

Uninstall: close the client, delete this folder and the Start menu shortcut.
Saved logins stay in %APPDATA%\fluxer_client; delete that folder to remove them.

Licensed under AGPL-3.0-or-later. Source: https://github.com/fluxerapp/fluxer
"@
$readmePath = Join-Path $distDir 'README.txt'
Set-Content -LiteralPath $readmePath -Value $readme -Encoding UTF8

$hash = Get-FileHash -LiteralPath $distExe -Algorithm SHA256
Write-Host ""
Write-Host "Packaged: $distExe"
Write-Host "README:   $readmePath"
Write-Host ("SHA256:   {0}" -f $hash.Hash.ToLowerInvariant())

if ($IncludeVoiceWorker) {
    $workerDir = Join-Path $clientDir 'voice_worker'
    $workerExe = Join-Path $repoRoot 'target\vw\release\fluxer_voice_worker.exe'
    $previousWorkerTargetDir = $env:CARGO_TARGET_DIR
    Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Push-Location $workerDir
    try {
        & cargo build --release --jobs 4
        if ($LASTEXITCODE -ne 0) {
            throw "voice_worker cargo build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
        if ($null -ne $previousWorkerTargetDir) {
            $env:CARGO_TARGET_DIR = $previousWorkerTargetDir
        }
    }

    if (-not (Test-Path -LiteralPath $workerExe -PathType Leaf)) {
        throw "the release voice worker executable was not produced: $workerExe"
    }
    $distWorkerExe = Join-Path $distDir 'fluxer_voice_worker.exe'
    Copy-Item -LiteralPath $workerExe -Destination $distWorkerExe -Force
    $workerHash = Get-FileHash -LiteralPath $distWorkerExe -Algorithm SHA256
    Write-Host "Worker:   $distWorkerExe"
    Write-Host ("Worker SHA256: {0}" -f $workerHash.Hash.ToLowerInvariant())
}
