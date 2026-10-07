# libratchet: Ratchet & Clank movement for other games

Ratchet & Clank: Rift Apart's movement (jump, double jump, glide; dash, hover boots, grind,
swingshot and rift tether later), rebuilt as a small engine-independent Rust library. Like
libsm64 does for Mario: a host game passes in input and what the character touches, and gets
back how to move.

- **`crates/libratchet`**: the movement rules. No engine dependency.
- **`crates/libratchet-ffi`**: a C API (`include/ratchet.h`) so C/C++ hosts can use it.
- **`crates/libarsenal`**: the weapons, the same way: triggers, ammo, spread, projectiles,
  lobbed grenades and explosions, from the game's weapon configs.
- **`apps/sandbox`**: a Bevy test course to feel and measure the movement.
- **Planned**: a Skyrim SE host (SKSE plugin) so the Dragonborn moves like Ratchet. See
  [docs/skyrim-plan.md](docs/skyrim-plan.md).

Moves so far: run/walk, variable-height jump, double jump, glide, ground dash (Phantom Dash),
air dash, and the OmniWrench: 3-hit combo, Hyper-Strike (melee in the air) and the wrench
throw. Weapons so far: the Burst Pistol and the Shatterbomb, with aim mode (strafing,
over-the-shoulder camera).

Unofficial fan project, not affiliated with Insomniac Games, Sony, or Bethesda. Built with an AI
coding agent (Claude Code).

## Where the numbers come from

Rift Apart stores Ratchet's movement tuning in plain config files (`hero_traversal`,
`hero_shared`, ...). This repo **does not contain them**. You extract them from **your own
copy** of the game, and the library reads them at runtime:

```bash
git clone https://github.com/Tkachov/ALERT
pip install numpy
python tools/extract_riftapart_configs.py --game "<your Rift Apart folder>" --alert "<ALERT folder>"
```

That writes JSON into `riftapart-configs/` (gitignored): the movement configs, the wrench's
melee and throw configs, the Burst Pistol's and Shatterbomb's weapon configs, and
`hero_melee_clips.json` (each swing's length, hit time and lunge, read from Ratchet's wrench
animations). Without it, the sandbox uses generic default values and
says so on screen.

### Ratchet's model, textures and animations (optional)

Also from **your own copy**, converted into `riftapart-assets/ratchet.glb` (gitignored):

```bash
cargo build -p gdeflate-ffi --release
pip install pygltflib pillow numpy texture2ddecoder lz4
python tools/extract_riftapart_ratchet.py --game "<your Rift Apart folder>" --alert "<ALERT folder>"
```

That makes `ratchet.glb`, `clank.glb`, `wrench.glb`, `burstpistol.glb` and `shatterbomb.glb`.
The sandbox picks them up automatically: Ratchet plays Rift Apart's own idle, run, jump,
double jump, fall, glide, land, dash, air-dash, wrench and gun clips, Clank rides on his back
(backpack idle, heli-pack while gliding), and the OmniWrench and the Burst Pistol appear in his
hand. Without them, you get a capsule
with Lombax ears. Never share the `.glb` files: they're game data.

## Run the sandbox

```bash
cargo run -p sandbox
```

| Key | Action |
|---|---|
| WASD | Move (camera-relative) |
| Space | Jump. Hold longer to jump higher, press again in the air to double jump, hold to glide |
| Shift | Dash (on the ground or once in the air) |
| 1 / 2 / 3 | OmniWrench / Burst Pistol / Shatterbomb |
| Left mouse | Wrench out: swing. Gun out: fire (where the crosshair points) |
| Right mouse | Wrench out: throw it. Gun out: aim (strafe, camera over the shoulder) |
| E | Swing the wrench (any time): press again during a swing for the next hit (3-hit combo); in the air it's the Hyper-Strike |
| Q | Throw the wrench along the camera; it comes back to your hand |
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
