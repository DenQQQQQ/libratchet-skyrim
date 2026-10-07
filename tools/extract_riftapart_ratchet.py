# SPDX-License-Identifier: GPL-3.0-or-later
# Converts Ratchet's (and Clank's) model, textures and animation clips from YOUR OWN Rift Apart
# install into glTF binaries (ratchet.glb, clank.glb) for the sandbox. Uses ALERT (https://github.com/Tkachov/ALERT, GPLv3), so this
# script is GPLv3 too; the rest of the repository is MIT. The output is game data: keep it
# local (riftapart-assets/ is gitignored), never commit or share it.
#
# Needs: pip install pygltflib pillow numpy texture2ddecoder lz4
#        cargo build -p gdeflate-ffi --release   (GDeflate decoder for the high-res data)
# Usage:
#   python tools/extract_riftapart_ratchet.py --game "<Rift Apart folder>" --alert "<ALERT clone>"
#          [--character ratchet|clank|all] [--out riftapart-assets] [--tex-size 2048]
import argparse
import base64
import io
import json
import os
import re
import struct
import sys

import numpy as np
import pygltflib
import texture2ddecoder
from PIL import Image

# Meshes with these materials are effect overlays or placeholders, not part of the look.
SKIP_MATERIALS = ("compositeshell",)
TEX_RE = re.compile(rb"[A-Za-z0-9_/\\\.\-]+\.texture")

# DXGI format -> (decoder, bytes per 4x4 block or per pixel, is_block)
DECODERS = {
    71: ("bc1", 8), 72: ("bc1", 8),
    74: ("bc2", 16), 75: ("bc2", 16),
    77: ("bc3", 16), 78: ("bc3", 16),
    80: ("bc4", 8), 81: ("bc4", 8),
    83: ("bc5", 16), 84: ("bc5", 16),
    95: ("bc6", 16), 96: ("bc6", 16),
    98: ("bc7", 16), 99: ("bc7", 16),
    28: ("rgba8", 4), 29: ("rgba8", 4),
    87: ("bgra8", 4), 91: ("bgra8", 4),
    61: ("r8", 1),
}


def use_gdeflate_dll(dll_path):
    """Routes ALERT's GDeflate decompression through our source-built DLL (tools/gdeflate-ffi).
    Without it, ALERT silently returns zeros for GDeflate blocks (all high-res texture data)."""
    import ctypes
    import dat1lib.gdeflate

    lib = ctypes.CDLL(dll_path)
    lib.rcra_gdeflate_decompress.restype = ctypes.c_int64
    lib.rcra_gdeflate_decompress.argtypes = [ctypes.c_char_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t]

    def decompress(compressed, output_size):
        src = bytes(compressed)
        out = (ctypes.c_ubyte * output_size)()
        n = lib.rcra_gdeflate_decompress(src, len(src), ctypes.addressof(out), output_size)
        if n < 0:
            raise RuntimeError(f"GDeflate block failed ({n})")
        return bytearray(out)

    dat1lib.gdeflate.decompress = decompress


class Game:
    def __init__(self, game_dir, alert_dir, gdeflate_dll):
        sys.path.insert(0, alert_dir)
        import dat1lib
        import dat1lib.crc64
        import dat1lib.types.sections.texture.header as texheader

        use_gdeflate_dll(gdeflate_dll)

        self.dat1lib = dat1lib
        self.texheader = texheader
        with open(os.path.join(game_dir, "toc"), "rb") as f:
            self.toc = dat1lib.read(f)
        self.toc.set_archives_dir(game_dir)
        self._ids = None

    def entries(self, path):
        aid = self.dat1lib.crc64.hash(path.replace("/", "\\"))
        if self._ids is None:
            # One pass to index every asset id (the toc lookup is a linear scan otherwise).
            self._ids = {}
            for i, a in enumerate(self.toc.get_assets_section().ids):
                self._ids.setdefault(a, []).append(i)
        return [self.toc.get_asset_entry_by_index(i) for i in self._ids.get(aid, [])]

    def read(self, path, span=0):
        e = self.entries(path)
        return self.toc.extract_asset(e[span]) if len(e) > span else None

    def texture(self, path, max_size):
        """Decodes mip 0 (high-res part if present) to an RGBA PIL image, downscaled to max_size."""
        data = self.read(path)
        if data is None:
            return None
        tex = self.dat1lib.read(io.BytesIO(data))
        info = tex.dat1.get_section(self.texheader.TextureHeaderSection.TAG)
        hd = self.read(path, 1) if info.hd_mipmaps > 0 else None
        if hd is not None:
            w, h, pixels = info.hd_width, info.hd_height, hd
        else:
            w, h, pixels = info.sd_width, info.sd_height, tex._raw_dat1[0x80 - 36:]
        kind = DECODERS.get(info.fmt)
        if kind is None:
            print(f"    [!] {path}: unsupported format {info.fmt}")
            return None
        name, _ = kind
        if name.startswith("bc"):
            raw = getattr(texture2ddecoder, "decode_" + name)(bytes(pixels), w, h)
            img = Image.frombytes("RGBA", (w, h), raw, "raw", "BGRA")
        elif name == "rgba8":
            img = Image.frombytes("RGBA", (w, h), bytes(pixels[: w * h * 4]))
        elif name == "bgra8":
            img = Image.frombytes("RGBA", (w, h), bytes(pixels[: w * h * 4]), "raw", "BGRA")
        else:
            img = Image.frombytes("L", (w, h), bytes(pixels[: w * h])).convert("RGBA")
        if max(w, h) > max_size:
            s = max_size / max(w, h)
            img = img.resize((max(1, int(w * s)), max(1, int(h * s))), Image.LANCZOS)
        return img, info.fmt


def material_textures(game, material_path):
    data = game.read(material_path)
    if data is None:
        return []
    return sorted(set(t.decode().replace("/", "\\") for t in TEX_RE.findall(data)))


def pick(textures, suffix, material_path):
    """Picks the material's own `_<suffix>` map, preferring character textures over shared detail maps."""
    cands = [
        t for t in textures
        if t.lower().endswith("_" + suffix + ".texture") and "detail" not in t.lower() and "default_pink" not in t.lower()
    ]
    cands.sort(key=lambda t: (not t.lower().startswith("characters"), len(t)))
    return cands[0] if cands else None


def special_material(material_path, preset=None):
    """Materials drawn by special shaders in the game: 'glow', 'glass', 'blur' or None."""
    name = os.path.basename((material_path or "").replace("\\", "/")).lower()
    if "glow" in name or any(m in name for m in (preset or {}).get("glow_match", ())):
        return "glow"
    if "glass" in name:
        return "glass"
    if "rotorblades" in name:
        # Clank's spinning-blade smear (no textures; the game draws it with an effect shader).
        return "blur"
    return None


def image_uri(img, alpha):
    buf = io.BytesIO()
    if alpha:
        img.save(buf, "PNG", optimize=True)
        mime = "image/png"
    else:
        img.convert("RGB").save(buf, "JPEG", quality=90)
        mime = "image/jpeg"
    return f"data:{mime};base64," + base64.b64encode(buf.getvalue()).decode()


def normal_map(img):
    """BC5 normal maps store X/Y only: rebuild Z and flip Y for glTF (OpenGL-style)."""
    a = np.asarray(img.convert("RGBA")).astype(np.float32) / 255.0
    x = a[..., 0] * 2 - 1
    y = -(a[..., 1] * 2 - 1)
    z = np.sqrt(np.clip(1 - x * x - y * y, 0, 1))
    out = np.stack([x, y, z], axis=-1) * 0.5 + 0.5
    return Image.fromarray((out * 255).astype(np.uint8), "RGB")


def roughness_map(img):
    """Gloss (first channel) -> glTF metallic-roughness texture (G = roughness, B = metallic 0)."""
    g = np.asarray(img.convert("RGBA"))[..., 0].astype(np.float32) / 255.0
    r = (1.0 - g) * 255
    out = np.zeros(g.shape + (3,), np.uint8)
    out[..., 1] = r.astype(np.uint8)
    return Image.fromarray(out, "RGB")


COMPONENT_DTYPES = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
TYPE_SIZES = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def glb_read(path):
    data = open(path, "rb").read()
    magic, version, length = struct.unpack_from("<III", data, 0)
    assert magic == 0x46546C67, "not a GLB"
    off, j, blob = 12, None, bytearray()
    while off < length:
        clen, ctype = struct.unpack_from("<II", data, off)
        chunk = data[off + 8 : off + 8 + clen]
        if ctype == 0x4E4F534A:
            j = json.loads(chunk.decode("utf-8"))
        elif ctype == 0x004E4942:
            blob = bytearray(chunk)
        off += 8 + clen
    return j, blob


def glb_write(path, j, blob):
    js = json.dumps(j, separators=(",", ":")).encode("utf-8")
    js += b" " * ((-len(js)) % 4)
    blob = bytes(blob) + b"\0" * ((-len(blob)) % 4)
    total = 12 + 8 + len(js) + 8 + len(blob)
    with open(path, "wb") as f:
        f.write(struct.pack("<III", 0x46546C67, 2, total))
        f.write(struct.pack("<II", len(js), 0x4E4F534A) + js)
        f.write(struct.pack("<II", len(blob), 0x004E4942) + blob)


def read_accessor(j, blob, index):
    acc = j["accessors"][index]
    view = j["bufferViews"][acc["bufferView"]]
    dtype = np.dtype(COMPONENT_DTYPES[acc["componentType"]])
    n = TYPE_SIZES[acc["type"]]
    start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
    stride = view.get("byteStride") or n * dtype.itemsize
    count = acc["count"]
    raw = np.frombuffer(bytes(blob), np.uint8, count=stride * (count - 1) + n * dtype.itemsize, offset=start)
    rows = np.lib.stride_tricks.as_strided(raw, shape=(count, n * dtype.itemsize), strides=(stride, 1))
    return np.ascontiguousarray(rows).view(dtype).reshape(count, n)


def append_accessor(j, blob, data, component_type, kind):
    """Appends `data` to the binary chunk as a new buffer view + accessor; returns its index."""
    blob += b"\0" * ((-len(blob)) % 4)
    raw = np.ascontiguousarray(data).tobytes()
    j["bufferViews"].append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(raw), "target": 34962})
    blob += raw
    j["accessors"].append({"bufferView": len(j["bufferViews"]) - 1, "componentType": component_type, "count": len(data), "type": kind})
    return len(j["accessors"]) - 1


def apply_scale_compensation(j, blob, compensate):
    """Joints named in `compensate` don't inherit their parent's own scale; only their offset is
    scaled by it (like Maya's "segment scale compensate"). glTF always inherits scale, so each such
    parent P with a non-unit scale is split: P keeps its translation and rotation, a new child
    P_scaled takes P's scale, the skin and the inheriting children move under P_scaled, and each
    compensating child's translation is multiplied by P's scale at every key. Returns the names of
    the split joints."""
    nodes = j["nodes"]
    anims = j.get("animations", [])

    def channel(anim, node, path):
        return next((ch for ch in anim["channels"] if ch["target"].get("node") == node and ch["target"]["path"] == path), None)

    def add_vec3(values):
        index = append_accessor(j, blob, np.asarray(values, np.float32), 5126, "VEC3")
        j["bufferViews"][-1].pop("target", None)  # animation data, not a vertex buffer
        return index

    split = []
    for p in range(len(nodes)):
        kids = nodes[p].get("children", [])
        comp = [c for c in kids if nodes[c].get("name") in compensate]
        if not comp:
            continue
        rest = np.array(nodes[p].get("scale", [1.0, 1.0, 1.0]), np.float32)
        keys = {}
        for ai, anim in enumerate(anims):
            ch = channel(anim, p, "scale")
            if ch is not None:
                keys[ai] = (ch, read_accessor(j, blob, anim["samplers"][ch["sampler"]]["output"]))
        if np.allclose(rest, 1) and all(np.allclose(v, 1, atol=1e-4) for _, v in keys.values()):
            continue
        for c in comp:
            c_rest = np.array(nodes[c].get("translation", [0.0, 0.0, 0.0]), np.float32)
            nodes[c]["translation"] = (rest * c_rest).tolist()
            for ai, anim in enumerate(anims):
                if ai not in keys:
                    if not np.allclose(rest, 1):
                        tch = channel(anim, c, "translation")
                        if tch is not None:
                            sampler = anim["samplers"][tch["sampler"]]
                            sampler["output"] = add_vec3(read_accessor(j, blob, sampler["output"]) * rest)
                    continue
                sch, s_vals = keys[ai]
                s_sampler = anim["samplers"][sch["sampler"]]
                tch = channel(anim, c, "translation")
                if tch is None:
                    anim["samplers"].append({"input": s_sampler["input"], "output": add_vec3(s_vals * c_rest),
                                             "interpolation": s_sampler.get("interpolation", "LINEAR")})
                    anim["channels"].append({"sampler": len(anim["samplers"]) - 1, "target": {"node": c, "path": "translation"}})
                    continue
                t_sampler = anim["samplers"][tch["sampler"]]
                t_vals = read_accessor(j, blob, t_sampler["output"])
                assert len(t_vals) == len(s_vals), "scale and translation keys must line up"
                t_sampler["output"] = add_vec3(s_vals * t_vals)
        s = len(nodes)
        nodes.append({"name": nodes[p]["name"] + "_scaled", "scale": rest.tolist(), "children": [c for c in kids if c not in comp]})
        nodes[p].pop("scale", None)
        nodes[p]["children"] = comp + [s]
        for ch, _ in keys.values():
            ch["target"]["node"] = s
        for skin in j.get("skins", []):
            skin["joints"] = [s if x == p else x for x in skin["joints"]]
        split.append(nodes[p]["name"])
    return split


def fix_glb_for_bevy(path, compensate=()):
    """Float accessors must not be 'normalized' (ALERT marks UVs that way, which Bevy rejects),
    and Bevy skins with at most 4 influences: merge JOINTS_1/WEIGHTS_1 into the top 4. Also
    bakes the game's scale-compensating joints into a plain glTF hierarchy."""
    j, blob = glb_read(path)
    for acc in j["accessors"]:
        if acc.get("componentType") == 5126 and acc.get("normalized"):
            del acc["normalized"]
    merged = 0
    for mesh in j["meshes"]:
        for prim in mesh["primitives"]:
            at = prim["attributes"]
            if not all(k in at for k in ("JOINTS_0", "WEIGHTS_0", "JOINTS_1", "WEIGHTS_1")):
                continue
            jj = np.concatenate([read_accessor(j, blob, at["JOINTS_0"]), read_accessor(j, blob, at["JOINTS_1"])], axis=1).astype(np.uint16)
            ww = np.concatenate([read_accessor(j, blob, at["WEIGHTS_0"]), read_accessor(j, blob, at["WEIGHTS_1"])], axis=1).astype(np.float32)
            order = np.argsort(-ww, axis=1)[:, :4]
            j4 = np.take_along_axis(jj, order, axis=1)
            w4 = np.take_along_axis(ww, order, axis=1)
            total = w4.sum(axis=1, keepdims=True)
            w4 = np.where(total > 0, w4 / np.maximum(total, 1e-8), np.array([1, 0, 0, 0], np.float32)).astype(np.float32)
            at["JOINTS_0"] = append_accessor(j, blob, j4, 5123, "VEC4")
            at["WEIGHTS_0"] = append_accessor(j, blob, w4, 5126, "VEC4")
            del at["JOINTS_1"], at["WEIGHTS_1"]
            merged += 1
    # Ratchet's skeleton has several root joints (fx_root, sync, a_body). Bevy would give each its
    # own AnimationPlayer; put them under one rig_root so a single player drives the body.
    scene = j["scenes"][j.get("scene", 0)]
    joint_set = set(j["skins"][0]["joints"]) if j.get("skins") else set()
    roots = [n for n in scene["nodes"] if n in joint_set]
    if len(roots) > 1:
        j["nodes"].append({"name": "rig_root", "children": roots})
        rig = len(j["nodes"]) - 1
        scene["nodes"] = [n for n in scene["nodes"] if n not in joint_set] + [rig]
        j["skins"][0]["skeleton"] = rig
    split = apply_scale_compensation(j, blob, set(compensate))
    j["buffers"][0]["byteLength"] = len(blob) + ((-len(blob)) % 4)
    glb_write(path, j, blob)
    return merged, split


# What to convert per character: model, look, output name, and move state -> clip.
PRESETS = {
    "ratchet": {
        "model": "characters/hero/hero_ratchet/hero_ratchet.model",
        "out": "ratchet.glb",
        "clip_dir": "characters/hero/hero_ratchet/animations/",
        "clips": {
            # Unarmed "neutral" versions where they exist.
            "idle": "neutral_idle_hero_ratchet",
            "run": "neutral_run_hero_ratchet",
            "jump": "neutral_jump_single_hero_ratchet",
            "double_jump": "jump_double_hero_ratchet",
            "fall": "fall_loop_hero_ratchet",
            "glide": "fall_glide_hero_ratchet",
            "land": "neutral_jump_single_land_hero_ratchet",
            "dash": "dash_n_hero_ratchet",
            "air_dash": "dash_air_n_hero_ratchet",
            # OmniWrench: the 3-hit combo, Hyper-Strike (intro in the air, then the slam) and
            # the throw. The wrench model has matching clips (preset "wrench").
            "melee_1": "weapon_wrench_hit_1_hero_ratchet",
            "melee_2": "weapon_wrench_hit_2_hero_ratchet",
            "melee_3": "weapon_wrench_hit_3_hero_ratchet",
            "smash_intro": "weapon_wrench_hit_smash_intro_hero_ratchet",
            "smash": "weapon_wrench_hit_smash_hero_ratchet",
            "throw": "weapon_wrench_throw_hero_ratchet",
        },
    },
    "wrench": {
        # Held in Ratchet's right hand (RT_handle_bound, the igLoc_RT_gunAttach locator). Its
        # clips squash and stretch the wrench in time with his swings; the emissive strip glows
        # cyan (DefaultEmissiveColor in its config).
        "model": "equipment/weapon/wpn_wrench/wpn_wrench.model",
        "out": "wrench.glb",
        "glow": [0.0, 0.52, 0.65],
        "glow_match": ["wpn_wrench_emissive"],
        "clip_dir": "characters/hero/hero_ratchet/animations/",
        "clips": {
            "melee_1": "weapon_wrench_hit_1_wpn_wrench",
            "melee_2": "weapon_wrench_hit_2_wpn_wrench",
            "melee_3": "weapon_wrench_hit_3_wpn_wrench",
            "smash_intro": "weapon_wrench_hit_smash_intro_wpn_wrench",
            "smash": "weapon_wrench_hit_smash_wpn_wrench",
        },
    },
    "clank": {
        # His colour maps are dark: in the game the silver comes from metallic reflections.
        "metal": {"brighten": (0.2, 2.0), "metallic": 0.9},
        # Rides on Ratchet's back: the game attaches Clank's dyn_backpack_attach joint to
        # Ratchet's ClankAttach joint (locator igLoc_clankBackpack on both).
        "model": "characters/hero/hero_clank_ps4/hero_clank_ps4.model",
        # Look 0 is his body; looks 4-6 are the heli-pack (top rotor, left and right arm
        # rotors), which the game only switches on while it's out. Mesh nodes get glTF extras
        # {"look": n, "part": ...} so the sandbox can do the same.
        "looks": [0, 4, 5, 6],
        "look_parts": {4: "heli", 5: "heli", 6: "heli"},
        "out": "clank.glb",
        "clip_dir": "characters/hero/hero_clank_ps4/animations/",
        "clips": {
            "idle": "backpack_idle_a_hero_clank_ps4",
            "glide": "backpack_heli_glide_broken_hero_clank_ps4",
            "heli_extend": "backpack_heli_extend_broken_hero_clank_ps4",
            "heli_retract": "backpack_heli_retract_broken_hero_clank_ps4",
            "fall_glide": "fall_glide_hero_clank_ps4",
        },
    },
}


def add_animations(game, w, model, preset):
    """Decodes the preset's clips from the install and adds them as named glTF animations."""
    import riftapart_anim

    g = w.gltf
    joints = model.dat1.get_section(0x15DF9D3B).joints
    node_of_hash = {j.hash: g.skins[0].joints[i] for i, j in enumerate(joints)}
    report = {}
    for state, clip_name in preset["clips"].items():
        data = game.read(preset["clip_dir"] + clip_name + ".animclip")
        if data is None:
            print(f"  [!] clip missing: {clip_name}")
            continue
        asset = game.dat1lib.read(io.BytesIO(data))
        sections = {sec.TAG: sec._raw for sec in asset.dat1.sections}
        clip = riftapart_anim.decode(clip_name, sections)
        if clip is None:
            print(f"  [!] clip kind not supported: {clip_name}")
            continue
        if clip.additive:
            # Layered clips get "_additive" names and no scale channels (Bevy adds scales). Their
            # rotation offsets work as-is with Bevy's rule (offset · base): the throw looks right
            # that way, while re-expressing them in each joint's own space bent the tail wrongly.
            add_animation(w, g, clip, state + "_additive", node_of_hash)
            report[state] = {"clip": clip_name, "frames": clip.frames, "seconds": round(clip.duration, 3), "additive": True}
            print(f"  anim {state:12} {clip_name}: {clip.frames} frames ({clip.duration:.2f} s), additive")
            continue
        times = [f / clip.fps for f in range(clip.frames)]
        anim = pygltflib.Animation(name=state)
        t_data = w.make_buffer_func(times, lambda x: struct.pack("<f", x))
        _, t_view = w.create_buffer_view(t_data, None)
        t_acc, t_index = w.create_accessor()
        t_acc.bufferView, t_acc.byteOffset, t_acc.componentType = t_view, 0, pygltflib.FLOAT
        t_acc.count, t_acc.type, t_acc.min, t_acc.max = len(times), pygltflib.SCALAR, [times[0]], [times[-1]]
        mapped = 0
        for j, h in enumerate(clip.hashes):
            node = node_of_hash.get(h)
            if node is None:
                continue
            mapped += 1
            for path, values, kind, fmt in (
                ("rotation", clip.rotations[:, j], pygltflib.VEC4, "<4f"),
                ("translation", clip.translations[:, j], pygltflib.VEC3, "<3f"),
                # Keyed on every joint in every clip, so a scale never sticks after a switch.
                ("scale", clip.scales[:, j], pygltflib.VEC3, "<3f"),
            ):
                v_data = w.make_buffer_func(values.tolist(), lambda x, fmt=fmt: struct.pack(fmt, *x))
                _, v_view = w.create_buffer_view(v_data, None)
                acc, a_index = w.create_accessor()
                acc.bufferView, acc.byteOffset, acc.componentType = v_view, 0, pygltflib.FLOAT
                acc.count, acc.type = len(values), kind
                anim.samplers.append(pygltflib.AnimationSampler(input=t_index, output=a_index, interpolation=pygltflib.ANIM_LINEAR))
                anim.channels.append(pygltflib.AnimationChannel(
                    sampler=len(anim.samplers) - 1, target=pygltflib.AnimationChannelTarget(node=node, path=path)))
        g.animations.append(anim)
        report[state] = {"clip": clip_name, "frames": clip.frames, "seconds": round(clip.duration, 3), "joints": mapped}
        print(f"  anim {state:12} {clip_name}: {clip.frames} frames ({clip.duration:.2f} s), {mapped} joints")
    return report


def add_animation(w, g, clip, name, node_of_hash):
    """Adds an additive clip as a glTF animation with rotation and translation offsets only."""
    times = [f / clip.fps for f in range(clip.frames)]
    anim = pygltflib.Animation(name=name)
    t_data = w.make_buffer_func(times, lambda x: struct.pack("<f", x))
    _, t_view = w.create_buffer_view(t_data, None)
    t_acc, t_index = w.create_accessor()
    t_acc.bufferView, t_acc.byteOffset, t_acc.componentType = t_view, 0, pygltflib.FLOAT
    t_acc.count, t_acc.type, t_acc.min, t_acc.max = len(times), pygltflib.SCALAR, [times[0]], [times[-1]]
    for j, h in enumerate(clip.hashes):
        node = node_of_hash.get(h)
        if node is None:
            continue
        for path, values, kind, fmt in (("rotation", clip.rotations[:, j], pygltflib.VEC4, "<4f"),
                                        ("translation", clip.translations[:, j], pygltflib.VEC3, "<3f")):
            v_data = w.make_buffer_func(values.tolist(), lambda x, fmt=fmt: struct.pack(fmt, *x))
            _, v_view = w.create_buffer_view(v_data, None)
            acc, a_index = w.create_accessor()
            acc.bufferView, acc.byteOffset, acc.componentType = v_view, 0, pygltflib.FLOAT
            acc.count, acc.type = len(values), kind
            anim.samplers.append(pygltflib.AnimationSampler(input=t_index, output=a_index, interpolation=pygltflib.ANIM_LINEAR))
            anim.channels.append(pygltflib.AnimationChannel(
                sampler=len(anim.samplers) - 1, target=pygltflib.AnimationChannelTarget(node=node, path=path)))
    g.animations.append(anim)


def main():
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True)
    ap.add_argument("--alert", required=True)
    ap.add_argument("--out", default="riftapart-assets")
    ap.add_argument("--character", default="all", choices=["all", *PRESETS], help="which character(s) to convert")
    ap.add_argument("--look", type=int, default=0)
    ap.add_argument("--lod", type=int, default=0)
    ap.add_argument("--tex-size", type=int, default=2048)
    ap.add_argument("--no-anims", action="store_true", help="skip decoding animation clips")
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument("--gdeflate-dll", default=os.path.join(here, "..", "target", "release", "gdeflate_ffi.dll"),
                    help="build it with: cargo build -p gdeflate-ffi --release")
    a = ap.parse_args()

    game = Game(a.game, a.alert, a.gdeflate_dll)
    names = list(PRESETS) if a.character == "all" else [a.character]
    for name in names:
        print(f"== {name}")
        convert(game, a, PRESETS[name])


def convert(game, a, preset):
    import model_to_gltf as m2g

    m2g.ADD_SHAPEKEYS = False
    model = game.dat1lib.read(io.BytesIO(game.read(preset["model"])))

    w = m2g.GltfWriter()
    w.init(model)
    # UV = raw int16 × the model's own UV scale (ModelBuilt section, e.g. 1/1024). ALERT skips
    # that section for Rift Apart and its vertex reader already divides by 32768, so redo it.
    built = model.dat1.get_section(m2g.SECTION_BUILT)
    w.uv_scale = (built.get_uv_scale() if built else 1.0 / 16384.0) * 32768.0
    looks = preset.get("looks", [a.look])
    mesh_look = {}
    for look in looks:
        for mi in w.get_meshes_indexes_by_looks_and_lod([look], a.lod):
            mesh_look.setdefault(mi, look)
    all_meshes = sorted(mesh_look)
    keep, mesh_material = [], {}
    for mi in all_meshes:
        path = w.get_material_path(w.meshes[mi].get_material()) or None
        if path is None or any(s in path.lower() for s in SKIP_MATERIALS):
            print(f"  skip mesh {mi} ({path})")
            continue
        if pick(material_textures(game, path), "c", path) is None and not special_material(path, preset):
            # Fur shells, stitching and small emissive bits have no colour map; they need
            # special shaders, so leave them out for now.
            print(f"  skip mesh {mi} (no colour map: {path})")
            continue
        keep.append(mi)
        mesh_material[f"sm{mi:02}"] = path
    skin, rcra_skin = w.get_skin()
    w.add_skeleton(model)
    w.add_meshes(keep, skin, rcra_skin)
    anims = add_animations(game, w, model, preset) if not a.no_anims else {}
    g = w.gltf
    parts = preset.get("look_parts", {})
    for node in g.nodes:
        # ALERT names mesh nodes after the model's mesh index ("sm07", "sm119").
        if node.mesh is not None and node.name and node.name[2:].isdigit():
            look = mesh_look.get(int(node.name[2:]))
            if look is not None:
                node.extras = {"look": look, **({"part": parts[look]} if look in parts else {})}

    # Materials and textures.
    texture_cache = {}
    material_index = {}
    manifest = {"model": preset["model"], "looks": looks, "lod": a.lod, "materials": {}}

    def add_texture(path, kind):
        key = (path, kind)
        if key in texture_cache:
            return texture_cache[key]
        got = game.texture(path, a.tex_size if kind == "color" else max(512, a.tex_size // 2))
        if got is None:
            texture_cache[key] = None
            return None
        img, fmt = got
        if kind == "normal":
            img, alpha = normal_map(img), False
        elif kind == "rough":
            img, alpha = roughness_map(img), False
        else:
            alpha = np.asarray(img)[..., 3].min() < 250
            metal = preset.get("metal")
            if metal:
                lo, gain = metal["brighten"]
                rgba = np.asarray(img).astype(np.float32)
                rgba[..., :3] = np.clip(lo * 255 + gain * rgba[..., :3], 0, 255)
                img = Image.fromarray(rgba.astype(np.uint8), "RGBA")
        g.images.append(pygltflib.Image(uri=image_uri(img, alpha), name=os.path.basename(path)))
        g.textures.append(pygltflib.Texture(source=len(g.images) - 1))
        texture_cache[key] = (len(g.textures) - 1, alpha)
        print(f"    {kind:6} {img.size[0]}x{img.size[1]} fmt {fmt} {'alpha ' if alpha else ''}{path}")
        return texture_cache[key]

    for mesh in g.meshes:
        mpath = mesh_material.get(mesh.name)
        if mpath not in material_index:
            print(f"  material {mpath}")
            texs = material_textures(game, mpath)
            mat = pygltflib.Material(name=os.path.basename(mpath), doubleSided=True)
            pbr = pygltflib.PbrMetallicRoughness(baseColorFactor=[1, 1, 1, 1], metallicFactor=0.0, roughnessFactor=0.7)
            entry = {"textures": texs}
            special = special_material(mpath, preset)
            if special == "glow":
                # Clank's eyes and antenna glow green, the wrench's strip cyan; the game draws
                # them with a shader we don't have.
                glow = list(preset.get("glow", [0.35, 1.0, 0.45]))
                pbr.baseColorFactor = glow + [1.0]
                mat.emissiveFactor = glow
            elif special == "glass":
                pbr.baseColorFactor = [0.6, 0.9, 0.8, 0.25]
                pbr.roughnessFactor = 0.1
                mat.alphaMode = pygltflib.BLEND
            elif special == "blur":
                pbr.baseColorFactor = [0.8, 0.82, 0.85, 0.3]
                pbr.roughnessFactor = 0.4
                mat.alphaMode = pygltflib.BLEND
            if preset.get("metal") and not special:
                pbr.metallicFactor = preset["metal"]["metallic"]
            c = pick(texs, "c", mpath)
            if c and (t := add_texture(c, "color")):
                pbr.baseColorTexture = pygltflib.TextureInfo(index=t[0])
                entry["color"] = c
                if t[1]:
                    mat.alphaMode = pygltflib.MASK
                    mat.alphaCutoff = 0.4
            n = pick(texs, "n", mpath)
            if n and (t := add_texture(n, "normal")):
                mat.normalTexture = pygltflib.NormalMaterialTexture(index=t[0])
                entry["normal"] = n
            gl = pick(texs, "g", mpath)
            if gl and (t := add_texture(gl, "rough")):
                pbr.metallicRoughnessTexture = pygltflib.TextureInfo(index=t[0])
                pbr.roughnessFactor = 1.0
                entry["gloss"] = gl
            mat.pbrMetallicRoughness = pbr
            g.materials.append(mat)
            material_index[mpath] = len(g.materials) - 1
            manifest["materials"][mpath] = entry
        for p in mesh.primitives:
            p.material = material_index[mpath]

    manifest["bones"] = [g.nodes[j].name for j in g.skins[0].joints] if g.skins else []
    manifest["meshes"] = mesh_material
    manifest["animations"] = anims

    os.makedirs(a.out, exist_ok=True)
    g.convert_images(pygltflib.ImageFormat.BUFFERVIEW)
    g.convert_buffers(pygltflib.BufferFormat.BINARYBLOB)
    out = os.path.join(a.out, preset["out"])
    g.save_binary(out)
    # Edit the file directly: pygltflib drops JOINTS_1/WEIGHTS_1 when it reloads a file.
    # Bit 0 of a joint's flags: it doesn't inherit its parent's scale (every *_noScale helper has it).
    joint_defs = model.dat1.get_section(0x15DF9D3B).joints
    compensate = {model.dat1.get_string(jd.string_offset) for jd in joint_defs if jd.unknown2 & 1}
    merged, split = fix_glb_for_bevy(out, compensate)
    print(f"  merged 8 -> 4 bone influences on {merged} primitives")
    print(f"  {len(compensate)} scale-compensating joints; split {len(split)} scaled parents: {', '.join(split)}")
    with open(os.path.join(a.out, preset["out"].replace(".glb", ".json")), "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"wrote {out} ({os.path.getsize(out) / 1e6:.1f} MB), {len(g.meshes)} meshes, {len(g.materials)} materials, {len(g.textures)} textures")


if __name__ == "__main__":
    main()
