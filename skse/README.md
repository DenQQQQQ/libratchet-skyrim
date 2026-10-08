# Ratchet Movement: the SKSE plugin

libratchet on Skyrim SE's player, through `ratchet.dll` (`crates/libratchet-ffi`). The plan is
[docs/skyrim-plan.md](../docs/skyrim-plan.md); this folder's log is [MODLOG.md](MODLOG.md).

**License: GPL-3.0-or-later.** The plugin links CommonLibSSE-NG, which is GPL-3.0-or-later with a
modding exception (Skyrim and SKSE). libratchet and `ratchet.dll` stay MIT; the rest of the
repository is unchanged.

## Status
- Loads in Skyrim SE **1.6.1170** (AE layouts only; SE 1.5.97 and VR are off in the build), logs to
  `Documents\My Games\Skyrim Special Edition\SKSE\RatchetMovement.log`, loads `ratchet.dll` from
  `SKSE\Plugins\RatchetMovement\` and checks the C API version.
- **The jump only.** Skyrim's jump is switched off and libratchet's single jump (the press and the
  hold) drives the player's height. Double jumps, glides and dashes are masked off: a press in the
  air does nothing. Moving sideways is still Skyrim's.
- In game the jump matches the sandbox: a full hold peaks at 2.325 m at Dragonborn scale (1.55),
  as in the sandbox, and shorter holds within about 0.03 m (see [MODLOG.md](MODLOG.md)).

## Build
Needs MSVC (VS 2022 Build Tools), CMake 3.25+, Rust, and vcpkg (`VCPKG_ROOT`, or
`G:\dev\tools\vcpkg`). Ninja comes from vcpkg.

```powershell
powershell -ExecutionPolicy Bypass -File skse\build.ps1            # build
powershell -ExecutionPolicy Bypass -File skse\build.ps1 -Install   # build and install into MO2
```

- cargo builds `ratchet.dll` into `target/skse` (its own target folder, so it doesn't wait on the
  sandbox's builds).
- CMake fetches CommonLibSSE-NG (alandtse's fork, tag v11.0.0) and builds it once (slow, about
  10 minutes).
- The build output is staged in `target/skse/cmake/stage/SKSE/Plugins`.

`-Install` copies it into `G:\Modding\MO2\mods\Ratchet Movement` (a separate mod, easy to switch
off), with the extracted Rift Apart configs from `riftapart-configs/` in
`SKSE\Plugins\RatchetMovement\`. These are game data: they stay on this PC. It keeps an INI that's
already there. Enable the mod in MO2's left pane and start Skyrim through SKSE.

## Checking the jump
At load the plugin runs libratchet's flat-floor jump on its own (the same test as the sandbox
and `crates/libratchet/tests/movement.rs`) and logs the apex for a tap and for a full hold. Every
jump in game then logs its takeoff, apex and landing next to those numbers:

```
sandbox numbers at scale 1.55: tap apex 1.550 m at 0.267 s, full-hold apex 2.325 m at 0.367 s, air time 0.700 s
jump: apex 2.325 m at 0.369 s (sandbox 2.325 m at 0.367 s for a 0.245 s hold), landed -0.120 m after 0.714 s
```

`[Debug] bTraceJumps = 1` logs every frame of every jump.

## Automated tests in the game
```powershell
powershell -ExecutionPolicy Bypass -File skse\test\ingame.ps1            # build, install, test (~1 min)
powershell -ExecutionPolicy Bypass -File skse\test\ingame.ps1 -NoBuild   # test what's installed
```
The script drops a one-shot `RatchetMovement\autotest.ini` into the MO2 mod (the plugin deletes
it at start, so the next start is a normal game), starts SKSE through MO2 with the profile MO2 has
selected, and follows the log.
- From the main menu the plugin runs `coc qasmoke`, the flat test room. No save is loaded or
  written, and timed autosaves are off for the run (in memory only).
- It jumps by itself: a tap, 0.1 s, 0.15 s and a full hold. Each jump gets a PASS or FAIL against
  the sandbox: apex within 0.05 m, apex time within 0.03 s, back on the floor within 0.05 m, no
  health lost.
- Then it quits the game. The exit code is 0 when everything passed, 1 on a failure, 2 on a crash
  (it prints CrashLogger's log), 3 when there's no result.

Skyrim takes the screen while it runs; alt-tabbing away is fine (the Spire profile has
`bAlwaysActive=1`).

## How it works
- **Input.** The keys bound to Skyrim's Jump control are polled each frame, and Skyrim's jump
  handler says no on foot (on a horse it's still the horse's jump).
- **Each frame**, before Skyrim moves the player, the sim steps with the button and whether
  Skyrim's character controller is on the ground. During the sim's jump its vertical speed goes
  to the controller (`SetLinearVelocityImpl`) with the controller's own gravity off. Skyrim does
  the collision. On landing, Skyrim takes over again.
- **Steps aside** when the player is mounted, swimming, in furniture, ragdolled, in a kill move,
  bleeding out or dead.
- **Units.** libratchet is Y-up metres; Skyrim is Z-up. Havok units are metres (Skyrim's Havok
  scale is 0.0142875, so a metre is about 70 game units).
