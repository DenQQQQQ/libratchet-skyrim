# libratchet: Ratchet & Clank movement for other games

Ratchet & Clank: Rift Apart's movement (jump, double jump, glide; dash, hover boots, grind,
swingshot and rift tether later), rebuilt as a small engine-independent Rust library. Like
libsm64 does for Mario: a host game passes in input and what the character touches, and gets
back how to move.

- **`crates/libratchet`**: the movement rules. No engine dependency.
- **`crates/libratchet-ffi`**: a C API (`include/ratchet.h`) so C/C++ hosts can use it.
- **`apps/sandbox`**: a Bevy test course to feel and measure the movement.
- **Planned**: a Skyrim SE host (SKSE plugin) so the Dragonborn moves like Ratchet. See
  [docs/skyrim-plan.md](docs/skyrim-plan.md).

Moves so far: run/walk, variable-height jump, double jump, glide, ground dash (Phantom Dash),
air dash.

Unofficial fan project, not affiliated with Insomniac Games, Sony, or Bethesda. Built with an AI
coding agent (Claude Code).

## Where the numbers come from

Rift Apart stores Ratchet's movement tuning in plain config files (`hero_traversal`,
`hero_shared`, ...). This repo **does not contain them**. You extract them from **your own
copy** of the game, and the library reads them at runtime:

```bash
git clone https://github.com/Tkachov/ALERT
python tools/extract_riftapart_configs.py --game "<your Rift Apart folder>" --alert "<ALERT folder>"
```

That writes JSON into `riftapart-configs/` (gitignored). Without it, the sandbox uses generic
default values and says so on screen.

### Ratchet's model, textures and animations (optional)

Also from **your own copy**, converted into `riftapart-assets/ratchet.glb` (gitignored):

```bash
cargo build -p gdeflate-ffi --release
pip install pygltflib pillow numpy texture2ddecoder lz4
python tools/extract_riftapart_ratchet.py --game "<your Rift Apart folder>" --alert "<ALERT folder>"
```

The sandbox picks it up automatically and plays Rift Apart's own idle, run, jump, double jump,
fall, glide, land, dash and air-dash clips. Without it, you get a capsule with Lombax ears.
Never share the `.glb`: it's game data.

## Run the sandbox

```bash
cargo run -p sandbox
```

| Key | Action |
|---|---|
| WASD | Move (camera-relative) |
| Space | Jump. Hold longer to jump higher, press again in the air to double jump, hold to glide |
| Shift | Dash (on the ground or once in the air) |
| Ctrl | Walk |
| Mouse | Camera (click to grab the mouse, Esc to free it); scroll to zoom |
| Tab | Toggle Ratchet-sized / Dragonborn-sized (all distances ×1.55, same timing) |
| R | Reload tuning from disk |
| T | Clear the trail |
| Backspace | Respawn |

The trail is coloured by move: orange = ground, yellow = jump, magenta = double jump,
red = fall, cyan = glide, purple = dash, green = air dash. White crosses mark each jump's peak. The HUD shows the last jump's
peak height, air time and distance.

`cargo run -p sandbox -- --demo` plays a scripted run and saves screenshots to `screenshots/`.
Add `--record frames` to also save 30 fps frames for a video.

## Tests

```bash
cargo test --workspace
```

## Status

See [MODLOG.md](MODLOG.md) for what's confirmed vs. assumed.

## Licence

MIT, except `tools/extract_riftapart_configs.py` (GPLv3, since it uses ALERT).
