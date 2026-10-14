# Plan: libratchet in Skyrim SE (SKSE plugin)

Nothing here is built yet. This is the plan for the next step, written so it can be reviewed
before anything is installed. Items marked (verify) are from memory of how other SKSE movement
mods work and need checking against CommonLibSSE-NG before relying on them.

## Target
- Skyrim SE **1.6.1170** with SKSE (owner's install), managed by MO2.
- Address Library, Nemesis and OAR are already installed there.

## Pieces
1. **`ratchet.lib` / `ratchet.dll`**: from `crates/libratchet-ffi` (done). The C header is
   `crates/libratchet-ffi/include/ratchet.h`. A C++ smoke test already links and jumps to the
   right height through it.
2. **`skse/` plugin (C++23, CommonLibSSE-NG)**: thin. Each frame it:
   - reads input (keyboard: jump, dash) through an input event sink, and the camera yaw to make
     movement camera-relative;
   - reads the player's position and whether Havok says the player is on the ground, plus a
     downward ray for ground distance;
   - converts to libratchet space (Skyrim is **Z up**, about **70 units per metre**), calls
     `ratchet_step`, converts the velocity back;
   - applies it to the player's character controller (verify: `bhkCharacterController`
     velocity / gravity fields, as the Paraglider and Better Jumping mods do);
   - turns off fall damage while a Ratchet move is active (verify: reset fall start height /
     fall time on the controller);
   - optionally fires animation events for jump / double jump / glide / dash (vanilla
     animations first).
3. **Config**: an INI next to the DLL: key bindings, scale (default 1.55, "Dragonborn-sized"),
   path to the extracted Rift Apart configs (`riftapart-configs/`), on/off toggle.

## What the plugin tells the sim (added since this plan was written)
- **Each tick**, besides the stick, buttons and ground: the camera's forward and right (wall
  jumps take their side from it; the Swingshot picks targets in its view and pumps along it),
  the nearest wall (its normal, whether it takes wall jumps, and how far out it is), and during
  wall jumps whether a floor is ahead (`landing_ahead`). Buttons now include crouch (lets go of
  a ledge) and the Swingshot.
  - **The wall's distance.** As in the game, he sticks to a wall from up to 0.775 m out and is
    held 0.6 m off it (× scale), where his wall poses put his hands and feet on it. So the
    plugin should probe for the nearest wall within about 1 m × scale (a few horizontal
    sphere or ray casts around him at mid-body height) and pass `wall_distance`. Reporting
    only walls the character controller touches still works, but he's then pressed to the
    wall at the capsule's radius, and his hands go into it.
- **The level, through `ratchet_set_world`:**
  - **Rays.** Catching a ledge needs them, and they're all Skyrim can offer for ledges: it has no
    placed ledge paths, so only lips he catches on the way down.
  - **Placed ledges and Swingshot targets.** Skyrim has neither. Targets would have to be placed
    by the plugin (markers, or anchor points it picks out), which is a design question for later.
  - **Walls.** Skyrim has no marked wall-jump walls either, so every wall is a plain wall (one
    wall jump per jump) unless the plugin marks some.
- **Paused frames** can pass a tick of 0 s; the sim ignores it.

## Build tooling needed (not installed yet; needs the owner's OK)
- vcpkg (git clone + bootstrap) for CommonLibSSE-NG and its dependencies.
- CMake (installed) + Ninja (ships with VS Build Tools) + MSVC 14.44 (installed).

## Test plan
1. Build the plugin, install it into MO2 as a **separate mod** (easy to switch off).
2. Load a save in an open area (e.g. outside Whiterun), and test jump heights against the
   sandbox numbers (Dragonborn-sized: 2.3 m jump, 4.3 m max with double jump).
3. Check fall damage, swimming, horses and cutscenes don't break (the plugin should step aside
   whenever the player isn't in normal on-foot control).

## Known risks
- Skyrim's character controller has its own ground snapping, slopes and stairs handling that
  fights custom velocities. The sandbox host model (sim gives velocity, host does collision)
  was chosen to match this.
- Animation: vanilla jump animations won't match a double jump or glide at first.
- Other movement mods (TDM, Better Jumping, Paraglider) may conflict; check the load order.
