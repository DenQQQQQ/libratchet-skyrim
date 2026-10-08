# SPDX-License-Identifier: GPL-3.0-or-later
"""Rift Apart's sounds for Skyrim.

Reads the owner's extraction (riftapart-assets/sounds: sounds.json and its .ogg files, made by
tools/extract_riftapart_sounds.py) and writes, into a folder laid out like Skyrim's Data folder:

- every file sounds.json names, as mono 16-bit PCM WAV, under Sound/FX/RatchetMovement/<folder>/
  (mono: Skyrim places its sounds in the world, and it places mono ones);
- SKSE/Plugins/RatchetMovement/sounds.txt, the moments in lines simple enough to read in C++:

      moment <name> <level_db> <chance>
      layer <volume> <delay> <loop 0|1> <pitch low, cents> <pitch high, cents> <file>|<seconds> [...]

  Files are paths under Data (Sound\\FX\\RatchetMovement\\run_foley\\1060039170.wav), each with
  its length (loops start their next round when it's over).

These are game data: the output stays on this PC (target/ and the MO2 mod), never in the repo.
"""
import argparse
import json
import os

import numpy as np
import soundfile as sf

SOUND_DIR = "Sound\\FX\\RatchetMovement"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--sounds", required=True, help="the extraction's sounds folder (with sounds.json)")
    ap.add_argument("--out", required=True, help="folder laid out like Skyrim's Data folder")
    a = ap.parse_args()

    manifest = json.load(open(os.path.join(a.sounds, "sounds.json"), encoding="utf-8"))
    lines = []
    converted = {}
    total = 0
    for name, moment in manifest["moments"].items():
        lines.append(f"moment {name} {float(moment.get('level_db', 0.0))} {float(moment.get('chance', 1.0))}")
        for layer in moment.get("layers", []):
            files = []
            for f in layer.get("files", []):
                if f not in converted:
                    data, rate = sf.read(os.path.join(a.sounds, *f.split("/")), dtype="float32", always_2d=True)
                    mono = np.clip(data.mean(axis=1), -1.0, 1.0)
                    rel = SOUND_DIR + "\\" + os.path.splitext(f.replace("/", "\\"))[0] + ".wav"
                    out = os.path.join(a.out, *rel.split("\\"))
                    os.makedirs(os.path.dirname(out), exist_ok=True)
                    sf.write(out, mono, rate, subtype="PCM_16")
                    converted[f] = f"{rel}|{len(mono) / rate:.3f}"
                    total += os.path.getsize(out)
                files.append(converted[f])
            if not files:
                continue
            pitch = layer.get("pitch", [0.0, 0.0])
            lines.append(f"layer {float(layer.get('volume', 1.0))} {float(layer.get('delay', 0.0))} {int(bool(layer.get('loop')))} "
                         f"{float(pitch[0])} {float(pitch[1])} " + " ".join(files))
    txt = os.path.join(a.out, "SKSE", "Plugins", "RatchetMovement", "sounds.txt")
    os.makedirs(os.path.dirname(txt), exist_ok=True)
    with open(txt, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(lines) + "\n")
    print(f"{len(manifest['moments'])} moments, {len(converted)} files, {total / 1e6:.1f} MB of WAV; {txt}")


if __name__ == "__main__":
    main()
