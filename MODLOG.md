# MODLOG

A running log of what changed, what was tested, and what's known vs. assumed.

## 2026-10-07: libratchet v0 + sandbox (overnight, agent-run)

### Known (from the user's own Rift Apart configs, build 23987138)
- Movement tuning lives in `configs\hero\hero_traversal.config` (HeroTraversalConfig) and
  `configs\hero\hero_shared.config` (SharedHeroConfig), readable with ALERT.
- Jump kinds share one shape: HeightMin/HeightMax, TimeToPeak (or gravity when
  TimeToPeakIsGravity), ButtonMin/MaxHoldTime, TerminalVelocity, NoGlideTime,
  DoubleJumpTimeStart, DoubleJumpHeightMin, MaxTotalJump, MomentumWeight*, NoFallTime, NoInputTime.
- `MaximumMidairJumps = 1`.
- The field names exist as plain strings in RiftApart.exe (config reflection), so Ghidra can
  find where each field is read.

### Built
- `libratchet`: ground / jump / double jump / fall / glide / dash / air dash state machine
  driven by that data.
- `libratchet-ffi`: C API + header. A C++ program built with MSVC 14.44 links it and jumps to
  the expected height (`tests-c/smoke.cpp`).
- `sandbox`: Bevy test course (measuring pole, height steps, gap course, low ceiling, 12 m
  glide tower with landing pads), HUD with live measurements, coloured motion trail, a
  Ratchet/Dragonborn scale toggle, and a scripted `--demo` with screenshots and optional frame
  recording.
- `tools/extract_riftapart_configs.py`: pulls the configs out of the user's own install.

### Tested
- `cargo test --workspace`: 15 tests pass (apex heights and timing, tap vs hold, double jump cap,
  one mid-air jump, glide speed, glide height minimum, ledge falls, run/stop, scaling, dash
  distance, air dash, the real configs loading, C API).
- With the real Rift Apart configs, a full jump peaks at 1.50 m after 0.27 s, matching
  `SingleJumpConfig` exactly; jump + double jump tops out at the 2.80 m `MaxTotalJump`.
- `--demo` run: screenshots in `screenshots/`, video `recordings/libratchet_demo.mp4`
  (34.5 s). Glide from the 12 m tower: 24.5 m in 4.2 s. Dash: covers its full distance in about half a
  second (6.5 m in 0.44–0.48 s with the default values in the tests).

## 2026-10-07 (night): playtest fixes: the wrench in hand, the Shatterbomb's glove, dashes

The user's playtest found five problems:
- the wrench could swing (and Hyper-Strike) while it was thrown;
- the wrench didn't show while walking with it selected, and the Shatterbomb showed nothing;
- half of the wrench's head vanished after the combo;
- dashing while aiming looked wrong;
- a dash and then a jump gave a huge speed boost.

Two research subagents read the game (jump momentum in RiftApart.exe; the dash clips and their
rules). The Ghidra MCP server timed out at Claude Code's 30 s startup limit (it takes about
23 s warm: Python imports, Ghidra's JVM, the project), so the agents used a read-only pyghidra
helper that doesn't lock the project.

### Known (the user's install)
- **Grips.** Ratchet's animset picks clips by grip style:
  - With the wrench (kGripStyleMelee) and with grenades (kGripStyleThrown), standing, walking,
    running, jumping and landing use the wrench-carrying clips. These are `weapon_wrench_idle`,
    `walk_forward_wrench`, `weapon_wrench_run` with its turn leans, `jump_single_wrench`,
    `jump_double_wrench` and the three wrench landings.
  - Gliding uses `helipack_wrench_glide_loop`, or `helipack_glove_hand_glide_loop` with the
    glove.
  - Standing while aiming with the glove uses `weapon_glove_idle`.
- **The Shatterbomb's glove.** Its weapon config's GloveModelOverride names a glove model
  (`hero_ratchet_bangle_glove_magnamissile`).
  - It's skinned to Ratchet's skeleton: his right forearm, wrist and fingers.
  - The animset's GloveAdditive driver layers `weapon_glove_hand_partial` over it, a held pose of
    the hand's 21 joints.
- **The wrench's own clips.**
  - Its third-hit clip holds the back hammer at 3% scale for the whole swing (a static scale in
    the clip).
  - Between swings it holds `idle_wpn_wrench`, its rest pose with every scale at 1. We never
    played that, so after a combo the back hammer stayed shrunk.
- **Pose clips.** Some clips have no sample data, only the base state (a held pose).
  - They have base records for their real joints only (the header's joint count at 0x20), not
    for the 0xFFFFFFFF padding hash that ends the hash list.
  - The wrench's Hyper-Strike clips have the same short base, with samples.
- **Melee while the wrench is out (RiftApart.exe):** the melee component doesn't start melee
  while the weapon is thrown.
- **Dashing (RiftApart.exe and the animset).** The Dash and Air_Dash drivers each have eight
  clips, `dash_{n,ne,e,se,s,sw,w,nw}` and `dash_air_*`. They're picked by the dash direction
  relative to Ratchet's facing (the same Strafe_Angle that strafing uses).
  - Strafing when the dash starts (aiming or firing), he keeps facing the camera through the
    whole dash, so the dash goes sideways or back relative to him.
  - Otherwise he faces the dash direction and plays the forward clip.
  - Both the clip and the strafing flag are set once, when the dash starts.
  - Without stick input the dash goes ahead (the aim, while strafing).
  - The dash suspends the weapon: no aim poses, no firing.
  - A ground dash can be jumped out of from 0.1 s.

### Built
- **Converter:**
  - the wrench-carrying set, the glove's hand pose, the wrench's idle and Hyper-Strike clips;
  - pose clips and short base states decode now (`riftapart_anim.decode`);
  - the glove model (`shatterbomb_glove.glb`);
  - the eight-way dashes.
- **Melee while thrown:** melee does nothing while the wrench is out of the hand (thrown, or
  leaving it). A press held through the catch doesn't swing either.
- **The wrench in hand:**
  - It's in his hand whenever it's the weapon out; with another weapon, only during melee, as
    before.
  - It plays its own clip with each swing and the Hyper-Strike, and its idle pose in between,
    so the back hammer is back after the third hit.
- **Wrench-carrying clips:** with the wrench or the Shatterbomb out, the moves use the
  wrench-carrying clips. The run's turn leans work for both runs, and the idle fidgets play with
  the wrench in hand.
- **The glove:** with the Shatterbomb out he wears its glove.
  - The glove's meshes are skinned to his own joints, matched by name (the converter's
    `_scaled` nodes where he has them).
  - His own right glove mesh hides, and the hand-pose layer goes on.
- **Dashes (sim):**
  - a dash started while strafing keeps him facing the aim (snapped every tick);
  - a stick under 0.07 counts as no input;
  - jumping out of a ground dash waits for 0.1 s.
- **Dashes (sandbox):** each dash plays the clip for its direction. No firing, no aim layers,
  and the pistol and wrench are put away for its length; a cooked grenade waits.
- **Tests:** 33 libratchet tests pass (new: the strafing dash, jumping out of a dash).
- **Demo:** `--demo-part carry`, shots `50_wrench_idle` … `5b_dash_back_aiming`.

### Derived or assumed
- **The wrench with the glove on:** we assume it isn't in his hand outside melee. The glove-hand
  clips and the hand pose suggest an empty hand, but the game's rule wasn't found.
- **The glove's hand pose** plays as an override of those 21 joints (20 times the move clips'
  weight); how the game blends its GloveAdditive track wasn't read.
- **The wrench during fidgets:** its fidget clips are in the game's path list but not in the PC
  build, so it holds its idle pose through his fidgets.
- **Glove grip, missing animset entries:** the double jump and the fall use the wrench versions,
  since the animset has no thrown-grip entry for them.
- **Steering a dash:** the game lets the stick steer a dash's direction while it runs; ours
  doesn't steer yet.
- **The ground dash's movement:** the game moves it by the clip's root motion stretched to the
  config distance. Ours is still the accelerate-and-brake profile from the overnight entry.

## 2026-10-07: the game's sounds

The user asked for sounds next. A research subagent worked out the game's audio and wrote the
extractor; the sandbox side and the demo soundtrack mixer are ours.

### Known (the user's install)
- **Storage:** sounds are Wwise (bank format 135).
  - The `soundanks\*.soundbank` assets wrap Wwise banks with embedded .wem audio, the
    event/container hierarchy and every event's name.
  - Longer sounds are streamed .wem assets.
  - Sound effects are Opus (about 30,700, most of Ratchet's own) or Wwise Vorbis (about 14,100,
    most weapon sounds).
- **Animation clips:** clips call sounds by Wwise event name at set times, for example:
  - `Play_FOL_ratchet_run` on each run step;
  - `Play_FOL_ratchet_jump_swish`, `HERO_VFX_SWITCH_JUMP` (an effort grunt) and
    `Play_FOL_ratchet_jump` as a jump starts;
  - `Play_WPN_Wrench_Swing` in the swing clips (the wrench's own clips too).
  - Footsteps are the clips' FootstepEvents.
- **Code-driven sounds:** the guns, explosions, the heli-pack and the dashes are played by code
  through the game's event names.
- **Missing sounds:** the test dummy's sound events are in no bank, so it's silent in the game too.

### Built
- **Extractor:** `tools/extract_riftapart_sounds.py` (+ `riftapart_wwise.py`) follows each
  moment's events through the banks, with Ratchet's default switches and concrete footsteps.
  - It repacks Wwise Vorbis into standard .ogg, losslessly; a ww2ogg port reads the codebooks
    from the user's RiftApart.exe at run time.
  - It converts Opus with the user's ffmpeg; the user approved this.
  - It writes 38 moments and 881 files (14 MB), plus `sounds.json`. That holds the moments'
    layers, delays, pitch ranges, volumes and chances, and the event-name map.
- **The sandbox's `sound.rs`:** for each cue it plays one random variation per layer.
  - Each layer gets its delay and pitch.
  - Loops: a looping file, or the heli-pack's rotor chops retriggered every 0.09 s.
  - World sounds fade with distance.
  - **Clip-timed sounds:** clips fire their sound events as their time passes, looping clips
    included, plus footsteps at their FootstepEvents. The jump clip's landing sound plays at
    the real landing instead.
  - **Code-timed sounds:** landing, dashes, the heli-pack, hits, the slam, the wrench throw,
    loop and catch, pistol shots, bolt impacts, and the grenade throw and explosion.
- **Converter:** it writes each clip's sound events into ratchet.json and wrench.json
  (`riftapart_anim.clip_sound_events`).
- **Demo soundtracks:**
  - A recording logs every sound it plays.
  - `tools/mix_demo_audio.py` mixes them with ffmpeg under the video, with each sound's pitch.
  - `--demo-part sounds` runs through the moves and the combat section.

### Derived or assumed
- **Balance between moments:** the game's raw per-sound levels (`level_db`) span about 45 dB
  before its buses and loudness mixing, which we don't have. We play each moment at half its
  difference from a reference level, within -18..+3 dB, and the master volume is ours. It
  wasn't balanced by ear.
- **Defaults:** three switch groups with unknown names use their first option. Footsteps are
  concrete only. The pistol's two alternating variants are merged into random pools.
- **Which events play when:** which game moment fires each dash sound is read from the event
  names, not traced.

## 2026-10-07: the grenade windup and throw arc, the guns' aim poses, offscreen demos

The two "not played yet" items from the hip-fire entry.

### Known (the user's install)
- **Throw input:** the Shatterbomb's trigger is `kCookedSingleShot`. The game's PC prompt is
  "Hold [Fire] Throw Arc, Release [Fire] Throw".
- **Windup clips:** while the button is held, Ratchet's animset plays
  `weapon_glove_windup(_partial)` (the Strafe_Fire_Loop driver), both additive.
- **Aim offsets:** while strafing, the animset layers additive aim-offset clips, chosen by grip
  and speed:
  - pistol: `strafe_one_hand_{idle,run}_aim_{vertical,horizontal}`;
  - wrench/glove: `strafe_wrench_*`.
  - Each clip has 21 poses with the middle one neutral.
  - Posing the skeleton with each frame shows the pistol arm reaching +87° up and 45° down
    (vertical clip), and -64° to +96° sideways (horizontal clip).

### Built
- **Hold-to-throw:**
  - `libarsenal::Trigger::Cooked`: holding winds the shot up (`Weapon::cooking`), and it leaves
    on release. The Shatterbomb loads it from its config.
  - While held, the sandbox draws the throw arc as a thick dashed line with a ring where it
    lands. It also plays the windup layer: whole-body standing, upper-body moving.
- **Aim poses:** while aiming or firing, the aim-offset layers bend the arm and upper body toward
  the crosshair. They scrub the poses by the angle up/down and to the side of his facing,
  standing or running, for the pistol grip and the wrench/glove grip.
- **Offscreen demos:**
  - Demo runs render into an offscreen image instead of the window, so screenshots and
    recordings work while the window is minimised or behind a game. The window doesn't take
    focus.
  - Every demo run steps time exactly 1/60 s per frame.
  - A grenade's release no longer steps the weapon a second time; with a release-fired
    trigger, that would have thrown twice.

### Derived or assumed
- **Pose ranges:** the aim poses span 90° up, 45° down and 90° to each side, read off the
  pistol's measured reach. The game's own parameter ranges weren't decoded, and the
  wrench/glove poses are assumed to use the same ranges.
- **Arc look:** the arc's colour and style are ours.

## 2026-10-07: firing while moving, the game's turn, aim speeds and the grenade throws

Follow-up to the playtest fixes. The reports were "holding S while shooting just snaps Ratchet
180 degrees … then he goes back to walking" and "Ratchet starts sliding while he throws" the
Shatterbomb. A research subagent traced both to the game's data and code.

### Known (the user's install)
- **Speeds:** the game's motion config reads only `hero_shared` → `Motion.StandardSpeeds`
  (and the mag-boot set):
  - `NormalSpeedConfig`: run 4–5.75 m/s forward, backward and sideways.
  - `AimSpeedConfig`: walk 0.2–1, run 2.
  - The other speed blocks in hero_shared, `StandAimSpeedConfig` among them, are never read.
    Our aim mode used `StandAimSpeedConfig` (6.5–7.75 m/s).
  - Ratchet's walking strafe clips are authored at about 2 m/s.
- **Grenade throws:** in Ratchet's animation set, the Shatterbomb uses the underhand glove
  throw. Both versions are additive:
  - standing: the whole-body clip (0.83 s), with an `AnimFireWeaponEvent` at 0.1 s;
  - moving: the upper-body clip (0.53 s), with the event at 0.065 s.
  - The overhand clips the sandbox played are never used. With the glove out, the base clips
    are the wrench/glove strafe set and a glove idle.
- **Firing overlay:** each shot plays an additive upper-body clip
  (`weapon_one_hand_fire_stand_additive`).

### Read from RiftApart.exe
- **Strafing on fire:** every weapon strafes while it fires. No weapon sets the "no strafe on
  fire" flag. Strafing lasts while the trigger is held or the gun is firing, plus 0.1 s after
  the newest shot. There is no lingering gun-up stance; then Ratchet goes back to running.
- **The ground turn:**
  - The mover takes, each tick, an exponential share of the remaining angle (gain), plus part
    of last tick's turn (damp). It is capped per second and never overshoots.
  - Running or strafing: gain −4, damp −25, cap 1500°/s. A 180° turn is 90 % done in about
    0.25 s.
  - Standing while aiming or firing: −15, −8, 2500°/s, close to a snap.

### Built
- **Hip-fire:** firing a gun from the hip strafes toward the aim at the normal speeds,
  backpedalling if you hold back. It keeps strafing for 0.1 s after the last shot, then he
  runs again. The 1 s stance (ours) is gone.
  - `Input.aim_mode` separates aim mode (the aim button) from strafing while firing. Aim mode
    has the aim speeds and aim jumps. The C API's `aiming`: 1 = aim mode, 2 = strafing.
- **The turn:** libratchet turns Ratchet on the ground with the game's mover turn, toward the
  stick when running and toward the aim when strafing. It used a constant 14–28 rad/s (ours,
  near-instant). In the air and during moves the constant rate stays.
- **Aim speeds:** aim mode takes `StandardSpeeds.AimSpeedConfig` (2 m/s). Walking strafe
  clips (one-handed, and wrench/glove) play at that pace.
- **Clips:**
  - The Shatterbomb plays the game's underhand throws as additive layers: whole-body standing,
    upper-body moving or in the air. The grenade leaves the hand at each clip's
    AnimFireWeapon time.
  - With the glove or the wrench out, strafing uses the game's wrench/glove strafe set.
  - The pistol plays its fire overlay with every shot.
- **Converter and demo:** the converter's `ratchet.json` lists each clip's events.
  `--demo-part combat` adds the pistol fired from the hip while backing away.

### Derived or assumed
- **Which speed set:** aim mode → `AimSpeedConfig` and hip-fire → `NormalSpeedConfig` is our
  reading of the names; the code's choice wasn't traced. The 2 m/s walking strafe clips back it.
- **Running turn target:** the turn's target while running is the stick direction. The game
  passes it in the run message; we didn't trace what fills it.
- **Not built:** a turn-start variant (gain −1.5, damp −40, blending in over 0.33–0.66 s)
  exists for some transitions; we don't use it.
- **Not played yet:** the glove windup clip while holding the trigger, and the pistol's aim
  offsets.

## 2026-10-07: playtest fixes: the combo's timing, aim + melee throws, throwing on the move, the jump's eyes

The user playtested. They reported:
- the combo needs a pause after it, and its timing window felt bigger in Rift Apart;
- they asked whether you can jump and throw the wrench, and whether you have to crouch to throw it;
- firing while moving snaps Ratchet around;
- the Shatterbomb throw slides;
- his eyes look odd while jumping.

Everything below comes from the game's files and code; anything ours is marked.

### Known (the user's install)
- **Clip events:** each animation clip has a triggers section listing timed events.
  - The layout was worked out from the files: an 8-byte header, the event records, then a row
    per event with its time (a fraction of the clip) and record.
  - Every record starts with its event type's hash. The game's type tables name them:
    `AllowEarlyTransitionEvent`, `MeleeDamageImpactEvent`, `MeleeDamageBegin/EndEvent`,
    `MeleeStreakBegin/EndEvent`, `WeaponThrownEvent` and so on.
  - `tools/riftapart_anim.py` reads them, and `hero_melee_clips.json` lists them per clip.
- **The swings:**
  - Hits (MeleeDamageImpact) are at 0.085 s, 0.091 s and 0.366 s. The old estimate from the
    wrench's motion was 0.05 s, 0.12 s and 0.30 s.
  - Each swing has two AllowEarlyTransition events: 0.205/0.534 s, 0.233/0.490 s and
    0.600/0.903 s. The Hyper-Strike landing's are 0.212/0.414 s.
- **Throw controls:**
  - The game's own text says "Hold [Aim] and [Melee] to Throw Wrench" and lists "Melee Throw:
    [Aim] + [Melee]".
  - There's also an optional one-button "Melee Throw" shortcut.
  - There's no crouch button: Ratchet only crouches by himself in crawl-space volumes. Crouch +
    melee is from the older games.
  - The wrench's weapon config gates the throw on aim mode (`kRequireAimMode`), and aim mode
    works in the air, so you can jump and throw.
- **Throw clips:**
  - `weapon_wrench_throw_partial` is an upper-body additive throw; it lets go of the wrench at
    0.068 s (WeaponThrown).
  - `weapon_glove_throw_overhand_partial` is the grenade throw for the arm only (30 joints).
- **Eyes:**
  - Ratchet's animation set drives his eyes with a layer of their own over the body: Eye_Aim,
    blinks and eyebrow gestures, naming the eye-socket and eyeball joints.
  - The jump and double-jump clips scale the eyeballs to half size, which shrank his irises to
    pinpricks. That never shows in the game, because the eye layer drives those joints.
  - The helmet's squash and stretch in the same clips (0.80 × 1.44 × 0.83, about volume-neutral)
    is authored.

### Read from RiftApart.exe (Ghidra)
- **The melee state:**
  - It reads its clip's first two AllowEarlyTransition events.
  - At the first, an event handler unlocks the next swing. The next swing then starts at once if
    a melee press is queued, or as soon as one comes.
  - From the first, the stick moves you out too (only with no press queued), as do the
    swingshot and aim.
  - At the second, or at the clip's end, the swing is over.
  - A jump cuts a swing at any time. Phantom Dash is never checked during a swing.
- **The melee component:**
  - It catches presses up to 0.25 s old and queues one swing ahead; the last swing takes none.
  - After a swing ends, the combo stays alive for 0.35 s.
  - The third swing arms a 0.4 s rest at its first window and again at its second; presses are
    ignored during the rest, then the combo starts over.
  - Melee while aiming isn't a swing.
  - When mashing, hit 2 starts 0.205 s into hit 1, hit 3 0.233 s into hit 2, and hit 1 again
    1.3 s into hit 3.

### Built
- libratchet's combo follows that:
  - `chain_delay` and `cancel_delay` (ours) are gone.
  - `SwingParams` has `early_exit`/`exit`. `MeleeParams` has `press_buffer`, `combo_timeout`
    and `combo_cooldown`.
  - The C API's `RatchetSwingInfo` reports both windows.
- **Throwing:**
  - Right mouse aims with any weapon. Melee while aiming (E, or left mouse with the wrench out)
    throws, on the ground or in the air.
  - Q is the one-button shortcut.
  - The throw uses the game's upper-body clip, and the wrench leaves the hand at the clip's
    WeaponThrown time (`ThrowParams.release`).
- A Shatterbomb thrown while walking, strafing or jumping plays the arm-only throw over the move.
  Standing still, the full-body throw plays as before.
- The converter keeps the eye joints at their rest scale in every clip. The game's eye layer
  isn't played yet: its clips are a kind our decoder doesn't read.
- **Demo parts:**
  - `--demo-part combat`: mashing (three swings, the rest, the first again), a press just after a
    swing, aiming and throwing mid-jump, and a Shatterbomb thrown on the walk.
  - `--demo-part face`: his face up close through a jump.

### Derived or assumed
- The run transition's stick threshold is 0.07 (the game's), applied to our keyboard stick.
- When the third swing's rest is re-armed at its second window, both happen on the same frame
  in the game. We re-arm it.

## 2026-10-07: the game's test dummy as the combat dummies

The capsule dummies are replaced by the game's test-dummy enemy.

### Known (the user's install)
- **The model.** `characters/enemy/enm_testdummy_ps4` is a 1.9 m robot balanced on a glowing
  ball. Its materials are the body, a glass shell, an emissive inner part and the energy core
  (teal (0, 1, 0.72), intensity 2).
- **Its animation set:**
  - passive and engaged idles (the engaged one holds the arms straight out);
  - get-hit, knockback (two), knockdown, three falls backward, a twitch;
  - taunt, cheers and boos;
  - turns, runs, and attacks (fire, spray, melee).

### Ours (assumed or simplified)
- **Which reaction plays when.**
  - A hit plays get-hit. Knocked flying (over 3 m/s) plays knockback.
  - Out of health plays a fall backward. Otherwise the passive idle.
- **Facing.** They turn to face Ratchet at 4 rad/s.
- **Idle speeds.** They vary a little, so the dummies drift out of step.
- **Hit volume.** Matched to the model's height.
- **Arena.** The front dummies are spread out for the bigger model.

### Built
- **Converter:** a `testdummy` preset (model, glow, six clips).
- **Sandbox, `dummy_model.rs`:** wears the model on each dummy and picks its clips.
- **Sandbox, `combat.rs`:**
  - Dummies have a height, and can be "posed" (no physics lean or fall-over).
  - `knock` returns chest-height hit points.
- **Video:** `recordings/test_dummies_wrench_and_guns.mp4`.

## 2026-10-07: the OmniWrench's effects, from the game's data; F1 hides the debug overlay

The wrench had no effects at all. The user asked for F1 to hide the debug info so they can
record videos.

### Known (the user's install)
- **Swing streak.** The wrench's melee config names a `StreakConfig` (`wpn_Wrench_Streak`):
  - the material `wpn_Wrench_Swipe`: the `swipe_wrench` texture, colour (0.014, 0.53, 1) at
    intensity 4;
  - style kConstant;
  - swings: width ×2.5 at offset 0.3; the Hyper-Strike: ×3 at 0.5.
- **The wrench's effect conduit** (`wpn_wrench_vfx`):
  - the impact: sparks, a flash, an anamorphic flare, an impact blast;
  - the thrown wrench's trail: `trail_basic_01`, blue (0, 0.47, 1);
  - the Hyper-Strike's scorch: light blue (0.21, 0.70, 0.99);
  - a crater decal (`tb_raft_crater`).
- **Particle colour keys.** Hit (0.21, 0.67, 1). Slam (0.34, 0.78, 1). Trail (0.52, 0.85, 1)
  fading to (0.05, 0.58, 1).
- **Locators.** The wrench model has `igLoc_Damage` at the hammer head, `igLoc_Damage_Base` and
  `igLoc_dyn_fx_attach_00` (0.41 m up the shaft).

### Ours (assumed or simplified)
- **The streak.** It spans 0.2–0.62 m along the shaft and lingers 0.12 s.
- **Effect motion.** The sizes, timing and counts of the hit, slam and trail pieces.

### Built
- **`tools/extract_riftapart_vfx.py`:** the swipe, trail (as a mask), blast-mask and crater
  textures, and the wrench's palette entries.
- **`vfx.rs`:**
  - The swing streak: a ribbon through where the wrench head has been, rebuilt every frame.
  - Wrench hits: a flash and an anamorphic streak, a light, sparks, smoke.
  - The Hyper-Strike slam: a flash, the impact-blast shapes and blast masks, a ground ring,
    dust, debris, and the crater with blue embers.
  - The thrown wrench's trail.
- **`combat.rs`, `wrench.rs`:** `knock` returns where it hit, and swings, the Hyper-Strike and
  the thrown wrench spawn the effects there.
- **Debug overlay:** F1 hides or shows the HUD, the help text and the trails. `--clean` starts
  with them hidden. The demo videos are recorded that way now.
- **Video:** `recordings/wrench_effects.mp4`.

### Fixed (Bevy behaviour worth knowing)
- **The ribbon didn't draw.** An entity spawned hidden and shown later didn't draw its rebuilt
  mesh. The ribbon stays visible and collapses to a zero-area triangle instead.
- **A render error at startup.** An empty mesh (no vertices) logs one, so the ribbon is never
  empty.

## 2026-10-07: walking, leaning into turns, landings, fidgets and the wrench catch

The user had flagged the throwing and the walking to come back to.

### Known (the user's install)
- **Locomotion clips.** The authored speed of each comes from its root motion over its length:
  - `neutral_walk`: 4.27 s, 5.06 m of travel, so 1.18 m/s.
  - `neutral_run`: 1.33 s, 7.35 m, so 5.51 m/s.
  - The run's `lean_left` and `lean_right` versions: the same length, in place.
  - Landings: while walking (`neutral_jump_single_land_walk`, 1.56 m/s) and while running
    (`..._land_run`, 5.78 m/s).
  - The game's walk speeds (1.25–1.5 m/s) and run speeds (4–5.75 m/s) bracket these.
- **Idle fidgets.** `idle_look_around_quick` and `idle_color_clean_shoe` (he cleans his shoe).
- **The wrench catch.** `weapon_wrench_throw_catch_partial` (0.33 s) is a full-body clip; the
  "partial" in its name means it's meant for the upper body.
- **Not used yet:**
  - run starts by direction, 180° run turns and 90° turns in place;
  - wrench-holding walk, run and idle;
  - the throw's wind-up and its "waiting for the wrench" idle.

### Ours (assumed or simplified)
- **Walk or run.** Walk below 2.4 m/s, run above 3 m/s, keeping the current one in between.
- **Leans.** Full lean at a 5 rad/s turn, smoothed over 0.15 s.
- **Fidgets.** One plays after 7 s standing still.
- **The catch.** It uses the upper body only (the `spine_1` branch), weighted 6:1 over the move
  clip under it. Bevy's blend nodes average by weight. The throw layer fades out when the wrench
  is caught.

### Built
- **Converter:**
  - The new clips.
  - `ratchet.json` records each clip's root-motion distance (`root_distance`).
  - `root_motion` moved into `riftapart_anim.py`, shared with the configs tool.
- **Sandbox (`ratchet_model.rs`):**
  - Switches between walking and running.
  - Plays walk, run, strafe and moving-landing clips at actual speed ÷ authored speed, so the
    feet stay planted.
  - Blends the leans in step with the run by turn rate, and picks the landing by speed.
  - Plays the fidgets, and the catch through a joint mask.
- **Demo:**
  - `--demo-part moves`, shots `30_walk` … `35_fidget`.
  - The wrench section is framed on him.
  - The script's screenshots no longer make the recorder drop a frame.
- **Video:** `recordings/walking_and_throwing.mp4`.

## 2026-10-07: the weapons' effects, shells and camera, from the game's data

The user found the bullets and effects basic and asked to take more from the game. Everything
below that isn't marked "ours" comes from their install.

### Known (the user's install)
- **Which art the effects use.** Followed from the weapon configs through actors, conduits and
  `.visualeffect` files to their materials, textures and meshes:
  - The Burst Pistol's bolt (`burst_pistol_gun_projectile_fast`): a tracer mesh
    (`burst_pistol_tracer_wiggle`), a glow shape (`gbl_projectile_shape_vfx`), a "star wars"
    tracer texture, a soft glow, a smoke trail (`gbl_smoke_sim_trail_00`) and sparks.
  - Its impact (`burst_pistol_projectile_impact`): an impact flare, a glow, sparks, smoke puffs
    and a green scorch decal.
  - The Shatterbomb's explosion (`wpn_protogrenade_explosion_sub`): the shockwave hemisphere,
    lens flares and glows, debris and "dimensional shards", smoke and a scorch mark.
- **Colours.**
  - Material parameter overrides. The layout is decoded: section F5260180 holds a count and a
    data offset, then (u16 offset, u16 size, u32 name hash) entries right before the values.
    They give the bolt's green (0.03, 0.78, 0.07) at intensity 6, the impact decal's greens,
    the shell glass tint, and the Shatterbomb scorch's violet (0.17, 0.06, 0.81).
  - The particle data isn't decoded, but its half-float value table (section B1F4C248) has
    colour-like keys. They give the bolt's lime glow, the impact flash, and the explosion's
    white-hot to orange fire, violet energy, magenta shards and grey smoke.
- **Shells.** The Burst Pistol's `ShellCasingData` names a shell actor
  (`burst_pistol_shell_01`: glass, an inner part, metal ends). Shells eject at 2–4 m/s, spin up
  to (45, 5, 45), from the gun's `igLoc_shell_eject` locator. That locator is on the barrel
  joint and points out of the gun's right side and up.
- **Camera.** The weapon's `ZoomConfig` points at the camera contexts
  (`CameraAimContextConfig`):
  - Default: a pivot 1.5 m above the feet, 4.65 m back, FOV 76°. Moving: 6 m back, pivot
    1.65 m, FOV 82°. Looking up: 3 m back.
  - Strafe (aiming): pivot 1.6 m, 0.5 m to the right, 4.5 m back, FOV 72°.
  - The FOVs are horizontal (82° vertical would be far too wide).
- **Camera shakes.** A `CameraShakingConfig` has three sine curves per axis (magnitude,
  frequency, phase), a duration and a fade-out.
  - The Burst Pistol's recoil (`camerashake_recoilsmall`) lasts 0.08 s.
  - The Shatterbomb's explosion (`camera_CameraShake_MediumExplosion`) holds 0.1 s and fades
    over 0.7 s. It's full strength within 30 m and gone past 50 m.

### Ours (assumed or simplified)
- **Effect motion.** The particle systems aren't decoded, so the effects' timing, sizes, speeds
  and counts are ours. Palette keys are picked by offset in a table we can't read yet.
- **Units we guessed.** The shake curves are read as degrees of camera yaw, pitch and roll. The
  shell spin is read as rad/s (as degrees the shells would barely turn).
- **Camera blending.** How the game blends its contexts isn't known. Ours: by pitch (full "down"
  at 1.2 rad, "up" at −0.6) and by speed (2.5 to 6 m/s). Zoomed in closer than the game's
  distance, the camera looks lower, toward his chest. The demo has a close-up mode.
- **Small things.** Ratchet keeps the gun up for 1 s after firing. The bolt's 2 m streak, its
  head glow, the 0.35 s trail and the lights' brightness are ours.

### Built
- **`tools/extract_riftapart_vfx.py`:** adds the smoke trail texture and the shell model, and
  writes a palette to `vfx.json` (material parameters and effect keys). The muzzle flash becomes
  a white mask to tint.
- **`tools/extract_riftapart_ratchet.py`:** exports model locators (`igLoc_*`) as glTF nodes
  under their joints.
- **`tools/extract_riftapart_configs.py`:** also extracts the two camera contexts and the two
  shakes.
- **libarsenal:** `ShellParams` (from `ShellCasingData`) and `ShakeRef` (the recoil and
  explosion shakes, with radii). 12 tests.
- **Sandbox, `vfx.rs`:**
  - Bolts: the game's tracer mesh and glow shape, a streak stretched by speed with a glow at its
    head that faces the camera, a green light, and a smoke trail.
  - Muzzle flash: the game's flash mesh and flash shape, a glow and a burst of light.
  - Impacts: flare, glow, light, sparks that bounce, smoke, and a bullet mark that glows green
    and cools.
  - Shells that tumble out of the ejector and bounce on the floor.
  - The Shatterbomb blast: a burst of light, a white-hot flash, an orange fireball, the game's
    shockwave dome in violet, violet shards, smoke, debris, and a scorch mark with violet embers.
    The grenade glows violet in flight.
- **Sandbox, `camera.rs`:** the game's camera contexts and shakes. The follow camera uses them,
  turning the horizontal FOV into Bevy's vertical one.
- **Look:** HDR and a light bloom.
- **Demo:** side views of hip fire (bolts crossing the screen, shells) and of a Shatterbomb, and
  close-up framing for the animation shots. Video: `recordings/weapons_game_effects.mp4`.

### Fixed
- **Effects cast shadows.** That put a blue ring and grey squares on the floor under the blast.
  Effect pieces no longer cast shadows.
- **Black and magenta blobs in the bolts.** In Bevy, `LinearRgba * k` scales alpha too. A "6×
  bright" colour had alpha 6, so blending subtracted the background. The effect materials now
  reset alpha to 1.
- **The sky turned grey-blue with HDR.** The clear colour now goes through the tonemapper too,
  so it's set to the colour that comes out as before.

## 2026-10-07 (night): weapons: the Burst Pistol and the Shatterbomb

The user said "weapons next!". There's a new engine-independent crate, `libarsenal`, plus aim
mode in libratchet and the two weapons in the sandbox.

### Known (the user's install)
- **Weapon configs.** Each weapon item (`configs\equipment\wpn_<name>_item.config`) points to
  a `WeaponConfig`. It holds:
  - a behaviour class (`Prius`, weapon-specific code);
  - triggers (auto, single or cooked, rounds per second);
  - ammo;
  - shot motion (flight speed, gravity, range, collision radius; grenades aim at the target);
  - an explosion (inner and outer radius, knockback tiers);
  - 10 levels of upgrades (damage, rate of fire, ammo, range).
  The game has 56 weapon item configs.
- **The Shatterbomb is `wpn_protogrenade`.** Its upgrades spawn 4 shards, there are shard
  models, and the projectile's material is called `proj_Shatter_grenade_godray`.
- **Burst Pistol (level 1):**
  - 11.4 damage, 150 ammo;
  - 3 shots/s (`kRateOfFire`);
  - shots at 80 m/s with no gravity, out to 45 m;
  - spread starts at 0.1°, grows 0.05° a shot after 3 shots, up to 4°, and recovers at 25°/s;
  - while aiming: 0.01° a shot, up to 0.65°.
- **Shatterbomb (level 1):**
  - 0.8 throws/s, 6 ammo;
  - lobbed at 30 m/s under 30 m/s² gravity, aimed at most 30 m out;
  - explodes on impact: 170.5 damage within 1 m, falling to 85 at 6 m;
  - knockback tiers out to 3, 6 and 20 m.
- **Aim mode:**
  - `StandAimSpeedConfig`: runs at 6.5–7.75 m/s, 5 backward, walks at 2.
  - `AimModeSingleJumpConfig` / `AimModeDoubleJumpConfig`: fixed 1 m jumps, a 1.8 m total cap,
    0.32 s and 0.28 s to the peak.
- **Mounting:** the pistol uses the same hand joint as the wrench (`RT_handle_bound`). In the
  one-handed aim pose that joint's axes line up with the model (Y up, Z forward), and so does
  the gun's.
- **Clips:**
  - one-handed stance (`strafe_one_hand_idle`) and 8-way strafing
    (`strafe_one_hand_run_{n,ne,e,...}`);
  - holding the pistol (`relaxed_one_hand_idle`, `run_forward_pistol`);
  - the overhand grenade throw (`weapon_glove_throw_overhand`);
  - the gun's own fire clip (its barrel turns);
  - the grenade model `proj_protogrenade_grenade`.

### Ours (assumed or simplified)
- **Rates:** the Burst Pistol fires 3/s while aiming and 6/s from the hip. The game's
  full-trigger rapid fire is probably one of the `kMisc` values; we took `kMisc3` = 6.
- **Aim speeds:** of the two aim speed configs, we use `StandAimSpeedConfig` for strafing.
- **Knockback distances** per knockback level (kOne 0.4 m … kFour 3.5 m).
- **Grenade release:** 0.16 s into the throw.
- **Sandbox:** the dummies' 150 health; empty weapons refill after 1.5 s.
- **Not used yet:** the recoil clips. They're additive arm layers, and with Bevy's rule
  (offset × base) the arm gets shoved down. One of them turns the forearm 175°, so either our
  decode or our composition is off for those. Needs the game's additive rules from Ghidra.

### Built
- **`crates/libarsenal`:**
  - `WeaponParams` with generic defaults and `from_riftapart_config`.
  - `Weapon`: auto or single triggers, aimed vs hip-fire rate and spread, ammo, a
    deterministic spread cone.
  - `Projectile`: exact gravity steps, range and lifetime.
  - `lob_velocity`: the low arc to the aimed point, 45° when out of reach.
  - `ExplosionParams`: linear damage falloff and knockback tiers.
  - 12 tests, including loading the real configs.
- **libratchet aim mode:**
  - `Input.aim` makes him face the aim direction (turning twice as fast).
  - The aim speeds apply (slower backing up), and so do the aim-mode jumps.
  - 3 tests. The C API has `aiming`, `aim_x`, `aim_z`.
- **Sandbox (`weapons.rs`):**
  - 1/2/3 pick the weapon. Left mouse fires, right mouse aims: the camera moves in over the
    right shoulder (blending over 0.13 s, `Aiming.StandardAimCameraBlend`) and a crosshair
    appears.
  - Shots go where the camera ray first hits the level or a dummy.
  - Bolts with a muzzle flash, the grenade with the game's model, a fireball, and explosion
    damage and knockback by distance.
  - Dummies have health; at zero they fall over, then get back up after 2.5 s.
  - Ratchet's stance, strafing and throw clips are wired in.
- **Demo:** `--demo-part weapons`, shots `21_burst_pistol_aim` … `26_shatterbomb_blast`.
  Video: `recordings/weapons_burstpistol_shatterbomb.mp4`.

## 2026-10-07 (evening): the OmniWrench

The user asked for the wrench next. The sandbox now has the 3-hit combo, the Hyper-Strike and
the throw, with Ratchet's own clips and the wrench model in his hand. Target dummies in a new
arena take the hits.

### Known (configs from the user's install)
- **Combo** (`configs\equipment\wpn_wrench_melee.config`): three hits of 20 damage each,
  knocking a standard enemy back 2.5, 2 and 4 m.
- **Hyper-Strike** (melee in the air):
  - It hops by `MeleeSmashJumpConfig` (1 m in 0.242 s, in `hero_traversal`).
  - It needs 0.33 m of room below (`MeleeSmashMinHeight`, `hero_shared`).
  - It comes down at 1000 m/s² (`MeleeSmashDescendGravity`), capped at 70 m/s.
  - The slam does 50 damage in a 3 m radius.
- **Throw** (`wpn_wrench_weapon.config`, `WeaponWrenchPrius`):
  - Out at 30 m/s, up to 15 m.
  - Back at up to 60 m/s with a "jerk" of 60.
  - Spins at 1800°/s and does 30 damage.
- **Where the wrench goes:** the game mounts weapons on the `igLoc_RT_gunAttach` locator. It
  sits on Ratchet's `RT_handle_bound` joint with no offset, and the wrench's origin is its
  grip.
  - Not `prop_attach`: that joint moves backward as Ratchet lunges, so it marks where the
    swing started.

### Read from RiftApart.exe
- **Root motion** (clip section `E08AA35F`, sampled by the game's motion-data function):
  - byte 0 = format (0 = 16-byte keys), byte 2 = key count;
  - keys start at `0x7c + int32 @0x3c`;
  - each key = int16 quaternion / 32768, then int16 xyz × 2^-n, with n in byte 14.
- **Sanity check:** Ratchet's run clip covers 5.5 m/s, and his configured run speed is
  5.75 m/s.
- **The swings lunge 0.79, 1.10 and 1.60 m.** libratchet moves him by that curve over each
  swing.

### Derived or assumed
- **Hit times:** 0.05, 0.12 and 0.35 s. That's the moment the wrench moves fastest near each
  swing's biggest lunge, computed by `tools/extract_riftapart_configs.py` into
  `hero_melee_clips.json`. The game's own hit timing hasn't been found.
- **Combo timing (ours):**
  - A press during a swing queues the next one, 0.1 s after the hit.
  - Moving, jumping or dashing cuts a swing from 0.25 s after the hit.
  - Pressing on after the third hit starts over.
- **Hyper-Strike motion:** it drifts forward by the intro clip's root motion while rising, then
  slams straight down.
- **Throw path:** out at a constant speed, then home on the hand with the jerk-limited speed.
  A dummy or a wall turns it back.
- **Sandbox only:**
  - The swing reaches 1.8 m.
  - Knockbacks slide at 20 m/s² deceleration, so a hit travels its configured distance.
  - The slam pushes dummies 3 m.
- **Not decoded:** the wrench's own Hyper-Strike clips use a smaller base-state layout, so the
  wrench doesn't squash and stretch during the slam.
- **Dash data point:** the dash clip's root motion covers 5.96 m, while `DodgeConfig.Distance`
  is 7. That bears on assumption 9.

### Built
- **libratchet:**
  - New states `Melee`, `Smash`, `SmashLand`.
  - Events `MeleeSwing`, `MeleeHit`, `SmashStarted`, `SmashImpact`.
  - `Input.melee`.
  - `WrenchThrow` for the throw.
  - `MeleeParams`/`SwingParams`/`ThrowParams` in `Tuning`, filled from the configs when they're
    present.
- **C API:**
  - `melee` input, states 7–9 and event bits 8–11.
  - `ratchet_swing_info`, `ratchet_smash_info`.
  - `ratchet_wrench_throw`/`_step`/`_free`.
- **Converter:** Ratchet's six wrench clips, plus `wrench.glb` (the model with its three swing
  clips).
- **Sandbox:**
  - Left mouse/E melee, right mouse/Q throw.
  - The wrench shows in his hand while swinging.
  - Target dummies, camera shake on the slam, a HUD line.
  - Demo shots `13_wrench_ready` … `20_wrench_hit`.
  - Videos `recordings/omniwrench_demo.mp4` and `omniwrench_combo_slowmo.mp4`.

### Fix: the throw sank Ratchet into the floor (reported by the user)
- **Cause:** `weapon_wrench_throw_hero_ratchet` is an additive clip (flag `0x40` in its
  header). Its values are offsets from its first frame, so its pelvis sits at (0, 0, 0).
  Played as a normal clip, that put his hips on the ground. Of the clips we use, only the
  throw clips have this flag.
- **The converter** marks additive clips. It writes them as `<state>_additive` with rotation
  and translation offsets only: Bevy adds scales in additive blending, so scale channels are
  dropped.
- **The sandbox** builds Ratchet's animation graph as an additive node over a blend node. The
  move clips crossfade in the blend node, and the throw plays on top of whatever he's doing,
  fading in over 0.08 s and out over 0.25 s. He can throw while running.
- **How the game combines rotation offsets** isn't known. Bevy applies them in the parent's
  space (offset · base), and that looks right. Re-expressing them in each joint's own space,
  around the idle pose, bent the tail up over his shoulder.
- `--demo-part wrench` runs only the wrench section of the demo. Shot `21_running_throw`.

### Tested
- `cargo test --workspace`: 24 libratchet tests, including the following:
  - lunge distance and hit timing;
  - combo order and looping;
  - move-cancel timing;
  - the Hyper-Strike hop, slam and minimum height;
  - the throw's round trip and early return;
  - the real configs (3 swings, 1000 m/s² slam).
- 3 C API tests.
- The C++ smoke test (MSVC) now also swings and throws through the header.

## 2026-10-07 (late afternoon): Clank's head size and heli-pack

The user compared Clank with a screenshot from the game: his head was too big and the
heli-pack rotors never showed. Both are fixed.
- **Scale channel decoded** (`tools/riftapart_anim.py`). Clips carry a static scale per joint
  (in the Base State section after the per-joint records: two joints per 16-byte row, 4.12
  fixed point) plus animated scale (channel 0 of the packed samples). As a backpack, Clank
  shrinks himself: `a_body` ×0.84, `head_scale_tweak` ×0.8, `head` ×0.55, plus his limbs.
  The heli-pack is hidden or shown by scaling its joints.
- **Joints that don't inherit scale.** Bit 0 of a joint's flags (ALERT calls the field
  `unknown2`) marks a joint that ignores its parent's own scale. Its offset still stretches
  with that scale, like Maya's "segment scale compensate". Every `*_noScale` helper joint has
  this bit, and so do the joints above Clank's telescoping rotor stem. Without the rule, the
  top rotor squashes into a vertical line. *Inferred from the data, not found in the game's
  code.*
  - glTF and Bevy always inherit scale, so the converter bakes the rule in. Such a parent gets
    a `<name>_scaled` child that carries its scale, and each compensating child's offset is
    multiplied by the parent's scale at every key.
  - This affects 26 of Clank's joints and 24 of Ratchet's (tail, spine, limbs, eyes).
- **Heli-pack meshes.** These are model looks 4–6 (top rotor with its own antenna, left and
  right arm rotors), separate from look 0, so the converter never exported them. It now
  exports looks 0, 4, 5 and 6 and tags each mesh node with glTF extras
  `{"look": n, "part": "heli"}`. The sandbox shows them only while the heli-pack is out
  (extend, glide, retract), copying the game's look switching. Before this, his own antenna
  and the heli antenna overlapped in idle.
- **Rotor spin.** The clips hold the rotors still, so the game must spin them in code. The
  sandbox spins the three `*BladeSpin` joints at 22 rad/s (a guess). The spin axis is the
  rotation from blade A to blade B.
- **Blur discs.** `hero_Clank_Rotorblades.material` has no textures (it's an effect shader in
  the game), so it's drawn as translucent grey.
- **Demo:** `12_glide_heli_a/b`. Video: `recordings/clank_heli_closeup.mp4` (half speed).

## 2026-10-07 (afternoon): Clank, git

- **Clank** rides on Ratchet's back like in the game. The game's Clank Pack config attaches Clank
  (`hero_clank_ps4.model`) so his `dyn_backpack_attach` joint sits on Ratchet's `ClankAttach`
  joint (both carry the `igLoc_clankBackpack` locator). The sandbox re-snaps that every frame,
  because the backpack clips move the attach joint.
- **Clank's clips:** backpack idle, and the broken heli-pack extend → glide → retract while
  Ratchet glides (the game's Clank Pack config sets `BrokenClank`).
- **Clank's look:** his colour maps are dark because the game's silver comes from metallic
  reflections. The converter brightens them and marks the materials metallic. The sandbox
  camera got a sky-to-ground environment light so metal has something to reflect. Glow and
  glass materials (eyes, antenna) use the game's pink placeholder textures, so they get a green
  glow and a tinted transparent finish instead.
- The converter now has presets: `--character ratchet|clank|all` (default all).
- **Git:** repository initialised, first commit; game data stays in gitignored folders.

## 2026-10-07 (day): Ratchet's real model, textures and animations in the sandbox

`tools/extract_riftapart_ratchet.py` (GPLv3, uses ALERT) converts, from the user's own install,
into `riftapart-assets/ratchet.glb` (gitignored, about 28 MB):
- **Model:** `hero_ratchet.model` look 0, 22 textured meshes, a 205-bone skin. Fixed on the way:
  ALERT's UV scale for Rift Apart models (UVs came out as 0), float UVs marked "normalized",
  and 8 bone influences merged into the 4 Bevy supports. All the skeleton's root joints go
  under one `rig_root` so a single AnimationPlayer drives the whole body.
- **Textures:** colour, normal (BC5-style X/Y → rebuilt Z, Y flipped) and gloss → roughness,
  decoded from BC7 etc. The high-res mips are GDeflate-compressed. ALERT needs a prebuilt
  libdeflate.dll for that and silently returns zeros without it, so `tools/gdeflate-ffi` (built
  from source via the `gdeflate` crate) decodes them.
- **Animations:** `tools/riftapart_anim.py` decodes the game's bit-packed joint clips (format
  notes at the top of that file): idle, run, jump, double jump (flip), fall, glide, land, dash
  and air dash. They're added as named glTF animations.
- **Sandbox:** loads the model when present (capsule otherwise), plays the clips by move state
  with crossfades (the run speed follows the actual speed), and falls back to a procedural pose
  if a file has no clips. Demo close-ups: `05_close_idle` … `11_close_dash`.
- Not yet: fur shells, the scale channel (squash and stretch on 12 joints), root motion, the
  phase-dash shell effect.

## 2026-10-07 (morning): jump and glide checked against RiftApart.exe

Read with pyghidra-mcp; the details and addresses are in the private
`riftapart-decomp/JOURNAL.md`. No game code was copied, only the rules. Confirmed and now in
libratchet:
- **Gravity comes from the tap height:** `g = 2·HeightMin/TimeToPeak²`, take-off speed
  `2·HeightMin/TimeToPeak`. A tap peaks at exactly TimeToPeak.
- **Holding** (until ButtonMaxHoldTime) moves the target from HeightMin towards HeightMax:
  `lerp(HeightMin, HeightMax, clamp((t − ButtonMin)/(ButtonMax − ButtonMin)))`. Every tick the
  rise is re-aimed with `v = sqrt(2·g·(target − risen))`. Letting go freezes it. (Overnight I
  had launched for HeightMax and cut on release; that was wrong.)
- **Same gravity after the peak** (assumption 2 below was right).
- **Vertical integration:** `v' = max(v − g·dt, −terminal)`, move by `(v + v')/2·dt`. Same as
  libratchet.
- **Double-jump cap:** both heights are clamped to `MaxTotalJump − height gained`; if already
  past it, a fixed 0.5 m. Gravity follows the clamped tap height, so a capped double jump still
  peaks after TimeToPeak.
- **Glide:** when it starts, the fall-speed limit is the current fall speed clamped to 4–12 m/s,
  kept for the whole glide (replaces the made-up 20 m/s² "catch").

With the real configs: gravity 27.4 m/s², tap 1.000 m at 0.267 s, held 1.500 m at 0.367 s.
18 tests pass.

Still open: the glide's other start conditions (only `stateTime ≥ NoGlideTime` is confirmed;
the 1.3 m height minimum is still our guess), whether take-off speed replaces or adds to the
current vertical speed (we replace), and the dash, which looks animation-driven in the game.

### Assumptions (to replace with Ghidra facts or measurements)
1. ~~Variable jump: launch for HeightMax and cut on release.~~ Replaced by the confirmed model above.
2. Gravity after the peak = the same jump's gravity. **Confirmed.** Walking off a ledge uses
   FallJumpConfig (gravity 25, terminal 50).
3. Glide starts when jump is held, you're descending, NoGlideTime has passed since that jump
   started (**confirmed**), and you're at least GlideJumpConfig.DoubleJumpHeightMin (1.3 m)
   above the ground (still a guess).
4. ~~Glide catches a faster fall at 20 m/s².~~ Replaced by the confirmed 4–12 m/s entry clamp.
5. ~~A capped double jump keeps at least 35%.~~ Replaced by the confirmed clamp / 0.5 m rule.
6. Ground acceleration 35 m/s², deceleration 45, air 18, glide 10, turning 14 rad/s (now only in
   the air; the ground turn is the game's mover turn, see the hip-fire entry). No
   accel values were found for standard running (hover boots have RunAccel/RunDecel).
7. MomentumWeight*, NoFallTime and NoInputTime aren't used yet.
8. Which jump set is live: we use `hero_traversal.JumpConfigs`. `hero_shared.Traversal` has
   different values (single jump 2.1 m) and may be a leftover.
9. Dash (DodgeConfig: Distance 7, MaxSpeed 30, Acceleration 100, Deceleration 50): speed up,
   then brake in time to be back at run speed after exactly `Distance`. A first try that
   braked *after* the 7 m went about 16 m, which looked far too long for the Phantom Dash.
10. Air dash (AirDashConfig: Distance 7, MaxSpeed 12, no accel/decel given): flat (no gravity)
    at 12 m/s for 7 m, once per air time, then falls with speed capped to the air speed.
11. Shared.Traversal also has DashTime 0.5 / DashDistance 10 (unused; probably a leftover like
    its jump set).
12. Joint flag bit 0 = "don't inherit the parent's scale, but scale the offset" (Maya-style
    segment scale compensate). This fits every rig we've converted, but the matching code
    hasn't been found in RiftApart.exe.
13. Clank's rotors spin at 22 rad/s. The blur discs are translucent grey. The heli looks
    (4–6) are only on while the heli-pack is out. Looks 1–3 (body without arms, left arm,
    right arm) aren't used.
14. ~~Swing hit times come from the wrench's fastest moment near each lunge peak.~~ Replaced by
    the clips' MeleeDamageImpact events (see the playtest-fixes entry).
15. ~~Combo chaining is 0.1 s after the hit; cancelling is 0.25 s after the hit.~~ Replaced by the
    clips' AllowEarlyTransition events and the game's melee rules (playtest-fixes entry).
16. The Hyper-Strike drifts forward only while rising, then drops straight down.
17. The thrown wrench flies out at constant speed, then returns with the configured jerk and
    top speed.
