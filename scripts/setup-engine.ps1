#Requires -Version 5.1
<#
.SYNOPSIS
    Downloads the bundled rendering engine (headless x64) and installs it
    into the local runtime directory of the MD -> ALL project.

.DESCRIPTION
    Queries the engine's GitHub releases API, downloads the
    portable x64 ZIP, and extracts it to <project-root>/chromium/.
    Also creates a directory junction at target/debug/chromium and
    target/release/chromium so `cargo run` works without copying the binary.

.PARAMETER Force
    Re-download even if the engine is already present.

.PARAMETER TargetDir
    Override the extraction target (default: <script-dir>/../chromium/).
#>
param(
    [switch]$Force,
    [string]$TargetDir = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── Paths ───────────────────────────────────────────────────────────────────
$ProjectRoot = Split-Path $PSScriptRoot -Parent
$ChromiumDir  = if ($TargetDir) { $TargetDir } else { Join-Path $ProjectRoot "chromium" }
$TmpDir       = Join-Path $env:TEMP "md2all-chromium-setup"

Write-Host ""
Write-Host "  MD -> ALL — Rendering Engine Setup" -ForegroundColor Cyan
Write-Host "  Project root : $ProjectRoot"
Write-Host "  Install path : $ChromiumDir"
Write-Host ""

# ── Already installed? ───────────────────────────────────────────────────────
$ChromeExe = Join-Path $ChromiumDir "chrome.exe"
if ((Test-Path $ChromeExe) -and -not $Force) {
    $existing = (Get-Item $ChromeExe).VersionInfo.FileVersion
    Write-Host "  [OK] rendering engine already installed: $existing" -ForegroundColor Green
    Write-Host "       Use -Force to re-download."
    Write-Host ""
    # Create-DevJunctions is defined further below — dot-source inline to avoid forward-ref issue
    $targets = @(
        (Join-Path $ProjectRoot "target\debug\chromium"),
        (Join-Path $ProjectRoot "target\release\chromium")
    )
    foreach ($link in $targets) {
        $parent = Split-Path $link -Parent
        if (-not (Test-Path $parent)) { continue }
        if (Test-Path $link) {
            $existing_link = Get-Item $link
            if ($existing_link.LinkType -eq "Junction") {
                Write-Host "  [OK] Junction already exists: $link" -ForegroundColor DarkGray
                continue
            }
            Remove-Item -Force -Recurse $link
        }
        try {
            New-Item -ItemType Junction -Path $link -Target $ChromiumDir | Out-Null
            Write-Host "  [+] Junction: $link -> $ChromiumDir"
        } catch {
            Write-Host "  [!] Could not create junction at $link : $_" -ForegroundColor Yellow
        }
    }
    Write-Host ""
    Write-Host "  Setup complete (already installed)." -ForegroundColor Green
    Write-Host ""
    exit 0
}

# ── Query GitHub releases API ────────────────────────────────────────────────
Write-Host "  Querying GitHub releases API..." -ForegroundColor Yellow
$ApiUrl  = "https://api.github.com/repos/ungoogled-software/ungoogled-chromium-windows/releases/latest"
$Headers = @{ "User-Agent" = "mdall-setup/1.0" }

try {
    $Release = Invoke-RestMethod -Uri $ApiUrl -Headers $Headers -TimeoutSec 30
} catch {
    Write-Error "  Failed to query GitHub API: $_"
    exit 1
}

$TagName = $Release.tag_name
Write-Host "  Latest release: $TagName" -ForegroundColor Cyan

# Find portable x64 ZIP asset
$Asset = $Release.assets | Where-Object { $_.name -match "_windows_x64\.zip$" } | Select-Object -First 1
if (-not $Asset) {
    Write-Error "  No portable x64 ZIP found in release $TagName"
    exit 1
}

$ZipName = $Asset.name
$ZipUrl  = $Asset.browser_download_url
$ZipSize = [math]::Round($Asset.size / 1MB, 1)

Write-Host "  Asset : $ZipName ($ZipSize MB)"
Write-Host "  URL   : $ZipUrl"
Write-Host ""

# ── Download ─────────────────────────────────────────────────────────────────
if (-not (Test-Path $TmpDir)) {
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null
}
$ZipPath = Join-Path $TmpDir $ZipName

if (Test-Path $ZipPath) {
    Write-Host "  [cache] Using previously downloaded archive."
} else {
    Write-Host "  Downloading $ZipName..." -ForegroundColor Yellow
    try {
        $ProgressPreference = "SilentlyContinue"   # speeds up Invoke-WebRequest significantly
        Invoke-WebRequest -Uri $ZipUrl -OutFile $ZipPath -TimeoutSec 600
        $ProgressPreference = "Continue"
    } catch {
        Write-Error "  Download failed: $_"
        exit 1
    }
    Write-Host "  Download complete: $([math]::Round((Get-Item $ZipPath).Length / 1MB, 1)) MB on disk"
}

# ── Extract ──────────────────────────────────────────────────────────────────
Write-Host "  Extracting to $ChromiumDir ..." -ForegroundColor Yellow

# Remove old install if Force
if ((Test-Path $ChromiumDir) -and $Force) {
    Remove-Item -Recurse -Force $ChromiumDir
}

$ExtractTmp = Join-Path $TmpDir "extracted"
if (Test-Path $ExtractTmp) { Remove-Item -Recurse -Force $ExtractTmp }
New-Item -ItemType Directory -Path $ExtractTmp -Force | Out-Null

Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::ExtractToDirectory($ZipPath, $ExtractTmp)

# The ZIP may extract into a sub-folder named after the release —
# detect and unwrap one level if needed.
# Force array with @() so .Count works even when there is exactly one child.
$Children = @(Get-ChildItem $ExtractTmp)
if ($Children.Count -eq 1 -and $Children[0].PSIsContainer) {
    $Inner = $Children[0].FullName
    Write-Host "  Unwrapping sub-folder: $($Children[0].Name)"
} else {
    $Inner = $ExtractTmp
}

# Move to final location
if (-not (Test-Path $ChromiumDir)) {
    New-Item -ItemType Directory -Path $ChromiumDir -Force | Out-Null
}
Get-ChildItem $Inner | ForEach-Object {
    Move-Item $_.FullName (Join-Path $ChromiumDir $_.Name) -Force
}

# Verify
if (-not (Test-Path $ChromeExe)) {
    Write-Error "  Extraction failed — chrome.exe not found at $ChromeExe"
    exit 1
}

$Version = (Get-Item $ChromeExe).VersionInfo.FileVersion
Write-Host "  [OK] Installed: rendering engine $Version" -ForegroundColor Green

# ── Write version pin file ───────────────────────────────────────────────────
"$TagName" | Set-Content (Join-Path $ChromiumDir "VERSION.txt") -Encoding UTF8
Write-Host "  Version pinned to: $TagName"

# ── Dev junctions ────────────────────────────────────────────────────────────
function Create-DevJunctions {
    param([string]$Root, [string]$ChromeDir)

    $Targets = @(
        (Join-Path $Root "target\debug\chromium"),
        (Join-Path $Root "target\release\chromium")
    )

    foreach ($Link in $Targets) {
        $Parent = Split-Path $Link -Parent
        if (-not (Test-Path $Parent)) { continue }   # target/ sub-dir may not exist yet

        if (Test-Path $Link) {
            $Existing = Get-Item $Link
            if ($Existing.LinkType -eq "Junction" -and $Existing.Target -eq $ChromeDir) {
                Write-Host "  [OK] Junction already correct: $Link" -ForegroundColor DarkGray
                continue
            }
            Remove-Item -Force -Recurse $Link
        }

        try {
            New-Item -ItemType Junction -Path $Link -Target $ChromeDir | Out-Null
            Write-Host "  [+] Junction: $Link → $ChromeDir"
        } catch {
            Write-Host "  [!] Could not create junction at $Link : $_" -ForegroundColor Yellow
            Write-Host "      You can manually copy chromium/ into that folder."
        }
    }
}

Create-DevJunctions $ProjectRoot $ChromiumDir

# ── Cleanup temp ─────────────────────────────────────────────────────────────
Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "  Setup complete." -ForegroundColor Green
Write-Host "  The bundled rendering engine is ready at: $ChromiumDir"
Write-Host "  cargo run / cargo build will pick it up automatically."
Write-Host ""
