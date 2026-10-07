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
6. Ground acceleration 35 m/s², deceleration 45, air 18, glide 10, turning 14 rad/s. No
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
