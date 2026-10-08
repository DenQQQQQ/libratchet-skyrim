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
- **Ratchet is the player:** his model, textures and move clips replace the Dragonborn in third
  person, converted from the owner's extraction (see [Ratchet's model](#ratchets-model)).
- **The jump, with air control.** Skyrim's jump is switched off and libratchet's single jump (the
  press and the hold) drives the player: up and down, and across with the stick (camera-relative,
  as in Rift Apart). Double jumps, glides and dashes are masked off: a press in the air does
  nothing. On the ground Skyrim still moves him.
- In game the jumps match the sandbox replaying the same frames: a full hold peaks at 2.325 m at
  Dragonborn scale (1.55) in both, shorter holds within 0.01 m, a running jump lands within
  0.11 m (one frame) of the replay (see [MODLOG.md](MODLOG.md)).

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
`SKSE\Plugins\RatchetMovement\` and Ratchet's converted model. These are game data: they stay on
this PC. It keeps an INI that's already there. Enable the mod in MO2's left pane and start Skyrim
through SKSE.

## Ratchet's model
`build.ps1 -Install` converts `riftapart-assets/ratchet.glb` (made by
`tools/extract_riftapart_ratchet.py`) with `skse/tools/ratchet_to_skyrim.py` (through `uv`, Python
3.12 with numpy and pillow) into `target/skse/ratchet`, when the glb or the tools are newer than
the last conversion, and copies it into the mod:
- `meshes/RatchetMovement/ratchet.nif`: his skeleton and skinned shapes (each with its own shader
  property: shapes sharing one hang Skyrim's renderer);
- `textures/RatchetMovement/*.dds`: colour, normal and specular maps;
- `SKSE/Plugins/RatchetMovement/ratchet.anim`: his move clips.

The plugin hangs him under the player's third-person 3D, hides the Dragonborn's meshes, and
picks and blends his clips by the sandbox's rules. INI: `bRatchet = 0` keeps the Dragonborn.
`skse/tools/nif.py` is the NIF reader/writer the converter uses.

## Checking the jump
At load the plugin runs libratchet's flat-floor jump on its own (the same test as the sandbox
and `crates/libratchet/tests/movement.rs`) and logs the apex for a tap and for a full hold. Every
jump in game then logs its apex, distance and landing next to a fresh sim's replay of the same
frames (time steps, button, stick) on a flat floor:

```
sandbox numbers at scale 1.55: tap apex 1.550 m at 0.267 s, full-hold apex 2.325 m at 0.367 s, air time 0.700 s
jump: apex 2.325 m at 0.369 s, 7.09 m across (sandbox 2.325 m at 0.361 s, 6.98 m for these frames), held 0.369 s, landed -0.949 m after 0.767 s
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
- From the main menu the plugin loads the most recent save, then runs `coc Whiterun` (`-Start`
  for another place; `-Start "coc qasmoke"` is flat but too small for the running jump). No save
  is written, and timed autosaves are off for the run (in memory only).
- It jumps by itself: a tap, 0.1 s, 0.15 s and a full hold, a standing jump steered forward, and
  a running jump. Before each one he's moved back to the start, facing the heading with the most
  room.
- Each jump gets a PASS or FAIL against a fresh sim replaying the same frames (time steps,
  button, stick) on a flat floor down to where he landed: apex within 0.03 m, apex time within
  0.025 s, distance within 0.2 m, no health lost (and back on the floor within 0.05 m in qasmoke).
- Then it quits the game. The exit code is 0 when everything passed, 1 on a failure, 2 on a crash
  (it prints CrashLogger's log), 3 when there's no result, 4 when the game hung (the main thread's
  stack goes to `hang.txt` first, through cdb).

The game runs in a small window in the corner behind other windows; `-Fullscreen` gives it the
screen. Screenshots (idle, each jump's top, the end) go to `target/skse/shots/<run>/`. Other
options: `-NoBuild`, `-HoldEnd <s>`, `-CameraYaw <radians>` (2.5 looks at his front), `-NoShots`.

## How it works
- **Input.** The keys bound to Skyrim's Jump control are polled each frame, and Skyrim's jump
  handler says no on foot (on a horse it's still the horse's jump). The move stick is turned by
  the camera into the world.
- **Each frame**, before Skyrim moves the player, the sim steps with the button, the stick,
  whether Skyrim's character controller is on the ground and how far down the ground is (a ray
  from his feet). On the ground Skyrim moves him and the sim starts from his speed across the
  ground. During the sim's jump its whole velocity goes to the controller
  (`SetLinearVelocityImpl`), with the controller's own gravity and air control (`acrobatics`)
  off. Skyrim does the collision. On landing, Skyrim takes over again.
- **Steps aside** when the player is mounted, swimming, in furniture, ragdolled, in a kill move,
  bleeding out or dead.
- **Units.** libratchet is Y-up metres; Skyrim is Z-up. Havok units are metres (Skyrim's Havok
  scale is 0.0142875, so a metre is about 70 game units).
