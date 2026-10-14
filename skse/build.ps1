# SPDX-License-Identifier: GPL-3.0-or-later
# Builds the SKSE plugin (and ratchet.dll through cargo) and optionally installs it as an MO2 mod.
#
#   powershell -File skse\build.ps1            # configure + build
#   powershell -File skse\build.ps1 -Install   # ... then copy into the MO2 mod folder, and enable
#                                              # RatchetMovement.esl in the MO2 profile
#
# Output: target\skse\cmake\stage\SKSE\Plugins (RatchetMovement.dll + .pdb, ratchet.dll, the INI),
# target\skse\plugin\RatchetMovement.esl (his guns as Skyrim weapons).
#
# This PC's paths can go in skse\local.psd1 (not in git; see local.example.psd1) instead of on the
# command line.
param(
    [switch]$Install,
    # The MO2 mod folder -Install fills, and the MO2 profile (a name, or its folder) whose
    # plugins.txt gets RatchetMovement.esl.
    [string]$ModDir = "",
    [string]$Mo2Profile = "",
    # RAD Video Tools' encoder (radgametools.com, unpacked from its installer), for the startup
    # video; without it, Skyrim's own plays.
    [string]$RadVideo = "",
    [string]$ConfigsDir = "",
    # Visual Studio (or its Build Tools), when cl.exe isn't on PATH; found with vswhere otherwise.
    [string]$VsDir = "",
    [string]$VcpkgRoot = ""
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot

$localFile = Join-Path $here "local.psd1"
if (Test-Path $localFile) {
    $local = Import-PowerShellDataFile $localFile
    foreach ($k in "ModDir", "Mo2Profile", "RadVideo", "VsDir", "VcpkgRoot") {
        if (-not (Get-Variable $k -ValueOnly) -and $local[$k]) { Set-Variable $k $local[$k] }
    }
}
if (-not $VcpkgRoot) { $VcpkgRoot = $env:VCPKG_ROOT }
if (-not $VcpkgRoot -or -not (Test-Path (Join-Path $VcpkgRoot "vcpkg.exe"))) { throw "vcpkg not found: set VCPKG_ROOT or pass -VcpkgRoot" }

# MSVC x64 environment (Ninja needs cl, link and the SDK on PATH/INCLUDE/LIB).
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    if (-not $VsDir) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
        if (Test-Path $vswhere) { $VsDir = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath }
    }
    $vcvars = Join-Path "$VsDir" "VC\Auxiliary\Build\vcvars64.bat"
    if (-not $VsDir -or -not (Test-Path $vcvars)) { throw "MSVC not found: install VS 2022 (or its Build Tools) with C++, or pass -VsDir" }
    cmd /c "`"$vcvars`" >nul && set" | ForEach-Object {
        if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($matches[1])" -Value $matches[2] }
    }
}

$env:VCPKG_ROOT = $VcpkgRoot
# vcpkg's binary cache next to vcpkg rather than in the user's AppData.
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

# His guns as Skyrim weapons (skse\tools\ratchet_plugin.py), written record by record.
$pluginOut = Join-Path $here "..\target\skse\plugin"
uv run --quiet python -I (Join-Path $here "tools\ratchet_plugin.py") --out $pluginOut | Out-Host
if ($LASTEXITCODE -ne 0) { throw "writing RatchetMovement.esl failed" }

# A plugin enabled in an MO2 profile: in plugins.txt (enabled: a leading *) and loadorder.txt, at
# the end, each file's own line ends kept. MO2 reads them again at its next refresh (F5); while it
# runs it may write its own list over them before then.
function Enable-Plugin([string]$profileDir, [string]$name) {
    foreach ($file in @(@{ Name = "plugins.txt"; Line = "*$name" }, @{ Name = "loadorder.txt"; Line = $name })) {
        $path = Join-Path $profileDir $file.Name
        if (-not (Test-Path $path)) { Write-Warning "No $path`: enable $name in MO2"; return }
        $text = [IO.File]::ReadAllText($path)
        $lines = $text -split "\r?\n"
        if ($lines | Where-Object { $_.TrimStart('*') -ieq $name }) { continue }
        $eol = if ($text.Contains("`r`n")) { "`r`n" } else { "`n" }
        if ($text.Length -gt 0 -and -not $text.EndsWith("`n")) { $text += $eol }
        [IO.File]::WriteAllText($path, $text + $file.Line + $eol, [Text.Encoding]::ASCII)
        Write-Host "$name added to $path"
    }
}

if ($Install) {
    if (-not $ModDir) { throw "-Install needs the mod folder: pass -ModDir or set ModDir in skse\local.psd1" }
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

    # The game data extracted from the owner's own Rift Apart install, converted (skse\convert.ps1).
    & (Join-Path $here "convert.ps1") -ModDir $ModDir -Configs $ConfigsDir -RadVideo $RadVideo

    # His guns as Skyrim weapons: the plugin file in the mod's root, enabled in the profile.
    Copy-Item (Join-Path $pluginOut "RatchetMovement.esl") $ModDir -Force
    if ($Mo2Profile) {
        $profileDir = if (Test-Path $Mo2Profile -PathType Container) { $Mo2Profile } else { Join-Path (Split-Path (Split-Path $ModDir)) "profiles\$Mo2Profile" }
        Enable-Plugin $profileDir "RatchetMovement.esl"
    } else {
        Write-Host "No -Mo2Profile: tick RatchetMovement.esl in MO2's plugin list"
    }
    if (Get-Process ModOrganizer -ErrorAction SilentlyContinue) {
        Write-Host "MO2 is running: refresh it (F5) and check RatchetMovement.esl is ticked"
    }
    Write-Host "Installed to $dest"
}
