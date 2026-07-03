#Requires -Version 5.1
<#
.SYNOPSIS
    Pre-release verification gate for MD -> ALL.

.DESCRIPTION
    Runs every check that must pass before a release is published, in one go, and
    reports a single PASS/FAIL verdict. It is the local mirror of the CI build plus
    the checks CI does not run (the editor binary test suite, the DOCX reversibility
    guarantee, and the authorship-tell scan). Run it before tagging a release.

    Checks, in order:
      1. cargo check  (whole workspace compiles)
      2. cargo test -p mdall-core         (conversion + safety suite)
      3. cargo test --bin mdall           (editor / WYSIWYG suite - CI skips this)
      4. cargo build --release --workspace --bins   (the shipping binaries)
      5. Smoke conversion: a document with inline + display LaTeX -> HTML and DOCX,
         both non-empty, and the DOCX still carries the md-to-all-source.xml entry
         that makes the round-trip reversible (a "never break" guarantee).
      6. Authorship-tell scan over src/ and crates/ (hard fail) + an em-dash scan
         over *.rs (reported as a warning).
      7. Version report: the Cargo.toml version that a tag would publish.

    Exit code is 0 only if every hard check passes; non-zero otherwise, so it can
    gate a release script or a CI step.

.PARAMETER SkipReleaseBuild
    Skip the release build + smoke conversion (checks 4-5). Useful for a fast
    compile/test-only pass; do NOT skip before an actual release.
#>
param(
    [switch]$SkipReleaseBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Continue'   # native command failures are read via $LASTEXITCODE

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root

$results = [System.Collections.Generic.List[object]]::new()
$warnings = [System.Collections.Generic.List[string]]::new()
function Record($name, $ok, $detail = '') {
    $results.Add([pscustomobject]@{ Name = $name; Ok = [bool]$ok; Detail = $detail })
    $tag = if ($ok) { 'PASS' } else { 'FAIL' }
    $color = if ($ok) { 'Green' } else { 'Red' }
    Write-Host ("  [{0}] {1}{2}" -f $tag, $name, $(if ($detail) { " - $detail" } else { '' })) -ForegroundColor $color
}

function Invoke-Cargo($name, [string[]]$cargoArgs) {
    Write-Host "==> $name" -ForegroundColor Cyan
    & cargo @cargoArgs
    $ok = ($LASTEXITCODE -eq 0)
    Record $name $ok $(if (-not $ok) { "cargo exit $LASTEXITCODE" } else { '' })
    return $ok
}

Write-Host "MD -> ALL release verification" -ForegroundColor White
Write-Host "root: $root`n" -ForegroundColor DarkGray

# 1-3: compile + the two test suites.
$checkOk = Invoke-Cargo 'cargo check (workspace)' @('check', '--workspace', '--quiet')
$coreOk  = Invoke-Cargo 'cargo test -p mdall-core' @('test', '-p', 'mdall-core', '--quiet')
$binOk   = Invoke-Cargo 'cargo test --bin mdall'   @('test', '--bin', 'mdall', '--quiet')

# 4-5: release build + end-to-end smoke conversion (incl. DOCX reversibility).
$buildOk = $true
if (-not $SkipReleaseBuild) {
    $buildOk = Invoke-Cargo 'cargo build --release --workspace --bins' @('build', '--release', '--workspace', '--bins', '--quiet')

    if ($buildOk) {
        Write-Host "==> Smoke conversion (md -> html, docx)" -ForegroundColor Cyan
        $conv = Join-Path $root 'target/release/mdall-convert.exe'
        if (-not (Test-Path $conv)) { $conv = Join-Path $root 'target/release/mdall-convert' }
        $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("mdall-smoke-" + [System.IO.Path]::GetRandomFileName())
        New-Item -ItemType Directory -Force $tmp | Out-Null
        try {
            $md   = Join-Path $tmp 'smoke.md'
            $html = Join-Path $tmp 'smoke.html'
            $docx = Join-Path $tmp 'smoke.docx'
            "# Title`n`nInline `$E=mc^2`$ and a block:`n`n`$`$\int_0^1 x\,dx = \tfrac12`$`$`n" |
                Set-Content -Path $md -Encoding UTF8
            & $conv $md $html  | Out-Null
            $h1 = $LASTEXITCODE
            & $conv $md $docx  | Out-Null
            $h2 = $LASTEXITCODE

            $htmlOk = ($h1 -eq 0) -and (Test-Path $html) -and ((Get-Item $html).Length -gt 0)
            Record 'smoke: md -> html (non-empty)' $htmlOk

            $docxNonEmpty = ($h2 -eq 0) -and (Test-Path $docx) -and ((Get-Item $docx).Length -gt 0)
            Record 'smoke: md -> docx (non-empty)' $docxNonEmpty

            # The reversibility guarantee: the DOCX must still carry the recovery
            # source entry (legacy literal, matched on re-import).
            $hasSource = $false
            if ($docxNonEmpty) {
                Add-Type -AssemblyName System.IO.Compression.FileSystem
                $zip = [System.IO.Compression.ZipFile]::OpenRead($docx)
                try { $hasSource = @($zip.Entries | Where-Object { $_.FullName -eq 'md-to-all-source.xml' }).Count -gt 0 }
                finally { $zip.Dispose() }
            }
            Record 'docx reversibility (md-to-all-source.xml present)' $hasSource
        }
        finally {
            Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
        }
    } else {
        Record 'smoke conversion' $false 'skipped: release build failed'
    }
} else {
    Write-Host "==> Release build + smoke conversion SKIPPED (-SkipReleaseBuild)`n" -ForegroundColor DarkYellow
}

# 6: authorship-tell scan (hard fail) + em-dash scan (warning).
Write-Host "==> Authorship-tell scan (src + crates)" -ForegroundColor Cyan
$pattern = 'claude|anthropic|co-authored|openai|\bGPT\b'
$scan = Get-ChildItem -Path (Join-Path $root 'src'), (Join-Path $root 'crates') -Recurse -File -Include *.rs, *.toml, *.md -ErrorAction SilentlyContinue |
    Select-String -Pattern $pattern -List -ErrorAction SilentlyContinue
$tellsOk = -not $scan
Record 'no authorship tells in source' $tellsOk
if ($scan) { $scan | ForEach-Object { Write-Host "      $($_.Path):$($_.LineNumber)" -ForegroundColor Red } }

$emdash = Get-ChildItem -Path (Join-Path $root 'src'), (Join-Path $root 'crates') -Recurse -File -Include *.rs -ErrorAction SilentlyContinue |
    Select-String -Pattern ([char]0x2014) -List -ErrorAction SilentlyContinue
if ($emdash) {
    $warnings.Add("em-dash (U+2014) found in $($emdash.Count) .rs file(s); the project style uses '-' / '->'")
    $emdash | ForEach-Object { Write-Host "      em-dash: $($_.Path):$($_.LineNumber)" -ForegroundColor DarkYellow }
}

# 7: version report (what a tag would publish).
$verMatch = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
$ver = if ($verMatch) { $verMatch.Matches.Groups[1].Value } else { '(unknown)' }

# Summary.
Write-Host "`n--- summary ---" -ForegroundColor White
$failed = @($results | Where-Object { -not $_.Ok })
foreach ($w in $warnings) { Write-Host "  WARN: $w" -ForegroundColor DarkYellow }
Write-Host ("  version to publish: {0}" -f $ver) -ForegroundColor Cyan

Pop-Location

if ($failed.Count -gt 0) {
    Write-Host ("`nVERIFY: FAIL ({0} check(s) failed)" -f $failed.Count) -ForegroundColor Red
    exit 1
} else {
    Write-Host "`nVERIFY: PASS - all release checks green" -ForegroundColor Green
    exit 0
}
