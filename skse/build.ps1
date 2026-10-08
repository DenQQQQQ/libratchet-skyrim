# SPDX-License-Identifier: GPL-3.0-or-later
# Builds the SKSE plugin (and ratchet.dll through cargo) and optionally installs it as an MO2 mod.
#
#   powershell -File skse\build.ps1            # configure + build
#   powershell -File skse\build.ps1 -Install   # ... then copy into the MO2 mod folder
#
# Output: target\skse\cmake\stage\SKSE\Plugins (RatchetMovement.dll + .pdb, ratchet.dll, the INI).
param(
    [switch]$Install,
    [string]$ModDir = "G:\Modding\MO2\mods\Ratchet Movement",
    [string]$ConfigsDir = "",
    [string]$VsDir = "G:\dev\VS2022BuildTools",
    [string]$VcpkgRoot = $(if ($env:VCPKG_ROOT) { $env:VCPKG_ROOT } else { "G:\dev\tools\vcpkg" })
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot

# MSVC x64 environment (Ninja needs cl, link and the SDK on PATH/INCLUDE/LIB).
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    $vcvars = Join-Path $VsDir "VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path $vcvars)) { throw "vcvars64.bat not found under $VsDir (pass -VsDir)" }
    cmd /c "`"$vcvars`" >nul && set" | ForEach-Object {
        if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($matches[1])" -Value $matches[2] }
    }
}

$env:VCPKG_ROOT = $VcpkgRoot
# Keep vcpkg's binary cache off C: (nearly full on the owner's PC).
if (-not $env:VCPKG_DEFAULT_BINARY_CACHE) {
    $env:VCPKG_DEFAULT_BINARY_CACHE = Join-Path $VcpkgRoot "archives"
    New-Item -ItemType Directory -Force $env:VCPKG_DEFAULT_BINARY_CACHE | Out-Null
}
if (-not (Get-Command ninja.exe -ErrorAction SilentlyContinue)) {
    $ninja = & (Join-Path $VcpkgRoot "vcpkg.exe") fetch ninja | Select-Object -Last 1
    $env:PATH = (Split-Path $ninja) + ";" + $env:PATH
}

Push-Location $here
try {
    cmake --preset release
    if ($LASTEXITCODE -ne 0) { throw "cmake configure failed" }
    cmake --build --preset release
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
} finally {
    Pop-Location
}

$stage = Join-Path $here "..\target\skse\cmake\stage\SKSE\Plugins"
Write-Host "Built: $((Resolve-Path $stage).Path)"

if ($Install) {
    $dest = Join-Path $ModDir "SKSE\Plugins"
    New-Item -ItemType Directory -Force (Join-Path $dest "RatchetMovement") | Out-Null
    foreach ($f in "RatchetMovement.dll", "RatchetMovement.pdb", "RatchetMovement\ratchet.dll") {
        Copy-Item (Join-Path $stage $f) (Join-Path $dest $f) -Force
    }
    # Older installs had ratchet.dll next to the plugin, where SKSE probes it.
    $old = Join-Path $dest "ratchet.dll"
    if (Test-Path $old) { Remove-Item $old -Force }
    # Keep the owner's edited INI; a new one only goes in if there's none.
    $ini = Join-Path $dest "RatchetMovement.ini"
    if (-not (Test-Path $ini)) { Copy-Item (Join-Path $stage "RatchetMovement.ini") $ini }

    # The owner's extracted Rift Apart configs (game data: local only), for the same tuning as the
    # sandbox. From this checkout, else from the main checkout when building in a worktree.
    if (-not $ConfigsDir) {
        $ConfigsDir = Join-Path $here "..\riftapart-configs"
        if (-not (Test-Path $ConfigsDir)) {
            $common = git -C $here rev-parse --path-format=absolute --git-common-dir
            $ConfigsDir = Join-Path (Split-Path $common) "riftapart-configs"
        }
    }
    if (Test-Path $ConfigsDir) {
        $cfgDest = Join-Path $dest "RatchetMovement"
        New-Item -ItemType Directory -Force $cfgDest | Out-Null
        Copy-Item (Join-Path $ConfigsDir "*.json") $cfgDest -Force
        Write-Host "Configs from $ConfigsDir"
    } else {
        Write-Warning "No riftapart-configs found: the plugin will use libratchet's built-in tuning"
    }
    Write-Host "Installed to $dest"
}
