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
- **Clank rides on his back,** placed the way the sandbox and the game place him, playing his
  backpack idle. While Ratchet glides, Clank's heli-pack unfolds (top rotor and both arm rotors,
  spinning) and folds away when he lands.
- **Jump, double jump, glide, dash and air dash, with air control.** Skyrim's jump is switched
  off. libratchet gets the jump button as it is and drives the player in the air: the jump (the
  press and the hold), a second press for the double jump, holding on for the glide, and across
  with the stick (camera-relative, as in Rift Apart). The Sprint control dashes, on the ground
  and in the air (Skyrim's own sprint is off on foot). A double jump, glide or air dash also works
  out of a fall off a ledge. The sim walks and runs him on the ground too, with Rift Apart's quick
  starts (`bSimGround = 0` gives Skyrim's locomotion back).
- In game the moves match the sandbox replaying the same frames: at Ratchet's size (scale 1) a
  full hold peaks at 1.500 m in both, shorter holds within 0.02 m, the double jump at 2.800 m in
  both, a ground dash 7.00 m (6.99), and every landing within one frame of the replay (see
  [MODLOG.md](MODLOG.md)).
- A glide comes down softly: the height it sinks doesn't count toward fall damage.
- **He fights:** the OmniWrench (the sim's three-swing combo with its lunges, the Hyper-Strike in
  the air, the throw), the Burst Pistol (hip fire and aimed, strafing) and the Shatterbomb (hold
  fire to wind the throw up, let go to lob it onto the crosshair; its blast hurts everyone within
  6 m). Hits are spells cast by the player, so Skyrim's rules follow (the target fights back,
  crimes, kills, difficulty). The wrench and the pistol are in his hand when he uses them, the
  Shatterbomb's glove while it's picked.

## Controls
| | keyboard / mouse | gamepad | Skyrim control (INI) |
|---|---|---|---|
| jump, double jump, glide (hold) | Space | Y | Jump |
| dash, air dash | Left Alt | LB | Sprint (`sDashControl`) |
| wrench (press again for the combo; in the air: Hyper-Strike) | R | X | Ready Weapon (`sMeleeControl`) |
| fire the Burst Pistol (hold), or wind up a Shatterbomb (hold, let go to throw) | left mouse | RT | Right Attack/Block (`sFireControl`) |
| pick the Burst Pistol / the Shatterbomb | 1 / 2 | D-pad left / right | Hotkey1 / Hotkey2 (`sPistolControl`, `sShatterbombControl`) |
| aim | right mouse | LT | Left Attack/Block (`sAimControl`) |
| throw the wrench | Z | RB | Shout (`sThrowControl`) |
| let go of a ledge | Ctrl | left stick | Sneak |

On foot, Skyrim's own jump, sprint, attacks, blocks, weapon drawing and shouts are off (`bCombat
= 0` gives the fighting back), and so are the favorites on the two hotkeys that pick his guns. On
a horse everything is Skyrim's.

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

The same tool converts `wrench.glb` and `burstpistol.glb` (models only) and `clank.glb`
(`clank.nif` with his heli-pack's shapes named `RK_heli_*`, and `clank.anim` with his backpack
clips).

The plugin hangs him under the player's third-person 3D, hides the Dragonborn's meshes, and
picks and blends his clips by the sandbox's rules. Clank hangs on Ratchet's `ClankAttach` bone.
INI: `bRatchet = 0` keeps the Dragonborn, `bClank = 0` leaves Clank off. `skse/tools/nif.py` is
the NIF reader/writer the converter uses.

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
- It moves by itself: a tap, 0.1 s, 0.15 s and a full hold, a standing jump steered forward, a
  running jump, a double jump, a glide, a ground dash and an air dash. Before each one he's moved
  back to the start, facing the heading with the most room.
- A wall jump: a wall piece is placed across his way 2 m ahead. He has to stick to it, jump off
  it on a new press, and land at least a metre further out than where he stuck. A jump at a
  bear: he mustn't take its body for a wall. Last, a ledge: from a platform placed 4 m up he
  jumps at a capped wall 3 m ahead, and has to catch its lip and climb up on a new press.
- Then it fights a cow (placed ahead with its AI off and 1000 health): the wrench combo, a
  Hyper-Strike, the pistol from the hip and aimed, a wrench throw and a Shatterbomb. Each passes
  if the cow lost at least the expected damage (× the save's difficulty multiplier) from enough
  hits.
- Each jump gets a PASS or FAIL against a fresh sim replaying the same frames (time steps,
  button, stick) on a flat floor down to where he landed: apex within 0.03 m, apex time within
  0.025 s, air time within 0.03 s, distance within 0.2 m, no health lost (and back on the floor
  within 0.05 m in qasmoke). A ground dash: distance within 0.25 m, time the same. The glide
  also has to bring Clank's heli-pack out.
- Then it quits the game. The exit code is 0 when everything passed, 1 on a failure, 2 on a crash
  (it prints CrashLogger's log), 3 when there's no result, 4 when the game hung (the main thread's
  stack goes to `hang.txt` first, through cdb).

The game runs in a small window in the corner behind other windows; `-Fullscreen` gives it the
screen. Screenshots (idle, each jump's top, double jump and glide, the heli-pack out, the end) go to
`target/skse/shots/<run>/`. Other
options: `-NoBuild`, `-HoldEnd <s>`, `-CameraYaw <radians>` (2.5 looks at his front), `-NoShots`,
`-Only <name>` (only the cases whose names contain it).

## How it works
- **Input.** The keys bound to Skyrim's Jump control and to the dash's (INI `sDashControl`,
  Sprint by default) are polled each frame. Skyrim's jump and sprint handlers say no on foot (on a
  horse they're still the horse's). The move stick is turned by the camera into the world.
- **Each frame**, before Skyrim moves the player, the sim steps with the button, the stick,
  whether Skyrim's character controller is on the ground and how far down the ground is (a ray
  from his feet). On the ground Skyrim moves him and the sim starts from his speed across the
  ground. From the sim's jump (or a double jump or glide out of Skyrim's own fall) to the landing,
  its whole velocity goes to the controller (`SetLinearVelocityImpl`), with the controller's own
  gravity and air control (`acrobatics`) off. Skyrim does the collision. On landing, Skyrim takes
  over again.
- **Ground dash.** Skyrim's on-ground state would move him by its own locomotion, so for the
  sim's ground dash the controller is held in its air state every frame, with the sim's velocity
  pressing him down onto the ground. A ray from his feet tells the sim he's still on the ground
  (within 0.3 m).
- **Animations.** With `bRatchet = 1` his own clips play. Skyrim's behaviour graph (the hidden
  Dragonborn's, or the visible one with `bRatchet = 0`) gets its jump events: a takeoff for each
  jump and double jump, `JumpFall` at each top (alternated with `JumpFallDirectional` every 4 s,
  since the vanilla fall lands itself at 5.2 s), and the matching landing (INI `bAnimations`).
- **Steps aside** when the player is mounted, swimming, in furniture, ragdolled, in a kill move,
  bleeding out or dead.
- **Units.** libratchet is Y-up metres; Skyrim is Z-up. Havok units are metres (Skyrim's Havok
  scale is 0.0142875, so a metre is about 70 game units).
