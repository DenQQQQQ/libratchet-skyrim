# SPDX-License-Identifier: GPL-3.0-or-later
# Reads Rift Apart's Wwise sound data for extract_riftapart_sounds.py: soundbank assets, the
# event/container hierarchy, embedded and streamed .wem files, and repacks Wwise Vorbis .wem files
# into standard Ogg Vorbis. Formats worked out for this project from the files themselves; the Vorbis
# repacking follows ww2ogg, reimplemented here: Copyright (c) 2002, Xiph.org Foundation; Copyright
# (c) 2009-2016, Adam Gashlin; used under its BSD-3-Clause license
# (https://github.com/hcs64/ww2ogg/blob/master/COPYING). Reads assets through
# ALERT's dat1lib (GPLv3). Runs on the user's own game files only.
#
# Layout:
#   .soundbank asset (sound\banks\<name>.soundbank): DAT1 section 53F25238 is a plain Wwise bank,
#     version 135 (Wwise 2019.2): BKHD, DIDX/DATA (embedded .wem files), HIRC (events, actions,
#     sounds, containers). Wwise ids are the 32-bit FNV-1 hash of the lower-cased name.
#   Streamed .wem files are header-less assets with id 0xE000000000000000 | wem id.
#   .wem codecs: fmt 0xFFFF = Wwise Vorbis (stripped headers, codebooks referenced by id from the
#     library built into the game's sound engine), fmt 0x3040 = a complete Ogg Opus stream in the
#     data chunk.
import collections
import contextlib
import io
import os
import struct
import sys

BANK_SECTION = 0x53F25238
STREAMED_WEM = 0xE000000000000000

# HIRC object types and action types used here.
SOUND, ACTION, EVENT, RANSEQ, SWITCH, ACTOR_MIXER, LAYER = 2, 3, 4, 5, 6, 7, 9
PLAY = 0x0403
# Property ids (AkPropID). Delay is in ms (actions), initial delay in seconds (nodes).
P_VOLUME, P_PITCH, P_MAKEUP_GAIN, P_DELAY, P_PROBABILITY, P_LOOP, P_INITIAL_DELAY = 0, 2, 6, 15, 17, 58, 59
# Sound plugin id >> 16.
CODEC_VORBIS, CODEC_OPUS, CODEC_SILENCE, CODEC_HAPTICS = 4, 19, 101, 131


def fnv1(name):
    """Wwise short id: 32-bit FNV-1 of the lower-cased name (events, switches, banks)."""
    h = 2166136261
    for c in name.lower().encode():
        h = ((h * 16777619) & 0xFFFFFFFF) ^ c
    return h


def as_float(raw):
    return struct.unpack("<f", struct.pack("<I", raw))[0]


# ---------------------------------------------------------------------------------------------
# Game files (ALERT's toc reader). The sound archives are stored uncompressed.

class GameFiles:
    def __init__(self, game_dir, alert_dir):
        if alert_dir not in sys.path:
            sys.path.insert(0, alert_dir)
        import dat1lib
        import dat1lib.crc64

        self.dat1lib = dat1lib
        self.crc64 = dat1lib.crc64
        with open(os.path.join(game_dir, "toc"), "rb") as f, contextlib.redirect_stdout(io.StringIO()):
            self.toc = dat1lib.read(f)
        self.toc.set_archives_dir(game_dir)
        self.archives = [a.filename.split(b"\0", 1)[0].decode() for a in self.toc.get_archives_section().archives]
        self.index = collections.defaultdict(list)
        for i, aid in enumerate(self.toc.get_assets_section().ids):
            self.index[aid].append(i)

    def entries(self, asset_id):
        return [self.toc.get_asset_entry_by_index(i) for i in self.index.get(asset_id, [])]

    def pick(self, asset_id, lang):
        """The language-neutral copy if there is one, else the one for `lang` (archive suffix, e.g. 'us')."""
        ents = self.entries(asset_id)
        for e in ents:
            if "." not in self.archives[e.archive].rsplit("\\", 1)[-1]:
                return e
        for e in ents:
            if self.archives[e.archive].endswith("." + lang):
                return e
        return ents[0] if ents else None

    def read(self, entry):
        return bytes(self.toc.extract_asset(entry))

    def soundbank(self, path, lang="us"):
        e = self.pick(self.crc64.hash(path.replace("/", "\\")), lang)
        if e is None:
            raise KeyError(path)
        with contextlib.redirect_stdout(io.StringIO()):
            asset = self.dat1lib.read(io.BytesIO(self.read(e)))
        d = asset.dat1
        return Bank(bytes(d._sections_data[d._sections_map[BANK_SECTION]]), path)

    def streamed_wem(self, wem_id, lang="us"):
        e = self.pick(STREAMED_WEM | wem_id, lang)
        return self.read(e) if e is not None else None


# ---------------------------------------------------------------------------------------------
# Wwise bank (HIRC parsed as far as event -> actions -> containers -> sounds -> media).

def _var(d, p):
    v = 0
    while True:
        c = d[p]
        p += 1
        v = (v << 7) | (c & 0x7F)
        if not c & 0x80:
            return v, p


def _props(d, p):
    """AkPropBundle (u8 count, ids, u32 values) then the ranged-modifier bundle (u8 count, ids, min/max)."""
    n = d[p]
    ids = d[p + 1:p + 1 + n]
    vals = struct.unpack_from("<%dI" % n, d, p + 1 + n)
    p += 1 + 5 * n
    m = d[p]
    rids = d[p + 1:p + 1 + m]
    rng = struct.unpack_from("<%dI" % (2 * m), d, p + 1 + m)
    p += 1 + 9 * m
    return dict(zip(ids, vals)), {rids[i]: (rng[2 * i], rng[2 * i + 1]) for i in range(m)}, p


class Bank:
    def __init__(self, b, name=""):
        self.name = name
        self.b = b
        chunks = {}
        o = 0
        while o + 8 <= len(b):
            size = struct.unpack_from("<I", b, o + 4)[0]
            chunks[b[o:o + 4]] = (o + 8, size)
            o += 8 + size
        self.version = struct.unpack_from("<I", b, chunks[b"BKHD"][0])[0]
        self.media = {}
        if b"DIDX" in chunks:
            o, size = chunks[b"DIDX"]
            data = chunks[b"DATA"][0]
            for k in range(size // 12):
                mid, off, msize = struct.unpack_from("<III", b, o + 12 * k)
                self.media[mid] = (data + off, msize)
        self.objs = {}
        if b"HIRC" in chunks:
            o = chunks[b"HIRC"][0]
            p = o + 4
            for _ in range(struct.unpack_from("<I", b, o)[0]):
                t = b[p]
                size = struct.unpack_from("<I", b, p + 1)[0]
                d = b[p + 5:p + 5 + size]
                ob = self._parse(t, d)
                self.objs[ob["id"]] = ob
                p += 5 + size
        self.children = collections.defaultdict(list)
        for ob in self.objs.values():
            if ob.get("parent"):
                self.children[ob["parent"]].append(ob["id"])
        for ob in self.objs.values():
            if ob["type"] in (RANSEQ, SWITCH):
                self._container_tail(ob)

    def wem(self, mid):
        if mid not in self.media:
            return None
        o, size = self.media[mid]
        return self.b[o:o + size]

    @staticmethod
    def _node_base(ob, d, p):
        # NodeBaseParams: u8 override-parent-fx, u8 fx count (+ u8 bypass and 7 bytes per fx),
        # u8 override-attachment, u32 output bus, u32 parent id, u8 flags, then the property bundles.
        nfx = d[p + 1]
        p += 2 + (1 + 7 * nfx if nfx else 0) + 1
        ob["parent"] = struct.unpack_from("<I", d, p + 4)[0]
        ob["props"], ob["ranged"], ob["after_props"] = _props(d, p + 9)

    def _parse(self, t, d):
        ob = {"type": t, "id": struct.unpack_from("<I", d, 0)[0], "raw": d}
        try:
            if t == SOUND:
                plugin, stream, source, msize = struct.unpack_from("<IBII", d, 4)
                p = 18
                if plugin & 0xF == 2:  # source plugin: its parameters follow
                    p += 4 + struct.unpack_from("<I", d, p)[0]
                ob.update(codec=plugin >> 16, stream=stream, source=source, msize=msize)
                self._node_base(ob, d, p)
            elif t in (RANSEQ, SWITCH, ACTOR_MIXER, LAYER):
                self._node_base(ob, d, 4)
            elif t == ACTION:
                atype, target = struct.unpack_from("<HI", d, 4)
                ob.update(atype=atype, target=target)
                ob["props"], ob["ranged"], _ = _props(d, 11)
            elif t == EVENT:
                n, p = _var(d, 4)
                ob["actions"] = list(struct.unpack_from("<%dI" % n, d, p))
        except (struct.error, IndexError) as e:
            ob["error"] = str(e)
        return ob

    def _container_tail(self, ob):
        # The type-specific fields sit around the children list (u32 count + sorted ids), which
        # is found by searching for the ids of the objects that name this one as their parent.
        d = ob["raw"]
        kids = sorted(self.children.get(ob["id"], []))
        pos = d.find(struct.pack("<I%dI" % len(kids), len(kids), *kids), ob.get("after_props", 4))
        if pos < 0:
            return
        q = pos + 4 + 4 * len(kids)
        try:
            if ob["type"] == RANSEQ:
                (loops, _, _, ttime, _, _, avoid, tmode, rmode, mode, bits) = struct.unpack_from("<HHHfffHBBBB", d, pos - 24)
                ob.update(loop_count=loops, transition_time=ttime, transition_mode=tmode, sequence=mode == 1,
                          continuous=bool(bits & 0x08))
            else:
                _, group, default, _ = struct.unpack_from("<BIIB", d, pos - 10)
                switches = {}
                for _ in range(struct.unpack_from("<I", d, q)[0]):
                    value, n = struct.unpack_from("<II", d, q + 4)
                    switches[value] = list(struct.unpack_from("<%dI" % n, d, q + 12))
                    q += 8 + 4 * n
                ob.update(group=group, default=default, switches=switches)
        except struct.error:
            pass


class Hierarchy:
    """Resolves objects across banks (an event's bank plus e.g. a per-character media bank) and
    flattens what a Play action starts into layers: each layer plays one variation at a time."""

    def __init__(self, banks, switches):
        self.banks = banks
        self.switches = switches  # group id -> value id
        self.assumed = []  # switch groups without a default where the first non-empty branch was used

    def find(self, oid):
        hit = None
        for bk in self.banks:
            ob = bk.objs.get(oid)
            if ob is None:
                continue
            if ob["type"] not in (RANSEQ, SWITCH, ACTOR_MIXER, LAYER) or bk.children.get(oid):
                return bk, ob
            hit = hit or (bk, ob)
        return hit or (None, None)

    def chain(self, oid):
        """The object and its parents (the actor-mixer hierarchy) as (bank, object) pairs."""
        out = []
        seen = set()
        while oid and oid not in seen:
            seen.add(oid)
            bk, ob = self.find(oid)
            if ob is None:
                break
            out.append((bk, ob))
            oid = ob.get("parent")
        return out

    def flatten(self, oid):
        bk, ob = self.find(oid)
        if ob is None:
            return []
        t = ob["type"]
        if t == SOUND:
            if ob["codec"] == CODEC_HAPTICS:
                return []
            return [{"sounds": [(bk, ob)], "loop": ob["props"].get(P_LOOP) == 0}]
        kids = sorted(bk.children.get(oid, []))
        if t == SWITCH:
            value = self.switches.get(ob.get("group"), ob.get("default"))
            switches = ob.get("switches") or {}
            chosen = switches.get(value, [])
            if not chosen and not value:
                chosen = next((c for c in switches.values() if c), [])
                if chosen:
                    self.assumed.append("%08X" % ob.get("group", 0))
            kids = chosen
        layers = [self.flatten(k) for k in kids]
        layers = [ls for ls in layers if ls]
        if t != RANSEQ:
            return [layer for ls in layers for layer in ls]
        if not layers:
            return []
        if all(len(ls) == len(layers[0]) for ls in layers):
            # Random/sequence container: pool its children's variations. Children that are
            # themselves layered (e.g. two alternating multi-layer shots) are pooled layer by layer.
            pooled = []
            for i in range(len(layers[0])):
                pooled.append({"sounds": [s for ls in layers for s in ls[i]["sounds"]],
                               "loop": any(ls[i]["loop"] for ls in layers)})
        else:
            pooled = max(layers, key=len)
        if ob.get("continuous"):
            for layer in pooled:
                layer["loop"] = layer["loop"] or ob.get("loop_count") == 0
                if ob.get("transition_mode") in (3, 5):  # delay / trigger rate (ms) between variations
                    layer["interval"] = round(ob["transition_time"] / 1000.0, 4)
        if ob.get("sequence") and all(len(ls) == 1 and len(ls[0]["sounds"]) == 1 for ls in layers):
            pooled[0]["sequence"] = True  # plays its files in order
        return pooled

    def gain_db(self, oid):
        """Volume + make-up gain summed over the object and its parents (buses not included)."""
        return sum(as_float(ob["props"].get(P_VOLUME, 0)) + as_float(ob["props"].get(P_MAKEUP_GAIN, 0))
                   for _, ob in self.chain(oid) if "props" in ob)

    def initial_delay(self, oid):
        """Initial delay (s) summed over the object and its parents."""
        return sum(as_float(ob.get("props", {}).get(P_INITIAL_DELAY, 0)) for _, ob in self.chain(oid))

    def pitch_range(self, oid):
        """Pitch randomisation (cents) summed over the object and its parents."""
        lo = hi = 0.0
        for _, ob in self.chain(oid):
            base = as_float(ob.get("props", {}).get(P_PITCH, 0))
            rlo, rhi = ob.get("ranged", {}).get(P_PITCH, (0, 0))
            lo += base + as_float(rlo)
            hi += base + as_float(rhi)
        return lo, hi

    def event(self, name):
        """Flattened layers of an event's Play actions, with each action's delay and probability."""
        bk, ev = self.find(fnv1(name))
        if ev is None or ev["type"] != EVENT:
            raise KeyError(name)
        out = []
        for aid in ev["actions"]:
            _, act = self.find(aid)
            if act is None or act.get("atype") != PLAY:
                continue
            delay = act["props"].get(P_DELAY, 0) / 1000.0
            probability = as_float(act["props"][P_PROBABILITY]) / 100.0 if P_PROBABILITY in act["props"] else 1.0
            for layer in self.flatten(act["target"]):
                layer["delay"] = delay
                layer["probability"] = probability
                out.append(layer)
        return out


# ---------------------------------------------------------------------------------------------
# .wem files

def riff_chunks(w):
    if w[:4] != b"RIFF" or w[8:12] != b"WAVE":
        raise ValueError("not a RIFF/WAVE .wem")
    out = {}
    o = 12
    while o + 8 <= len(w):
        size = struct.unpack_from("<I", w, o + 4)[0]
        out.setdefault(w[o:o + 4], (o + 8, size))
        o += 8 + size + (size & 1)
    return out


def wem_format(w):
    fmt = riff_chunks(w)[b"fmt "][0]
    return struct.unpack_from("<H", w, fmt)[0]


def wem_opus(w):
    """fmt 0x3040: the data chunk is already a complete Ogg Opus file."""
    o, size = riff_chunks(w)[b"data"]
    if w[o:o + 4] != b"OggS":
        raise ValueError("Opus .wem without an Ogg stream")
    return w[o:o + size]


# ---------------------------------------------------------------------------------------------
# Wwise Vorbis -> Ogg Vorbis (lossless: headers are rebuilt, audio packets get their packet-type
# and window bits back). Bits are packed LSB first, as in Vorbis.

class BitReader:
    __slots__ = ("d", "pos")

    def __init__(self, d, byte=0):
        self.d = d
        self.pos = byte * 8

    def read(self, n):
        v = shift = 0
        while n:
            off = self.pos & 7
            take = min(8 - off, n)
            v |= ((self.d[self.pos >> 3] >> off) & ((1 << take) - 1)) << shift
            shift += take
            n -= take
            self.pos += take
        return v


class BitWriter:
    def __init__(self):
        self.out = bytearray()
        self.acc = self.n = 0

    def write(self, v, n):
        self.acc |= (v & ((1 << n) - 1)) << self.n
        self.n += n
        while self.n >= 8:
            self.out.append(self.acc & 0xFF)
            self.acc >>= 8
            self.n -= 8

    def copy(self, r, n):
        self.write(r.read(n), n)

    def getvalue(self):
        return bytes(self.out) + (bytes([self.acc & 0xFF]) if self.n else b"")


def ilog(v):
    return v.bit_length()


def quantvals(entries, dims):
    vals = entries >> ((ilog(entries) - 1) * (dims - 1) // dims)
    while True:
        if vals ** dims <= entries < (vals + 1) ** dims:
            return vals
        vals += -1 if vals ** dims > entries else 1


def _codebook(r, w=None):
    """Reads one packed Wwise codebook from r; writes the standard Vorbis codebook to w if given."""
    def cp(n):
        v = r.read(n)
        if w:
            w.write(v, n)
        return v

    dims, entries = r.read(4), r.read(14)
    if not dims or not entries:
        raise ValueError("bad codebook")
    if w:
        w.write(0x564342, 24)
        w.write(dims, 16)
        w.write(entries, 24)
    if cp(1):  # ordered
        cp(5)
        cur = 0
        while cur < entries:
            cur += cp(ilog(entries - cur))
        if cur > entries:
            raise ValueError("bad codebook")
    else:
        length_bits, sparse = r.read(3), r.read(1)
        if not 1 <= length_bits <= 5:
            raise ValueError("bad codebook")
        if w:
            w.write(sparse, 1)
        for _ in range(entries):
            if not sparse or cp(1):
                v = r.read(length_bits)
                if w:
                    w.write(v, 5)
    lookup = r.read(1)
    if w:
        w.write(lookup, 4)
    if lookup:
        cp(32), cp(32)
        value_bits = cp(4) + 1
        cp(1)
        for _ in range(quantvals(entries, dims)):
            cp(value_bits)


def packed_codebook_size(blob):
    """Size in bytes of a packed codebook by ww2ogg's rule (bits / 8 + 1), or None if it isn't one."""
    try:
        r = BitReader(blob)
        _codebook(r)
        return r.pos // 8 + 1
    except (ValueError, IndexError, ZeroDivisionError):
        return None


def find_vorbis_codebooks(exe_path):
    """Finds the sound engine's Vorbis codebook library in the game executable: an array of pointers
    to packed codebooks, increasing by each codebook's size. Scans the non-code sections for long
    runs of such pointers and keeps the run whose targets parse as codebooks of exactly that size."""
    import numpy as np

    data = open(exe_path, "rb").read()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("not a PE file: " + exe_path)
    nsec, = struct.unpack_from("<H", data, pe + 6)
    opt_size, = struct.unpack_from("<H", data, pe + 20)
    image_base, = struct.unpack_from("<Q", data, pe + 48)
    image_size, = struct.unpack_from("<I", data, pe + 80)
    sections = []
    for i in range(nsec):
        vsize, va, rsize, raw = struct.unpack_from("<IIII", data, pe + 24 + opt_size + 40 * i + 8)
        flags, = struct.unpack_from("<I", data, pe + 24 + opt_size + 40 * i + 36)
        sections.append((va, max(vsize, rsize), raw, rsize, flags))

    def file_offset(va):
        rva = va - image_base
        for sva, span, raw, rsize, _ in sections:
            if sva <= rva < sva + span and rva - sva < rsize:
                return raw + rva - sva
        return None

    runs = []
    for _, _, raw, rsize, flags in sections:
        if flags & 0x20000000 or rsize < 4096:  # skip code
            continue
        start = raw + (-raw % 8)
        a = np.frombuffer(data, dtype="<u8", count=(raw + rsize - start) // 8, offset=start)
        inside = (a >= image_base) & (a < image_base + image_size)
        step = np.diff(a.astype(np.int64))
        ok = inside[:-1] & inside[1:] & (step >= 2) & (step < 4096)
        edges = np.flatnonzero(np.diff(np.concatenate(([0], ok.view(np.int8), [0]))))
        for s, e in zip(edges[::2], edges[1::2]):
            if e - s >= 255:
                runs.append((int(e - s) + 1, start + 8 * int(s)))
    for count, off in sorted(runs, reverse=True):
        ptrs = struct.unpack_from("<%dQ" % count, data, off)
        books = []
        for i in range(count - 1):
            fo = file_offset(ptrs[i])
            books.append(data[fo:fo + ptrs[i + 1] - ptrs[i]] if fo is not None else b"")
        good = sum(packed_codebook_size(b) == len(b) for b in books)
        if good >= 0.95 * len(books):
            return books
    raise RuntimeError("Vorbis codebook library not found in " + exe_path)


CRC_TABLE = []
for _i in range(256):
    _r = _i << 24
    for _ in range(8):
        _r = ((_r << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if _r & 0x80000000 else (_r << 1) & 0xFFFFFFFF
    CRC_TABLE.append(_r)


def ogg_crc(page):
    c = 0
    for x in page:
        c = ((c << 8) & 0xFFFFFFFF) ^ CRC_TABLE[(c >> 24) ^ x]
    return c


class OggWriter:
    def __init__(self, serial):
        self.serial = serial
        self.seq = 0
        self.out = bytearray()

    def packets(self, packets, bos=False, eos=False, max_body=4096):
        """Writes (packet, granule) pairs starting on a fresh page. A page's granule is that of the
        last packet completed on it (-1 if none)."""
        segs, body, state = [], bytearray(), {"granule": -1, "cont": False, "first": True}

        def flush(last, next_cont):
            flags = (1 if state["cont"] else 0) | (2 if bos and state["first"] else 0) | (4 if eos and last else 0)
            page = bytearray(struct.pack("<4sBBqIIIB", b"OggS", 0, flags, state["granule"], self.serial, self.seq, 0, len(segs)))
            page += bytes(segs) + body
            page[22:26] = struct.pack("<I", ogg_crc(page))
            self.out += page
            self.seq += 1
            segs.clear()
            body.clear()
            state.update(granule=-1, cont=next_cont, first=False)

        for pkt, granule in packets:
            if segs and len(body) >= max_body:
                flush(False, False)
            lacing = [255] * (len(pkt) // 255) + [len(pkt) % 255]
            p = 0
            for j, lace in enumerate(lacing):
                if len(segs) == 255:
                    flush(False, j > 0)
                segs.append(lace)
                body += pkt[p:p + lace]
                p += lace
            state["granule"] = granule
        flush(True, False)


def _vorbis_header(kind):
    w = BitWriter()
    w.write(kind, 8)
    for c in b"vorbis":
        w.write(c, 8)
    return w


def _setup_header(packet, channels, books):
    """Rebuilds the Vorbis setup header from Wwise's stripped one. Returns (header, mode block flags)."""
    r = BitReader(packet)
    w = _vorbis_header(5)
    count = r.read(8) + 1
    w.write(count - 1, 8)
    for _ in range(count):
        book = r.read(10)
        if book >= len(books):
            raise ValueError("codebook id %d out of range" % book)
        _codebook(BitReader(books[book]), w)
    w.write(0, 6)   # time domain transforms: one placeholder
    w.write(0, 16)
    floors = r.read(6) + 1
    w.write(floors - 1, 6)
    for _ in range(floors):
        w.write(1, 16)  # floor type 1
        parts = r.read(5)
        w.write(parts, 5)
        classes = []
        for _ in range(parts):
            c = r.read(4)
            w.write(c, 4)
            classes.append(c)
        dims = []
        for _ in range(max(classes, default=-1) + 1):
            d = r.read(3)
            w.write(d, 3)
            dims.append(d + 1)
            sub = r.read(2)
            w.write(sub, 2)
            if sub:
                w.copy(r, 8)
            for _ in range(1 << sub):
                w.copy(r, 8)
        w.copy(r, 2)
        rangebits = r.read(4)
        w.write(rangebits, 4)
        for c in classes:
            for _ in range(dims[c]):
                w.copy(r, rangebits)
    residues = r.read(6) + 1
    w.write(residues - 1, 6)
    for _ in range(residues):
        rtype = r.read(2)
        if rtype > 2:
            raise ValueError("bad residue type")
        w.write(rtype, 16)
        for n in (24, 24, 24):
            w.copy(r, n)
        ncls = r.read(6) + 1
        w.write(ncls - 1, 6)
        w.copy(r, 8)
        cascade = []
        for _ in range(ncls):
            low = r.read(3)
            w.write(low, 3)
            flag = r.read(1)
            w.write(flag, 1)
            high = r.read(5) if flag else 0
            if flag:
                w.write(high, 5)
            cascade.append(high * 8 + low)
        for c in cascade:
            for k in range(8):
                if c & (1 << k):
                    w.copy(r, 8)
    mappings = r.read(6) + 1
    w.write(mappings - 1, 6)
    for _ in range(mappings):
        w.write(0, 16)  # mapping type 0
        submaps = 1
        flag = r.read(1)
        w.write(flag, 1)
        if flag:
            submaps = r.read(4) + 1
            w.write(submaps - 1, 4)
        coupled = r.read(1)
        w.write(coupled, 1)
        if coupled:
            steps = r.read(8) + 1
            w.write(steps - 1, 8)
            for _ in range(2 * steps):
                w.copy(r, ilog(channels - 1))
        reserved = r.read(2)
        if reserved:
            raise ValueError("bad mapping")
        w.write(0, 2)
        if submaps > 1:
            for _ in range(channels):
                w.copy(r, 4)
        for _ in range(submaps):
            for n in (8, 8, 8):
                w.copy(r, n)
    modes = r.read(6) + 1
    w.write(modes - 1, 6)
    blockflags = []
    for _ in range(modes):
        flag = r.read(1)
        w.write(flag, 1)
        blockflags.append(bool(flag))
        w.write(0, 16)  # window type
        w.write(0, 16)  # transform type
        w.copy(r, 8)
    w.write(1, 1)  # framing
    if (r.pos + 7) // 8 != len(packet):
        raise ValueError("setup packet size mismatch")
    return w.getvalue(), blockflags


_SETUP_CACHE = {}


def wem_vorbis_to_ogg(w, books, serial=1):
    """fmt 0xFFFF .wem -> Ogg Vorbis bytes. Handles the layout this game uses: a 0x42-byte fmt
    chunk carrying the Vorbis fields, 2-byte packet sizes, a stripped setup packet with codebook ids."""
    ch = riff_chunks(w)
    fmt, fmt_size = ch[b"fmt "]
    tag, channels, rate, avg_bytes = struct.unpack_from("<HHII", w, fmt)
    if tag != 0xFFFF or fmt_size != 0x42:
        raise ValueError("unsupported Vorbis .wem layout (fmt %#x, %d bytes)" % (tag, fmt_size))
    vorb = fmt + 0x18
    samples, mod_signal = struct.unpack_from("<II", w, vorb)
    setup_off, audio_off = struct.unpack_from("<II", w, vorb + 0x10)
    bs0, bs1 = w[vorb + 0x28], w[vorb + 0x29]
    # Packets keep their type/window bits unless the encoder stripped them (the usual case).
    stripped = mod_signal not in (0x4A, 0x4B, 0x69, 0x70)
    data_off, data_size = ch[b"data"]
    data = w[data_off:data_off + data_size]

    def packet(o):
        size = struct.unpack_from("<H", data, o)[0]
        return data[o + 2:o + 2 + size], o + 2 + size

    setup, end = packet(setup_off)
    if end != audio_off:
        raise ValueError("first audio packet doesn't follow the setup packet")
    key = (setup, channels)
    if key not in _SETUP_CACHE:
        _SETUP_CACHE[key] = _setup_header(setup, channels, books)
    setup_header, blockflags = _SETUP_CACHE[key]
    mode_bits = ilog(len(blockflags) - 1)

    ident = _vorbis_header(1)
    ident.write(0, 32)
    ident.write(channels, 8)
    ident.write(rate, 32)
    ident.write(0, 32)
    ident.write(avg_bytes * 8, 32)
    ident.write(0, 32)
    ident.write(bs0, 4)
    ident.write(bs1, 4)
    ident.write(1, 1)
    comment = _vorbis_header(3)
    vendor = b"Wwise Vorbis repacked for riftapart-skyrim"
    comment.write(len(vendor), 32)
    for c in vendor:
        comment.write(c, 8)
    comment.write(0, 32)
    comment.write(1, 1)

    raw = []
    o = audio_off
    while o + 2 <= len(data):
        pkt, o = packet(o)
        if pkt:
            raw.append(pkt)
    mask = (1 << mode_bits) - 1
    modes = [(p[0] & mask) if stripped else ((p[0] >> 1) & mask) for p in raw]
    audio = []
    granule = prev_size = 0
    for i, pkt in enumerate(raw):
        long_block = blockflags[modes[i]]
        if stripped:
            # Put back the packet-type bit (0) and, for long blocks, the previous/next window flags.
            prefix, plen = modes[i] << 1, 1 + mode_bits
            if long_block:
                nxt = blockflags[modes[i + 1]] if i + 1 < len(raw) else False
                prv = blockflags[modes[i - 1]] if i > 0 else False
                prefix |= (prv << plen) | (nxt << (plen + 1))
                plen += 2
            bits = plen + len(pkt) * 8 - mode_bits
            pkt = (prefix | ((int.from_bytes(pkt, "little") >> mode_bits) << plen)).to_bytes((bits + 7) // 8, "little")
        size = (1 << bs1) if long_block else (1 << bs0)
        if prev_size:
            granule += (prev_size + size) // 4
        prev_size = size
        audio.append([pkt, granule])
    if audio and 0 < samples < granule:
        for a in audio:  # trim the encoder's padding at the end
            a[1] = min(a[1], samples)

    ogg = OggWriter(serial)
    ogg.packets([(ident.getvalue(), 0)], bos=True)
    ogg.packets([(comment.getvalue(), 0), (setup_header, 0)])
    ogg.packets([tuple(a) for a in audio], eos=True)
    return bytes(ogg.out), {"channels": channels, "rate": rate, "samples": min(samples, granule) if samples else granule}
