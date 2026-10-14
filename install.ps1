# SPDX-License-Identifier: GPL-3.0-or-later
# Ratchet Movement's installer: makes the Skyrim mod from YOUR OWN copy of Ratchet & Clank: Rift
# Apart.
#
# The release holds only this project's own files (the SKSE plugin, ratchet.dll, the .esl and the
# tools). Everything that comes from Rift Apart (Ratchet's model, textures, clips and sounds, the
# effects' art, the menu's logo and music, the movement configs) is read here from the game
# installed on this PC and converted for Skyrim. Nothing is uploaded anywhere.
#
#   Install.bat                    double-click: finds Rift Apart, writes output\Ratchet Movement
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-RiftApart <folder>] [-ModDir <folder>]
#       [-Zip] [-RadVideo <radvideo64.exe>] [-Language us] [-KeepWork]
#
# It downloads what it needs into cache\ next to it (uv, which brings Python 3.12 and the Python
# packages; ALERT, the Rift Apart asset reader; ffmpeg if there's none on PATH). Delete this folder
# afterwards and nothing is left on the PC but the mod.
param(
    # Rift Apart's install folder (with RiftApart.exe and toc). Found through Steam or Epic when
    # not given.
    [string]$RiftApart = "",
    # Where to write the mod (default: output\Ratchet Movement next to this script). Give MO2's
    # mods\Ratchet Movement to install straight into Mod Organizer 2.
    [string]$ModDir = "",
    # Also pack the mod into a .zip beside it, for Vortex (or MO2's "Install a new mod from an
    # archive").
    [switch]$Zip,
    # RAD Video Tools' radvideo64.exe, for Rift Apart's title card as Skyrim's startup video
    # (optional; without it Skyrim's own plays).
    [string]$RadVideo = "",
    # Rift Apart's voice language archive to read his sounds from (us, de, fr, ...).
    [string]$Language = "us",
    # Keep the extracted and converted files (work\) afterwards.
    [switch]$KeepWork
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # Windows PowerShell's progress bar slows downloads a lot
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$root = $PSScriptRoot
$cache = Join-Path $root "cache"
$work = Join-Path $root "work"

# Pinned downloads.
$uvVersion = "0.5.25"
$uvSha256 = "51EA202A472E552C04E6A13AC7EECE0D56748750B9726E4B94F4F3DC96B5F77F"
$alertCommit = "bc90ed9b53fbcaa3b7acf94b36f00e77eab0dacb"
$pyExtract = @("--no-project", "--python", "3.12", "--with", "numpy==2.3.3", "--with", "pillow==11.3.0",
    "--with", "pygltflib==1.16.5", "--with", "texture2ddecoder==1.0.6")

function Step([string]$text) { Write-Host ""; Write-Host "== $text" -ForegroundColor Cyan }
function Fail([string]$text) { Write-Host ""; Write-Host $text -ForegroundColor Red; exit 1 }

# --- Rift Apart

# Steam: its library folders, then the app's manifest (Rift Apart is app 1895880).
function Find-SteamGame([string]$appId) {
    $steam = $null
    foreach ($key in "HKCU:\Software\Valve\Steam", "HKLM:\SOFTWARE\WOW6432Node\Valve\Steam", "HKLM:\SOFTWARE\Valve\Steam") {
        $item = Get-ItemProperty $key -ErrorAction SilentlyContinue
        if ($item -and $item.SteamPath) { $steam = $item.SteamPath; break }
        if ($item -and $item.InstallPath) { $steam = $item.InstallPath; break }
    }
    if (-not $steam) { return $null }
    $libraries = @($steam)
    $vdf = Join-Path $steam "steamapps\libraryfolders.vdf"
    if (Test-Path $vdf) {
        foreach ($m in [regex]::Matches([IO.File]::ReadAllText($vdf), '"path"\s+"([^"]+)"')) {
            $libraries += $m.Groups[1].Value -replace '\\\\', '\'
        }
    }
    foreach ($lib in $libraries | Select-Object -Unique) {
        $acf = Join-Path $lib "steamapps\appmanifest_$appId.acf"
        if (-not (Test-Path $acf)) { continue }
        $m = [regex]::Match([IO.File]::ReadAllText($acf), '"installdir"\s+"([^"]+)"')
        if ($m.Success) {
            $dir = Join-Path $lib "steamapps\common\$($m.Groups[1].Value)"
            if (Test-Path $dir) { return $dir }
        }
    }
    return $null
}

# Epic: the launcher's install manifests.
function Find-EpicGame([string]$namePattern) {
    $manifests = Join-Path $env:ProgramData "Epic\EpicGamesLauncher\Data\Manifests"
    if (-not (Test-Path $manifests)) { return $null }
    foreach ($f in Get-ChildItem $manifests -Filter *.item) {
        try { $item = Get-Content $f.FullName -Raw | ConvertFrom-Json } catch { continue }
        if ($item.DisplayName -match $namePattern -and $item.InstallLocation -and (Test-Path $item.InstallLocation)) {
            return $item.InstallLocation
        }
    }
    return $null
}

function Test-RiftApart([string]$dir) {
    return $dir -and (Test-Path (Join-Path $dir "RiftApart.exe")) -and (Test-Path (Join-Path $dir "toc"))
}

# --- the release's own files (or a source checkout's build)

function Find-Prebuilt {
    $mod = Join-Path $root "mod"
    if (Test-Path (Join-Path $mod "SKSE\Plugins\RatchetMovement.dll")) {
        return @{ Mod = $mod; Gdeflate = (Join-Path $root "tools\gdeflate_ffi.dll") }
    }
    # A source checkout after skse\build.ps1 and cargo build -p gdeflate-ffi --release.
    $stage = Join-Path $root "target\skse\cmake\stage\SKSE\Plugins"
    $esl = Join-Path $root "target\skse\plugin\RatchetMovement.esl"
    if ((Test-Path (Join-Path $stage "RatchetMovement.dll")) -and (Test-Path $esl)) {
        $gd = @("target\skse\release\gdeflate_ffi.dll", "target\release\gdeflate_ffi.dll") | ForEach-Object { Join-Path $root $_ } | Where-Object { Test-Path $_ } | Select-Object -First 1
        return @{ Stage = $stage; Esl = $esl; Gdeflate = $gd }
    }
    return $null
}

# --- downloads

function Get-File([string]$url, [string]$dest) {
    Write-Host "  downloading $url"
    $tmp = "$dest.part"
    Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
    Move-Item -Force $tmp $dest
}

function Get-Uv {
    $exe = Join-Path $cache "uv\uv.exe"
    if (Test-Path $exe) { return $exe }
    New-Item -ItemType Directory -Force (Split-Path $exe) | Out-Null
    $zip = Join-Path $cache "uv.zip"
    Get-File "https://github.com/astral-sh/uv/releases/download/$uvVersion/uv-x86_64-pc-windows-msvc.zip" $zip
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
    if ($hash -ne $uvSha256) { Remove-Item $zip; throw "uv's download doesn't match its checksum ($hash)" }
    Expand-Archive -Force $zip (Split-Path $exe)
    Remove-Item $zip
    return $exe
}

function Get-Alert {
    $dir = Join-Path $cache "ALERT-$alertCommit"
    if (Test-Path (Join-Path $dir "dat1lib")) { return $dir }
    $zip = Join-Path $cache "alert.zip"
    Get-File "https://github.com/Tkachov/ALERT/archive/$alertCommit.zip" $zip
    Expand-Archive -Force $zip $cache
    Remove-Item $zip
    if (-not (Test-Path (Join-Path $dir "dat1lib"))) { throw "ALERT's download has no dat1lib" }
    return $dir
}

# ffmpeg (Rift Apart's music and voices are Opus): the one on PATH, else imageio-ffmpeg's build.
function Get-Ffmpeg {
    $onPath = Get-Command ffmpeg.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $exe = Join-Path $cache "bin\ffmpeg.exe"
    if (Test-Path $exe) { return $exe }
    New-Item -ItemType Directory -Force (Split-Path $exe) | Out-Null
    $ErrorActionPreference = "Continue"
    $found = uv run --no-project --python 3.12 --with imageio-ffmpeg==0.6.0 python -c "import imageio_ffmpeg; print(imageio_ffmpeg.get_ffmpeg_exe())"
    if ($LASTEXITCODE -ne 0 -or -not $found -or -not (Test-Path $found.Trim())) { throw "couldn't get ffmpeg" }
    Copy-Item $found.Trim() $exe
    return $exe
}

function Invoke-Tool([string]$what, [string[]]$toolArgs) {
    # The tools' warnings on stderr aren't errors (Windows PowerShell makes them ones when its own
    # output is redirected); the exit code is what counts.
    $ErrorActionPreference = "Continue"
    $t = Get-Date
    uv run @pyExtract @toolArgs | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$what failed" }
    Write-Host ("  done in {0:n0} s" -f ((Get-Date) - $t).TotalSeconds)
}

# ---

try {
    $started = Get-Date
    Write-Host "Ratchet Movement: making the mod from your own Rift Apart install." -ForegroundColor Cyan

    $prebuilt = Find-Prebuilt
    if (-not $prebuilt) { Fail "No mod files next to this script: download the release zip (or build it: skse\build.ps1)." }
    if (-not $prebuilt.Gdeflate -or -not (Test-Path $prebuilt.Gdeflate)) { Fail "No gdeflate_ffi.dll (cargo build -p gdeflate-ffi --release)." }

    Step "Finding Rift Apart"
    if (-not $RiftApart) { $RiftApart = Find-SteamGame "1895880" }
    if (-not (Test-RiftApart $RiftApart)) { $RiftApart = Find-EpicGame "Rift Apart" }
    if (-not (Test-RiftApart $RiftApart)) {
        Fail "Rift Apart not found. Run it again with its folder (the one with RiftApart.exe):`n  install.ps1 -RiftApart `"D:\Games\Ratchet & Clank - Rift Apart`""
    }
    Write-Host "  $RiftApart"

    if (-not $ModDir) { $ModDir = Join-Path $root "output\Ratchet Movement" }
    $ModDir = [IO.Path]::GetFullPath($ModDir)
    # An old copy of this mod is replaced, keeping its INI (the user's settings) and MO2's
    # meta.ini; any other folder is left alone.
    $kept = @{}
    if (Test-Path $ModDir) {
        $ours = Test-Path (Join-Path $ModDir "SKSE\Plugins\RatchetMovement.dll")
        $empty = @(Get-ChildItem -Force $ModDir).Count -eq 0
        if (-not $ours -and -not $empty) { Fail "$ModDir already exists and isn't this mod: give another -ModDir." }
        if ($ours) {
            foreach ($f in "SKSE\Plugins\RatchetMovement.ini", "meta.ini") {
                $p = Join-Path $ModDir $f
                if (Test-Path $p) { $kept[$f] = [IO.File]::ReadAllBytes($p) }
            }
            # Emptied, not deleted: a running MO2 keeps the mod in its list.
            Get-ChildItem -Force $ModDir | Remove-Item -Recurse -Force
        }
    }

    $drive = [IO.Path]::GetPathRoot($root)
    $free = (Get-PSDrive -PSProvider FileSystem | Where-Object { $drive -like "$($_.Root)*" } | Select-Object -First 1).Free
    if ($free -and $free -lt 3GB) { Write-Warning ("Only {0:n1} GB free on {1}: the install needs about 3 GB while it works (the mod is about 0.6 GB)" -f ($free / 1GB), $drive) }

    Step "Getting the tools (first run only)"
    New-Item -ItemType Directory -Force $cache | Out-Null
    $uvCmd = Get-Command uv.exe -ErrorAction SilentlyContinue
    $uv = if ($uvCmd) { $uvCmd.Source } else { Get-Uv }
    $env:PATH = (Split-Path $uv) + ";" + (Join-Path $cache "bin") + ";" + $env:PATH
    # Python and its packages stay in cache\, as does uv's own cache.
    $env:UV_CACHE_DIR = Join-Path $cache "uv-cache"
    $env:UV_PYTHON_INSTALL_DIR = Join-Path $cache "python"
    $env:UV_PYTHON_PREFERENCE = "only-managed"
    $env:PYTHONUTF8 = "1"
    $env:PYTHONIOENCODING = "utf-8"
    Write-Host "  uv: $uv"
    $alert = Get-Alert
    Write-Host "  ALERT: $alert"
    $ffmpeg = Get-Ffmpeg
    Write-Host "  ffmpeg: $ffmpeg"
    if (-not $RadVideo) {
        $RadVideo = @("${env:ProgramFiles(x86)}\RADVideo\radvideo64.exe", "$env:ProgramFiles\RADVideo\radvideo64.exe", (Join-Path $root "RADTools\bin\radvideo64.exe")) |
            Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if ($RadVideo) { Write-Host "  RAD Video Tools: $RadVideo" } else { Write-Host "  RAD Video Tools: not found (Skyrim keeps its own startup video; see INSTALL.md)" }

    $tools = Join-Path $root "tools"
    $assets = Join-Path $work "riftapart-assets"
    $configs = Join-Path $work "riftapart-configs"
    New-Item -ItemType Directory -Force $assets, $configs | Out-Null

    Step "1/6 Movement configs"
    Invoke-Tool "reading the configs" @((Join-Path $tools "extract_riftapart_configs.py"), "--game", $RiftApart, "--alert", $alert, "--out", $configs)
    Step "2/6 Ratchet, Clank, the weapons and the bolts (models, textures, animations)"
    Invoke-Tool "reading the models" @((Join-Path $tools "extract_riftapart_ratchet.py"), "--game", $RiftApart, "--alert", $alert, "--out", $assets, "--gdeflate-dll", $prebuilt.Gdeflate)
    Step "3/6 Weapon effects"
    Invoke-Tool "reading the effects" @((Join-Path $tools "extract_riftapart_vfx.py"), "--game", $RiftApart, "--alert", $alert, "--out", $assets, "--gdeflate-dll", $prebuilt.Gdeflate)
    Step "4/6 Sounds"
    Invoke-Tool "reading the sounds" @((Join-Path $tools "extract_riftapart_sounds.py"), "--game", $RiftApart, "--alert", $alert, "--out", $assets, "--lang", $Language)
    Step "5/6 Main menu logo and music"
    Invoke-Tool "reading the menu" @((Join-Path $tools "extract_riftapart_menu.py"), "--game", $RiftApart, "--alert", $alert, "--out", (Join-Path $assets "menu"), "--gdeflate-dll", $prebuilt.Gdeflate)

    Step "6/6 Converting for Skyrim into $ModDir"
    $t = Get-Date
    $plugins = Join-Path $ModDir "SKSE\Plugins"
    New-Item -ItemType Directory -Force (Join-Path $plugins "RatchetMovement") | Out-Null
    if ($prebuilt.Mod) {
        Copy-Item -Recurse -Force (Join-Path $prebuilt.Mod "*") $ModDir
    } else {
        foreach ($f in "RatchetMovement.dll", "RatchetMovement.ini", "RatchetMovement\ratchet.dll") {
            Copy-Item (Join-Path $prebuilt.Stage $f) (Join-Path $plugins $f) -Force
        }
        Copy-Item $prebuilt.Esl $ModDir -Force
    }
    foreach ($f in $kept.Keys) { [IO.File]::WriteAllBytes((Join-Path $ModDir $f), $kept[$f]) }
    & (Join-Path $root "skse\convert.ps1") -ModDir $ModDir -Assets $assets -Configs $configs -Work (Join-Path $work "converted") -RadVideo $RadVideo
    Write-Host ("  done in {0:n0} s" -f ((Get-Date) - $t).TotalSeconds)

    if ($Zip) {
        Step "Packing the .zip"
        $zipPath = "$ModDir.zip"
        if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
        # Entry names with "/" (Windows PowerShell's ZipFile.CreateFromDirectory writes "\").
        Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
        $stream = [IO.File]::Open($zipPath, [IO.FileMode]::CreateNew)
        $archive = New-Object IO.Compression.ZipArchive($stream, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($f in Get-ChildItem -Recurse -File $ModDir) {
                $entry = $f.FullName.Substring($ModDir.TrimEnd('\').Length + 1).Replace('\', '/')
                [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $f.FullName, $entry, [IO.Compression.CompressionLevel]::Optimal)
            }
        } finally {
            $archive.Dispose()
            $stream.Dispose()
        }
        Write-Host "  $zipPath"
    }

    if (-not $KeepWork) { Remove-Item -Recurse -Force $work }

    $size = (Get-ChildItem -Recurse -File $ModDir | Measure-Object Length -Sum).Sum
    Write-Host ""
    Write-Host ("Done in {0:n0} min. The mod ({1:n0} MB) is in:" -f ((Get-Date) - $started).TotalMinutes, ($size / 1MB)) -ForegroundColor Green
    Write-Host "  $ModDir"
    Write-Host ""
    Write-Host "Next (see INSTALL.md):"
    Write-Host "  Mod Organizer 2: move that folder into MO2's mods folder (or run again with -ModDir <MO2>\mods\Ratchet Movement),"
    Write-Host "                   press F5, tick 'Ratchet Movement' on the left and RatchetMovement.esl on the right."
    Write-Host "  Vortex:          run again with -Zip and drop the .zip onto Vortex's Mods page, then enable and deploy."
    Write-Host "  Start Skyrim through SKSE."
    Write-Host ""
    Write-Host "The mod holds files from your own copy of Rift Apart: keep it to yourself, don't upload or share it."
    exit 0
} catch {
    Write-Host ""
    Write-Host "Install failed: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "Run it again with -KeepWork and look in work\ for what's there; INSTALL.md has the troubleshooting." -ForegroundColor Red
    exit 1
}
