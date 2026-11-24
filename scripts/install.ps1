# Install commitsmith for the current user, with as little PowerShell as possible.
#
#   iwr https://raw.githubusercontent.com/mhinte/commitsmith/main/scripts/install.ps1 -OutFile install.ps1
#   .\install.ps1                       # latest release
#   .\install.ps1 -Binary .\commitsmith.exe -Alias smith
#
# The binary does the real work via `commitsmith --install`: it copies itself to
# %LOCALAPPDATA%\Programs\commitsmith, adds that directory to the *user* PATH and
# creates the optional short command. No admin rights, no profile edits.
[CmdletBinding()]
param(
    # Existing binary to install. Without it the latest release is downloaded.
    [string]$Binary,
    # Optional second command name, e.g. "smith".
    [string]$Alias,
    # Release tag, e.g. v0.3.0. Defaults to the latest release.
    [string]$Tag
)

$ErrorActionPreference = 'Stop'
$owner = 'mhinte'
$repo = 'commitsmith'

$tempDir = $null
try {
    # --- 1. find a binary ----------------------------------------------------
    if ($Binary) {
        if (-not (Test-Path $Binary)) { throw "binary not found: $Binary" }
        $exe = (Resolve-Path $Binary).Path
    }
    else {
        $api = if ($Tag) { "https://api.github.com/repos/$owner/$repo/releases/tags/$Tag" }
               else { "https://api.github.com/repos/$owner/$repo/releases/latest" }
        Write-Host "  asking $api" -ForegroundColor Cyan
        $release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'commitsmith-installer' }

        $asset = $release.assets | Where-Object { $_.name -like '*windows*' -and $_.name -like '*.zip' } | Select-Object -First 1
        if (-not $asset) { throw "release $($release.tag_name) has no Windows archive" }

        $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
        $zip = Join-Path $tempDir $asset.name
        Write-Host "  downloading $($asset.name)" -ForegroundColor Cyan
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
        Expand-Archive -Path $zip -DestinationPath $tempDir -Force

        $found = Get-ChildItem -Path $tempDir -Recurse -Filter 'commitsmith.exe' | Select-Object -First 1
        if (-not $found) { throw "the archive did not contain commitsmith.exe" }
        $exe = $found.FullName
    }

    # --- 2. let the binary install itself ------------------------------------
    $installArgs = @('--install')
    if ($Alias) { $installArgs += @('--alias', $Alias) }
    & $exe @installArgs
    if ($LASTEXITCODE -ne 0) { throw "commitsmith --install failed with exit code $LASTEXITCODE" }
}
finally {
    if ($tempDir -and (Test-Path $tempDir)) { Remove-Item -Recurse -Force $tempDir }
}

Write-Host ''
Write-Host 'Done. Open a new terminal, then run: commitsmith --version' -ForegroundColor Green
