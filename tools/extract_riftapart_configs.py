# SPDX-License-Identifier: GPL-3.0-or-later
# Extracts Ratchet's movement configs from YOUR OWN Rift Apart install as JSON, for libratchet.
# Uses ALERT's dat1lib (https://github.com/Tkachov/ALERT, GPLv3), so this script is GPLv3 too.
# The rest of this repository is MIT. The JSON it writes is game data: keep it local, never commit it.
#
# Usage:
#   python tools/extract_riftapart_configs.py --game "<Rift Apart folder>" --alert "<ALERT clone>" [--out riftapart-configs]
import argparse, io, json, os, struct, sys

# Paths under configs/ (".config" is added).
CONFIGS = ["hero/hero_traversal", "hero/hero_shared", "hero/hero_mover", "hero/hero_hoverboot", "hero/hero_grind",
           "hero/hero_slide", "hero/hero_portalzip", "equipment/wpn_wrench_melee", "equipment/wpn_wrench_weapon",
           "equipment/wpn_burstpistol_weapon", "equipment/wpn_protogrenade_weapon"]

# Ratchet's wrench clips, analysed into hero_melee_clips.json (timing and root motion).
CLIP_DIR = "characters/hero/hero_ratchet/animations/"
COMBO_CLIPS = ["weapon_wrench_hit_1_hero_ratchet", "weapon_wrench_hit_2_hero_ratchet", "weapon_wrench_hit_3_hero_ratchet"]
SMASH_INTRO_CLIP = "weapon_wrench_hit_smash_intro_hero_ratchet"
SMASH_LAND_CLIP = "weapon_wrench_hit_smash_hero_ratchet"
RATCHET_MODEL = "characters/hero/hero_ratchet/hero_ratchet.model"
MOTION = 0xE08AA35F


def root_motion(sections):
    """Root motion keys (Motion Data section) as a list of (x, y, z) positions, one per frame.
    Layout from the game's sampler: byte 0 format (0 = 16-byte keys), byte 2 key count, keys at
    0x7c + int32 @0x3c; key = int16 quat xyzw, int16 xyz scaled by 2^-byte14."""
    raw = sections.get(MOTION)
    if raw is None or raw[0] != 0:
        return []
    start = 0x7C + struct.unpack_from("<i", raw, 0x3C)[0]
    keys = []
    for k in range(raw[2]):
        off = start + 16 * k
        if off + 16 > len(raw):
            break
        x, y, z = struct.unpack_from("<3h", raw, off + 8)
        s = 2.0 ** -raw[off + 14]
        keys.append((x * s, y * s, z * s))
    return keys


def analyse_clips(read, dat1lib):
    """Durations, forward lunge (root motion) and hit time of the wrench swings. The hit time
    isn't stored anywhere we know of: we take the moment the wrench (prop_attach joint) moves
    fastest around the swing's biggest lunge."""
    import numpy as np
    import riftapart_anim

    model = dat1lib.read(io.BytesIO(read(RATCHET_MODEL)))
    joints = model.dat1.get_section(0x15DF9D3B).joints
    prop_hash = next(j.hash for j in joints if model.dat1.get_string(j.string_offset) == "prop_attach")

    def clip_info(name, want_hit):
        data = read(CLIP_DIR + name + ".animclip")
        if data is None:
            return None
        sections = {s.TAG: s._raw for s in dat1lib.read(io.BytesIO(data)).dat1.sections}
        clip = riftapart_anim.decode(name, sections)
        if clip is None:
            return None
        keys = root_motion(sections)
        lunge = [round(z - keys[0][2], 4) for _, _, z in keys] if keys else []
        drop = round(keys[-1][1] - keys[0][1], 4) if keys else 0.0
        info = {"Clip": name, "Duration": round(clip.duration, 4), "Rate": round(clip.fps, 3), "Lunge": lunge, "Drop": drop}
        if want_hit:
            p = clip.translations[:, list(clip.hashes).index(prop_hash)]
            speed = np.linalg.norm(np.diff(p, axis=0), axis=1)
            lunge_speed = np.diff(np.array(lunge)) if len(lunge) > 1 else np.zeros(1)
            lunge_peak = int(np.argmax(lunge_speed)) if len(lunge_speed) else 0
            peaks = [i for i in range(len(speed))
                     if speed[i] >= 0.25 * speed.max() and (i == 0 or speed[i] >= speed[i - 1])
                     and (i == len(speed) - 1 or speed[i] >= speed[i + 1])]
            k = min(peaks, key=lambda i: abs(i - lunge_peak)) if peaks else int(np.argmax(speed))
            info["HitTime"] = round((k + 0.5) / clip.fps, 4)
        return info

    out = {"_note": "Derived from Ratchet's wrench clips by extract_riftapart_configs.py; HitTime is an estimate.",
           "Combo": [clip_info(c, True) for c in COMBO_CLIPS],
           "SmashIntro": clip_info(SMASH_INTRO_CLIP, False),
           "SmashLand": clip_info(SMASH_LAND_CLIP, False)}
    out["Combo"] = [c for c in out["Combo"] if c]
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True, help="Rift Apart install folder (has 'toc' and 'd')")
    ap.add_argument("--alert", required=True, help="folder of a clone of github.com/Tkachov/ALERT")
    ap.add_argument("--out", default="riftapart-configs")
    a = ap.parse_args()

    sys.path.insert(0, a.alert)
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import dat1lib, dat1lib.crc64
    import dat1lib.types.sections.config.serialized as ser

    with open(os.path.join(a.game, "toc"), "rb") as f:
        toc = dat1lib.read(f)
    toc.set_archives_dir(a.game)
    os.makedirs(a.out, exist_ok=True)

    def read(path):
        entries = toc.get_asset_entries_by_assetid(dat1lib.crc64.hash(path.replace("/", "\\")))
        return toc.extract_asset(entries[0]) if entries else None

    for name in CONFIGS:
        path = "configs/" + name + ".config"
        data = read(path)
        if data is None:
            print(f"[!] {path} not found")
            continue
        cfg = dat1lib.read(io.BytesIO(data))
        kind = cfg.dat1.get_section(ser.ConfigTypeSection.TAG).root["Type"]
        content = cfg.dat1.get_section(ser.ConfigContentSection.TAG).root
        base = name.split("/")[-1]
        with open(os.path.join(a.out, base + ".json"), "w") as f:
            json.dump({"_type": kind, "_asset": path.replace("/", "\\"), **content}, f, indent=2)
        print(f"{base}.json  ({kind})")

    clips = analyse_clips(read, dat1lib)
    with open(os.path.join(a.out, "hero_melee_clips.json"), "w") as f:
        json.dump(clips, f, indent=2)
    print("hero_melee_clips.json  (" + ", ".join(f"{c['Clip']}: hit {c['HitTime']} s, lunge {c['Lunge'][-1] if c['Lunge'] else 0} m"
                                             for c in clips["Combo"]) + ")")

if __name__ == "__main__":
    main()
