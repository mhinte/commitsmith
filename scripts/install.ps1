#!/usr/bin/env pwsh
# Install coggit for the current user and make it reachable everywhere.
#
#   iwr https://raw.githubusercontent.com/mhinte/coggit/main/scripts/install.ps1 | iex
#   .\install.ps1 -Binary .\coggit.exe -Alias cog
#
# Installs to %LOCALAPPDATA%\Programs\coggit, adds that directory to the
# user PATH (no admin rights needed) and points a PowerShell alias at it.
[CmdletBinding()]
param(
    # Existing binary to install. Without it the latest release is downloaded.
    [string]$Binary,
    # Extra command name, e.g. "cog" for a short alias. Optional.
    [string]$Alias,
    # Release tag to download, e.g. v0.2.0. Defaults to the latest release.
    [string]$Tag
)

$ErrorActionPreference = 'Stop'
$repo = "mhinte/coggit"
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\coggit'
$target = Join-Path $installDir 'coggit.exe'

function Write-Step($message) { Write-Host "  $message" -ForegroundColor Cyan }
function Write-Ok($message) { Write-Host "  $message" -ForegroundColor Green }

# --- 1. get the binary -------------------------------------------------------
if ($Binary) {
    if (-not (Test-Path $Binary)) { throw "binary not found: $Binary" }
    Write-Step "using $Binary"
    $source = (Resolve-Path $Binary).Path
} else {
    $api = if ($Tag) { "https://api.github.com/repos/$repo/releases/tags/$Tag" }
           else { "https://api.github.com/repos/$repo/releases/latest" }
    Write-Step "asking $api"
    $release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'coggit-installer' }

    # Windows builds are zip archives in the release; fall back to nothing so a
    # missing asset fails with a clear message instead of a 404 later.
    $asset = $release.assets | Where-Object { $_.name -like '*windows*' -and $_.name -like '*.zip' } | Select-Object -First 1
    if (-not $asset) {
        throw "release $($release.tag_name) has no Windows archive yet; build it with 'cargo build --release' and pass -Binary"
    }

    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("coggit-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $tmp | Out-Null
    $zip = Join-Path $tmp $asset.name
    Write-Step "downloading $($asset.name)"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
    Expand-Archive -Path $zip -DestinationPath $tmp -Force

    $found = Get-ChildItem -Path $tmp -Recurse -Filter 'coggit.exe' | Select-Object -First 1
    if (-not $found) { throw "the archive did not contain coggit.exe" }
    $source = $found.FullName
}

# --- 2. copy it into place ---------------------------------------------------
New-Item -ItemType Directory -Path $installDir -Force | Out-Null
Copy-Item -Path $source -Destination $target -Force
Write-Ok "installed $target"

# Short alias as a copy, so cmd, PowerShell and Windows Terminal all find it.
if ($Alias) {
    $aliasPath = Join-Path $installDir "$Alias.exe"
    Copy-Item -Path $target -Destination $aliasPath -Force
    Write-Ok "alias $aliasPath"
}

# --- 3. put the directory on the user PATH -----------------------------------
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not $userPath) { $userPath = '' }
$entries = $userPath -split ';' | Where-Object { $_ }
if ($entries -notcontains $installDir) {
    $newPath = (@($entries) + $installDir) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Ok "added $installDir to your user PATH"
    Write-Host "  open a new terminal so the change takes effect" -ForegroundColor Yellow
} else {
    Write-Ok "$installDir is already on your user PATH"
}

# --- 4. PowerShell alias, only when a real alias name was given --------------
# `iwr ... | iex` sets $PSCommandPath to nothing, so writing to $PROFILE there
# would be surprising; that path is opt-in via -Alias.
if (-not $Alias) {
    & $target --version
    Write-Host ""
    Write-Host "Done. coggit is on your user PATH; open a new terminal." -ForegroundColor Green
    return
}

$profileDir = Split-Path -Parent $PROFILE.CurrentUserAllHosts
if (-not (Test-Path $profileDir)) { New-Item -ItemType Directory -Path $profileDir -Force | Out-Null }
$marker = '# >>> coggit >>>'
if (-not (Test-Path $PROFILE.CurrentUserAllHosts) -or
    -not (Select-String -Path $PROFILE.CurrentUserAllHosts -SimpleMatch $marker -Quiet)) {
    $lines = @(
        '',
        $marker,
        "Set-Alias -Name $Alias -Value '$target' -Scope Global -Force -ErrorAction SilentlyContinue",
        '# <<< coggit <<<'
    )
    Add-Content -Path $PROFILE.CurrentUserAllHosts -Value $lines
    Write-Ok "added the $Alias alias to $PROFILE.CurrentUserAllHosts"
}

& $target --version
Write-Host ""
Write-Host "Done. Try: $Alias --version" -ForegroundColor Green
