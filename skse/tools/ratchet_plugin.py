# SPDX-License-Identifier: GPL-3.0-or-later
"""RatchetMovement.esl: Ratchet's guns as Skyrim weapons.

A light plugin (no load-order slot) written here, record by record, so no Creation Kit is needed.
It holds no game data: names, numbers, and the paths of the models the converter makes.

  RatchetBurstPistol  WEAP  0x800  the Burst Pistol: equipped, it's the gun he fires
  RatchetShatterbomb  WEAP  0x801  the Shatterbomb, the same

The SKSE plugin puts both in the player's inventory once a game (it keeps that in its co-save).
Equipping one (the inventory, a favourite's hotkey) makes it the gun; with neither equipped he has
only his OmniWrench. Skyrim's own swing of the weapon never happens on foot: the plugin's controls
have him there. Their models are his (meshes\\RatchetMovement\\*.nif).

  uv run python skse/tools/ratchet_plugin.py --out target/skse/plugin
"""
import argparse
import os
import struct

SE_RECORD_VERSION = 44
ESM, ESL = 0x1, 0x200
MASTERS = ["Skyrim.esm"]
OWN = len(MASTERS) << 24  # our records' load-order byte: the next after the masters

# Skyrim.esm forms the weapons refer to.
EITHER_HAND = 0x00013F44   # EquipType EitherHand
VENDOR_WEAPON = 0x0008F958  # Keyword VendorItemWeapon (a vendor buys and sells them)
ONE_HANDED = 6              # actor value: the skill
ANIM_ONE_HAND_DAGGER = 2


def sub(kind, data):
    assert len(data) < 0x10000, kind
    return kind.encode("ascii") + struct.pack("<H", len(data)) + data


def zstring(s):
    return s.encode("cp1252") + b"\0"


def record(kind, form_id, subrecords, flags=0):
    data = b"".join(subrecords)
    return kind.encode("ascii") + struct.pack("<IIIHHHH", len(data), flags, form_id, 0, 0, SE_RECORD_VERSION, 0) + data


def group(label, records):
    data = b"".join(records)
    return b"GRUP" + struct.pack("<I", 24 + len(data)) + label.encode("ascii") + struct.pack("<iHHI", 0, 0, 0, 0) + data


def weapon(form_id, editor_id, name, model, value, weight, damage, bounds):
    dnam = bytearray(100)
    struct.pack_into("<B", dnam, 0x00, ANIM_ONE_HAND_DAGGER)
    struct.pack_into("<ff", dnam, 0x04, 1.0, 1.0)      # speed, reach
    struct.pack_into("<BB", dnam, 0x19, 0xFF, 1)       # attack animation: the default; projectiles
    struct.pack_into("<f", dnam, 0x2C, 1.0)            # attack animation speed (0 would freeze a swing off foot)
    struct.pack_into("<i", dnam, 0x4C, ONE_HANDED)     # skill
    struct.pack_into("<i", dnam, 0x58, -1)             # resist: none
    crdt = struct.pack("<HHfIIII", 0, 0, 1.0, 0, 0, 0, 0)  # SE: 24 bytes, no critical damage
    assert len(crdt) == 24
    return record("WEAP", form_id, [
        sub("EDID", zstring(editor_id)),
        sub("OBND", struct.pack("<6h", *bounds)),
        sub("FULL", zstring(name)),
        sub("MODL", zstring(model)),
        sub("ETYP", struct.pack("<I", EITHER_HAND)),
        sub("KSIZ", struct.pack("<I", 1)),
        sub("KWDA", struct.pack("<I", VENDOR_WEAPON)),
        sub("DATA", struct.pack("<Ifh", value, weight, damage)),
        sub("DNAM", bytes(dnam)),
        sub("CRDT", crdt),
        sub("VNAM", struct.pack("<I", 1)),  # detection: normal
    ])


def plugin():
    pistol = weapon(OWN | 0x800, "RatchetBurstPistol", "Burst Pistol", "RatchetMovement\\burstpistol.nif",
                    value=100, weight=2.0, damage=11, bounds=(-8, -14, -6, 8, 14, 6))
    bomb = weapon(OWN | 0x801, "RatchetShatterbomb", "Shatterbomb", "RatchetMovement\\shatterbomb.nif",
                  value=150, weight=1.0, damage=170, bounds=(-6, -6, -6, 6, 6, 6))
    weapons = [pistol, bomb]
    groups = [group("WEAP", weapons)]
    # HEDR: the records and groups after TES4, and the next free object ID.
    header = [sub("HEDR", struct.pack("<fiI", 1.7, len(weapons) + len(groups), 0x802)), sub("CNAM", zstring("riftapart-skyrim"))]
    for master in MASTERS:
        header += [sub("MAST", zstring(master)), sub("DATA", struct.pack("<Q", 0))]
    tes4 = record("TES4", 0, header, flags=ESM | ESL)
    return tes4 + b"".join(groups)


def read(data):
    """Parse it back: every record and group's size has to add up exactly. Returns (type, form ID,
    editor ID, subrecord types) per record."""
    out = []

    def records(buf, at, end):
        while at < end:
            kind = buf[at:at + 4].decode("ascii")
            size = struct.unpack_from("<I", buf, at + 4)[0]
            if kind == "GRUP":
                assert size >= 24 and at + size <= end, ("group size", at, size)
                records(buf, at + 24, at + size)
                at += size
                continue
            form_id = struct.unpack_from("<I", buf, at + 12)[0]
            version = struct.unpack_from("<H", buf, at + 20)[0]
            assert version == SE_RECORD_VERSION, (kind, version)
            fields, p, stop = [], at + 24, at + 24 + size
            editor_id = ""
            while p < stop:
                t = buf[p:p + 4].decode("ascii")
                n = struct.unpack_from("<H", buf, p + 4)[0]
                if t == "EDID":
                    editor_id = buf[p + 6:p + 6 + n - 1].decode("cp1252")
                fields.append((t, n))
                p += 6 + n
            assert p == stop, ("record size", kind, form_id)
            out.append((kind, form_id, editor_id, fields))
            at = stop
        assert at == end, ("overrun", at, end)

    records(data, 0, len(data))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", required=True, help="folder for RatchetMovement.esl")
    a = ap.parse_args()
    data = plugin()
    parsed = read(data)
    sizes = {"DATA": 10, "DNAM": 100, "CRDT": 24, "VNAM": 4, "ETYP": 4, "OBND": 12, "HEDR": 12}
    for kind, form_id, editor_id, fields in parsed:
        for t, n in fields:
            if t in sizes and not (kind == "TES4" and t == "DATA"):
                assert n == sizes[t], (editor_id, t, n)
    os.makedirs(a.out, exist_ok=True)
    path = os.path.join(a.out, "RatchetMovement.esl")
    with open(path, "wb") as f:
        f.write(data)
    print(f"{path}: {len(data)} bytes")
    for kind, form_id, editor_id, fields in parsed:
        print(f"  {kind} {form_id:08X} {editor_id} [{' '.join(t for t, _ in fields)}]")


if __name__ == "__main__":
    main()
