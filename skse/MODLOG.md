# MODLOG: the SKSE plugin

The plugin's own log, kept in `skse/` while it's built on the `skse` branch (the repo-wide
`MODLOG.md` gets a summary when the branch is merged).

## 2026-10-08: the plugin loads and links ratchet.dll

### Tooling (installed with the owner's OK)
- vcpkg at `G:\dev\tools\vcpkg` (baseline 89be95ec), Ninja from it (`vcpkg fetch ninja`).
- CommonLibSSE-NG: **alandtse's fork, tag v11.0.0** (released 2026-10-04), fetched by CMake. It's
  the maintained NG line (CharmedBaryon's original stopped in 2024). Built for AE only (1.6.x,
  both struct layouts); SE 1.5.97 and VR are switched off.
- **License:** that fork is GPL-3.0-or-later with a modding exception, so `skse/` is
  GPL-3.0-or-later. An MIT fork exists (eddoursul/CommonLibSSE-GG) if that ever matters.
- vcpkg deps: spdlog, directxtk, rapidcsv (what CommonLib's CMake asks for), triplet
  x64-windows-static-md.

### Built
- `RatchetMovement.dll`: logs to `My Games\Skyrim Special Edition\SKSE\RatchetMovement.log`,
  reads `RatchetMovement.ini`, loads `ratchet.dll` from its own folder (delay-loaded: SKSE
  loads plugins by full path, which doesn't put their folder on the DLL search path), and
  checks `ratchet_api_version()` against the header.
- At load it runs libratchet's flat-floor jump on its own (tap and full hold, 1/60 s ticks) and
  logs the apex: the numbers the in-game jump is checked against.
- `skse/build.ps1`: cargo builds `ratchet.dll` into `target/skse`, CMake builds the plugin,
  `-Install` copies it and the extracted configs into the MO2 mod `Ratchet Movement`.
- Gotcha: a header named `Ratchet.h` in `skse/src` shadowed `ratchet.h` (Windows paths ignore
  case). The plugin's is `RatchetDll.h`.
- Gotcha: `StructCompatibility::Dependent` makes SKSE 2.2.6 refuse the plugin on 1.6.1170 ("only
  compatible with versions earlier than 1.6.629"). CommonLib picks the layout at run time, so the
  plugin declares `Independent`.
- `ratchet.dll` lives in `SKSE/Plugins/RatchetMovement/` with the configs: SKSE tries every DLL in
  `SKSE/Plugins` as a plugin (it logged "no version data" for it).

### Confirmed in game (owner's playtest, Skyrim 1.6.1170, Spire profile)
- The plugin loads, logs, loads `ratchet.dll` (C API 1) and the owner's Rift Apart configs.
- Jump is bound to keyboard 0x39 (Space) and gamepad 0x8000 (Y in XInput's numbering, Skyrim's
  default jump button; first written down here as A).
- **The jump matches the sandbox.** 25 jumps at scale 1.55, the game running at about 100 fps:
  - every full hold peaked at **2.325 m** (sandbox: 2.325 m) at 0.368-0.374 s (sandbox 0.367 s);
  - shorter holds land within about 0.03 m of the sandbox's apex for the same hold (e.g.
    1.889 vs 1.893 m for 0.109 s, 1.747 vs 1.750 m for 0.075 s). The rest is the hold's frame
    quantisation (the reference runs at 1/60 s);
  - air time on flat ground 0.71-0.74 s (sandbox 0.700 s; the ground wasn't flat).
- **How the speed gets in:** each frame, before `PlayerCharacter::Update` (vfunc 0xAD), the sim
  steps and its vertical speed goes to the controller with `SetLinearVelocityImpl`, with the
  controller's own `gravity` at 0 while the jump is the sim's. The takeoff forces
  `context.currentState = kInAir` and sets `kJumping`. Per frame, the height Skyrim actually moved
  matches the commanded speed within the noise of the timing, rising and falling (no lag, no
  fight).
- Skyrim's jump is blocked by `JumpHandler::CanProcess` (vfunc 1) returning false on foot. The
  button is read by polling the bound key on each device (the keyboard's DirectInput state
  directly: CommonLib's `BSWin32KeyboardDevice::IsPressed` doesn't link).

### Fixed after the playtest
- **Every jump killed him.** `fallStartHeight` is in game units (Skyrim writes the Havok height ×
  69.99 there); the plugin wrote the Havok height, so each landing counted as a ~60 m fall. Now
  falls count from the jump's highest point, in game units, as Skyrim's own jumps do.
- **Coming up level with a ledge.** Twice he rose onto something about 1 m up: Skyrim stood him
  on it, but the sim still had him rising and kept pushing up for about 0.2 s. Skyrim's "on the
  ground" while the sim is going up (after the first 0.05 s) now zeroes the sim's vertical speed,
  so it lands.
- **Ceilings:** rising less than a quarter of what the sim asked for in a frame is reported as
  `hit_ceiling`.
- Skyrim's jump animations are sent with the sim's jumps (`JumpStandingStart` or
  `JumpDirectionalStart` at the takeoff, `JumpFall` at the apex, `JumpLand` on landing), as
  ALYSLC does (INI `bAnimations`).

### Second playtest (the fixes, with the animations)
- 15 jumps, no deaths: the fall start follows him up and never drops below the takeoff.
- Full holds still peak at 2.325 m. The jump animation's own takeoff doesn't disturb the speed:
  in flight, the height Skyrim moved him matches the sim's speed within 0.2 m/s every frame
  (the takeoff frame within 1.8 m/s, frame-time jitter). The last 2-3 frames before the
  controller reports the landing are the ground stopping him.
- The owner sees Skyrim's jump animation on the sim's jumps.

### Automated tests in the game (`skse/test/ingame.ps1`)
- The owner asked for automated tests instead of playtests. A one-shot trigger file puts the
  plugin in test mode. From the main menu it runs `coc qasmoke` through a console `Script`
  (`CompileAndRun`), jumps by itself (the button pressed in code), checks each landing against
  the sandbox and quits (`Main::quitGame`). The script starts SKSE through MO2
  (`moshortcut://:SKSE`, handed to the running MO2) and follows the log, which is flushed on
  every info line.
- First runs: 4/4 pass in qasmoke. Tap 1.550 m (sandbox 1.550), 0.1 s hold 1.877-1.881 m
  (1.893), 0.15 s 2.107-2.115 m (2.110), full hold 2.325 m (2.325). All landed back on the floor at
  +0.000 m with no health lost. 58 s for build + install + game + test.
- No save was written. Timed autosaves are switched off in memory for the run. The bSaveOn*
  switches aren't in Skyrim's INI collections under those names; `coc` from the main menu
  doesn't save anyway.
- Not covered yet: ceilings, rising onto a ledge, running jumps (they need placed geometry or
  movement input in the test).

### Choices (assumptions, not from the game)
- Jump only: a press counts if it began on the ground, and the sim stops seeing the button once
  the jump turns down, so the sim's double jump and glide can't start. No press buffer before
  landing (Skyrim has none either).
- Fall damage stays Skyrim's, counted from the apex: a jump never hurts more than stepping off
  the same height would. (The plan said to switch fall damage off during Ratchet moves; for the
  plain jump this keeps Skyrim's rules.)
- Sideways movement in the air is still Skyrim's.

## 2026-10-08: Ratchet on the player

### His model, textures and clips
- `skse/tools/ratchet_to_skyrim.py` converts the owner's `ratchet.glb` (riftapart-assets) into
  `meshes/RatchetMovement/ratchet.nif` (205 bones, 22 skinned shapes), 30 DDS textures and
  `ratchet.anim` (99 move clips, the plugin's own RANM format). These are game data: they're built
  into `target/skse/ratchet` and installed into the MO2 mod only. `build.ps1 -Install` converts
  again when the glb or the tools are newer.
- `skse/tools/nif.py` reads and writes Skyrim SE NIFs. Checked byte for byte against CBBE's body,
  hands and feet, which settled the conventions:
  - rotations are stored row-major and applied as R·v + t;
  - vertex positions are always full floats;
  - a skinned BSTriShape has no vertices of its own (they're in NiSkinPartition's global vertex
    data), and the partitions' triangles index those global vertices;
  - bone indices in the vertex data count over the whole skin, each partition's bone list only
    over that partition.
- The plugin hangs him under the player's third-person 3D (`BSModelDB::Demand`), hides the
  Dragonborn's own meshes every frame, samples his clips onto his joints and turns him to face
  where he goes. Clips are picked by the sandbox's rules (`apps/sandbox/src/ratchet_model.rs`,
  ported to C++): idle, walk and run matched to his speed, jump, fall, landings, with the
  sandbox's fade times.

### The freeze
- With his model shown the game froze in the renderer. A test-only hook on d3d11 `Map` counted
  13 million 3840-byte constant-buffer maps in one frame. The converter had shared one
  `BSLightingShaderProperty` between shapes with the same material. A shader property holds its
  shape's render passes, so shapes that shared one looped forever. Every shape now has its own
  shader property, texture set and alpha property.
- Texture names that differed only in case (`pants_c` / `Pants_c`) were one file on Windows, so
  his gloves and pants showed the wrong textures (the owner saw it). Each image's name now starts
  with its index.

### Tests load a save, and replay each jump's frames
- Starting a new game from the main menu asks AE's survival-mode question, which stopped the
  tests. They now load the most recent save, then run the `coc`. `LoadMostRecentSaveGame` finds
  nothing until `PopulateSaveList` has run.
- Each jump records its frames (time step, button, stick), and a fresh sim replays exactly those
  frames on a flat floor. Against the replay every hold lands within 0.008 m (full hold 2.325 m
  in both). The old comparison, against a fixed 1/240 s run, was up to 0.03 m off on short holds,
  and all of that was frame timing.
- Runs show a small window in the corner behind the owner's windows (`-Fullscreen` for Skyrim's
  own). Screenshots (idle, each jump's top, the end) go to `target/skse/shots/<run>/`, read back
  without waiting on the GPU. A hang is caught by the runner: main-thread stack through cdb, then
  kill.

## 2026-10-08: air control

### What changed
- **The stick goes to the sim, turned by the camera** (Rift Apart's controls are
  camera-relative). The move input from `PlayerControls` is turned by the camera's flat forward
  and right into the world, and the camera's forward goes in as the sim's view.
- **In the air the whole velocity is the sim's**, across as well as up. Skyrim's in-air state
  pulls the speed across the ground toward its own locomotion (the controller's `acrobatics`, 1
  in vanilla); that's 0 while the jump is the sim's, like `gravity`.
- **On the ground Skyrim still moves him**, and the sim starts from his real speed across the
  ground, so a running jump keeps it. Not up and down: standing, Skyrim's vertical speed can sit
  a hair above zero. The sim lands only when it isn't rising, so after a teleport it stayed in
  its fall and refused every jump.
- **Ground distance:** a ray down from his feet (a Havok pick on the character-controller layer,
  in his own collision group). The sim's double jump, glide and falls ask for it.
- His model faces the sim's facing.

### Tests
- Two new cases: a standing jump steered forward (stick forward from the takeoff) and a running
  jump (0.8 s run-up).
- Before each case he's moved back to the start, facing the heading with the most room. 16
  headings are checked for a wall at chest height, a ceiling under 4.3 m, and a low wall, step or
  drop on the way.
- qasmoke's open floor is about 7 m long, and the running jump hit its ceiling there. Whiterun
  has room (`-Start "coc Whiterun"`, now the default) but isn't flat, so the replay comes down
  onto a floor at the height the game landed him at.
- **6/6 pass in Whiterun:**

  | case | apex, m (replay) | across, m (replay) | landed, m | air time, s (replay) |
  |---|---|---|---|---|
  | tap | 1.558 (1.550) | 0 | +0.008 | 0.555 (0.546) |
  | 0.1 s hold | 1.884 (1.884) | 0 | +0.008 | 0.613 (0.603) |
  | 0.15 s hold | 2.109 (2.109) | 0 | +0.008 | 0.663 (0.653) |
  | full hold | 2.325 (2.325) | 0 | -0.025 | 0.715 (0.705) |
  | standing, steered forward | 2.325 (2.325) | 6.73 (6.62) | -0.497 | 0.741 (0.731) |
  | running | 2.325 (2.325) | 7.09 (6.98) | -0.949 | 0.767 (0.758) |

- The game lands every jump one frame after the replay (0.01 s). At the moving jumps' speed
  that frame is the extra 0.11 m. The apex also comes a frame late.

### Choices
- Presses in the air are still masked: no double jump or glide yet.

## 2026-10-08: double jump and glide

### What changed
- The sim gets the jump button as it is. Until now a press only counted if it began on the
  ground, and it was cut at the jump's top. Now the sim makes the double jump (a second press)
  and the glide (holding on) itself.
- **Out of Skyrim's own fall** (walking off a ledge) the sim is in its fall state already. A
  double jump or glide there hands the flight to the sim from that frame. Such flights aren't
  replayed (the replay would start them on the ground).
- **Fall damage:** a double jump starts the fall height over. While gliding, the fall height
  follows him down, so a glide lands softly however far it sank. Skyrim's damage still counts any
  plain fall after it.
- **Behaviour graph:** each double jump sends the takeoff event again (it restarts the clip).
  `JumpFall` goes at each top, then alternates with `JumpFallDirectional` every 4 s: MT_JumpFall
  plays once and lands the graph by itself at 5.2 s, and the same event twice does nothing. The
  landing event matches the fall state (`JumpLandDirectional` after the directional one).
- A landing and a new jump in the same step: the landing is reported first, then the new
  takeoff. Before, the takeoff came first and would have saved the controller's zeroed gravity
  as its own.

### What the sim does at Dragonborn scale (owner's configs, flat floor, 1/100 s)
Probed offline through `ratchet.dll` before writing the tests:
- a 0.3 s press, then again at 0.40 s for 0.1 s: double jump at 0.42 s (2.41 m up, 13.1 m/s),
  peak **4.340 m** at 0.72 s, landing at 1.17 s;
- held on from 0.40 s: glide from 0.93 s (3.39 m up, falling at 8.8 m/s, which becomes its
  sink-speed limit), landing at 1.31 s;
- with the stick forward the glide carries him 11.9 m, too far for the room the test can count
  on, so the glide case is standing;
- holding the first press on (2 s) never glides: coming down, he's never 2 m above the floor
  (the glide's minimum) after its no-glide time.

### Tests: 8/8 pass in Whiterun
- New cases: "double jump" (0.3 s, again at 0.4 s for 0.1 s) and "glide" (again at 0.4 s, held
  to the landing). Air time is checked now too (within 0.03 s).

  | case | apex, m (replay) | apex time, s (replay) | air time, s (replay) |
  |---|---|---|---|
  | double jump | 4.340 (4.340) | 0.726 (0.716) | 1.186 (1.176) |
  | glide | 4.340 (4.340) | 0.726 (0.718) | 1.332 (1.323) |

- The six earlier cases still pass with the button unmasked (full hold now held 0.5 s through
  its top: 2.325 m, the same).
- Screenshots show his double-jump clip at the second takeoff and the glide clip starting.

## 2026-10-08: dash and air dash

### The button
- The dash is on Skyrim's **Sprint** control (Left Alt, gamepad LB), polled like the jump. INI
  `sDashControl` names another control, or is empty for no dash. Rift Apart has no sprint, so
  Skyrim's sprint is off on foot (`SprintHandler::CanProcess`, vfunc 1, says no). On a horse it's
  still the gallop.

### Driving him along the ground
- On the ground Skyrim's controller moves him by its own locomotion, whatever velocity it's
  given. Its in-air state keeps the velocity it's given (the jump relies on that). So during the
  sim's ground dash the plugin puts the controller in `kInAir` every frame, with gravity and air
  control off, and gives it the sim's velocity. That velocity presses down at 2 m/s × scale, and
  the ground holds him up: Havok slides the capsule along it.
- **The controller stays `kInAir` all through the dash** (the traces read state 2 every frame),
  so Skyrim never says he stands. The ground under his feet does: a ray, and within 0.3 m counts
  as standing. Further than that (a slope falling away faster, or a ledge), the sim sees no
  ground, and the dash becomes a fall at the dash's speed, as in the sim.
- The dash ends on the sim's own clock (back on his feet at run speed): Skyrim's run takes over at
  that speed. No fall counts during the dash (the fall height follows him), and none of Skyrim's
  jump events go to the behaviour graph.
- **Air dash:** in the air the sim already has the controller. An air dash out of Skyrim's own
  fall (off a ledge) takes over like a double jump or glide.
- A jump out of a ground dash keeps the dash's speed. It isn't replayed, because a replay from
  standing can't start with that speed. A dash off a ledge isn't either.

### What the sim does at Dragonborn scale (owner's configs, flat floor, 1/100 s)
- Ground dash with the stick forward: 10.92 m in 0.53 s, ending at 8.9 m/s (run speed). Without
  the stick it goes where he faces.
- Air dash early in a jump (0.2 s): flat at 10.85 m/s for 0.6 s, then a fall at 9.3 m/s. He lands
  15.4 m out.

### Tests: 10/10 pass in Whiterun
- New cases: "dash" (one press, the stick forward) and "air dash" (jump 0.3 s, dash at 0.35 s, the
  stick forward). Ground dashes are checked against a replay of the same frames on a flat floor:
  distance within 0.25 m, time the same. The room the test looks for is 18 m now (it found 15 m).
  - dash: **10.82 m (replay 10.83) in 0.497 s (0.497)**, following the bridge 0.99 m down;
  - air dash: 17.90 m across (17.87), landed 1.08 m lower after 1.388 s (1.380).
- A screenshot asked for in the same frame as another used to replace it (the air dash and its
  "apex" come on the same step). Shots now queue, one a frame.

## 2026-10-08: walls and ledges wired in (not checked in game yet)
- **Walls:** each frame in the air, a probe like the sandbox's (`level::wall_probe`) looks from his
  middle (0.6 m × scale up), 1 m × scale out, with 16 rays. It takes the nearest steep face (normal
  within 45° of level), measures how far it is along its own normal, and checks two more rays
  0.25 m × scale to either side (the game's three probes). On a wall, the one he's on wins. Skyrim
  marks no wall-jump walls or zones, so every wall is plain (one wall jump per jump). During a wall
  jump, a 32-step sweep of his flight path says whether a floor comes first (`landing_ahead`).
- **Ledges:** `ratchet_set_world` gets a raycast callback into Skyrim's Havok world (no placed
  ledges or Swingshot targets), so the sim can catch lips on the way down. Sneak is the sim's
  crouch (it lets go).
- A wall stick, ledge grab or climb out of Skyrim's own fall hands over to the sim like a double
  jump. Sliding down a wall, hanging or climbing doesn't count toward fall damage. A wall jump
  sends the takeoff event again. His clips: `wall_stick_*`, `wall_slide_*`, `wall_jump_*` (left or
  right from the sim), `ledge_from_fall`, `ledge_idle`, `ledge_jump`. That's the simple version; the
  sandbox's own choice does more (see the clip-choice proposal).
- Flights a wall or ledge touched aren't replayed (the replay has a flat floor).
- With all this live, the ten tests still pass. One run failed the 0.15 s hold's apex time only
  (0.046 s late). Its frames ran 38-62 ms there, and the apex can only be seen at a frame's end. The
  time checks now allow the frame the apex or landing was seen in, plus 0.015 s / 0.02 s.
- No wall or ledge test yet: the test spot in Whiterun has no tall wall in reach.
