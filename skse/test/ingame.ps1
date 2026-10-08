# SPDX-License-Identifier: GPL-3.0-or-later
# Tests the plugin in the real game with nobody at the keyboard.
#
#   powershell -ExecutionPolicy Bypass -File skse\test\ingame.ps1            # build, install, test
#   powershell -ExecutionPolicy Bypass -File skse\test\ingame.ps1 -NoBuild   # test what's installed
#
# Drops a one-shot RatchetMovement\autotest.ini into the MO2 mod, starts SKSE through MO2 (the
# profile MO2 has selected), and follows RatchetMovement.log. From the main menu the plugin runs
# `coc qasmoke` (the flat test room: no save is loaded or written; autosaves are off for the run),
# makes its jumps, logs PASS/FAIL for each against the sandbox's numbers, and quits the game.
# Exit code 0 when everything passed.
param(
    [switch]$NoBuild,
    # Console command run from the main menu, or "load" for the most recent save.
    [string]$Start = "coc qasmoke",
    [int]$TimeoutSec = 300,
    [string]$Mo2 = "G:\Modding\MO2",
    [string]$ModDir = "G:\Modding\MO2\mods\Ratchet Movement",
    [string]$Executable = "SKSE"
)
$ErrorActionPreference = "Stop"

if (Get-Process SkyrimSE -ErrorAction SilentlyContinue) { throw "Skyrim is already running: close it first" }
if (-not $NoBuild) {
    powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "..\build.ps1") -Install | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
}

$docs = [Environment]::GetFolderPath("MyDocuments")
$sksedir = Join-Path $docs "My Games\Skyrim Special Edition\SKSE"
$log = Join-Path $sksedir "RatchetMovement.log"
$trigger = Join-Path $ModDir "SKSE\Plugins\RatchetMovement\autotest.ini"
$flat = if ($Start -eq "load") { 0 } else { 1 }
[IO.File]::WriteAllText($trigger, "[AutoTest]`r`nsStart=$Start`r`nbFlatGround=$flat`r`nbQuit=1`r`n")

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

    $shown = 0
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 300
        if ((Test-Path $log) -and (Get-Item $log).LastWriteTime -gt $launched) {
            $lines = @(Get-Content $log | Where-Object { $_ -match "autotest:|critical|\[E\]" })
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

# CrashLogger writes crash-*.log next to the SKSE logs.
$crash = Get-ChildItem $sksedir -Filter "crash-*.log" -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -gt $launched } | Select-Object -First 1
if ($crash) {
    Write-Host "CRASH: $($crash.FullName)"
    Get-Content $crash.FullName -TotalCount 25 | Out-Host
    exit 2
}
if (-not $result) { exit 3 }
Write-Host ("Result: {0} passed, {1} failed" -f $result.passed, $result.failed)
if ($result.failed -gt 0) { exit 1 }
exit 0
