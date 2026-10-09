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
- SKSE/Plugins/RatchetMovement/clip_sounds.txt, the moments his clips sound and when, as the
  sandbox reads them (apps/sandbox/src/ratchet_model.rs clip_sounds): from the converter's
  manifests next to the sounds folder (ratchet.json, and wrench.json for the swings), each clip's
  sound events (mapped to moments by sounds.json; silent ones left out, and the landing's, which
  plays at touchdown instead) and its Footstep events, one line each:

      <clip> <seconds> <moment>

These are game data: the output stays on this PC (target/ and the MO2 mod), never in the repo.
"""
import argparse
import json
import os

import numpy as np
import soundfile as sf

SOUND_DIR = "Sound\\FX\\RatchetMovement"
# The swings: the wrench's own clips sound them in the game (the sandbox's MELEE_CLIPS).
MELEE_CLIPS = ("melee_1", "melee_2", "melee_3")


def clip_sounds(manifests, events):
    """Each clip's sounds, "<clip> <seconds> <moment>" lines: an event both manifests call for at
    nearly the same time once, footsteps at the clips' Footstep events (a step at the very end of
    a looping clip is the one at its start)."""
    out = {}
    for path, only_swings in manifests:
        if not os.path.exists(path):
            continue
        doc = json.load(open(path, encoding="utf-8"))
        for name, a in doc.get("animations", {}).items():
            if only_swings and name not in MELEE_CLIPS:
                continue
            raw = out.setdefault(name, [])
            for t, event in a.get("sounds", []):
                if not any(e == event and abs(u - t) < 0.1 for u, e in raw):
                    raw.append((float(t), event))
            end = float(a.get("seconds", 1e9))
            for t in a.get("events", {}).get("Footstep", []):
                def near(u, t=t):
                    return abs(u - t) < 0.1 or (t > end - 0.05 and u < 0.05)
                if not any(e == "footstep" and near(u) for u, e in raw):
                    raw.append((float(t), "footstep"))
    lines = []
    for name, raw in sorted(out.items()):
        for t, event in sorted(raw):
            if event == "Play_FOL_ratchet_land":
                continue  # the sandbox plays the landing at touchdown instead
            moment = event if event == "footstep" else events.get(event)
            if moment:
                lines.append(f"{name} {t:.4f} {moment}")
    return lines


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

    assets = os.path.dirname(os.path.abspath(a.sounds))
    clips = clip_sounds([(os.path.join(assets, "ratchet.json"), False), (os.path.join(assets, "wrench.json"), True)],
                        manifest.get("events", {}))
    clip_txt = os.path.join(a.out, "SKSE", "Plugins", "RatchetMovement", "clip_sounds.txt")
    with open(clip_txt, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(clips) + "\n")
    print(f"{len(clips)} clip sounds; {clip_txt}")


if __name__ == "__main__":
    main()
