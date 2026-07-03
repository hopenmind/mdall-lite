#Requires -Version 5.1
<#
.SYNOPSIS
    Builds the self-extracting MD -> ALL installer.

.DESCRIPTION
    1. Builds mdall.exe (release)
    2. Builds the installer stub (release)
    3. Strips chromium/ to headless-PDF-only files
    4. Zips the payload (mdall.exe + stripped chromium/)
    5. Appends the ZIP to the stub exe with an 8-byte magic trailer
    -> Output: dist/mdall-<version>-<arch>-installer.exe

.PARAMETER SkipBuild
    Skip cargo build steps (use existing binaries).

.PARAMETER Arch
    Target triple for cargo-zigbuild (default: x86_64-pc-windows-msvc).
    Examples:
      x86_64-pc-windows-msvc   (Intel/AMD 64-bit)
      aarch64-pc-windows-msvc  (ARM64)
      i686-pc-windows-msvc     (32-bit legacy)
    Requires: cargo install cargo-zigbuild && rustup target add <triple>
#>
param(
    [switch]$SkipBuild,
    [string]$Arch = "x86_64-pc-windows-msvc"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Bundle a default spell-check dictionary (en_US) + its license into the payload,
# so the installed app has spell checking working out of the box.
function Get-DefaultDictionary {
    param([string]$DictDir)
    New-Item -ItemType Directory -Force $DictDir | Out-Null
    $base = 'https://raw.githubusercontent.com/wooorm/dictionaries/main/dictionaries/en'
    $files = @{
        'en_US.dic'         = "$base/index.dic"
        'en_US.aff'         = "$base/index.aff"
        'en_US.license.txt' = "$base/license"
    }
    foreach ($name in $files.Keys) {
        $dest = Join-Path $DictDir $name
        if (-not (Test-Path $dest)) {
            try {
                $ProgressPreference = "SilentlyContinue"
                Invoke-WebRequest -Uri $files[$name] -OutFile $dest -TimeoutSec 60
                $ProgressPreference = "Continue"
            } catch {
                Write-Host "  [!] dictionary fetch failed: $name ($_)" -ForegroundColor Yellow
            }
        }
    }
}

$ProjectRoot  = Split-Path $PSScriptRoot -Parent
$ChromiumDir  = Join-Path $ProjectRoot "chromium"
$DistDir      = Join-Path $ProjectRoot "dist"
$TmpDir       = Join-Path $env:TEMP "md2all-installer-build"

# Read version from Cargo.toml
$CargoToml = Get-Content (Join-Path $ProjectRoot "Cargo.toml") -Raw
$Version   = if ($CargoToml -match 'version\s*=\s*"([^"]+)"') { $Matches[1] } else { "unknown" }

# Derive short arch label for filename (e.g. x86_64-pc-windows-msvc → x64)
$ArchLabel = switch -Wildcard ($Arch) {
    "x86_64*"  { "x64" }
    "aarch64*" { "arm64" }
    "i686*"    { "x86" }
    default    { $Arch }
}

# cargo-zigbuild puts output under target/<triple>/release/ when a target is specified
$TargetSubdir = if ($Arch -eq "x86_64-pc-windows-msvc") { "release" } else { "$Arch\release" }
$AppExe       = Join-Path $ProjectRoot "target\$TargetSubdir\mdall.exe"
$StubExe      = Join-Path $ProjectRoot "installer\target\$TargetSubdir\installer.exe"
$OutputExe    = Join-Path $DistDir "mdall-$Version-$ArchLabel-installer.exe"

Write-Host ""
Write-Host "  MD -> ALL Installer Builder v$Version ($ArchLabel / $Arch)" -ForegroundColor Cyan
Write-Host "  Project root : $ProjectRoot"
Write-Host "  Output       : $OutputExe"
Write-Host ""

# ── Preflight ──────────────────────────────────────────────────────────────────
$ChromeExe = Join-Path $ChromiumDir "chrome.exe"
if (-not (Test-Path $ChromeExe)) {
    Write-Host "  [!] chromium/chrome.exe not found." -ForegroundColor Red
    Write-Host "      Run: .\scripts\setup-chromium.ps1" -ForegroundColor Yellow
    exit 1
}

# ── Build ──────────────────────────────────────────────────────────────────────
if (-not $SkipBuild) {
    # Use cargo-zigbuild for cross-arch support; falls back to plain cargo for native arch
    $useZig = $null -ne (Get-Command "cargo-zigbuild" -ErrorAction SilentlyContinue)
    $buildCmd = if ($useZig) { "cargo zigbuild" } else { "cargo build" }
    $targetFlag = if ($Arch -ne "x86_64-pc-windows-msvc" -or $useZig) { "--target $Arch" } else { "" }

    Write-Host "  Build tool: $buildCmd $targetFlag" -ForegroundColor DarkGray

    Write-Host "  Building mdall (release)..." -ForegroundColor Yellow
    Push-Location $ProjectRoot
    try {
        $cmd = "$buildCmd --release $targetFlag".Trim()
        Invoke-Expression $cmd
        if ($LASTEXITCODE -ne 0) { throw "Build failed: $cmd" }
    } finally { Pop-Location }

    Write-Host "  Building installer stub (release)..." -ForegroundColor Yellow
    Push-Location (Join-Path $ProjectRoot "installer")
    try {
        $cmd = "$buildCmd --release $targetFlag".Trim()
        Invoke-Expression $cmd
        if ($LASTEXITCODE -ne 0) { throw "Installer build failed: $cmd" }
    } finally { Pop-Location }

    Write-Host "  Build OK" -ForegroundColor Green
} else {
    foreach ($f in @($AppExe, $StubExe)) {
        if (-not (Test-Path $f)) { throw "-SkipBuild set but $f missing" }
    }
    Write-Host "  [skip] Using existing binaries."
}

# ── Strip Chromium ─────────────────────────────────────────────────────────────
Write-Host "  Stripping chromium to headless-PDF-only set..." -ForegroundColor Yellow

# Files needed for headless CDP PDF printing on Windows
$KeepFiles = @(
    "chrome.exe",
    "chrome.dll",
    "chrome_elf.dll",
    "resources.pak",
    "chrome_100_percent.pak",
    "chrome_200_percent.pak",
    "icudtl.dat",
    "v8_context_snapshot.bin",
    "snapshot_blob.bin",
    "libGLESv2.dll",
    "libEGL.dll",
    "vk_swiftshader.dll",
    "vk_swiftshader_icd.json",
    "vulkan-1.dll",
    "d3dcompiler_47.dll",
    "dxcompiler.dll",
    "dxil.dll",
    "VERSION.txt"
)

$StrippedDir = Join-Path $TmpDir "chromium-stripped"
if (Test-Path $StrippedDir) { Remove-Item -Recurse -Force $StrippedDir }
New-Item -ItemType Directory -Force $StrippedDir | Out-Null

# Copy kept root files
foreach ($f in $KeepFiles) {
    $src = Join-Path $ChromiumDir $f
    if (Test-Path $src) {
        Copy-Item $src (Join-Path $StrippedDir $f)
    }
}

# Copy only en-US locale
$LocalesOut = Join-Path $StrippedDir "locales"
New-Item -ItemType Directory -Force $LocalesOut | Out-Null
$enUS = Join-Path $ChromiumDir "locales\en-US.pak"
if (Test-Path $enUS) { Copy-Item $enUS (Join-Path $LocalesOut "en-US.pak") }

$strippedMB = [math]::Round((Get-ChildItem $StrippedDir -Recurse | Measure-Object -Property Length -Sum).Sum / 1MB, 1)
Write-Host "  Stripped chromium: $strippedMB MB (was $([math]::Round((Get-ChildItem $ChromiumDir -Recurse | Measure-Object -Property Length -Sum).Sum/1MB,1)) MB)"

# ── Build ZIP payload ──────────────────────────────────────────────────────────
Write-Host "  Creating payload ZIP..." -ForegroundColor Yellow
New-Item -ItemType Directory -Force $TmpDir | Out-Null
$ZipPath = Join-Path $TmpDir "payload.zip"
if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }

# Stage dir: mdall.exe + engine/ (the installer extracts engine/ to a private
# per-user folder, so it is never placed next to the application).
$StageDir = Join-Path $TmpDir "stage"
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force $StageDir | Out-Null

Copy-Item $AppExe $StageDir
Copy-Item -Recurse $StrippedDir (Join-Path $StageDir "engine")

# Default dictionary + license (spell check works out of the box).
Write-Host "  Bundling default en_US dictionary + license..." -ForegroundColor Yellow
Get-DefaultDictionary (Join-Path $StageDir "dictionaries")

Add-Type -AssemblyName System.IO.Compression.FileSystem
$ProgressPreference = "SilentlyContinue"
[System.IO.Compression.ZipFile]::CreateFromDirectory($StageDir, $ZipPath)
$ProgressPreference = "Continue"

$zipMB = [math]::Round((Get-Item $ZipPath).Length / 1MB, 1)
Write-Host "  Payload ZIP: $zipMB MB"

# ── Assemble installer exe ─────────────────────────────────────────────────────
Write-Host "  Assembling installer exe..." -ForegroundColor Yellow

New-Item -ItemType Directory -Force $DistDir | Out-Null
if (Test-Path $OutputExe) { Remove-Item $OutputExe -Force }

# Read stub and zip as byte arrays
$stubBytes = [System.IO.File]::ReadAllBytes($StubExe)
$zipBytes  = [System.IO.File]::ReadAllBytes($ZipPath)

# Trailer: magic(8) + zip_size(8 bytes LE)
$magic    = [System.Text.Encoding]::ASCII.GetBytes("MD2ALLST")
$zipSize  = [System.BitConverter]::GetBytes([uint64]$zipBytes.Length)

# Concatenate: stub + zip + magic + zip_size
$outStream = [System.IO.File]::OpenWrite($OutputExe)
try {
    $outStream.Write($stubBytes, 0, $stubBytes.Length)
    $outStream.Write($zipBytes,  0, $zipBytes.Length)
    $outStream.Write($magic,     0, $magic.Length)
    $outStream.Write($zipSize,   0, $zipSize.Length)
} finally {
    $outStream.Close()
}

$finalMB = [math]::Round((Get-Item $OutputExe).Length / 1MB, 1)
Write-Host "  Installer ready: $finalMB MB" -ForegroundColor Green
Write-Host "  -> $OutputExe" -ForegroundColor Cyan

# ── Cleanup ────────────────────────────────────────────────────────────────────
Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "  Done. Distribute: $OutputExe" -ForegroundColor Green
Write-Host "  Users run this single exe — everything extracts and launches automatically." -ForegroundColor DarkGray
Write-Host ""
