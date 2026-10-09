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
  for one). With `[Debug] bTraceJumps`, the ledge case logs every ray the sim casts into the
  world, relative to his feet, and the trace shows his facing.
