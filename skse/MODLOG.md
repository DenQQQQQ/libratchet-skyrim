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
- Jump is bound to keyboard 0x39 (Space) and gamepad 0x8000 (A).
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
  ALYSLC does (INI `bAnimations`). Not checked in game yet.

### Choices (assumptions, not from the game)
- Jump only: a press counts if it began on the ground, and the sim stops seeing the button once
  the jump turns down, so the sim's double jump and glide can't start. No press buffer before
  landing (Skyrim has none either).
- Fall damage stays Skyrim's, counted from the apex: a jump never hurts more than stepping off
  the same height would. (The plan said to switch fall damage off during Ratchet moves; for the
  plain jump this keeps Skyrim's rules.)
- Sideways movement in the air is still Skyrim's.
