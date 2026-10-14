# SPDX-License-Identifier: GPL-3.0-or-later
# Makes the release zip players download: this project's own files only, no game data. The player's
# install.ps1 then makes the game's part from their own Rift Apart.
#
#   powershell -File skse\package.ps1 [-Version 0.1.0] [-NoBuild]
#
# Output: target\release-pkg\RatchetMovement-<version>.zip, holding
#   Install.bat, install.ps1, INSTALL.md, LICENSE, LICENSES\
#   mod\                the mod's own files: RatchetMovement.esl, SKSE\Plugins\RatchetMovement.dll,
#                       RatchetMovement.ini, RatchetMovement\ratchet.dll
#   tools\              the extractors and gdeflate_ffi.dll (GDeflate decoder they load)
#   skse\convert.ps1, skse\tools\   the converters into Skyrim's formats
param(
    [string]$Version = "",
    # Package what's built already (skse\build.ps1, and gdeflate-ffi in target\skse\release).
    [switch]$NoBuild,
    # Passed on to build.ps1.
    [string]$VsDir = "",
    [string]$VcpkgRoot = ""
)
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $Version) { $Version = (git -C $repo describe --tags --always --dirty).Trim() }
$Version = $Version -replace '^v', ''

$target = Join-Path $repo "target\skse"
if (-not $NoBuild) {
    $buildArgs = @{}
    if ($VsDir) { $buildArgs.VsDir = $VsDir }
    if ($VcpkgRoot) { $buildArgs.VcpkgRoot = $VcpkgRoot }
    & (Join-Path $PSScriptRoot "build.ps1") @buildArgs
    $env:CARGO_TARGET_DIR = $target
    Push-Location $repo
    try {
        cargo build --release -p gdeflate-ffi
        if ($LASTEXITCODE -ne 0) { throw "building gdeflate-ffi failed" }
    } finally {
        Pop-Location
    }
}

$stage = Join-Path $target "cmake\stage\SKSE\Plugins"
$esl = Join-Path $target "plugin\RatchetMovement.esl"
$gdeflate = Join-Path $target "release\gdeflate_ffi.dll"
foreach ($f in (Join-Path $stage "RatchetMovement.dll"), (Join-Path $stage "RatchetMovement\ratchet.dll"), $esl, $gdeflate) {
    if (-not (Test-Path $f)) { throw "$f is missing: build first (drop -NoBuild)" }
}

$name = "RatchetMovement-$Version"
$out = Join-Path $repo "target\release-pkg"
$pkg = Join-Path $out $name
if (Test-Path $pkg) { Remove-Item -Recurse -Force $pkg }
New-Item -ItemType Directory -Force $pkg | Out-Null

function Copy-To([string]$from, [string]$to) {
    $dest = Join-Path $pkg $to
    New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
    Copy-Item -Force $from $dest
}

foreach ($f in "Install.bat", "install.ps1", "INSTALL.md", "LICENSE") { Copy-To (Join-Path $repo $f) $f }
Get-ChildItem (Join-Path $repo "LICENSES") -File | ForEach-Object { Copy-To $_.FullName "LICENSES\$($_.Name)" }
Copy-To (Join-Path $stage "RatchetMovement.dll") "mod\SKSE\Plugins\RatchetMovement.dll"
Copy-To (Join-Path $repo "skse\RatchetMovement.ini") "mod\SKSE\Plugins\RatchetMovement.ini"
Copy-To (Join-Path $stage "RatchetMovement\ratchet.dll") "mod\SKSE\Plugins\RatchetMovement\ratchet.dll"
Copy-To $esl "mod\RatchetMovement.esl"
Copy-To $gdeflate "tools\gdeflate_ffi.dll"
foreach ($f in "extract_riftapart_configs.py", "extract_riftapart_ratchet.py", "extract_riftapart_vfx.py", "extract_riftapart_sounds.py",
    "extract_riftapart_menu.py", "riftapart_anim.py", "riftapart_wwise.py") {
    Copy-To (Join-Path $repo "tools\$f") "tools\$f"
}
Copy-To (Join-Path $repo "skse\convert.ps1") "skse\convert.ps1"
Get-ChildItem (Join-Path $repo "skse\tools") -Filter *.py | ForEach-Object { Copy-To $_.FullName "skse\tools\$($_.Name)" }
$commit = (git -C $repo rev-parse HEAD).Trim()
[IO.File]::WriteAllText((Join-Path $pkg "VERSION.txt"), "Ratchet Movement $Version`r`nsource: commit $commit`r`n")

# Nothing from a game install may ship: only our scripts, docs and binaries.
$allowed = @(".bat", ".ps1", ".md", ".py", ".txt", ".dll", ".esl", ".ini", "")
$bad = @(Get-ChildItem -Recurse -File $pkg | Where-Object { $_.Extension.ToLower() -notin $allowed })
if ($bad) { throw "unexpected files in the package: $($bad.FullName -join ', ')" }

$zip = Join-Path $out "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
# Entry names with "/" (Windows PowerShell's ZipFile.CreateFromDirectory writes "\", which other
# unzippers take as part of the file name).
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$stream = [IO.File]::Open($zip, [IO.FileMode]::CreateNew)
$archive = New-Object IO.Compression.ZipArchive($stream, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($f in Get-ChildItem -Recurse -File $pkg) {
        $entry = "$name/" + $f.FullName.Substring($pkg.Length + 1).Replace('\', '/')
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $f.FullName, $entry, [IO.Compression.CompressionLevel]::Optimal)
    }
} finally {
    $archive.Dispose()
    $stream.Dispose()
}
Write-Host ("Packaged {0} ({1:n1} MB)" -f $zip, ((Get-Item $zip).Length / 1MB))
