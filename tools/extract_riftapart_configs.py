# SPDX-License-Identifier: GPL-3.0-or-later
# Extracts Ratchet's movement configs from YOUR OWN Rift Apart install as JSON, for libratchet.
# Uses ALERT's dat1lib (https://github.com/Tkachov/ALERT, GPLv3), so this script is GPLv3 too.
# The rest of this repository is MIT. The JSON it writes is game data: keep it local, never commit it.
#
# Usage:
#   python tools/extract_riftapart_configs.py --game "<Rift Apart folder>" --alert "<ALERT clone>" [--out riftapart-configs]
import argparse, io, json, os, sys

CONFIGS = ["hero_traversal", "hero_shared", "hero_mover", "hero_hoverboot", "hero_grind", "hero_slide", "hero_portalzip"]

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True, help="Rift Apart install folder (has 'toc' and 'd')")
    ap.add_argument("--alert", required=True, help="folder of a clone of github.com/Tkachov/ALERT")
    ap.add_argument("--out", default="riftapart-configs")
    a = ap.parse_args()

    sys.path.insert(0, a.alert)
    import dat1lib, dat1lib.crc64
    import dat1lib.types.sections.config.serialized as ser

    with open(os.path.join(a.game, "toc"), "rb") as f:
        toc = dat1lib.read(f)
    toc.set_archives_dir(a.game)
    os.makedirs(a.out, exist_ok=True)

    for name in CONFIGS:
        path = "configs\\hero\\" + name + ".config"
        entries = toc.get_asset_entries_by_assetid(dat1lib.crc64.hash(path))
        if not entries:
            print(f"[!] {path} not found")
            continue
        cfg = dat1lib.read(io.BytesIO(toc.extract_asset(entries[0])))
        kind = cfg.dat1.get_section(ser.ConfigTypeSection.TAG).root["Type"]
        content = cfg.dat1.get_section(ser.ConfigContentSection.TAG).root
        with open(os.path.join(a.out, name + ".json"), "w") as f:
            json.dump({"_type": kind, "_asset": path, **content}, f, indent=2)
        print(f"{name}.json  ({kind})")

if __name__ == "__main__":
    main()
