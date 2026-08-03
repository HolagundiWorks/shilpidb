<#
.SYNOPSIS
    ShilpiDB installer for Windows.

.DESCRIPTION
    Downloads the shilpid + shilpi binaries for x86_64 Windows from the
    latest (or -Version-pinned) GitHub Release and installs them to
    -InstallDir (default: $env:LOCALAPPDATA\ShilpiDB\bin), adding that
    directory to the current user's PATH.

.EXAMPLE
    irm https://raw.githubusercontent.com/HolagundiWorks/shilpidb/main/scripts/install.ps1 | iex

.EXAMPLE
    # Pin a version and/or install directory:
    $env:SHILPIDB_VERSION = "v0.1.1"
    irm .../install.ps1 | iex
#>

[CmdletBinding()]
param(
    [string]$Version = $env:SHILPIDB_VERSION,
    [string]$InstallDir = $(if ($env:SHILPIDB_INSTALL_DIR) { $env:SHILPIDB_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "ShilpiDB\bin" })
)

$ErrorActionPreference = "Stop"
$Repo = "HolagundiWorks/shilpidb"
$Target = "x86_64-pc-windows-msvc"

if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64" -and $env:PROCESSOR_ARCHITEW6432 -ne "AMD64") {
    Write-Warning "Only x86_64 Windows binaries are published today; this may not run on $env:PROCESSOR_ARCHITECTURE (Windows' x64 emulation may still work on ARM64)."
}

if (-not $Version) {
    Write-Host "Looking up the latest release..."
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
    $Version = $release.tag_name
    if (-not $Version) {
        throw "could not determine the latest release tag from the GitHub API"
    }
}

$archive = "shilpidb-$Version-$Target.zip"
$url = "https://github.com/$Repo/releases/download/$Version/$archive"

Write-Host "Installing ShilpiDB $Version ($Target) to $InstallDir..."

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmpDir | Out-Null
try {
    $archivePath = Join-Path $tmpDir $archive
    try {
        Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing
    } catch {
        throw "download failed: $url ($_)"
    }

    Expand-Archive -Path $archivePath -DestinationPath $tmpDir -Force

    $extractedDir = Join-Path $tmpDir "shilpidb-$Version-$Target"
    $shilpidExe = Join-Path $extractedDir "shilpid.exe"
    $shilpiExe = Join-Path $extractedDir "shilpi.exe"
    if (-not (Test-Path $shilpidExe) -or -not (Test-Path $shilpiExe)) {
        throw "archive didn't contain shilpid.exe/shilpi.exe as expected"
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item $shilpidExe (Join-Path $InstallDir "shilpid.exe") -Force
    Copy-Item $shilpiExe (Join-Path $InstallDir "shilpi.exe") -Force
} finally {
    Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "Installed:"
Write-Host "  $InstallDir\shilpid.exe"
Write-Host "  $InstallDir\shilpi.exe"
Write-Host ""

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$pathEntries = $userPath -split ";" | Where-Object { $_ -ne "" }
if ($pathEntries -notcontains $InstallDir) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
    Write-Host "Added $InstallDir to your user PATH. Restart your terminal for it to take effect."
} else {
    & (Join-Path $InstallDir "shilpid.exe") --version
    & (Join-Path $InstallDir "shilpi.exe") --version
}

Write-Host ""
Write-Host "Run 'shilpid --help' to start the server and 'shilpi --help' for the shell."
