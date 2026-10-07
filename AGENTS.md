# Rules for agents working on this repo

Project: Ratchet & Clank: Rift Apart movement, rebuilt as an engine-independent library
(`libratchet`), tested in a Bevy sandbox, and later plugged into Skyrim SE through SKSE.

## Hard rules
- **No game files in this repo.** No Rift Apart or Skyrim assets, configs, archives, or JSON
  extracted from them. Extracted data goes in `riftapart-configs/` (gitignored) or outside the
  repo (`../riftapart-decomp/`).
- **No decompiled code in this repo.** Ghidra output, offsets and notes stay in
  `../riftapart-decomp/`. Write our own code from our understanding of the behaviour.
- Single-player and offline only.
- Don't commit or push unless the owner asks.

## Layout
- `crates/libratchet`: the movement sim. Pure Rust (glam + serde_json). Y up, metres, seconds.
  The host owns collision: each tick it passes input + contacts in and gets a velocity out.
- `apps/sandbox`: Bevy 0.19 test scene. `cargo run -p sandbox` (dependencies are optimised even in dev builds).
  `--demo` plays a scripted run and saves screenshots to `screenshots/`.
- `tools/extract_riftapart_configs.py`: GPLv3 (uses ALERT). Writes the user's own Rift Apart
  movement configs as JSON.
- `tools/extract_riftapart_ratchet.py` + `tools/riftapart_anim.py`: GPLv3. Convert Ratchet's
  model, textures and animation clips from the user's install into
  `riftapart-assets/ratchet.glb` (gitignored game data). They need `tools/gdeflate-ffi` built
  in release (GDeflate decoder for the high-res data).
- `crates/libratchet-ffi`: C API (`include/ratchet.h`) for the planned SKSE plugin.

## Workflow
- Run `cargo test -p libratchet` after any sim change. The tests check the sim against the
  tuning numbers (apex heights, time to peak, caps, glide speed).
- Run `cargo run -p sandbox -- --demo` and look at the screenshots before you call a
  change done.
- Mark anything not backed by the game's data as an assumption in code (`// Assumption`) and in
  `MODLOG.md`. Replace assumptions with facts from Ghidra or measurements.
- Log every confirmed fact and every change in `MODLOG.md`.

## Environment (owner's PC)
- Rust: `RUSTUP_HOME=G:\dev\tools\rust\rustup`, `CARGO_HOME=G:\dev\tools\rust\cargo`.
- Ghidra 12.1.4 + JDK 21: `G:\dev\tools`. Project `../riftapart-decomp/ghidra/RiftApart.gpr`.
  The MCP server `pyghidra` (pyghidra-mcp, stdio) is registered locally for this project and
  opens that project. Only one process can have the project open at a time, so close it before
  running headless Ghidra scripts.
- Skyrim SE 1.6.1170 + SKSE, modded with MO2 at `G:\Modding\MO2`.
