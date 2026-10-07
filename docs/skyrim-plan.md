# Plan: libratchet in Skyrim SE (SKSE plugin)

Nothing here is built yet. This is the plan for the next step, written so it can be reviewed
before anything is installed. Items marked (verify) are from memory of how other SKSE movement
mods work and need checking against CommonLibSSE-NG before relying on them.

## Target
- Skyrim SE **1.6.1170** with SKSE (owner's install), managed by MO2 at `G:\Modding\MO2`.
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

## Build tooling needed (not installed yet; needs the owner's OK)
- vcpkg (git clone + bootstrap) for CommonLibSSE-NG and its dependencies.
- CMake (installed) + Ninja (ships with VS Build Tools) + MSVC 14.44 (installed at
  `G:\dev\VS2022BuildTools`).

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
