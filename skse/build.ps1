# SPDX-License-Identifier: GPL-3.0-or-later
# Builds the SKSE plugin (and ratchet.dll through cargo) and optionally installs it as an MO2 mod.
#
#   powershell -File skse\build.ps1            # configure + build
#   powershell -File skse\build.ps1 -Install   # ... then copy into the MO2 mod folder, and enable
#                                              # RatchetMovement.esl in the MO2 profile
#
# Output: target\skse\cmake\stage\SKSE\Plugins (RatchetMovement.dll + .pdb, ratchet.dll, the INI),
# target\skse\plugin\RatchetMovement.esl (his guns as Skyrim weapons).
param(
    [switch]$Install,
    [string]$ModDir = "G:\Modding\MO2\mods\Ratchet Movement",
    [string]$Mo2Profile = "Spire",
    # RAD Video Tools' encoder (the owner's download: radgametools.com, unpacked from its
    # installer), for the startup video; without it, Skyrim's own plays.
    [string]$RadVideo = "G:\dev\tools\RADTools\bin\radvideo64.exe",
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
    # Tools that aren't the models' or the effects' converters (their changes don't make those stale).
    $notConverters = @("ratchet_plugin.py", "ratchet_menu_to_skyrim.py", "ratchet_intro_to_skyrim.py")
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
            $stale = (Get-Item $glb).LastWriteTime -gt $made -or @(Get-ChildItem $tools -Filter *.py | Where-Object { $_.Name -notin $notConverters -and $_.LastWriteTime -gt $made }).Count -gt 0
        }
        if ($stale) {
            Write-Host "Converting Ratchet from $glb..."
            uv run --python 3.12 --with numpy --with pillow (Join-Path $tools "ratchet_to_skyrim.py") --glb $glb --out $converted | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "converting Ratchet failed" }
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
            $wglb = Join-Path (Split-Path $glb) "$(if ($w.Glb) { $w.Glb } else { $w.Name }).glb"
            $wnif = Join-Path $converted "meshes\RatchetMovement\$($w.Name).nif"
            if (-not (Test-Path $wglb)) { Write-Warning "No $wglb`: $($w.Without)"; continue }
            $wstale = -not (Test-Path $wnif)
            if (-not $wstale) {
                $made = (Get-Item $wnif).LastWriteTime
                $wstale = (Get-Item $wglb).LastWriteTime -gt $made -or @(Get-ChildItem $tools -Filter *.py | Where-Object { $_.Name -notin $notConverters -and $_.LastWriteTime -gt $made }).Count -gt 0
            }
            if ($wstale) {
                Write-Host "Converting $($w.Name) from $wglb..."
                $clipArgs = @()
                if (-not $w.Clips) { $clipArgs += "--no-clips" }
                if ($w.TextureDir) { $clipArgs += @("--texture-dir", $w.TextureDir) }
                if ($w.Pose) { $clipArgs += @("--pose", $w.Pose) }
                if ($w.Loadscreen) { $clipArgs += "--loadscreen" }
                uv run --python 3.12 --with numpy --with pillow (Join-Path $tools "ratchet_to_skyrim.py") --glb $wglb --out $converted --name $w.Name --prefix $w.Prefix @clipArgs | Out-Host
                if ($LASTEXITCODE -ne 0) { throw "converting $($w.Name) failed" }
            }
        }
        # His sounds (skse\tools\ratchet_sounds_to_skyrim.py): WAV files and sounds.txt, converted
        # again when the extraction's manifest or the tool is newer.
        $soundsSrc = Join-Path (Split-Path $glb) "sounds"
        $soundsOut = Join-Path $here "..\target\skse\sounds"
        $soundsTxt = Join-Path $soundsOut "SKSE\Plugins\RatchetMovement\sounds.txt"
        if (Test-Path (Join-Path $soundsSrc "sounds.json")) {
            $sstale = -not (Test-Path $soundsTxt)
            if (-not $sstale) {
                $made = (Get-Item $soundsTxt).LastWriteTime
                $sstale = (Get-Item (Join-Path $soundsSrc "sounds.json")).LastWriteTime -gt $made -or (Get-Item (Join-Path $tools "ratchet_sounds_to_skyrim.py")).LastWriteTime -gt $made
            }
            if ($sstale) {
                Write-Host "Converting his sounds from $soundsSrc..."
                uv run --python 3.12 --with numpy --with soundfile (Join-Path $tools "ratchet_sounds_to_skyrim.py") --sounds $soundsSrc --out $soundsOut | Out-Host
                if ($LASTEXITCODE -ne 0) { throw "converting the sounds failed" }
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
        Write-Warning "No riftapart-assets\ratchet.glb: no Ratchet model (tools\extract_riftapart_ratchet.py makes it)"
    }

    # His weapons' effect art (skse\tools\ratchet_vfx_to_skyrim.py): textures, the effect pieces'
    # NIF templates and fx.ini's palette, converted again when the extracted art or the tools are
    # newer than the last conversion.
    $vfxSrc = Join-Path (Find-GameData "riftapart-assets") "vfx"
    $vfxOut = Join-Path $here "..\target\skse\vfx"
    $fxIni = Join-Path $vfxOut "SKSE\Plugins\RatchetMovement\fx.ini"
    if (Test-Path $vfxSrc) {
        $tools = Join-Path $here "tools"
        $vstale = -not (Test-Path $fxIni)
        if (-not $vstale) {
            $made = (Get-Item $fxIni).LastWriteTime
            $vstale = @(Get-ChildItem $vfxSrc -File | Where-Object { $_.LastWriteTime -gt $made }).Count -gt 0 -or @(Get-ChildItem $tools -Filter *.py | Where-Object { $_.Name -notin $notConverters -and $_.LastWriteTime -gt $made }).Count -gt 0
        }
        if ($vstale) {
            Write-Host "Converting his weapons' effect art from $vfxSrc..."
            uv run --python 3.12 --with numpy --with pillow (Join-Path $tools "ratchet_vfx_to_skyrim.py") --vfx $vfxSrc --out $vfxOut | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "converting the effect art failed" }
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
    # card as the video at launch, from riftapart-assets\menu (tools\extract_riftapart_menu.py
    # makes it); made again when the extraction or a tool is newer.
    $menuSrc = Join-Path (Find-GameData "riftapart-assets") "menu"
    $menuOut = Join-Path $here "..\target\skse\menu"
    if (Test-Path $menuSrc) {
        $menuTool = Join-Path $here "tools\ratchet_menu_to_skyrim.py"
        $introTool = Join-Path $here "tools\ratchet_intro_to_skyrim.py"
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
            uv run --python 3.12 --with numpy --with pillow --with soundfile $menuTool --menu $menuSrc --out $menuOut | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "converting the menu failed" }
            if (Test-Path $RadVideo) {
                Write-Host "Making the startup video with $RadVideo..."
                uv run --python 3.12 --with numpy --with pillow --with soundfile $introTool --menu $menuSrc --rad $RadVideo --out $menuOut --work (Join-Path $here "..\target\skse\intro-frames") | Out-Host
                if ($LASTEXITCODE -ne 0) { throw "making the startup video failed" }
            } else {
                Write-Warning "No $RadVideo`: Skyrim's own startup video (see -RadVideo)"
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

    # His guns as Skyrim weapons: the plugin file in the mod's root, enabled in the profile.
    Copy-Item (Join-Path $pluginOut "RatchetMovement.esl") $ModDir -Force
    Enable-Plugin (Join-Path (Split-Path (Split-Path $ModDir)) "profiles\$Mo2Profile") "RatchetMovement.esl"
    if (Get-Process ModOrganizer -ErrorAction SilentlyContinue) {
        Write-Host "MO2 is running: refresh it (F5) and check RatchetMovement.esl is ticked"
    }
    Write-Host "Installed to $dest"
}
