# SPDX-License-Identifier: GPL-3.0-or-later
# Tests the plugin in the real game with nobody at the keyboard.
#
#   powershell -ExecutionPolicy Bypass -File skse\test\ingame.ps1            # build, install, test
#   powershell -ExecutionPolicy Bypass -File skse\test\ingame.ps1 -NoBuild   # test what's installed
#
# Drops a one-shot RatchetMovement\autotest.ini into the MO2 mod, starts SKSE through MO2 (the
# profile MO2 has selected), and follows RatchetMovement.log. From the main menu the plugin loads
# the most recent save and runs `coc Whiterun` (no save is written; autosaves are off for the
# run), makes its jumps, logs PASS/FAIL for each against the sandbox replaying the same frames,
# and quits the game. Exit code 0 when everything passed.
param(
    [switch]$NoBuild,
    # Where to test: the most recent save is loaded first (a new game asks AE's survival-mode
    # question), then this coc runs; "load" stays where the save is. qasmoke is flat (the landing
    # height is checked there) but too small for the running jump.
    [string]$Start = "coc Whiterun",
    [int]$TimeoutSec = 300,
    # Seconds he stands still after the last jump before the game quits (time for screenshots).
    [int]$HoldEnd = 0,
    # Steps first that find what hangs the renderer (third person alone, Ratchet hidden, each of
    # his shapes alone, a camera switch), each logged as it starts.
    [switch]$Diagnose,
    # Skyrim's own window (as the profile sets it, full screen) instead of a small one in the
    # corner behind the other windows.
    [switch]$Fullscreen,
    [switch]$NoShots,
    # Only the cases whose names contain this ("ledge", "wall"...), or any of several: "guard|bear".
    [string]$Only = "",
    # No per-frame trace in the log (on by default for test runs).
    [switch]$NoTrace,
    # Ratchet casts no shadows (bisecting the renderer hang).
    [switch]$NoShadows,
    # Log the engine's Map calls from the site the renderer hang sits in.
    [switch]$WatchMap,
    # The third-person camera's turn round him from behind, radians (2.5: in front, three quarters).
    [double]$CameraYaw = 0,
    # Record Skyrim's own audio (its process only) to audio.wav with the screenshots, and check it
    # for clipping and dropouts (test/audio_check.py).
    [switch]$Audio,
    # MO2's folder, the mod's folder in it (default: <Mo2>\mods\Ratchet Movement), and Visual
    # Studio for the audio recorder: from skse\local.psd1 when not given.
    [string]$VsDir = "",
    [string]$Mo2 = "",
    [string]$ModDir = "",
    [string]$Executable = "SKSE"
)
$ErrorActionPreference = "Stop"

$localFile = Join-Path $PSScriptRoot "..\local.psd1"
if (Test-Path $localFile) {
    $local = Import-PowerShellDataFile $localFile
    foreach ($k in "VsDir", "Mo2", "ModDir") {
        if (-not (Get-Variable $k -ValueOnly) -and $local[$k]) { Set-Variable $k $local[$k] }
    }
}
if (-not $Mo2) { throw "MO2's folder: pass -Mo2 or set Mo2 in skse\local.psd1" }
if (-not $ModDir) { $ModDir = Join-Path $Mo2 "mods\Ratchet Movement" }

if (Get-Process SkyrimSE -ErrorAction SilentlyContinue) { throw "Skyrim is already running: close it first" }
if (-not $NoBuild) {
    powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "..\build.ps1") -Install | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
}

$docs = [Environment]::GetFolderPath("MyDocuments")
$sksedir = Join-Path $docs "My Games\Skyrim Special Edition\SKSE"
$log = Join-Path $sksedir "RatchetMovement.log"
$trigger = Join-Path $ModDir "SKSE\Plugins\RatchetMovement\autotest.ini"
# Only the test room's floor is flat enough to check the landing height on.
$flat = if ($Start -match "qasmoke") { 1 } else { 0 }
# The plugin's screenshots of this run (idle, each jump's top, the end).
$shots = Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path ("target\skse\shots\" + (Get-Date -Format "yyyyMMdd-HHmmss"))
New-Item -ItemType Directory -Force $shots | Out-Null
[IO.File]::WriteAllText($trigger, "[AutoTest]`r`nsStart=$Start`r`nbFlatGround=$flat`r`nbQuit=1`r`nfHoldEnd=$HoldEnd`r`nsShots=$shots`r`nbDiagnose=$([int][bool]$Diagnose)`r`nbCorner=$([int](-not $Fullscreen))`r`nbShots=$([int](-not $NoShots))`r`nbNoShadows=$([int][bool]$NoShadows)`r`nbWatchMap=$([int][bool]$WatchMap)`r`nfCameraYaw=$($CameraYaw.ToString([Globalization.CultureInfo]::InvariantCulture))`r`nsOnly=$Only`r`nbTraceJumps=$([int](-not $NoTrace))`r`n")
$TimeoutSec += $HoldEnd
$cdb = "C:\Program Files (x86)\Windows Kits\10\Debuggers\x64\cdb.exe"

# The audio recorder, built when it's older than its source.
$recorder = Join-Path $PSScriptRoot "..\..\target\skse\tools\audio_capture.exe"
if ($Audio) {
    $source = Join-Path $PSScriptRoot "audio_capture.cpp"
    if (-not (Test-Path $recorder) -or (Get-Item $source).LastWriteTime -gt (Get-Item $recorder).LastWriteTime) {
        $dir = Split-Path $recorder
        New-Item -ItemType Directory -Force $dir | Out-Null
        if (-not $VsDir) {
            $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
            if (Test-Path $vswhere) { $VsDir = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath }
            if (-not $VsDir) { throw "Visual Studio not found for the audio recorder: pass -VsDir" }
        }
        $vcvars = Join-Path $VsDir "VC\Auxiliary\Build\vcvars64.bat"
        cmd /c "`"$vcvars`" >nul 2>nul && cl /nologo /EHsc /O2 /std:c++20 `"$source`" /Fo`"$dir\\`" /Fe`"$recorder`" ole32.lib mmdevapi.lib" | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "couldn't build the audio recorder" }
    }
}
$recording = $null

$launched = Get-Date
$result = $null
$proc = $null
try {
    Write-Host "Starting $Executable through MO2..."
    Start-Process -FilePath (Join-Path $Mo2 "ModOrganizer.exe") -ArgumentList "`"moshortcut://:$Executable`""
    $deadline = $launched.AddSeconds($TimeoutSec)
    while (-not $proc -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $proc = Get-Process SkyrimSE -ErrorAction SilentlyContinue | Select-Object -First 1
    }
    if (-not $proc) { throw "Skyrim didn't start within $TimeoutSec s" }
    Write-Host "Skyrim is running (pid $($proc.Id))"
    if ($Audio) {
        $recording = Start-Process -FilePath $recorder -ArgumentList $proc.Id, "`"$(Join-Path $shots 'audio.wav')`"", $TimeoutSec -PassThru -WindowStyle Hidden
    }

    $shown = 0
    $stuckSince = $null
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 300
        # Hung (its window stops answering): the main thread's stack, then stop it.
        $proc.Refresh()
        if (-not $proc.HasExited -and -not $proc.Responding) {
            if (-not $stuckSince) { $stuckSince = Get-Date }
            if (((Get-Date) - $stuckSince).TotalSeconds -gt 15) {
                Write-Host "HANG: Skyrim stopped responding"
                if (Test-Path $cdb) {
                    & $cdb -pv -p $proc.Id -c "~0 k 40; q" 2>&1 | Out-File (Join-Path $shots "hang.txt")
                    Write-Host "  main thread's stack in $(Join-Path $shots 'hang.txt')"
                }
                Stop-Process -Id $proc.Id -Force
                $result = @{ passed = 0; failed = 1; hang = $true }
                break
            }
        } else {
            $stuckSince = $null
        }
        if ((Test-Path $log) -and (Get-Item $log).LastWriteTime -gt $launched) {
            $lines = @(Get-Content $log | Where-Object { $_ -match "autotest:|ratchet model|critical|\[E\]" })
            for (; $shown -lt $lines.Count; $shown++) { Write-Host ($lines[$shown] -replace '^(\[[^\]]*\] ){3}', '') }
            $done = $lines | Where-Object { $_ -match "autotest: done, (\d+) passed, (\d+) failed" } | Select-Object -First 1
            if ($done) {
                $null = $done -match "autotest: done, (\d+) passed, (\d+) failed"
                $result = @{ passed = [int]$matches[1]; failed = [int]$matches[2] }
                break
            }
        }
        if ($proc.HasExited) { Write-Host "Skyrim exited before the test finished (exit code $($proc.ExitCode))"; break }
    }
    if (-not $result -and -not $proc.HasExited) { Write-Host "Timed out after $TimeoutSec s" }
    if (-not $proc.HasExited -and -not $proc.WaitForExit(30000)) {
        Write-Host "Skyrim didn't quit: stopping pid $($proc.Id)"
        Stop-Process -Id $proc.Id -Force
    }
} finally {
    if (Test-Path $trigger) { Remove-Item $trigger -Force }
}

# The recorder stops when Skyrim does.
if ($recording) {
    if (-not $recording.WaitForExit(30000)) { Stop-Process -Id $recording.Id -Force }
    $wav = Join-Path $shots "audio.wav"
    if (Test-Path $wav) {
        uv run --quiet --with numpy python (Join-Path $PSScriptRoot "audio_check.py") $wav $log | Out-Host
    } else {
        Write-Host "No audio recorded"
    }
}

# CrashLogger writes crash-*.log next to the SKSE logs.
$crash = Get-ChildItem $sksedir -Filter "crash-*.log" -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -gt $launched } | Select-Object -First 1
if ($crash) {
    Write-Host "CRASH: $($crash.FullName)"
    Get-Content $crash.FullName -TotalCount 25 | Out-Host
    exit 2
}
if (-not $result) { exit 3 }
$pngs = @(Get-ChildItem $shots -Filter *.png -ErrorAction SilentlyContinue)
if ($pngs.Count) { Write-Host "Screenshots: $shots ($($pngs.Count))" }
if ($result.hang) { exit 4 }
Write-Host ("Result: {0} passed, {1} failed" -f $result.passed, $result.failed)
if ($result.failed -gt 0) { exit 1 }
exit 0
