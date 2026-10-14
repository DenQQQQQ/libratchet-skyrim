# SPDX-License-Identifier: GPL-3.0-or-later
# Converts the Rift Apart data extracted from the user's own install (tools\extract_riftapart_*.py)
# into Skyrim's formats and puts it into a mod folder laid out like Skyrim's Data folder: the
# configs, Ratchet's, Clank's, the weapons' and the bolts' models with their textures and clips,
# his sounds, his weapons' effect art, and Rift Apart's main menu and startup video.
#
#   powershell -File skse\convert.ps1 -ModDir <mod folder> [-Assets riftapart-assets]
#       [-Configs riftapart-configs] [-Work target\skse] [-RadVideo radvideo64.exe]
#
# build.ps1 -Install runs it for the owner's MO2 mod, install.ps1 for a player's. Each part is
# converted again only when its extraction or a converter is newer than the last conversion in
# -Work, and skipped with a warning when its extraction is missing. Runs the converters through
# uv (Python 3.12). The output is game data: it stays on the user's PC.
param(
    [Parameter(Mandatory = $true)][string]$ModDir,
    # The extractions (default: this checkout's riftapart-assets and riftapart-configs, else the
    # main checkout's when this is a worktree).
    [string]$Assets = "",
    [string]$Configs = "",
    # Where the converted files are kept between runs (default: target\skse).
    [string]$Work = "",
    # RAD Video Tools' encoder (radgametools.com), for the startup video; without it, Skyrim's own plays.
    [string]$RadVideo = ""
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$tools = Join-Path $here "tools"

# The converters' Python packages, pinned (uv fetches them).
$py = @("--python", "3.12", "--with", "numpy==2.3.3", "--with", "pillow==11.3.0")
$pySound = @("--python", "3.12", "--with", "numpy==2.3.3", "--with", "pillow==11.3.0", "--with", "soundfile==0.13.1")

# A converter through uv. Its warnings on stderr aren't errors (Windows PowerShell makes them ones
# when its own output is redirected); the exit code is what counts.
function Invoke-Py([string]$what, [string[]]$pyArgs) {
    $ErrorActionPreference = "Continue"
    uv run @pyArgs | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$what failed" }
}

if (Test-Path (Join-Path $ModDir "Skyrim.esm")) { throw "$ModDir is Skyrim's Data folder: give a mod folder of its own" }
New-Item -ItemType Directory -Force $ModDir | Out-Null

# Game data extracted from the user's own Rift Apart install: from this checkout, else from the
# main checkout when this is a worktree.
function Find-GameData([string]$name) {
    $dir = Join-Path $here "..\$name"
    if (-not (Test-Path $dir)) {
        $common = git -C $here rev-parse --path-format=absolute --git-common-dir 2>$null
        if ($common) { $dir = Join-Path (Split-Path $common) $name }
    }
    return $dir
}
if (-not $Assets) { $Assets = Find-GameData "riftapart-assets" }
if (-not $Configs) { $Configs = Find-GameData "riftapart-configs" }
if (-not $Work) { $Work = Join-Path $here "..\target\skse" }

# Tools that aren't the models' or the effects' converters (their changes don't make those stale).
$notConverters = @("ratchet_plugin.py", "ratchet_menu_to_skyrim.py", "ratchet_intro_to_skyrim.py")
function Test-ToolsNewer([datetime]$made) {
    return @(Get-ChildItem $tools -Filter *.py | Where-Object { $_.Name -notin $notConverters -and $_.LastWriteTime -gt $made }).Count -gt 0
}

# The configs, for the same tuning as the sandbox.
if (Test-Path $Configs) {
    $cfgDest = Join-Path $ModDir "SKSE\Plugins\RatchetMovement"
    New-Item -ItemType Directory -Force $cfgDest | Out-Null
    Copy-Item (Join-Path $Configs "*.json") $cfgDest -Force
    Write-Host "Configs from $Configs"
} else {
    Write-Warning "No $Configs`: the plugin will use libratchet's built-in tuning"
}

# Ratchet's model, textures and clips (skse\tools\ratchet_to_skyrim.py), converted again when the
# extracted model or the tools are newer than the last conversion.
$glb = Join-Path $Assets "ratchet.glb"
$converted = Join-Path $Work "ratchet"
$nif = Join-Path $converted "meshes\RatchetMovement\ratchet.nif"
if (Test-Path $glb) {
    $stale = -not (Test-Path $nif)
    if (-not $stale) {
        $made = (Get-Item $nif).LastWriteTime
        $stale = (Get-Item $glb).LastWriteTime -gt $made -or (Test-ToolsNewer $made)
    }
    if ($stale) {
        Write-Host "Converting Ratchet from $glb..."
        Invoke-Py "converting Ratchet" ($py + @((Join-Path $tools "ratchet_to_skyrim.py"), "--glb", $glb, "--out", $converted))
    }
    # His OmniWrench and Burst Pistol with their own clips (the wrench's squash and stretch, the
    # pistol's slide), the Shatterbomb (its bomb, and the glove that throws it, skinned to his
    # own joints: models only), and Clank with his clips (he rides on Ratchet's back).
    $others = @(
        @{ Name = "wrench"; Prefix = "RW_"; Clips = $true; Without = "he'll fight without it in his hand" },
        @{ Name = "burstpistol"; Prefix = "RP_"; Clips = $true; Without = "he'll fight without it in his hand" },
        @{ Name = "shatterbomb"; Prefix = "RS_"; Clips = $false; Without = "his Shatterbombs fly unseen" },
        @{ Name = "shatterbomb_glove"; Prefix = "RG_"; Clips = $false; Without = "he throws Shatterbombs with his bare glove" },
        @{ Name = "clank"; Prefix = "RK_"; Clips = $true; Without = "Ratchet goes without Clank on his back" }
    )
    # The twelve bolt pickups (tools\extract_riftapart_ratchet.py --character bolts): rigid
    # models, their two metal materials' textures shared in one folder.
    foreach ($tier in "bronze", "silver", "golden", "platinum") {
        foreach ($look in 1..3) {
            $others += @{ Name = "bolt_${tier}_0$look"; Prefix = "RB_"; Clips = $false; TextureDir = "RatchetMovement\bolts"; Without = "its bolts are drawn as the bronze ones, or not at all" }
        }
    }
    # The loading screens' models (RatchetMovement.esl's RatchetLoad* statics): Ratchet and
    # Clank standing in their idles' first frame, the weapons and a gold bolt, each in
    # Skyrim's axes and units at its root, sharing the models' own textures.
    $others += @(
        @{ Name = "loadscreen_ratchet"; Glb = "ratchet"; Prefix = "RC_"; Clips = $false; TextureDir = "RatchetMovement\ratchet"; Pose = "idle"; Loadscreen = $true; Without = "a loading screen without him" },
        @{ Name = "loadscreen_clank"; Glb = "clank"; Prefix = "RK_"; Clips = $false; TextureDir = "RatchetMovement\clank"; Pose = "idle"; Loadscreen = $true; Without = "a loading screen without Clank" },
        @{ Name = "loadscreen_wrench"; Glb = "wrench"; Prefix = "RW_"; Clips = $false; TextureDir = "RatchetMovement\wrench"; Loadscreen = $true; Without = "a loading screen without the wrench" },
        @{ Name = "loadscreen_burstpistol"; Glb = "burstpistol"; Prefix = "RP_"; Clips = $false; TextureDir = "RatchetMovement\burstpistol"; Loadscreen = $true; Without = "a loading screen without the pistol" },
        @{ Name = "loadscreen_shatterbomb"; Glb = "shatterbomb"; Prefix = "RS_"; Clips = $false; TextureDir = "RatchetMovement\shatterbomb"; Loadscreen = $true; Without = "a loading screen without the Shatterbomb" },
        @{ Name = "loadscreen_bolt"; Glb = "bolt_golden_01"; Prefix = "RB_"; Clips = $false; TextureDir = "RatchetMovement\bolts"; Loadscreen = $true; Without = "a loading screen without the bolt" }
    )
    foreach ($w in $others) {
        $wglb = Join-Path $Assets "$(if ($w.Glb) { $w.Glb } else { $w.Name }).glb"
        $wnif = Join-Path $converted "meshes\RatchetMovement\$($w.Name).nif"
        if (-not (Test-Path $wglb)) { Write-Warning "No $wglb`: $($w.Without)"; continue }
        $wstale = -not (Test-Path $wnif)
        if (-not $wstale) {
            $made = (Get-Item $wnif).LastWriteTime
            $wstale = (Get-Item $wglb).LastWriteTime -gt $made -or (Test-ToolsNewer $made)
        }
        if ($wstale) {
            Write-Host "Converting $($w.Name) from $wglb..."
            $clipArgs = @()
            if (-not $w.Clips) { $clipArgs += "--no-clips" }
            if ($w.TextureDir) { $clipArgs += @("--texture-dir", $w.TextureDir) }
            if ($w.Pose) { $clipArgs += @("--pose", $w.Pose) }
            if ($w.Loadscreen) { $clipArgs += "--loadscreen" }
            Invoke-Py "converting $($w.Name)" ($py + @((Join-Path $tools "ratchet_to_skyrim.py"), "--glb", $wglb, "--out", $converted, "--name", $w.Name, "--prefix", $w.Prefix) + $clipArgs)
        }
    }
    # His sounds (skse\tools\ratchet_sounds_to_skyrim.py): WAV files and sounds.txt, converted
    # again when the extraction's manifest or the tool is newer.
    $soundsSrc = Join-Path $Assets "sounds"
    $soundsOut = Join-Path $Work "sounds"
    $soundsTxt = Join-Path $soundsOut "SKSE\Plugins\RatchetMovement\sounds.txt"
    if (Test-Path (Join-Path $soundsSrc "sounds.json")) {
        $sstale = -not (Test-Path $soundsTxt)
        if (-not $sstale) {
            $made = (Get-Item $soundsTxt).LastWriteTime
            $sstale = (Get-Item (Join-Path $soundsSrc "sounds.json")).LastWriteTime -gt $made -or (Get-Item (Join-Path $tools "ratchet_sounds_to_skyrim.py")).LastWriteTime -gt $made
        }
        if ($sstale) {
            Write-Host "Converting his sounds from $soundsSrc..."
            Invoke-Py "converting the sounds" ($pySound + @((Join-Path $tools "ratchet_sounds_to_skyrim.py"), "--sounds", $soundsSrc, "--out", $soundsOut))
        }
    } else {
        Write-Warning "No $soundsSrc\sounds.json: he'll be silent (tools\extract_riftapart_sounds.py makes it)"
    }
    # No stale files from an older conversion.
    foreach ($old in "meshes\RatchetMovement", "textures\RatchetMovement", "Sound\FX\RatchetMovement") {
        $p = Join-Path $ModDir $old
        if (Test-Path $p) { Remove-Item -Recurse -Force $p }
    }
    Copy-Item -Recurse -Force (Join-Path $converted "*") $ModDir
    Write-Host "Ratchet's model from $converted"
    if (Test-Path $soundsTxt) {
        Copy-Item -Recurse -Force (Join-Path $soundsOut "*") $ModDir
        Write-Host "His sounds from $soundsOut"
    }
} else {
    Write-Warning "No $glb`: no Ratchet model (tools\extract_riftapart_ratchet.py makes it)"
}

# His weapons' effect art (skse\tools\ratchet_vfx_to_skyrim.py): textures, the effect pieces' NIF
# templates and fx.ini's palette, converted again when the extracted art or the tools are newer
# than the last conversion.
$vfxSrc = Join-Path $Assets "vfx"
$vfxOut = Join-Path $Work "vfx"
$fxIni = Join-Path $vfxOut "SKSE\Plugins\RatchetMovement\fx.ini"
if (Test-Path $vfxSrc) {
    $vstale = -not (Test-Path $fxIni)
    if (-not $vstale) {
        $made = (Get-Item $fxIni).LastWriteTime
        $vstale = @(Get-ChildItem $vfxSrc -File | Where-Object { $_.LastWriteTime -gt $made }).Count -gt 0 -or (Test-ToolsNewer $made)
    }
    if ($vstale) {
        Write-Host "Converting his weapons' effect art from $vfxSrc..."
        Invoke-Py "converting the effect art" ($py + @((Join-Path $tools "ratchet_vfx_to_skyrim.py"), "--vfx", $vfxSrc, "--out", $vfxOut))
    }
    # No stale files from an older conversion.
    foreach ($old in "meshes\RatchetMovement\fx", "textures\RatchetMovement\fx") {
        $p = Join-Path $ModDir $old
        if (Test-Path $p) { Remove-Item -Recurse -Force $p }
    }
    Copy-Item -Recurse -Force (Join-Path $vfxOut "*") $ModDir
    Write-Host "His weapons' effects from $vfxOut"
} else {
    Write-Warning "No $vfxSrc`: no effect art for his weapons (tools\extract_riftapart_vfx.py makes it)"
}

# Rift Apart's main menu and startup video (skse\tools\ratchet_menu_to_skyrim.py,
# ratchet_intro_to_skyrim.py): its logo where Skyrim's emblem is, its menu music, and its title
# card as the video at launch, from riftapart-assets\menu (tools\extract_riftapart_menu.py makes
# it); made again when the extraction or a tool is newer.
$menuSrc = Join-Path $Assets "menu"
$menuOut = Join-Path $Work "menu"
if (Test-Path $menuSrc) {
    $menuTool = Join-Path $tools "ratchet_menu_to_skyrim.py"
    $introTool = Join-Path $tools "ratchet_intro_to_skyrim.py"
    $mstale = -not (Test-Path $menuOut)
    if (-not $mstale) {
        $made = @(Get-ChildItem $menuOut -Recurse -File | Sort-Object LastWriteTime | Select-Object -First 1)
        $made = if ($made) { $made[0].LastWriteTime } else { [datetime]::MinValue }
        $mstale = @(Get-ChildItem $menuSrc -File | Where-Object { $_.LastWriteTime -gt $made }).Count -gt 0 -or
            (Get-Item $menuTool).LastWriteTime -gt $made -or (Get-Item $introTool).LastWriteTime -gt $made
    }
    if ($mstale) {
        Write-Host "Converting Rift Apart's menu from $menuSrc..."
        if (Test-Path $menuOut) { Remove-Item -Recurse -Force $menuOut }
        Invoke-Py "converting the menu" ($pySound + @($menuTool, "--menu", $menuSrc, "--out", $menuOut))
        if ($RadVideo -and (Test-Path $RadVideo)) {
            Write-Host "Making the startup video with $RadVideo..."
            Invoke-Py "making the startup video" ($pySound + @($introTool, "--menu", $menuSrc, "--rad", $RadVideo, "--out", $menuOut, "--work", (Join-Path $Work "intro-frames")))
        } else {
            Write-Warning "No RAD Video Tools (-RadVideo): Skyrim's own startup video"
        }
    }
    foreach ($old in "meshes\interface\logo", "Video") {
        $p = Join-Path $ModDir $old
        if (Test-Path $p) { Remove-Item -Recurse -Force $p }
    }
    if (Test-Path $menuOut) {
        Copy-Item -Recurse -Force (Join-Path $menuOut "*") $ModDir
        Write-Host "Rift Apart's menu from $menuOut"
    }
} else {
    Write-Warning "No $menuSrc`: Skyrim's own main menu (tools\extract_riftapart_menu.py makes it)"
}
