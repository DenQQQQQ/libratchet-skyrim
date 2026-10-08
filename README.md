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
air dash, wall jumps (on the teal walls of the wall-jump shaft, a wall-jump zone whose camera
swings round to look along it, as in the game), and the OmniWrench: 3-hit
combo, Hyper-Strike (melee in the air) and the wrench throw. Weapons so far: the Burst Pistol and the Shatterbomb, with aim mode (strafing,
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
melee and throw configs, the Burst Pistol's and Shatterbomb's weapon configs, the camera's
placement and shakes, and
`hero_melee_clips.json` (each swing's length, lunge and events, read from Ratchet's wrench
animations: the hit, the two combo windows, the throw's release). Without it, the sandbox uses generic default values and
says so on screen.

### Ratchet's model, textures and animations (optional)

Also from **your own copy**, converted into `riftapart-assets/ratchet.glb` (gitignored):

```bash
cargo build -p gdeflate-ffi --release
pip install pygltflib pillow numpy texture2ddecoder lz4
python tools/extract_riftapart_ratchet.py --game "<your Rift Apart folder>" --alert "<ALERT folder>"
```

That makes `ratchet.glb`, `clank.glb`, `wrench.glb`, `burstpistol.glb`, `shatterbomb.glb`,
`shatterbomb_glove.glb` and `testdummy.glb` (the game's test-dummy robot, which the combat dummies
wear). The sandbox picks them up automatically: Ratchet plays Rift Apart's own idle, run, jump,
double jump, fall, glide (the heli-pack's intro, swaying loop and lean), land, dash (in eight
directions), air-dash, strafe-jump, wrench and gun clips, per weapon grip. Clank rides on his
back (backpack idle, heli-pack while gliding). The OmniWrench and the Burst Pistol appear in his
hand, and the Shatterbomb's glove on it. Without them, you get a capsule with Lombax ears. Never
share the `.glb` files: they're game data.

The weapon effects (muzzle flashes, bolts, impacts, shells, the Shatterbomb's blast) use the
game's own effect textures, meshes and colours, exported the same way into
`riftapart-assets/vfx/`:

```bash
python tools/extract_riftapart_vfx.py --game "<your Rift Apart folder>" --alert "<ALERT folder>"
```

Without them, the guns fire plain shapes.

The sounds (steps, foley, jumps, the heli-pack, dashes, the wrench, the guns, the explosion)
are the game's own too, extracted into `riftapart-assets/sounds/` with a map from gameplay moments
and animation sound events to the files:

```bash
python tools/extract_riftapart_sounds.py --game "<your Rift Apart folder>" --alert "<ALERT folder>"
```

Weapon sounds are Wwise Vorbis and get repacked into plain `.ogg` with no tools. Ratchet's own
sounds are Opus, which Bevy can't play: the extractor converts them with `ffmpeg` if it's on
your PATH (`--opus keep` keeps the `.opus` files instead). Without the folder the sandbox is silent.
Recorded demos log their sounds; `tools/mix_demo_audio.py` lays them under the video.

## Run the sandbox

```bash
cargo run -p sandbox
```

| Key | Action |
|---|---|
| WASD | Move (camera-relative) |
| Space | Jump. Hold longer to jump higher, press again in the air to double jump, hold to glide. On a wall (jump, fall or glide into it, at least 1 m up), press to jump off it; left alone he slides down and lets go. The teal wall-jump walls take him any number of times; plain walls only once per jump and only facing them, and pushing into one jumps up it with the double jump |
| Shift | Dash (on the ground or once in the air) |
| C | Let go of a ledge (the game's crouch / ledge drop button) |
| 1 / 2 | Burst Pistol / Shatterbomb (not while the wrench is thrown). The wrench isn't one to pick: as in the game, melee swings it whatever gun is out |
| Left mouse | Fire (where the crosshair points); Shatterbomb: hold to wind up and see the throw arc, release to throw |
| Right mouse | Aim (aim mode: strafe at the game's aim pace, camera over the shoulder; jumps flip sideways and backward). Melee while aiming throws the wrench, as in the game ("Hold Aim and Melee to Throw Wrench"), also in the air. Firing without aiming strafes too, at run speed, while the gun fires |
| F | Melee, as in the game: press again during a swing, or just after it, for the next hit (3-hit combo, with a short rest after the third); in the air it's the Hyper-Strike; while aiming it throws the wrench |
| Q | Throw the wrench along the camera in one press (the game's optional "Melee Throw" shortcut); it comes back to your hand |
| Ctrl | Walk |
| Mouse | Camera (click to grab the mouse, Esc to free it); scroll to zoom |
| Tab | Toggle Ratchet-sized / Dragonborn-sized (all distances ×1.55, same timing) |
| R | Reload tuning from disk |
| T | Clear the trail |
| F1 | Hide/show the debug overlay (HUD, help, trails), e.g. for recording videos. `--clean` starts with it hidden |
| Backspace | Respawn |

The trail is coloured by move: orange = ground, yellow = jump, magenta = double jump,
red = fall, cyan = glide, purple = dash, green = air dash. White crosses mark each jump's peak. The HUD shows the last jump's
peak height, air time and distance.

`cargo run -p sandbox -- --demo` plays a scripted run and saves screenshots to `screenshots/`.
Add `--record frames` to also save 30 fps frames for a video, `--clean` to hide the overlay, and
`--demo-part wrench|weapons|moves|combat|face|sounds|carry|dashjump|throwswitch|strafejump|walljump|ledges`
to play one section (combat: the combo's timing, aiming and throwing mid-jump, a Shatterbomb on the
walk; face: his face through a jump; carry: the weapon in hand, the whole wrench after the
combo, the Shatterbomb's glove, dashing while aiming; dashjump: jumps out of a dash beside a
running jump; throwswitch: switching weapons around a wrench throw; strafejump: the strafe-jump
flips and a long glide; walljump: up the wall-jump shaft onto its ledge, a slide down a wall, and the plain walls beside it;
ledges: falling short of a platform and catching its lip, then climbing up; the placed yellow ledges on
a wall: grabbing one, going up to the next, jumping up over it, and letting go).

## Tests

```bash
cargo test --workspace
```

## Status

See [MODLOG.md](MODLOG.md) for what's confirmed vs. assumed.

## Licence

MIT, except the extractors that use ALERT (GPLv3): `tools/extract_riftapart_*.py`,
`tools/riftapart_anim.py` and `tools/riftapart_wwise.py` (whose Vorbis repacking follows ww2ogg,
BSD-3-Clause).
