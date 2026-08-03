# Build shilpid and shilpi, then stage them as Tauri sidecar binaries under
# gui/src-tauri/binaries/, named per Tauri's <name>-<target-triple>.exe
# convention. Run this before `cargo tauri dev` / `cargo tauri build` — Tauri
# does not build these itself, it only bundles whatever's already there.
#
# Usage: powershell -File scripts/prepare-sidecars.ps1 [-Release]

param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$BuildProfile = if ($Release) { "release" } else { "debug" }
$CargoArgs = @("build", "--workspace", "--bins")
if ($Release) { $CargoArgs += "--release" }

Write-Host "Building shilpid and shilpi ($BuildProfile)..."
& cargo @CargoArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$Triple = (& rustc -vV | Select-String "^host:").ToString().Split(" ")[1]
$BinDir = Join-Path $RepoRoot "gui\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

foreach ($name in @("shilpid", "shilpi")) {
    $src = Join-Path $RepoRoot "target\$BuildProfile\$name.exe"
    $dst = Join-Path $BinDir "$name-$Triple.exe"
    Copy-Item -Path $src -Destination $dst -Force
    Write-Host "Staged $dst"
}
