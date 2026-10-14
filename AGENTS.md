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
  `riftapart-assets/ratchet.glb` (gitignored game data), and the other models the sandbox
  uses (Clank, the weapons, the test dummy, the twelve bolt pickups: `--character bolts`). They
  need `tools/gdeflate-ffi` built in release (GDeflate decoder for the high-res data).
- `crates/libratchet-ffi`: C API (`include/ratchet.h`) for the SKSE plugin (`skse/`, see its README).
- Players' install: `install.ps1` / `Install.bat` (GPLv3) extract from the player's own Rift Apart
  and run `skse/convert.ps1` (the converters into Skyrim's formats, shared with `skse/build.ps1
  -Install`); `skse/package.ps1` makes the release zip (our files only, checked for game data);
  `.github/workflows/release.yml` runs it for a tag. The player's guide is `INSTALL.md`.
- `crates/libarsenal`: weapons (trigger, ammo, spread, projectiles, lobbed aim, explosion
  falloff, shell and camera-shake data). Pure Rust like libratchet; the host does aiming,
  collision, damage and effects. Run `cargo test -p libarsenal` after weapon changes.
- `crates/libbolts`: the bolts (currency): an enemy's reward and its split into pickups, the
  burst, bounces, rest, the swarm to the hero, the caps, and the HUD's tally. Pure Rust like
  libarsenal; the host gives it a segment query and draws, sounds and shows the results. Run
  `cargo test -p libbolts` after bolt changes. Its C API is in `libratchet-ffi`
  (`ratchet_bolts_*`, `ratchet_tally_*`).
- `tools/extract_riftapart_sounds.py` + `tools/riftapart_wwise.py`: GPLv3. Extract the game's
  sounds for the sandbox's moments into `riftapart-assets/sounds/` (gitignored) with
  `sounds.json`: Wwise banks and streamed .wem, Vorbis repacked (codebooks found in the user's
  RiftApart.exe at run time), Opus converted with the user's ffmpeg. The sandbox's `sound.rs`
  plays them; animation clips fire theirs by Wwise event name (the converter writes each clip's
  sound events into ratchet.json). `tools/mix_demo_audio.py` mixes a recorded demo's sounds.log
  under its video.
- `tools/extract_riftapart_vfx.py`: GPLv3. Exports the effect art the sandbox's weapon effects
  use (`riftapart-assets/vfx/`, gitignored), with a colour palette read from the game's effect
  materials and particle data.
- `tools/extract_riftapart_menu.py`: GPLv3. Exports the title logo, the act 1 title card, the
  menu music (the pause menu's playlist: an intro and its seamless loop) and the press-start
  sound into `riftapart-assets/menu/` (gitignored), for the Skyrim plugin's main menu, startup
  video and loading screens. The install's `dag` file lists every asset path.
- Bevy gotchas:
  - `LinearRgba * k` scales alpha too. Reset alpha when you brighten a colour for a material
    (see `vfx.rs`).
  - A mesh entity spawned `Visibility::Hidden` and shown later didn't draw its rebuilt mesh.
    Keep dynamic meshes visible and collapse them instead (the swing streak).
  - With HDR, the clear colour goes through the tonemapper too.
  - A model's scene can be spawned a second time just after it loads (its asset reports a
    change), with new entities. Set up animation graphs per player entity, not once (see
    `clank.rs`'s `on_ready`), and look up locators again when new ones appear.

## Workflow
- Run `cargo test -p libratchet` after any sim change. The tests check the sim against the
  tuning numbers (apex heights, time to peak, caps, glide speed).
- Run `cargo run -p sandbox -- --demo` and look at the screenshots before you call a
  change done.
- Mark anything not backed by the game's data as an assumption in code (`// Assumption`) and in
  `MODLOG.md`. Replace assumptions with facts from Ghidra or measurements.
- Log every confirmed fact and every change in `MODLOG.md`.
- Don't run `cargo fmt` over the repo: there's no rustfmt config and the code isn't
  rustfmt-formatted (lines run to about 160 columns). Match the surrounding style by hand.

## Environment
- This PC's tool paths (Rust, Ghidra, MO2...) are in `AGENTS.local.md` when it exists (not in
  git). Build paths for `skse/build.ps1` and `skse/test/ingame.ps1` go in `skse/local.psd1` (not
  in git; see `skse/local.example.psd1`).
- Publishing: nothing that identifies the owner (name, emails, usernames, machine paths, account
  links) goes into commits, files or releases.
