# SPDX-License-Identifier: GPL-3.0-or-later
"""Looks up Skyrim code offsets in an Address Library database (versionlib-*.bin, AE format 2).

  python addrlib.py <versionlib-1-6-1170-0.bin> 0xe4ad24 0x150bedd ...

For each offset (from SkyrimSE.exe's base, as a debugger prints SkyrimSE+0x...), prints the ID of
the nearest address at or before it and the distance: the ID names the function in CommonLib
(RELOCATION_ID(se, ae): the second number is the AE ID). Grep CommonLib's sources for the ID.
"""
import bisect
import struct
import sys


def load(path):
    data = open(path, "rb").read()
    pos = 0

    def take(fmt):
        nonlocal pos
        v = struct.unpack_from("<" + fmt, data, pos)
        pos += struct.calcsize("<" + fmt)
        return v[0] if len(v) == 1 else v

    fmt = take("i")
    if fmt != 2:
        raise ValueError(f"format {fmt}: only the AE format (2) is read")
    take("4i")
    name_len = take("i")
    pos += name_len
    ptr_size = take("i")
    count = take("i")
    pairs = []
    pvid = poff = 0
    for _ in range(count):
        t = take("B")
        lo, hi = t & 0xF, t >> 4
        if lo == 0:
            vid = take("Q")
        elif lo == 1:
            vid = pvid + 1
        elif lo == 2:
            vid = pvid + take("B")
        elif lo == 3:
            vid = pvid - take("B")
        elif lo == 4:
            vid = pvid + take("H")
        elif lo == 5:
            vid = pvid - take("H")
        elif lo == 6:
            vid = take("H")
        else:
            vid = take("I")
        base = poff // ptr_size if hi & 8 else poff
        h = hi & 7
        if h == 0:
            off = take("Q")
        elif h == 1:
            off = base + 1
        elif h == 2:
            off = base + take("B")
        elif h == 3:
            off = base - take("B")
        elif h == 4:
            off = base + take("H")
        elif h == 5:
            off = base - take("H")
        elif h == 6:
            off = take("H")
        else:
            off = take("I")
        if hi & 8:
            off *= ptr_size
        pairs.append((off, vid))
        pvid, poff = vid, off
    pairs.sort()
    return pairs


if __name__ == "__main__":
    pairs = load(sys.argv[1])
    offsets = [p[0] for p in pairs]
    for arg in sys.argv[2:]:
        target = int(arg, 16)
        i = bisect.bisect_right(offsets, target) - 1
        off, vid = pairs[i]
        print(f"{arg}: ID {vid} at {off:#x} (+{target - off:#x})")
