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

## 2026-10-08: the OmniWrench and the Burst Pistol (the owner: "I want guns and wrench")

### What there was to build on
A survey of the repo found:
- **The wrench** is all in the C API: the three-swing combo (lunges, timing, `MELEE_HIT`), the
  Hyper-Strike, `ratchet_swing_info` (20 damage a swing, knockback 2.5 / 2 / 4 m), and
  `ratchet_smash_info` (50 within 3 m). The thrown wrench is `ratchet_wrench_throw`/`_step`.
- **Guns** live in `crates/libarsenal`, which has no C API. Until it gets one (the second request
  in `PROPOSAL-clip-choice-api.md`), the plugin has a small C++ stand-in for the Burst Pistol's
  trigger, with its game numbers (wpn_burstpistol_weapon.json, as the sandbox reads it): 11.36 a
  shot, 6 shots/s from the hip and 3 aimed, a spread that grows.
- **The models:** `wrench.glb` and `burstpistol.glb`, each skinned to its own small skeleton like
  Ratchet. Both go on his `igLoc_RT_gunAttach` with no offset, as in the sandbox and the game.
- **The clips:** the converter had left out every weapon clip; it keeps them all now (170).

### How it works in Skyrim
- **Buttons** (INI `[Combat]`, Rift Apart's controller layout on Skyrim's controls): fire on
  Right Attack (LMB / RT = R2), aim on Left Attack (RMB / LT = L2), the wrench on Ready Weapon
  (R / X = Square), the throw on Shout (Z / RB = R1). On foot, Skyrim's own attacks, blocks,
  weapon drawing and shouts are switched off (their handlers' `CanProcess`).
- **Hits are spells cast by the player**, so Skyrim's rules follow: the target fights back, crimes
  count, kills are his, and the difficulty scales the damage. They're built at run time from
  vanilla records:
  - `AlchDamageHealth` (`0003EB42`), copied without resistance or skill: health damage, its size
    set per cast;
  - `StaggerAttackFFContact` (`0007809B`): a stagger;
  - `FireboltProjectile01` (`00012E84`): the bolt a shot rides (36 m/s; the game's is 80 m/s ×
    scale).

  The forms only live for the session. Every effect is instant, so nothing of them is saved.
- **Swings:** the sim's state 7 (and the slam's landing, 9) move him like the ground dash does,
  so the lunges are the sim's. On `MELEE_HIT`, everyone within 1.8 m × scale and in front (the
  sandbox's reach and cone) takes the swing's damage and a stagger (full on the third swing).
- **Hyper-Strike:** melee in the air. Its hop and slam are a flight; the height it falls doesn't
  count toward fall damage. On `SMASH_IMPACT`, everyone within 3 m × scale takes 50.
- **The pistol:** the bolt leaves his gun hand for the point the camera looks at (a ray up to
  45 m × scale), scattered by the spread. While aiming or firing he strafes (the sim's
  `aiming` 1 or 2, facing the camera's flat forward): the strafe clips play by the way he moves.
  No firing while dashing, swinging, on walls or ledges.
- **The throw:** 0.068 s after the press the wrench leaves his hand, flat along the camera's
  look. The sim flies it out and back. Walls turn it back (a ray each frame). Anyone within
  0.5 m × scale of its path takes 30, once per throw. His model shows it flying (laid flat,
  spinning), and it's back in his hand on the catch. No swings or shots while it's out.
- His model holds the wrench while swinging (and just after) and the pistol while aiming or
  firing (and 2 s after).

### Tests (fights against a cow with its AI off and 1000 health)
- First run: every hit landed (3 swings, the slam, 6 of 6 hip shots and 3 of 3 aimed shots at
  8 m), but each did half its damage: 10 a swing, 25 for the slam, 5.68 a shot. That's
  Skyrim's difficulty scaling of the player's damage (Master: ×0.5, `fDiffMultHPByPCVH`). It
  stays (Skyrim's rule), and the tests now expect damage × the save's difficulty multiplier.
- The owner played the first combat build right after: dozens of Hyper-Strikes ("slam hits 1,
  50 damage within 4.6 m") and swings in the log, nothing wrong.

## 2026-10-09: his sounds, his weapons in hand, and his own size

- **Ratchet-sized by default (`fScale = 1.0`).** The owner: he "feels too big", lore size would
  be more fun. At scale 1 his model is about 1.2 m tall, and the sim uses Rift Apart's own
  numbers (a 1.5 m full jump, 2.8 m with the double jump, a 7 m dash). Skyrim's collision
  capsule and camera stay the Dragonborn's. Shrinking the player actor too is open.
- **Sounds:** `skse/tools/ratchet_sounds_to_skyrim.py` turns the owner's extracted sounds
  (`riftapart-assets/sounds`, 38 moments) into mono 16-bit WAV under `Data\Sound\FX\
  RatchetMovement` (881 files, 54 MB) and `sounds.txt`. The plugin plays a moment's layers like
  the game: one random file each, at its volume, after its delay, at a random pitch in its
  range, with the moment's chance. It uses Skyrim's own audio (`GetSoundHandleByFile`). Moments
  played: jump (+ swish, footstep, voice), double jump, landing (+ footstep, voice), Phantom
  Dash and its end, air dash, the glide's heli-pack (extend, loop, retract), swings (+ foley,
  voice), the slam, each wrench hit, the throw (+ voice), its whir while it flies, the catch,
  pistol shots, and the bolt's impact when it would arrive. Levels: 15 dB is full volume (ours).
- **The wrench and the pistol are in his hand** (the converted NIFs on igLoc_RT_gunAttach), and
  the thrown wrench flies as its own model. Built and installed, not yet seen in game.

## 2026-10-09: the owner's first fights, and the fixes

The owner played the combat build and reported three things:

- **"Throwing doesn't work", and the throw's sound was spammed.**
  - The throw did fly: the log now shows it going 15 m out and back in 1.64 s.
  - The sound: loops started a new round whenever Skyrim's handle said the sound wasn't playing,
    and right after `Play()` it says that, so a new round started every frame. Loops now start
    the next round when the longest layer of the last one is over (the converter writes each
    file's length into sounds.txt). Same for the glide's heli-pack.
  - The flying wrench was invisible: it hung under the player's root, and everything there except
    Ratchet's model is the Dragonborn's, which the model culls every frame. It hangs under his
    model now.
- **"Slow acceleration".** Measured in game: from standing, Skyrim's locomotion does nothing for
  0.14 s, then ramps at about 18 m/s² and reaches its run (4.64 m/s) after 0.4 s. Rift Apart's
  Ratchet is at his run (5.74 m/s at scale 1) in about 0.2 s. So **the sim now walks and runs him
  too** (INI `bSimGround`, on): while he's moving on the ground he's in the same air state as the
  dash. His real velocity (after collisions) feeds back into the sim each frame, and he follows
  the ground down within 0.3 m. Standing still, Skyrim's ground state has him (waiting,
  sitting, talking). Walking (the run toggle off) and sneaking halve the stick, and auto-move
  pushes it forward. In game the run-up now goes from 0 to 5.74 m/s in 0.22 s.
- **"Damage doesn't trigger hostility until the NPC dies."** A plugin-cast spell hurts, but with
  the player blamed it still raised no reaction. A hit on someone not already fighting him now
  also runs the console's `SendAssaultAlarm` on them (the crime and the witnesses) and
  `StartCombat`. Papyrus's `Actor.SendAssaultAlarm` couldn't be dispatched on actors without
  scripts. In game: a neutral hunter fights back after one swing, and one swing at a Whiterun
  guard costs a 40-gold bounty.
- Hits now count to the nearest point of the target's body (a segment along its heading, from
  its bounds), not its middle: a lunge along a cow's 2.5 m side used to miss.

Tests: **17/17 pass at lore size**, now with a wrench throw, a hunter who has to fight back and a
guard who has to report the assault.

### Additive layers (his aim, recoil and throw)
- Screenshots showed the pistol hanging at his side while aiming. In the sandbox, additive layers
  bend his upper body toward the crosshair. The plugin now plays additive clips the way Bevy's
  additive blend does: translations added, rotations as `slerp(identity, change, weight) * pose`.
  - Aiming: `aim_pistol_v/h_additive` (or `_run_` while moving), scrubbed by the crosshair's
    angle up or down (up to 90°, down to 45°) and to the side (90°). Fade 0.08 s.
  - `fire_pistol_additive` restarts with each shot (fade 0.03 s in, 0.12 s out).
  - `throw_additive` restarts with each throw (0.08 in, 0.25 out).
- In game, by the third aimed shot his arm is out in front, pointing at the target.
- Fixed with it: sampling a clip left joints it doesn't key wherever the last clip had put
  them. They're back at rest now.
- The flying wrench, checked by a log line: drawn 0.00 m from where the sim has it, shown.
- The test place clears hostiles that wander in (a wolf and a mudcrab bit him on the Whiterun
  road) by killing them. Disabling them instead crashed Skyrim's pathfinding thread on the way
  out, since a path request was still in flight.

## 2026-10-09: Clank on his back

The owner asked for Clank on his back.

- **The model.** `ratchet_to_skyrim.py --name clank --prefix RK_` converts the owner's
  `clank.glb`: 29 skinned shapes on 175 joints (`clank.nif`), 17 textures, and his 5 clips
  (`clank.anim`: backpack idle, heli extend, glide, retract, and a fall glide the sandbox doesn't
  use). 11 shapes are his heli-pack (glTF extras `"part": "heli"`); the converter now puts the
  part in their names (`RK_heli_sm119`), so the plugin can find them.
- **Materials the converter didn't handle before** (Clank has them, and the wrench's glow strip):
  - a colour factor and no texture (his antenna and eye glows, his glass, the rotor blades): a
    4×4 texture of that colour; with a texture, the texture times the factor. glTF's factors are
    linear, Skyrim's colours gamma, so the factor goes through ^(1/2.2) first;
  - an emissive colour: the shader's own-emit flag with that colour (his green glows);
  - `BLEND` materials: alpha blending without z writes (the rotor blades and antenna glass are
    see-through). `MASK` stays an alpha test. Ratchet's own materials have none of these, so his
    look doesn't change.
- **The plugin** (`src/Clank.cpp`) hangs him the way `apps/sandbox/src/clank.rs` does: his
  `dyn_backpack_attach` joint exactly on Ratchet's `ClankAttach` bone. His root is the inverse of
  where that joint is in his model this frame, its place and turn only (his clips scale his body
  to 0.84 on Ratchet's back; undoing that made him 1.19 times too big in the sandbox). He plays
  his backpack idle. As a glide starts the heli-pack's shapes show and he plays its extend
  (0.23 s), then the glide loop, then the retract (0.3 s) as it ends, 0.1 s fades between them.
  The three rotors spin at 22 rad/s about the axis between their blades' rest frames, as the
  sandbox spins them. INI `bClank`.
- **Checked:** a log line says where his head is from Ratchet's chest and which way he faces. In
  game: 0.09 m behind, 0.15 m above, 0.11 m to the side, facing -0.60 (backward, turned with
  Ratchet's idle stance). The sandbox's math on the same two idle clips, offline from the glbs,
  gives 0.08-0.10 m, 0.15 m, 0.10-0.11 m and -0.55 to -0.64. Screenshots show him on the back,
  and in the glide the top rotor on its stem with the green bulb and both arm rotors.
- The glide test now also has to bring the heli-pack out, with a screenshot once it's all the way
  out. **18/18 pass.**

## 2026-10-09: the first wall test

- Walls were wired to the sim on 10-08 but never seen working: the test spot in Whiterun has no
  tall wall in reach. The test now places one: a flat Dragonsreach wall piece
  (`WRIntCastleWallDoubleHighExStr01Quad`, 14.6 m by 7.3 m) across his way, its face 2 m ahead
  (`TESDataHandler::CreateReferenceAtLocation`, removed again after the case).
- The case: a full jump at it with the stick forward. Once he sticks the stick is let go, and
  0.3 s later the jump is pressed again. It passes if he stuck, jumped off, and landed at least a
  metre further out than where he stuck.
- In game: **he stuck 0.59 m out from the face, 1.24 m up, jumped off, and landed 4.62 m out**,
  0.96 s after the takeoff. Screenshots show him on the wall facing it (Clank toward the camera),
  then flying back off it. **19/19 pass.**
- The probe's rays hit what his capsule hits, other actors' bodies too. Could he stick to a big
  creature's side? A case jumps at a bear (EncBear, AI off) 2 m ahead with the stick forward: he
  landed on its back and never stuck. A body is round, so the probe's two side rays come back at
  an angle and the three-ray check turns it down. **20/20 pass.**

## 2026-10-09: the first ledge test, and Skyrim's capsule standing on edges

- Rift Apart catches a lip only with at least 3 m of drop below it (`GroundClearance.MinHeight`,
  0.5 m under the lip) and no more than 3.5 m above where he last stood
  (`RecoveryMaxHeightAboveLastGround`). On flat ground both hold only for a lip exactly 3.5 m up.
  So the ledge case builds a drop: a floor piece (`WRIntFloorSTMid01Large`, 14.6 m square) 4 m
  up as a platform he's put on, and 2.5 m ahead of him a wall reaching 2.05 m above it. A full
  jump with the stick forward tops out about a metre short of the wall. That's inside the
  catch's reach (1.2 m) and outside the wall stick's (0.775 m). Once he hangs, the stick is let
  go and jump is pressed 0.8 s later.
- **The Dragonsreach wall piece has almost no top.** Its face is solid, but rays straight down
  onto its top mostly pass through, apart from a sliver sloped 14°, about 0.2 m below the
  top. Its walls meet Dragonsreach's ceiling, so nobody stands on them. The catch's rays found no
  top, and the sliver gave Skyrim's capsule something to "stand" on. A second floor piece now
  caps the wall, flush with its face. The case measures the ledge with rays as it starts (the
  log shows the top at 2.05 m all along it, and the face 2.51 m ahead up to 2.0 m).
- **Skyrim's capsule standing on an edge isn't a landing any more.** His capsule is still the
  Dragonborn's, half again his height. In a sim flight, Skyrim's controller says he stands
  whenever the capsule rests on something, even an edge at his chest with 4 m of air under his
  feet, and the flight ended there. That's how the first run "landed" on the sliver. Now, in a
  flight, such a stand counts as a landing only with ground within 0.5 m under his feet (a
  capsule on a 50° slope is about 0.2 m above it), or once it has lasted 0.25 s (an edge he
  really rests on). Until then the controller is put back in its air state and the sim flies
  on, so the capsule slides off the edge. With the capped ledge the catch also passes without
  this; it's for the decorative tops Skyrim's buildings are full of.
- In game: **he caught the lip at the top of his jump (1.50 m up), hung, climbed on the press,
  and stood on the ledge (+2.12 m)**. Screenshots show him reaching for the lip, then hanging by
  both hands. **21/21 pass.**
- Test tools: `ingame.ps1 -Only <name>` runs only the cases whose names contain it (about 40 s
  for one). Test runs trace every frame on their own now (`autotest.ini` turns `bTraceJumps`
  on, `-NoTrace` doesn't), and the ledge case logs every ray the sim casts into the world,
  relative to his feet. The trace shows his facing.
- The INI installed in the MO2 mod was still the first milestone's: `bTraceJumps = 1` (every
  jump traced frame by frame in the owner's own games) and none of the newer options listed.
  It's the current template now, with the owner's values (all the same as its defaults,
  `fScale = 1.0` among them) and tracing off.

## 2026-10-09: the Shatterbomb (the owner: "I want guns")

- **The second gun.** Keys 1 and 2 (Skyrim's Hotkey1 and Hotkey2, INI `sPistolControl` and
  `sShatterbombControl`) pick the Burst Pistol or the Shatterbomb, as in the sandbox. Skyrim's
  favorites on those two hotkeys don't equip on foot then; the other hotkeys still do.
- **Its numbers** are the owner's `wpn_protogrenade_weapon.json`, read the way libarsenal reads
  it (its level-1 upgrades win). It's a cooked trigger: fire held winds the throw up, letting go
  throws, 0.8 throws a second, 6 in the glove (filled again 1.5 s after the last, the sandbox's
  convenience). The throw is lobbed at 30 m/s under 30 m/s² to come down on the crosshair's point,
  at most 30 m out, solved for the lower arc as libarsenal's `lob_velocity` does. It flies
  exactly under constant gravity and goes off on the first thing a ray along its path meets
  (bodies too), or after 8 s. The blast does 170.5 damage within 1 m, falling to 85.25 at 6 m
  (measured to the nearest point of each body at its middle height), and a full stagger within
  3 m, half one further out. Ratchet isn't hurt. Like the pistol's trigger, this is a C++ stand-in
  for libarsenal, which has no C API yet (see the clip-choice proposal).
- **His glove.** `shatterbomb_glove.glb` is skinned to its own copy of his skeleton. The plugin
  hangs it under his model and points its skin's bones at his own nodes by name (his `_scaled`
  joints where he has them, as his own skin uses), as the sandbox does: all 42 of its joints
  were found. His own right glove (`sm05`) hides while it's on. It's off on walls, ledges and
  swings, where the game holsters the weapon. His hand takes the glove's grip (`glove_hand`, a
  clip of the hand's joints only, over whatever he does), and he idles with the glove up
  (`glove_idle`).
- **The throw's poses.** The game's grenade layers: the wind-up while fire is held, then the throw
  on each one, whole-body standing still and the arm's alone on the move or in the air (the
  sandbox's choice). The bomb leaves his hand 0.1 s in.
- **The bomb and its blast.** Each bomb in the air is a copy of `shatterbomb.nif`
  (`NiObject::Clone`), tumbling. The blast looks like Lightning Rune's (`ExplosionRuneShock01`):
  a runtime copy with no damage, push, shock or sounds, set off at a marker placed for the
  moment. `TESForm::Copy` left the explosion's model behind, so its data and model are copied by
  hand. Its screen effect is left out (a radial blur over the whole screen, too much for a
  grenade a second). The sounds are Rift Apart's (`grenade_throw`, `grenade_explode`), and the
  camera shakes within 30 m.
- In game: the Shatterbomb case winds up 0.4 s and throws at the cow. The bomb flies low (3°
  up for 5 m at 30 m/s) and goes off on the cow: 170.5 damage, 85.2 at the save's Master
  difficulty. Screenshots show the glove on his hand and the blast's white-blue burst at the
  cow. **22/22 pass.**
- Test log: each fight now logs where he and its target are. Moved back for a later fight, the
  cow ends up 6.1 m away, not 8 m (not looked into; an earlier stagger may still be moving
  it). The fights pass either way.
- **Fixed: the caught wrench floated off his hand.** After a throw the wrench was hung back on his
  hand with the flight's transform still on it, so every swing after the first throw showed it
  about 1.5 m to his right (the test screenshots since the throw went in show it). It's put back
  in his hand's own space now; the swing after the throw has it in his hand.
- **Test fixes.**
  - The ledge case failed once in five runs: at the top of his jump he was only 0.47 m from the
    wall, inside the wall stick's reach. One run reached it still rising (1.35 m up) and stuck
    to the wall before the catch, which only looks on the way down. The wall is 3 m ahead now:
    the catch comes 0.92-0.96 m out, outside the stick's reach. 4 of 4 runs passed.
  - The fights' cow: moved back for the next fight with its AI off, only its position moved,
    not its 3D. The pictures showed it by Ratchet while the shots, the thrown wrench and the
    Shatterbomb hit its body 6 m out. Its 3D goes with it now (`Update3DPosition`).

### A review of the night's code, and its fixes
An independent review of the night's changes found no crash paths. The math checked out: the
attach inverse, the rotor axes, the lob and the flight against libarsenal. It found these, now
fixed:
- **Off foot, the guns kept their buttons.** Riding, swimming or knocked down, the plugin's step
  stops before the guns, so a Shatterbomb throw being wound up stayed latched. The first frame
  back on foot threw it at whatever the crosshair was on, maybe a guard (an assault). Bombs in
  the air froze. Now each frame off foot drops the buttons and the wind-up, and flies the bombs
  and the thrown wrench on.
- **Carried off with bombs in the air** (a load door, fast travel, a coc): they flew on at the old
  place's coordinates and went off near wherever those landed in the new one. Now a new world,
  or 30 m in one step, drops them and puts the wrench back in his hand.
- **The edge rule and fall damage.** Putting the controller back into its air state each frame
  could let Skyrim count a landing each frame. The fall it measures now starts at each of those
  frames, so only the first touch-down counts.
- **First person.** Flying things hung under his model, which isn't drawn there. They hang under
  the first-person 3D in first person now.
- Smaller: the throw's log said 0.0 m for throws aimed past 30 m. The glove refilled only while
  the Shatterbomb was out; now it refills whichever gun is out. Looping clips' times (his and
  Clank's) wrap, so hours of idling can't run their float precision out.
- A new case throws a Shatterbomb in first person. The throw and the blast work there
  (screenshots: the cow 8 m out, the blast on it), and the next case is back behind him.
  **23/23 pass.** The knockdown case has no automated test: the test's own step doesn't run
  while he's off foot.

## 2026-10-09 (morning): "I keep sliding down tiny bumps in the road"

- **The cause: the night's edge rule.** In a sim flight it took Skyrim's "standing" as a landing
  only with ground within 0.5 m under his feet, by a ray from his middle. On a bumpy road that
  ray often finds nothing (it starts inside a bump, or falls into a crack). Then a walk-off fall
  went on while he was on the ground, the capsule sliding along the bumps in the fall pose. The
  owner's log shows it: a dozen "falls" of 0.4-1.6 s that barely changed height. One moved 2 cm
  in 0.8 s; one ended 11 cm higher than it began. The rule is gone: Skyrim's "standing" is a
  landing again. The ledge test never needed it.
- **The ground under him is measured the sandbox's way:** the nearest ground under his footprint
  (his middle and 0.35 m to each side, `level::ground_distance`), from 0.3 m above his feet so a
  bump he stands on still counts. Moving on the ground he's held to it within 0.35 m (the
  sandbox's `STEP_HEIGHT`, was 0.3 m and not scaled).
- **Brief falls aren't shown:** a sim fall off a bump or a step shorter than 0.15 s shows neither
  the fall pose nor a landing, and makes no landing sound (Skyrim's own short falls don't either).
- A new case runs 2 s along the road with no jump and fails on any fall longer than 0.3 s.
  Built and installed for the owner to try first; not run in game yet.

## 2026-10-09: the owner's second try (small uphills and stairs)

The owner played and listed: trouble up small slopes and stairs, crackling on the Shatterbomb, no
hostility from the pistol, the strafe flips and Clank's rotor sway missing, and Rift Apart's own
shots wanted.

- **Up slopes and stairs.** The trace of a run at stone stairs showed the cause. Moving on the
  ground, the controller was held in its air state with the sim's velocity, pressed straight down
  at 2 m/s. On a slope the solver keeps the downhill part of that press, and the sim went on each
  frame from the speed the controller kept. So every uphill frame cost speed: he stalled at the
  foot of the stairs and slid back down (from about 8 degrees at 100 fps). A research pass over
  the CommonLib headers, the executable's vtables and three released plugins (SkyrimDirectional-
  Combat, SimpleTimedBlockAddons, AnimationMotionRevolution) found the clean way: stay in Skyrim's
  ground state and write `velocityMod` inside its physics step (vfunc 8 of
  `bhkCharacterStateOnGround`). Measured: it's in m/s, in his own frame (x right, y forward;
  `forwardVec` points back), and taken with no lag (5.75 m/s forward asked, 5.75 m/s made).
  Skyrim keeps that speed along the slope: on the 30 degree ramp 6.64 m/s asked gave 5.7 m/s
  across it, the sim's 5.74. Two things went wrong at first, both fixed:
  - The slope came from the controller's own contacts, which on stairs are the steps' edges
    (normals 45 degrees off): the speed-up for the slope was wrong there. The slope comes from the
    rays under his footprint now (the treads: flat).
  - The sim went on from the controller's speed every frame. Up the stairs Skyrim moved him faster
    at times and the steps' edges turned him; fed back each frame, that ran away (9.4 m/s, then
    sideways off the stairs). Now the sim takes the controller's speed only when it's slower than
    asked (a wall).
- When Skyrim's state loses him over a bump (the ground still within 0.35 m), the controller is
  held in its air state with the sim's velocity as before, now along the slope under him and
  pressed into it along its normal, and rising onto a step ahead (looked for up to 0.12 s ahead).
  `bNativeGround = 0` uses that all the time: it passes the ramps too, but climbs stairs in
  bursts.
- **Tests:** new cases run up a 15 and a 30 degree ramp (a floor piece tilted: a reference's x and
  y angles tilt its up axis to (-cos x sin y, sin x, cos x cos y), measured) and the stone stairs
  outside Whiterun's gate, each until he's high enough. The 30 degree ramp: 4.0 m up in 1.46 s.
  The stairs: 4.5 m up in 1.25 s, no falls (before: -1.0 m, stalled). A landing during a case's
  setup (off the last case's ramp as it's deleted) isn't counted against the next case. The
  suite's time limit is 180 s (90 s cut off the last three cases). `-Only` takes several names
  (`"guard|bear|ledge"`). **28/28 pass** (full suite and the three it had cut off).
- **Crackling on the Shatterbomb.** Rift Apart's moments are levels in dB; the plugin played a
  moment above 15 dB with every layer clamped to full, and the bomb's flight hum (with an 18 s drone
  pitched down 1620 cents) was left playing. A moment's gain is capped at 1 now (layers keep their
  mix), and the bomb's hum layers belong to the bomb: they follow it and stop when it goes off.
  Not checked by ear yet.
- **The pistol raises hostility.** Its bolts are spells the player casts, which hurt but raise
  nothing until the target dies. Each shot now predicts what it will hit (a ray along it) and
  that actor is attacked as the wrench's hits do it: the assault alarm and combat when the bolt
  gets there. The damage effects are hostile and detrimental too. The case "a shot makes him
  fight" passes.
- **Clank** plays through a mixer that does what Bevy's animation graph does (per-channel
  weights, the first base layer sets, the next ones interpolate toward theirs by weight, additive
  layers on top) and Bevy's transitions (a new clip fades in, the old ones out). Ratchet's own
  clip choice (strafe flips, the glide's intro and lean, the run's lean and grip sets) is being
  ported from the sandbox into a library both use, and comes next.
- Known: the game sometimes crashes as the test quits it (a pathfinding job reading Whiterun's
  navmesh, twice in two days, both right after "done"). Not seen in play.

## 2026-10-09: his clips as the sandbox picks them, his steps, Rift Apart's shots

- **The clips.** A new crate, `crates/libratchet-anim`, ports the sandbox's clip choice
  (`ratchet_model.rs`: play_clips, setup_clips, lean_into_turns, blend_swing_loop) without Bevy,
  with Bevy 0.19's crossfades and clip clocks; ratchet.dll gives it as `ratchet_anim_*` (C API
  version 2). Each frame the plugin hands it the sim, how he really moves and the weapons' state,
  and blends the layers it gets back as Bevy's graph does (`Anim::Library::Mix`, the upper body
  from RC_spine_1). So the strafe jumps and landings, the glide's intro and its lean into turns,
  the run's leans, the eight dash directions, the grips, aim, recoil, throw and catch layers are
  the sandbox's. The plugin's own clip choice stays only as a fallback (no animator in the DLL).
  In game: the animator is on over 170 clips; the movement, pistol, wrench, wall, ledge and bear
  cases pass. Screenshots show the glide with the heli-pack out and the aimed pistol stance.
- **His steps and efforts.** The sandbox sounds steps, foley, swishes, voice efforts and the
  wrench's swing from the clips' own events, on the clip clock. The sound converter now writes
  `clip_sounds.txt` (428 sounds: each clip's sound events mapped to moments, its Footstep events
  as steps, the swings' from the wrench's clips, the landing's left to the touchdown), and the
  plugin plays them as the animator's clock passes them (`ratchet_anim_sound_due`). The moves'
  own sounds are then the sandbox's five: the jump's push-off, the landing, the dashes, the
  heli-pack and the slam. Before, he had no steps at all.
- **Test fix: the wall jump.** It failed now and then: he met the wall 2.0 m out at 1.05 m up,
  and below 1.0 m the sim drops him off it, before the test's second press 0.3 s on. The wall is
  2.4 m out: he meets it near the top of his jump (1.44 m up) and jumps off.
- **Rift Apart's shots.** The game's effect art (extracted by tools/extract_riftapart_vfx.py)
  is converted by `skse/tools/ratchet_vfx_to_skyrim.py` (new; build.ps1 runs it): its 19
  textures as DDS, a camera quad per texture (added and blended), its 7 effect meshes, all with
  Skyrim's effect shader, and `fx.ini` with the game's effect colours. Two findings on the way:
  SSE's effect shader block has no environment-map fields (those are Fallout 4's), and Skyrim's
  effect shaders want tangents in the vertex layout (5328 of the game's own 6633 effect shapes
  have them; none of the textured ones go without). `Fx.cpp` ports the sandbox's vfx.rs: every
  piece moves, grows, fades and shifts colour over its life as there (camera-facing, stretched
  along its velocity or a trail, spinning, bouncing), each copy with its own material.
  - The Burst Pistol flies its own bolts now, as the sandbox does: 80 m/s to 45 m (the game's
    ShotPrius), a ray each step; the first thing it meets stops it, a body takes the shot's damage
    (the hit raises the assault and combat), level geometry takes a mark. With them: the muzzle
    flash at the gun's igLoc_bulletEmit, the tracer, its glow and the streak behind it, the smoke
    trail, the impact (flare, glow, sparks bouncing off, smoke, the glowing bullet mark) and a
    spent shell tumbling out of igLoc_shell_eject. Without the art, Skyrim's spell bolt as before.
  - The Shatterbomb's blast (flash, fireball, the shockwave dome, crystal shards, smoke, debris,
    the scorch with its embers), the wrench's hits, the Hyper-Strike's landing and the thrown
    wrench's trail have theirs too. Skyrim's lightning-rune look is only the fallback now.
  - Hung first under Skyrim's shadow scene node, nothing was drawn. Under the player's cell's 3D
    (the root undoing that node's transform) it is: the screenshots show the blast's fireball,
    shards and dome at the cow, and the pistol's green flash at his hand. Marks sit 1-2 game units
    off what they're on (the sandbox's 4-10 mm is lost in Skyrim's depth buffer).
- **Tests.**
  - A new case jumps while aiming: the sandbox's choice plays `strafe_jump_idle`. Aiming, the sim
    jumps Rift Apart's strafe jump (1.0 m, not 1.5 m), which the flat replay can't follow (it
    doesn't aim): that case checks the clip only.
  - "Hitting a guard is a crime" failed in the full suite: the guard fought back, but no bounty.
    Bisected: no single case before it caused it; a bounty came exactly when a guard on patrol was
    near enough to see the hit. The victim alone never raised one. The case places a witness
    guard 4 m to the side now, and the test's clearing of hostiles spares him and pauses during
    the case (it killed the witnesses before they could report).
  - The game crashed now and then as the test quit it, in a pathfinding job reading Whiterun's
    navmesh (three times, each a second after "done", with actors in combat). The test now turns
    AI off (`ToggleAI`) and waits 1.5 s before quitting; no crash since.

## 2026-10-09 (afternoon): Rift Apart's camera, the throw arc, the effects' lights

The owner can't playtest for now: "keep going for 1-1". The demo's script (apps/sandbox/src/
demo.rs) was the checklist. Most of it now runs through the sim and the animator; what the demo
showed that Skyrim didn't: Rift Apart's camera, the Shatterbomb's throw arc, the effects' lights.
(The Swingshot needs swing targets, which Skyrim's world has none of.)

- **The camera** (`Camera.cpp`, INI `bRiftCamera`). The sandbox's camera_follow, with the game's
  configs (nlohmann-json added through vcpkg to read them): a pivot 1.5 m above his feet, 4.65 m
  back, up or down by the camera's pitch; running, 6 m back with FOV 82 (from 76); aiming, over his
  right shoulder (0.5 m right, 4.5 m back, FOV 72), blended in over 0.13 s; the pivot trailing him
  (the sandbox's -10/s). Skyrim keeps its mouse look and its other camera states (first person, a
  horse, furniture). Found on the way:
  - Skyrim asks its third-person state for the camera's turn through the vtable, but never for its
    place: hooked on its update, the place goes into the state and onto the camera's root.
  - Skyrim's FOV value is the screen's horizontal angle: its world camera at the owner's FOV 80
    draws 80 degrees across (1.778 wide). The game's angles go in as they are.
  - The game's shakes: the Burst Pistol's recoil each shot, the Shatterbomb's blast (full within
    30 m, gone at 50 m, the grenade's ShakeData), the slam's wobble. Skyrim's own explosion shake
    stands in when the camera is off.
  - Screenshots: the aimed pistol framed over his shoulder with the crosshair clear of him; the
    running jump from 6 m back.
- **The throw arc.** While a Shatterbomb throw winds up, where it would fly from his hand to the
  crosshair: dashed light-blue lines every other 0.03 s of its flight, as wide on screen at any
  distance (the sandbox's gizmo lines are 5 pixels), and a ring of the blast's inner radius where
  it lands. In first person the throw starts in front of the camera now (his third-person hand isn't
  posed there: the arc landed 2 m short).
- **Lights.** The sandbox's flash lights, as Skyrim point lights (no shadows), fading with their
  pieces: the muzzle flash, each bolt's light as it flies, the impacts, the wrench's hits, the
  slam and the blast (the blast lights the whole bridge for half a second), and the Shatterbomb's
  violet glow in flight. At most 8 at once (Skyrim lights a shape with only a few), the oldest
  going first. Bevy's lumens to Skyrim's fade: 40 000 lm is 1.
- **Tests: 29/29.**

## 2026-10-09 (evening): the sandbox's weapon rules, line by line

Read against weapons.rs, wrench.rs, combat.rs and libarsenal, rule by rule:

- **What's in his hand** (gun_visibility, weapon_away, held_wrench): the Burst Pistol whenever it's
  the weapon, not only while aiming or just after a shot; nothing while dashing, on a wall, a ledge
  or a swing, or while the wrench's throw has his hand (from the throw until just after the catch);
  the wrench while he swings and 0.35 s after.
- **Switching:** a pick while the wrench is out of his hand does nothing; a Shatterbomb already
  leaving his hand goes first, the switch right after it. Outside Skyrim's third-person camera on
  foot (first person, a horse, furniture), Skyrim's own field of view comes back each frame.
- **The throw:** melee pressed while aiming throws the wrench (Rift Apart's, the sandbox's
  throw_button), and he turns along the camera as it goes: `ratchet_set_facing`, C API version 3.
  A melee press held through a block (aiming, the wrench out) doesn't swing once the block ends,
  until it's let go; while the throw has his hand, firing from the hip doesn't strafe him and the
  pistol doesn't fire.
- **The Burst Pistol's rounds:** 150 (its level-1 ammo), one a shot; the gun out, empty for 1.5 s,
  fills both guns (the sandbox's REFILL_AFTER). Its wait between shots carries over frames as
  libarsenal's does: before, a shot came due only at the first frame after its wait, which lost up
  to a frame a shot (6 a second from the hip came out 5.5 at 50 fps).
- **Tests: 30/30** (new: melee while aiming throws the wrench into the cow 8 m out).

### Load doors and loads, with the effects

The effects hang under the player's cell's 3D, and no test had yet left a cell with any alive:
the node kept a bare pointer to a cell's 3D the game unloads at a load door or a load. Two new
cases (last in the suite: they leave him away from the start) shoot at the floor, go through the
Bannered Mare's door (`coc`) or load the most recent save, and shoot again. They passed even before
the fix (no crash), but by luck or by design was unknown. Now:

- The node holds a reference to the cell 3D it hangs under, so that 3D can't be freed while it's
  in use. It lets go as soon as the player is in another cell (and hangs again under the new one
  when there's something to draw). A load lets go before the old world goes.
- Carried off to another place (a load door, fast travel, a coc: another world, or 30 m in one
  step), the bolts in the air and the effects left behind go too, as the Shatterbombs and the
  thrown wrench already did. Before, the 5 longest-lived pieces from Whiterun went on drawing in
  the Bannered Mare at Whiterun's coordinates.
- The cases check the new shots' effects are alive under the new cell's 3D, none from the old
  place remain, and Ratchet is posed and Clank playing there. **Tests: 32/32.**

## 2026-10-09 (evening): an audit against the sandbox, its sounds, and listening without ears

A research agent read every sandbox module (and libarsenal, libratchet's wrench, libratchet-anim)
against `skse/src`, and ranked what a player would notice. Fixed from it so far:

- **The sounds, as sound.rs plays them.** The converter keeps each layer's `interval` and each
  file's own volume now, and bakes every looping file into one of at least 15 s (the file over and
  over: Skyrim's handles play a file once, and this way the seams are the file's own). `Sounds.cpp`
  is the sandbox's model: a moment's gain is its level's half-difference from 24 dB within -18..+3
  dB, times the file's volume, times 0.7; a loop's loop layers loop, a retriggered one plays a
  fresh file every interval (the glide's six rotor-chop layers, every 0.09 s: 145 files in a 2 s
  glide, measured); an owner's (the Shatterbomb's) loop until its blast, the rest after their
  delay; a sound away from him fades past 6 m. Before: the heli loop's chops played once per
  12.8 s, the bomb's whir stopped 0.29 s into its flight, and the foley sat 8-10 dB under the
  sandbox's mix. Loads stop everything.
- **Hearing it without the owner:** `skse/test/audio_capture.cpp` records Skyrim's own audio (the
  process's alone, Windows' process loopback: nothing else on the PC), and `audio_check.py` looks
  for samples over full scale, the loudest moments (lined up with the plugin's log) and dropouts,
  and with `--after` shows the loudness after a log line and how it pulses. `ingame.ps1 -Audio`
  does both. On the full suite: peak -5 dBFS (the main menu's music), the Shatterbomb's blast at
  0.28, nothing over full scale, no dropouts. The glide's sound pulses at 11 Hz (the chops).
- **The wrench:** thrown, it turns back off the first one it hits (it flew through everyone to
  15 m); one hit sound a swing or a throw's tick, none on the slam; no throw vocal (the sandbox
  has none); his throw clip and the wrench in his hand start at the press, not 68 ms later; it
  can be thrown in a dash; and it's in his hand for 0.35 s after the catch (his hand was empty).
- **The guns follow the button while put away or while he's busy** (libarsenal's `stow`): their
  waits run down and the pistol's spread settles meanwhile; a Shatterbomb held through a dash or
  a swing keeps its arc and wind-up and goes on its release after, and one let go meanwhile
  throws nothing (it was thrown the moment he was free). It leaves his hand at the throw clip's
  own moment: 0.1 s standing, 0.0653 s moving or in the air. Holding fire faces him along the
  camera in a dash or a swing too; the aimed spread grows from the first shot.
- **Tests: 34/34** (new: a long glide off a platform 8 m up, 17.3 m across in 3 s, as the replay).
  Once, the first run after the converter rewrote 73 MB of sounds, the combo's second swing came
  0.3 s late and its third never did; not again in three runs (first reads of the new files).

### The weapons' own clips, the thrown wrench's roll, the crosshair

- **The weapons' clips.** The OmniWrench and the Burst Pistol are converted with their clips now
  (`wrench.anim`: 13 joints, 6 clips; `burstpistol.anim`: 24 joints, `fire`), and played on their
  joints as the sandbox plays them (wrench.rs held_wrench, weapons.rs gun_clips): the wrench's
  `melee_1/2/3` with each swing and `smash_intro`/`smash` with the Hyper-Strike, in over 0.05 s, and
  its `idle` between, over 0.15 s (the squash and stretch: the third hit shrinks the back hammer,
  the idle brings it back); the pistol's 0.33 s `fire` (the slide) from its start with each shot.
  The combo and pistol cases check them: `idle, melee_1, melee_2, melee_3` and `fire` played.
- **The thrown wrench** lies as the sandbox's (Ry(spin) Rz(90°): Rift Apart's ThrowOrientation
  kXUp, the model's +x up); it was rolled 90° about its handle (+z up).
- **The camera's framing** follows the pitch only as far as the sandbox's mouse look goes (-0.25 to
  1.35 rad): further up, the framing stays (3.96 m back standing, not 3 m). Skyrim's look itself
  isn't clamped (aiming up at a dragon).
- **The crosshair.** Skyrim shows none in third person with no weapon drawn, and the plugin keeps
  Skyrim's weapons away: there was nothing to aim with. The sandbox's "+" (white at 90%, its dark
  shadow, 3.4% of the screen's height) is drawn now while the Rift Apart camera is on, a quad 30
  units in front of the camera, turned with it (its shakes too), drawn over everything (the
  converter makes the texture and a quad without the depth test; no game art). The aimed pistol's
  picture shows it at the screen's middle; in first person it's Skyrim's view (no "+" of ours).
- **The wheel zooms the Rift Apart camera** as the sandbox's does (main.rs camera_control): 0.6 m a
  notch, 2 to 40 m; nearer than 45% of the game's distance it frames lower, toward his chest, and
  stops pulling back while he runs. Zoomed in all the way, the next notch goes to first person
  (Skyrim's zoom did that; before, the wheel did nothing else on foot), and back in third person
  it's at the game's distance. Hooked on the third-person state's own input (its
  `PlayerInputHandler::ProcessButton`): the zoom's notches only, on foot. A new case sends
  notches through that handler: 5 out took the camera from 4.89 to 7.80 m from him, in to 2 m
  (2.15 m from him), one more first person, back at 4.65 m.
- Small numbers from the audit: the crosshair's ray reaches 80 m (the bolts still stop at 45 m:
  a shot at something further still goes its way), and walking is the stick at 0.45 (was 0.5).
- **Tests: 35/35.**

### The sound lab: does Skyrim place the plugin's sounds?

A case run only by name (`-Only "sound lab" -Audio`) plays one steady sound (the wrench's whir,
looped) for 1.4 s at a time: near the camera, at him, 10, 20, 40 and 80 m ahead, near the camera
at half volume, and 10 m to each side; `audio_check.py --each "sound lab:"` measures each. The
answer: Skyrim plays these handles unplaced (`GetSoundHandleByFile`, flags 0x1A). Every distance
measured the same (rms 0.025, 80 m as 0.3 m), left and right alike at either side, and half
volume half the amplitude (0.0126 against 0.0251 once the background's 0.0056 is taken out). So
the sandbox's own fade past 6 m from him (now in `Sounds.cpp`) is the only one, as in the sandbox,
whose Bevy audio isn't placed either.

### The bolts' and the Shatterbomb's width

The sandbox's bolts and grenades are 0.25 m wide (their projectile's radius): it samples each
step every 0.1 m and anything within that of a dummy is a hit (weapons.rs fly_bolts and
fly_grenades). Ours were bare rays, which hit only a body's collision. Now each step also looks
for a body within 0.25 m (× scale) of the path, every 0.1 m as the sandbox does, before what the
ray met: its middle line (as the wrench's hits find it), its bounds' half width, from its feet to
its height. Followers aren't in it, so the extra reach never hits them. A new case aims the
pistol 0.15 m past the cow's side: all three shots graze it (logged) and hit.

