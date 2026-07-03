#Requires -Version 5.1
<#
.SYNOPSIS
    Build MD -> ALL release binaries for the supported Windows targets.

.DESCRIPTION
    Builds the workspace binaries (mdall, mdall-convert, mdall-mcp) in release
    mode for:
      - x86_64-pc-windows-msvc   (Windows x64, the host, primary)
      - aarch64-pc-windows-msvc  (Windows ARM64, cross)

    Cross-compiling to ARM64 needs a cross toolchain. Two paths work:

      A. cargo-zigbuild + Zig (recommended, no Visual Studio components):
           cargo install --locked cargo-zigbuild
           pip install ziglang          # or install Zig and add it to PATH
         This script auto-detects cargo-zigbuild and uses it when present.
         It is the same path scripts/make-installer.ps1 uses for cross-arch.

      B. Native MSVC ARM64 tools: in the Visual Studio Installer add
         "MSVC v143 - VS 2022 C++ ARM64 build tools" and a "Windows SDK".
         Then plain cargo can target ARM64.

    The host (x64) build is required and a failure there is fatal. A cross
    (ARM64) failure is reported with the guidance above but does NOT fail the
    run, so the x64 artifact is always produced.

    Linux and macOS are intentionally not cross-built here: the binary reads
    system fonts and bundles a Windows rendering engine, so those platforms must be
    built on their own host. The pure-Rust Typst PDF tier and every other
    export work on all platforms once built natively.

.PARAMETER Targets
    Override the target triples to build (default: both Windows targets).

.PARAMETER X64Only
    Build only the host x64 target and skip ARM64.
#>
param(
    [string[]]$Targets = @('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc'),
    [switch]$X64Only
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$HostTarget = 'x86_64-pc-windows-msvc'
if ($X64Only) { $Targets = @($HostTarget) }

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $useZig = $null -ne (Get-Command 'cargo-zigbuild' -ErrorAction SilentlyContinue)
    $builder = if ($useZig) { 'cargo zigbuild' } else { 'cargo build' }
    Write-Host "Builder: $builder" -ForegroundColor DarkCyan

    $installed = rustup target list --installed
    foreach ($t in $Targets) {
        if ($installed -notcontains $t) {
            Write-Host "Installing rustup target $t ..." -ForegroundColor Cyan
            rustup target add $t
        }
    }

    if (-not $useZig -and ($Targets | Where-Object { $_ -ne $HostTarget })) {
        Write-Host "Note: cargo-zigbuild not found; cross targets need it (or the MSVC ARM64 tools). See the header of this script." -ForegroundColor Yellow
    }

    $ok = @()
    $crossFailed = @()
    foreach ($t in $Targets) {
        Write-Host "==> Building release for $t" -ForegroundColor Green
        if ($useZig) { cargo zigbuild --release --target $t } else { cargo build --release --target $t }
        if ($LASTEXITCODE -ne 0) {
            if ($t -eq $HostTarget) { throw "Host build failed for $t (this is fatal)." }
            Write-Host "    cross build FAILED for $t (non-fatal)" -ForegroundColor Yellow
            $crossFailed += $t
            continue
        }
        $bin = Join-Path $root "target/$t/release/mdall.exe"
        if (Test-Path $bin) {
            $mb = [math]::Round((Get-Item $bin).Length / 1MB, 1)
            Write-Host "    OK  $bin  ($mb MB)" -ForegroundColor Green
            $ok += $t
        }
    }

    Write-Host ""
    Write-Host "Built: $($ok -join ', ')" -ForegroundColor Green
    if ($crossFailed.Count -gt 0) {
        Write-Host "Not built: $($crossFailed -join ', ')" -ForegroundColor Yellow
        Write-Host "To enable ARM64, pick one path:" -ForegroundColor Yellow
        Write-Host "  A. cargo install --locked cargo-zigbuild ; pip install ziglang ; then re-run this script." -ForegroundColor Yellow
        Write-Host "  B. Add 'MSVC v143 - ARM64 build tools' + a Windows SDK in the Visual Studio Installer, then re-run." -ForegroundColor Yellow
    }
}
finally {
    Pop-Location
}
