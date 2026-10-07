# SPDX-License-Identifier: GPL-3.0-or-later
# Decodes Rift Apart .animclip joint animations (the bit-packed "Sample Data" kind) into
# per-frame local joint rotations and translations. Format worked out for this project from
# the files themselves and the game's clip loader (see MODLOG.md). Used by
# extract_riftapart_ratchet.py; runs on the user's own game files only.
#
# Layout (sections by tag):
#   9DF23F77 Built:        fps f32 @0x0c, duration f32 @0x10, frames u16 @0x22,
#                          elements u32 @0x24, row bytes u32 @0x28, keys u16 @0x30
#   A3B26640 Joint hashes: u32 per clip joint
#   2BB5BC8F Base state:   16 B per clip joint: int16 qx,qy,qz,qw (scaled; normalise),
#                          int16 x,y,z, u8, u8 pos_exp (position = int16 * 2^-pos_exp).
#                          Animated channels store their minimum here.
#                          Then scale rows, two joints per 16 B: u16 Ax,Bx,Ay,By,Az,Bz
#                          (4.12 fixed point) + u16 destA, destB (record offsets, joint*48).
#                          Joints not listed have scale 1. Scale isn't always inherited:
#                          see apply_scale_compensation in extract_riftapart_ratchet.py.
#   D070D358 Sample elem:  u32 dest offset[elements], then u8 bits[elements] (width = bits+1).
#                          dest = joint*48 + channel*16 + component*4 (a 48-byte record per
#                          joint): channel 0 = scale xyz (added to the static scale, 4.12),
#                          1 = rotation xyzw, 2 = translation xyz
#   3A7B4855 Sample data:  [u16 frame->key table, padded to 16 B, when keys < frames]
#                          then keys rows; each row packs every element LSB-first, unsigned,
#                          (bits+1) wide, rows padded to 4 bytes. value = base int16 + packed.
import struct

import numpy as np

BUILT, HASHES, BASE, ELEMS, DATA = 0x9DF23F77, 0xA3B26640, 0x2BB5BC8F, 0xD070D358, 0x3A7B4855


class Clip:
    def __init__(self, name, fps, frames, hashes, rotations, translations, scales):
        self.name = name
        self.fps = fps
        self.frames = frames  # number of frames
        self.hashes = hashes  # clip joint hashes
        self.rotations = rotations  # (frames, joints, 4) xyzw, normalised
        self.translations = translations  # (frames, joints, 3) metres
        self.scales = scales  # (frames, joints, 3)

    @property
    def duration(self):
        return (self.frames - 1) / self.fps if self.frames > 1 else 0.0


def _unpack_rows(data, n_keys, row_bytes, widths):
    """Unpacks LSB-first bit-packed rows into an int array (keys, elements)."""
    rows = np.frombuffer(data, np.uint8)[: n_keys * row_bytes].reshape(n_keys, row_bytes)
    bits = np.unpackbits(rows, axis=1, bitorder="little").astype(np.int64)
    starts = np.concatenate([[0], np.cumsum(widths)[:-1]])
    out = np.zeros((n_keys, len(widths)), np.int64)
    for i, (s, w) in enumerate(zip(starts, widths)):
        weights = 1 << np.arange(w, dtype=np.int64)
        out[:, i] = bits[:, s : s + w] @ weights
    return out


def decode(name, sections):
    """sections: {tag: raw bytes}. Returns a Clip, or None if this clip kind isn't supported."""
    b = sections.get(BUILT)
    if b is None or any(t not in sections for t in (HASHES, BASE, ELEMS, DATA)):
        return None
    flags = struct.unpack_from("<I", b, 0x08)[0]
    if flags & 0x2011 == 0:
        return None  # curve / pose / other clip kinds
    fps = struct.unpack_from("<f", b, 0x0C)[0] or 30.0
    frames = struct.unpack_from("<H", b, 0x22)[0]
    n_el = struct.unpack_from("<I", b, 0x24)[0]
    row_bytes = struct.unpack_from("<I", b, 0x28)[0]
    n_keys = struct.unpack_from("<H", b, 0x30)[0]

    hraw = sections[HASHES]
    n_joints = len(hraw) // 4
    hashes = list(struct.unpack_from(f"<{n_joints}I", hraw))
    base = sections[BASE]
    b16 = np.frombuffer(base[: n_joints * 16], np.int16).reshape(n_joints, 8).astype(np.float64)
    pos_exp = np.frombuffer(base[: n_joints * 16], np.uint8).reshape(n_joints, 16)[:, 15].astype(np.float64)

    # Static per-joint scales (4.12 fixed point), two joints per 16-byte row.
    static_scale = np.full((n_joints, 3), 4096.0)
    extra = base[n_joints * 16 :]
    for r in range(len(extra) // 16):
        v = struct.unpack_from("<8H", extra, r * 16)
        for k, dest in enumerate((v[6], v[7])):
            if dest // 48 < n_joints:
                static_scale[dest // 48] = (v[k], v[2 + k], v[4 + k])

    el = sections[ELEMS]
    offs = np.array(struct.unpack_from(f"<{n_el}I", el, 0), np.int64)
    widths = np.frombuffer(el[4 * n_el : 5 * n_el], np.uint8).astype(np.int64) + 1

    data = sections[DATA]
    rows_start = len(data) - n_keys * row_bytes
    if rows_start < 0:
        return None
    if n_keys < frames and rows_start >= 2 * frames:
        key_of_frame = np.frombuffer(data[: 2 * frames], np.uint16).astype(np.int64)
    else:
        key_of_frame = np.minimum(np.arange(frames), n_keys - 1)
    values = _unpack_rows(data[rows_start:], n_keys, row_bytes, widths)

    joint = offs // 48
    channel = (offs % 48) // 16
    comp = (offs % 16) // 4
    rot = np.repeat(b16[None, :, :4], n_keys, axis=0)
    pos = np.repeat(b16[None, :, 4:7], n_keys, axis=0)
    scl = np.repeat(static_scale[None], n_keys, axis=0)
    for i in range(n_el):
        if joint[i] >= n_joints:
            continue
        if channel[i] == 0 and comp[i] < 3:
            scl[:, joint[i], comp[i]] += values[:, i]
        elif channel[i] == 1:
            rot[:, joint[i], comp[i]] += values[:, i]
        elif channel[i] == 2 and comp[i] < 3:
            pos[:, joint[i], comp[i]] += values[:, i]
    norm = np.linalg.norm(rot, axis=2, keepdims=True)
    rot = np.where(norm > 0, rot / np.maximum(norm, 1e-9), np.array([0, 0, 0, 1.0]))
    # Keep neighbouring keys on the same hemisphere so linear interpolation doesn't flip.
    for k in range(1, n_keys):
        flip = np.sum(rot[k] * rot[k - 1], axis=1) < 0
        rot[k][flip] *= -1
    pos = pos * (2.0 ** -pos_exp)[None, :, None]

    scl = scl / 4096.0
    return Clip(name, fps, frames, hashes, rot[key_of_frame], pos[key_of_frame], scl[key_of_frame])
