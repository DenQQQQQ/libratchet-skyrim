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

    # Game data extracted from the owner's own Rift Apart install (local only, gitignored): from
    # this checkout, else from the main checkout when building in a worktree.
    function Find-GameData([string]$name) {
        $dir = Join-Path $here "..\$name"
        if (-not (Test-Path $dir)) {
            $common = git -C $here rev-parse --path-format=absolute --git-common-dir
            $dir = Join-Path (Split-Path $common) $name
        }
        return $dir
    }

    # The configs, for the same tuning as the sandbox.
    if (-not $ConfigsDir) { $ConfigsDir = Find-GameData "riftapart-configs" }
    if (Test-Path $ConfigsDir) {
        $cfgDest = Join-Path $dest "RatchetMovement"
        New-Item -ItemType Directory -Force $cfgDest | Out-Null
        Copy-Item (Join-Path $ConfigsDir "*.json") $cfgDest -Force
        Write-Host "Configs from $ConfigsDir"
    } else {
        Write-Warning "No riftapart-configs found: the plugin will use libratchet's built-in tuning"
    }

    # Ratchet's model, textures and clips (skse\tools\ratchet_to_skyrim.py), converted again when
    # the extracted model or the tools are newer than the last conversion.
    $glb = Join-Path (Find-GameData "riftapart-assets") "ratchet.glb"
    $converted = Join-Path $here "..\target\skse\ratchet"
    $nif = Join-Path $converted "meshes\RatchetMovement\ratchet.nif"
    if (Test-Path $glb) {
        $tools = Join-Path $here "tools"
        $stale = -not (Test-Path $nif)
        if (-not $stale) {
            $made = (Get-Item $nif).LastWriteTime
            $stale = (Get-Item $glb).LastWriteTime -gt $made -or @(Get-ChildItem $tools -Filter *.py | Where-Object { $_.LastWriteTime -gt $made }).Count -gt 0
        }
        if ($stale) {
            Write-Host "Converting Ratchet from $glb..."
            uv run --python 3.12 --with numpy --with pillow (Join-Path $tools "ratchet_to_skyrim.py") --glb $glb --out $converted | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "converting Ratchet failed" }
        }
        # No stale files from an older conversion.
        foreach ($old in "meshes\RatchetMovement", "textures\RatchetMovement") {
            $p = Join-Path $ModDir $old
            if (Test-Path $p) { Remove-Item -Recurse -Force $p }
        }
        Copy-Item -Recurse -Force (Join-Path $converted "*") $ModDir
        Write-Host "Ratchet's model from $converted"
    } else {
        Write-Warning "No riftapart-assets\ratchet.glb: no Ratchet model (tools\extract_riftapart_ratchet.py makes it)"
    }
    Write-Host "Installed to $dest"
}
