# Installing Ratchet Movement

Ratchet & Clank: Rift Apart's movement, weapons and bolts in Skyrim Special Edition: the double
jump, glide, dashes, wall jumps and ledges, the OmniWrench, the Burst Pistol and the Shatterbomb,
bolts bursting out of what you kill, Rift Apart's camera, sounds and effects, and Ratchet (with
Clank on his back) as the player.

**You need to own both games.** This mod ships no game files. Its installer reads Ratchet's model,
textures, animations and sounds, the effects' art, the menu's logo and music, and the movement
tuning from **your own** Rift Apart install, converts them for Skyrim on your PC, and writes a mod
folder for your mod manager. Nothing is uploaded anywhere.

## What you need

| | |
|---|---|
| Windows 10 or 11, 64-bit | |
| **Skyrim Special Edition / Anniversary Edition 1.6.1170** (Steam) | Tested. Other 1.6 versions with Address Library will probably work but are untested. 1.5.97 and Skyrim VR aren't supported. The GOG version is untested. |
| **[SKSE64](https://skse.silverlock.org/)** for your Skyrim version | |
| **[Address Library for SKSE Plugins](https://www.nexusmods.com/skyrimspecialedition/mods/32444)** | The Anniversary Edition ("All in one") file. |
| A mod manager | [Mod Organizer 2](https://github.com/ModOrganizer2/modorganizer) (tested) or Vortex. |
| **Ratchet & Clank: Rift Apart, installed on the same PC** | Steam version tested (build 23987138). The Epic version is untested. The installer only reads the files; the game doesn't have to run. |
| About 3 GB free disk space while installing | The finished mod is about 0.6 GB. |
| An internet connection the first time | The installer downloads its own tools (see [What the installer does](#what-the-installer-does)). |
| [Microsoft Visual C++ Redistributable](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist) (x64) | Most PCs already have it. |

Optional: [RAD Video Tools](https://www.radgametools.com/bnkdown.htm), to get Rift Apart's title
card as Skyrim's startup video ([below](#optional-the-startup-video)).

## Install

1. **Download** `RatchetMovement-<version>.zip` from the
   [Releases](../../releases) page and unzip it to a folder you can write to, for example
   `Downloads\RatchetMovement`. Don't use Program Files or Skyrim's own folder.
2. **Run `Install.bat`** (double-click it). If Windows asks whether to run it, choose Run.
   - It finds Rift Apart through Steam or the Epic launcher. If it can't, it says so. Then run it
     from PowerShell in that folder with the game's folder (the one with `RiftApart.exe`):
     ```powershell
     powershell -ExecutionPolicy Bypass -File install.ps1 -RiftApart "D:\SteamLibrary\steamapps\common\Ratchet & Clank - Rift Apart"
     ```
   - It takes about 10 minutes on a 16-thread PC. The first run also downloads about 200 MB of
     tools. When it's done, the mod is in `output\Ratchet Movement`.
3. **Add the mod to your mod manager.**
   - **Mod Organizer 2.** Move the `output\Ratchet Movement` folder into MO2's `mods` folder. Or
     install straight into it next time:
     `install.ps1 -ModDir "<MO2 folder>\mods\Ratchet Movement"` (its `mods` folder; with an
     instance in AppData it's `%LOCALAPPDATA%\ModOrganizer\<instance>\mods`). Then:
     1. In MO2, press **F5**.
     2. Tick **Ratchet Movement** in the left pane.
     3. On the right pane's Plugins tab, tick **RatchetMovement.esl**.
   - **Vortex.** Run `Install.bat -Zip`. It writes `output\Ratchet Movement.zip`. Then:
     1. Drop the zip onto Vortex's Mods page, and install and enable it.
     2. Deploy.
     3. Check that **RatchetMovement.esl** is enabled on the Plugins page.
4. **Start Skyrim through SKSE**: MO2's SKSE entry, or `skse64_loader.exe` with Vortex. Load a
   save. You're Ratchet.

To check that the mod loaded, look at its log:
`Documents\My Games\Skyrim Special Edition\SKSE\RatchetMovement.log`.

## Controls

| | Keyboard / mouse | Gamepad |
|---|---|---|
| Jump; press again for the double jump; hold to glide | Space | Y |
| Dash, air dash | Left Alt | LB |
| OmniWrench (press again for the combo; in the air, the Hyper-Strike) | R | X |
| Fire the Burst Pistol (hold), or wind up a Shatterbomb (hold, release to throw) | Left mouse | RT |
| Aim | Right mouse | LT |
| Throw the wrench | Z | RB |
| Let go of a ledge | Ctrl | Left stick |

The keys follow Skyrim's own controls (Jump, Sprint, Ready Weapon, the attack buttons, Shout,
Sneak), so remapping those in Skyrim's settings remaps these too.

Your first load gives you the Burst Pistol and the Shatterbomb as Skyrim weapons. Equip one to use
it. With neither equipped, Ratchet fights with his OmniWrench. On foot, Skyrim's own jump, sprint,
attacks, blocks and shouts are off. On a horse, everything is Skyrim's.

## Settings

`SKSE\Plugins\RatchetMovement.ini` in the mod folder (in MO2: right-click the mod, "Open in
Explorer"). Some of its keys:

- `bRatchet = 0` keeps your own character's look. `bClank = 0` leaves Clank off.
- `bCombat = 0` gives you Skyrim's fighting back.
- `bRiftCamera = 0` keeps Skyrim's camera.
- `bSounds = 0` turns his sounds off; `fSoundVolume` sets their volume.
- `bBolts = 0` turns the bolts off.
- `sDashControl`, `sFireControl` and the other `s...Control` keys pick the Skyrim control for each
  move.

Running the installer again keeps your INI.

## Optional: the startup video

Rift Apart's title card can replace Bethesda's logo at launch. Making the video needs RAD Video
Tools' Bink encoder:

1. Get RAD Video Tools from [radgametools.com](https://www.radgametools.com/bnkdown.htm). Either
   install it, or unpack its installer with 7-Zip.
2. Run the installer with the path to its `radvideo64.exe`:
   ```powershell
   powershell -ExecutionPolicy Bypass -File install.ps1 -RadVideo "C:\path\to\RADTools\bin\radvideo64.exe"
   ```

The installer also looks in RAD's default install folder and in a `RADTools` folder next to itself.
Without RAD Video Tools, everything else still works and Skyrim keeps its own startup video.

## Updating and uninstalling

- **Update:** unzip the new release and run its `Install.bat` again, with the same `-ModDir` if
  you used one. Your INI and MO2's notes on the mod are kept.
- **Uninstall:** disable or remove the mod in your mod manager. Then delete the installer's folder.
  Its `cache` folder holds everything it downloaded (uv, Python, ALERT, ffmpeg), and nothing else
  was installed on your PC.
- As with any mod that has a plugin, removing it in the middle of a playthrough is at your own risk.
  The two guns vanish from your inventory, and the bolts you've collected go with the mod.

## Troubleshooting

- **"Rift Apart not found"**: pass `-RiftApart "<the game's folder>"`, the folder with `RiftApart.exe`.
- **A download fails**: the installer needs to reach github.com and pypi.org. Run it again; it
  continues from what's already in `cache`.
- **It fails partway**: run it again with `-KeepWork`; the extracted files stay in `work\`. When you
  report a problem, include the installer's output, and remove your Windows user name from the
  paths in it first.
- **Skyrim crashes at start, or the log says the plugin didn't load**: check your Skyrim version
  (1.6.1170), that SKSE matches it, and that Address Library is installed and enabled.
- **You're still the Dragonborn**: check that the mod is enabled. Then open
  `RatchetMovement.log` and look for "ratchet model".
- **Another mod's main menu or startup video shows**: this mod replaces
  `meshes\interface\logo\logo.nif` and `Video\BGS_Logo.bik`. In MO2, whichever mod sits lower in
  the left pane wins.

## What the installer does

`install.ps1`:

1. Finds Rift Apart (Steam's library folders, then the Epic launcher's manifests).
2. Downloads its tools into `cache\` next to itself, on the first run only:
   - [uv](https://github.com/astral-sh/uv) (pinned version and checksum), which brings Python 3.12
     and the Python packages (numpy, Pillow, pygltflib, texture2ddecoder, soundfile; pinned);
   - [ALERT](https://github.com/Tkachov/ALERT) (pinned commit), the Rift Apart asset reader;
   - ffmpeg (from the imageio-ffmpeg package), unless one is already on your PATH.
3. Extracts from your Rift Apart into `work\`, with the scripts in `tools\`: the movement configs;
   the models, textures and animations of Ratchet, Clank, the weapons, the test dummy and the
   bolts; the effect art; the sounds; and the menu's logo and music.
4. Converts them into Skyrim's formats (NIF meshes, DDS textures, WAV sounds) with the scripts in
   `skse\tools\`, into the mod folder, next to the mod's own files from `mod\`: the SKSE plugin,
   `ratchet.dll` and `RatchetMovement.esl`.
5. Deletes `work\`, unless you pass `-KeepWork`.

Options: `-RiftApart <folder>`, `-ModDir <folder>`, `-Zip`, `-RadVideo <radvideo64.exe>`,
`-Language <us|de|fr|...>` (the voice archive to read his sounds from), `-KeepWork`.

## Building from source

See [skse/README.md](skse/README.md#build). Building needs Visual Studio 2022 (C++), CMake, Rust,
vcpkg and uv. `skse\package.ps1` makes the same release zip.

## Legal

Unofficial fan project, not affiliated with or endorsed by Insomniac Games, Sony Interactive
Entertainment, Bethesda Softworks or ZeniMax. Ratchet & Clank is a trademark of Sony Interactive
Entertainment LLC. The Elder Scrolls and Skyrim are trademarks of ZeniMax Media Inc.

This repository and its releases contain no files from either game. The installer converts files
from your own copy for your own use. **Don't share the mod folder or zip it makes**: they hold the
game's files.
