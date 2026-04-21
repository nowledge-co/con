# con — Windows terminal emulator installer
# Usage:  irm https://con-releases.nowledge.co/install.ps1 | iex

#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$Repo        = 'nowledge-co/con-terminal'
# Install under Programs\con-terminal, not Programs\con. CON is a
# reserved DOS device name: New-Item reports success on such a path,
# but the Win32 namespace redirects CON to the console device so
# Test-Path and Expand-Archive can never see the directory. Using
# con-terminal as the folder segment sidesteps the entire trap.
$InstallRoot = Join-Path $env:LOCALAPPDATA 'Programs\con-terminal'

# ── Terminal setup ──────────────────────────────────────────────────────────
# UTF-8 output is required for the block-drawing glyphs in the banner
# (U+2580/2584/2588). Modern Windows Terminal defaults to UTF-8, but
# conhost under powershell.exe often still falls back to a legacy
# codepage and renders "?" in place of `█`/`▀`/`▄`.
#
# VT processing (ENABLE_VIRTUAL_TERMINAL_PROCESSING) is the other half:
# Win10 1809+ conhost supports it, but the flag isn't always set when
# PowerShell is launched non-interactively (e.g. via `irm | iex` piped
# from an elevated prompt). Force it on so our ANSI colors render.

try { [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new() } catch {}
try { $OutputEncoding             = [System.Text.UTF8Encoding]::new() } catch {}

if (-not ('Native.ConsoleMode' -as [type])) {
    try {
        Add-Type -Namespace Native -Name ConsoleMode -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError=true)]
public static extern System.IntPtr GetStdHandle(int nStdHandle);
[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError=true)]
public static extern bool GetConsoleMode(System.IntPtr hConsoleHandle, out uint lpMode);
[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError=true)]
public static extern bool SetConsoleMode(System.IntPtr hConsoleHandle, uint dwMode);
'@ -ErrorAction SilentlyContinue
    } catch {}
}
try {
    $hOut = [Native.ConsoleMode]::GetStdHandle(-11)
    $mode = 0
    if ([Native.ConsoleMode]::GetConsoleMode($hOut, [ref]$mode)) {
        [Native.ConsoleMode]::SetConsoleMode($hOut, $mode -bor 0x0004) | Out-Null
    }
} catch {}

# ── Colors ──────────────────────────────────────────────────────────────────
# Palette mirrors install.sh on macOS: success green, dim gray, error
# red, and an 8-stop truecolor gradient blue → purple → pink for the
# banner. Truecolor is preferred over 256-color fallback because every
# terminal that can render the half-block glyphs also handles it.

$ESC   = [char]27
$RESET = "$ESC[0m"
$BOLD  = "$ESC[1m"
$OK    = "$ESC[38;2;0;210;160m"
$DIM   = "$ESC[38;2;140;150;175m"
$ERR   = "$ESC[38;2;230;57;70m"

# Gradient stops: #4ea8ff → #a855f7 → #ec4899, interpolated linearly.
$G0 = "$ESC[38;2;78;168;255m"
$G1 = "$ESC[38;2;104;144;253m"
$G2 = "$ESC[38;2;129;121;250m"
$G3 = "$ESC[38;2;155;97;248m"
$G4 = "$ESC[38;2;178;83;234m"
$G5 = "$ESC[38;2;197;79;207m"
$G6 = "$ESC[38;2;217;76;180m"
$G7 = "$ESC[38;2;236;72;153m"

# `u{XXXX} escape syntax is PS 6+ only — stock Windows 11 ships PS 5.1
# where those show up as literal text. Cast via [char] instead.
$CHK   = [char]0x2713  # ✓
$CROSS = [char]0x2717  # ✗
$MID   = [char]0x00B7  # ·

function Pass($msg) { Write-Host "   $OK$CHK$RESET  $msg" }
function Fail($msg) { Write-Host "   $ERR$CROSS$RESET  $msg" -ForegroundColor Red; exit 1 }

# ── Banner ──────────────────────────────────────────────────────────────────
# Exact output from: npx oh-my-logo "con" --palette-colors
# "#4ea8ff,#a855f7,#ec4899" --filled --block-font tiny --color
# Re-rendered through 8 truecolor stops for a smooth gradient.

$FULL = [char]0x2588  # █
$UP   = [char]0x2580  # ▀
$DN   = [char]0x2584  # ▄
$HL   = [char]0x2501  # ━

Write-Host ''
Write-Host "   $G0$FULL$UP$G1$UP$G2 $FULL$UP$G3$FULL$G4 $FULL$G5$DN$G6 $G7$FULL$RESET"
Write-Host "   $G0$FULL$DN$G1$DN$G2 $FULL$DN$G3$FULL$G4 $FULL$G5 $G6$UP$G7$FULL$RESET"
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
    Pass "$BOLD`con $channel$RESET  $DIM$version $MID $arch$RESET"
} else {
    Pass "$BOLD`con$RESET  $DIM$version $MID $arch$RESET"
}

# ── Download ────────────────────────────────────────────────────────────────

$tmp = New-Item -ItemType Directory -Force `
    -Path (Join-Path $env:TEMP "con-install-$([System.IO.Path]::GetRandomFileName())")
$zipPath = Join-Path $tmp $zipAsset.name

Write-Host -NoNewline "   $DIM$MID$RESET  downloading"
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
        Write-Host "   $DIM$MID$RESET  checksum unavailable, skipping verify" -ForegroundColor Yellow
    }
}

# ── Install ─────────────────────────────────────────────────────────────────

Write-Host -NoNewline "   $DIM$MID$RESET  installing"

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
# con.lnk would trip the reserved-name rule the same way. The Start
# Menu shows the filename without the .lnk suffix, so "Con Terminal"
# is what the user sees.
$lnk       = Join-Path $startMenu 'Con Terminal.lnk'
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
Write-Host "   $G0$HL$HL$G1$HL$HL$G2$HL$HL$G3$HL$HL$G4$HL$HL$G5$HL$HL$G6$HL$HL$G7$HL$HL$RESET"
Write-Host ''

try {
    Start-Process -FilePath $exePath
    Pass 'launched — enjoy!'
} catch {
    # Non-fatal: user can launch from Start Menu or by typing `con-app`.
}

Write-Host ''
