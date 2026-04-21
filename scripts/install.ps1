# con — Windows terminal emulator installer
# Usage:  irm https://con-releases.nowledge.co/install.ps1 | iex

#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$Repo        = 'nowledge-co/con-terminal'
$InstallRoot = Join-Path $env:LOCALAPPDATA 'Programs\con'

# ── Colors ──────────────────────────────────────────────────────────────────
# Matches install.sh palette: success green, dim gray, error red,
# gradient blue→purple→pink for the banner. True-color ANSI works on
# PowerShell 5.1+ conhost (Win10 1809+) and on Windows Terminal.

$ESC   = [char]27
$RESET = "$ESC[0m"
$BOLD  = "$ESC[1m"
$OK    = "$ESC[38;2;0;210;160m"
$DIM   = "$ESC[38;2;140;150;175m"
$ERR   = "$ESC[38;2;230;57;70m"

function Pass($msg) { Write-Host "   $OK`u{2713}$RESET  $msg" }
function Fail($msg) { Write-Host "   $ERR`u{2717}$RESET  $msg" -ForegroundColor Red; exit 1 }

# ── Banner ──────────────────────────────────────────────────────────────────

Write-Host ''
Write-Host "   $ESC[38;5;111m`u{2588}`u{2580}$ESC[38;5;105m`u{2580}$ESC[38;5;141m `u{2588}`u{2580}$ESC[38;5;177m`u{2588}$ESC[38;5;176m `u{2588}$ESC[38;5;170m`u{2584}$ESC[38;5;169m $ESC[38;5;205m`u{2588}$RESET"
Write-Host "   $ESC[38;5;111m`u{2588}`u{2584}$ESC[38;5;105m`u{2584}$ESC[38;5;141m `u{2588}`u{2584}$ESC[38;5;177m`u{2588}$ESC[38;5;176m `u{2588}$ESC[38;5;170m $ESC[38;5;169m`u{2580}$ESC[38;5;205m`u{2588}$RESET"
Write-Host ''

# ── Preflight ───────────────────────────────────────────────────────────────

if ($env:OS -ne 'Windows_NT') { Fail 'con requires Windows' }

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x86_64' }
    'ARM64' { Fail 'Windows on ARM64 is not yet supported (tracker: #34)' }
    default { Fail "unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
}

# ── Resolve ─────────────────────────────────────────────────────────────────

try {
    $release = Invoke-RestMethod `
        -Uri "https://api.github.com/repos/$Repo/releases/latest" `
        -Headers @{ 'User-Agent' = 'con-installer' } `
        -UseBasicParsing
} catch {
    Fail 'could not reach GitHub'
}

$tag     = $release.tag_name
$version = $tag -replace '^v', ''

$channel = switch -Regex ($version) {
    '-beta\.' { 'Beta' }
    '-dev\.'  { 'Dev' }
    default   { '' }
}

$zipAsset = $release.assets |
    Where-Object { $_.name -like "*-windows-$arch.zip" } |
    Select-Object -First 1

if (-not $zipAsset) { Fail "no ZIP found for windows-$arch" }

$sumAsset = $release.assets |
    Where-Object { $_.name -eq 'SHA256SUMS-windows.txt' } |
    Select-Object -First 1

if ($channel) {
    Pass "$BOLD`con $channel$RESET  $DIM$version `u{00b7} $arch$RESET"
} else {
    Pass "$BOLD`con$RESET  $DIM$version `u{00b7} $arch$RESET"
}

# ── Download ────────────────────────────────────────────────────────────────

$tmp = New-Item -ItemType Directory -Force `
    -Path (Join-Path $env:TEMP "con-install-$([System.IO.Path]::GetRandomFileName())")
$zipPath = Join-Path $tmp $zipAsset.name

Write-Host -NoNewline "   $DIM`u{00b7}$RESET  downloading"
try {
    # Invoke-WebRequest is ~10x slower than raw WebClient on large files
    # because it streams through PowerShell's progress pipeline; use
    # WebClient directly for the binary download.
    (New-Object System.Net.WebClient).DownloadFile($zipAsset.browser_download_url, $zipPath)
} catch {
    Write-Host "`r$(' ' * 80)`r" -NoNewline
    Fail 'download failed'
}
$sizeMB = '{0:N1}M' -f ($zipAsset.size / 1MB)
Write-Host "`r$(' ' * 40)`r" -NoNewline
Pass "downloaded  $DIM$sizeMB$RESET"

# ── Verify ──────────────────────────────────────────────────────────────────
# Sparkle signs the ZIP for in-app update verification; the SHA256SUMS
# file is the integrity guarantee for first-install. If it's missing
# (older releases or manual upload), warn but don't abort.

if ($sumAsset) {
    $sumsPath = Join-Path $tmp 'SHA256SUMS-windows.txt'
    try {
        (New-Object System.Net.WebClient).DownloadFile($sumAsset.browser_download_url, $sumsPath)
        $expected = (Select-String -Path $sumsPath -Pattern $zipAsset.name |
            Select-Object -First 1).Line -split '\s+' | Select-Object -First 1
        $actual = (Get-FileHash -Algorithm SHA256 $zipPath).Hash.ToLower()
        if ($expected -and ($actual -ne $expected.ToLower())) {
            Fail "checksum mismatch (expected $expected, got $actual)"
        }
        Pass "verified  $DIM`sha256$RESET"
    } catch {
        Write-Host "   $DIM`u{00b7}$RESET  checksum unavailable — skipping verify" -ForegroundColor Yellow
    }
}

# ── Install ─────────────────────────────────────────────────────────────────

Write-Host -NoNewline "   $DIM`u{00b7}$RESET  installing"

# Kill any running instance so we can overwrite the exe. `Stop-Process`
# is idempotent when the process isn't running.
Get-Process -Name 'con-app' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

if (Test-Path $InstallRoot) {
    Remove-Item -Path $InstallRoot -Recurse -Force -ErrorAction SilentlyContinue
}
New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null

# Expand-Archive creates a single top-level folder if the ZIP has one;
# our packager stages files flat so extract directly into $InstallRoot.
Expand-Archive -Path $zipPath -DestinationPath $InstallRoot -Force

$exePath = Join-Path $InstallRoot 'con-app.exe'
if (-not (Test-Path $exePath)) {
    Fail 'con-app.exe missing from archive'
}

Write-Host "`r$(' ' * 40)`r" -NoNewline
Pass "installed  $DIM$InstallRoot$RESET"

# ── PATH (HKCU) ─────────────────────────────────────────────────────────────
# Persist in the user Environment registry hive so new shells pick it
# up. Current session gets the updated PATH too via `[Environment]::...`.

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not $userPath) { $userPath = '' }
$pathSegments = $userPath -split ';' | Where-Object { $_ -and ($_ -ne $InstallRoot) }
if ($pathSegments -notcontains $InstallRoot) {
    $newPath = (@($pathSegments) + $InstallRoot) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    $env:Path = "$env:Path;$InstallRoot"
    Pass "added to PATH  $DIM(new shells)$RESET"
}

# ── Start Menu shortcut ─────────────────────────────────────────────────────

$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$lnk       = Join-Path $startMenu 'con.lnk'
try {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($lnk)
    $shortcut.TargetPath = $exePath
    $shortcut.WorkingDirectory = $InstallRoot
    $shortcut.IconLocation = "$exePath,0"
    $shortcut.Save()
} catch {
    # Start Menu entry is nice-to-have, not critical — silent on failure.
}

# ── Launch ──────────────────────────────────────────────────────────────────

Write-Host ''
Write-Host "   $ESC[38;5;111m`u{2501}`u{2501}$ESC[38;5;105m`u{2501}`u{2501}$ESC[38;5;141m`u{2501}`u{2501}$ESC[38;5;177m`u{2501}`u{2501}$ESC[38;5;176m`u{2501}`u{2501}$ESC[38;5;170m`u{2501}`u{2501}$ESC[38;5;169m`u{2501}`u{2501}$ESC[38;5;205m`u{2501}`u{2501}$RESET"
Write-Host ''

try {
    Start-Process -FilePath $exePath
    Pass 'launched — enjoy!'
} catch {
    # Non-fatal: user can launch from Start Menu or by typing `con-app`.
}

Write-Host ''
