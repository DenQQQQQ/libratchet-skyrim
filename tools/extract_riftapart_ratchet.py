# SPDX-License-Identifier: GPL-3.0-or-later
# Converts Ratchet's model and textures from YOUR OWN Rift Apart install into one glTF binary
# (ratchet.glb) for the sandbox. Uses ALERT (https://github.com/Tkachov/ALERT, GPLv3), so this
# script is GPLv3 too; the rest of the repository is MIT. The output is game data: keep it
# local (riftapart-assets/ is gitignored), never commit or share it.
#
# Needs: pip install pygltflib pillow numpy texture2ddecoder lz4
#        cargo build -p gdeflate-ffi --release   (GDeflate decoder for the high-res data)
# Usage:
#   python tools/extract_riftapart_ratchet.py --game "<Rift Apart folder>" --alert "<ALERT clone>"
#          [--out riftapart-assets] [--tex-size 2048]
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

DEFAULT_MODEL = "characters\\hero\\hero_ratchet\\hero_ratchet.model"
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
    cands = [t for t in textures if t.lower().endswith("_" + suffix + ".texture") and "detail" not in t.lower()]
    cands.sort(key=lambda t: (not t.lower().startswith("characters"), len(t)))
    return cands[0] if cands else None


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


def fix_glb_for_bevy(path):
    """Float accessors must not be 'normalized' (ALERT marks UVs that way, which Bevy rejects),
    and Bevy skins with at most 4 influences: merge JOINTS_1/WEIGHTS_1 into the top 4."""
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
    j["buffers"][0]["byteLength"] = len(blob) + ((-len(blob)) % 4)
    glb_write(path, j, blob)
    return merged


# Move state -> Rift Apart clip (unarmed "neutral" versions where they exist).
CLIPS = {
    "idle": "neutral_idle_hero_ratchet",
    "run": "neutral_run_hero_ratchet",
    "jump": "neutral_jump_single_hero_ratchet",
    "double_jump": "jump_double_hero_ratchet",
    "fall": "fall_loop_hero_ratchet",
    "glide": "fall_glide_hero_ratchet",
    "land": "neutral_jump_single_land_hero_ratchet",
    "dash": "dash_n_hero_ratchet",
    "air_dash": "dash_air_n_hero_ratchet",
}
CLIP_DIR = "characters\\hero\\hero_ratchet\\animations\\"


def add_animations(game, w, model):
    """Decodes the CLIPS from the install and adds them as named glTF animations."""
    import riftapart_anim

    g = w.gltf
    joints = model.dat1.get_section(0x15DF9D3B).joints
    node_of_hash = {j.hash: g.skins[0].joints[i] for i, j in enumerate(joints)}
    report = {}
    for state, clip_name in CLIPS.items():
        data = game.read(CLIP_DIR + clip_name + ".animclip")
        if data is None:
            print(f"  [!] clip missing: {clip_name}")
            continue
        asset = game.dat1lib.read(io.BytesIO(data))
        sections = {sec.TAG: sec._raw for sec in asset.dat1.sections}
        clip = riftapart_anim.decode(clip_name, sections)
        if clip is None:
            print(f"  [!] clip kind not supported: {clip_name}")
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


def main():
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True)
    ap.add_argument("--alert", required=True)
    ap.add_argument("--out", default="riftapart-assets")
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--look", type=int, default=0)
    ap.add_argument("--lod", type=int, default=0)
    ap.add_argument("--tex-size", type=int, default=2048)
    ap.add_argument("--no-anims", action="store_true", help="skip decoding animation clips")
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument("--gdeflate-dll", default=os.path.join(here, "..", "target", "release", "gdeflate_ffi.dll"),
                    help="build it with: cargo build -p gdeflate-ffi --release")
    a = ap.parse_args()

    game = Game(a.game, a.alert, a.gdeflate_dll)
    import model_to_gltf as m2g

    m2g.ADD_SHAPEKEYS = False
    model = game.dat1lib.read(io.BytesIO(game.read(a.model)))

    w = m2g.GltfWriter()
    w.init(model)
    # UV = raw int16 × the model's own UV scale (ModelBuilt section, e.g. 1/1024). ALERT skips
    # that section for Rift Apart and its vertex reader already divides by 32768, so redo it.
    built = model.dat1.get_section(m2g.SECTION_BUILT)
    w.uv_scale = (built.get_uv_scale() if built else 1.0 / 16384.0) * 32768.0
    all_meshes = w.get_meshes_indexes_by_looks_and_lod([a.look], a.lod)
    keep, mesh_material = [], {}
    for mi in all_meshes:
        path = w.get_material_path(w.meshes[mi].get_material()) or None
        if path is None or any(s in path.lower() for s in SKIP_MATERIALS):
            print(f"  skip mesh {mi} ({path})")
            continue
        if pick(material_textures(game, path), "c", path) is None:
            # Fur shells, stitching and small emissive bits have no colour map; they need
            # special shaders, so leave them out for now.
            print(f"  skip mesh {mi} (no colour map: {path})")
            continue
        keep.append(mi)
        mesh_material[f"sm{mi:02}"] = path
    skin, rcra_skin = w.get_skin()
    w.add_skeleton(model)
    w.add_meshes(keep, skin, rcra_skin)
    anims = add_animations(game, w, model) if not a.no_anims else {}
    g = w.gltf

    # Materials and textures.
    texture_cache = {}
    material_index = {}
    manifest = {"model": a.model, "look": a.look, "lod": a.lod, "materials": {}}

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
    out = os.path.join(a.out, "ratchet.glb")
    g.save_binary(out)
    # Edit the file directly: pygltflib drops JOINTS_1/WEIGHTS_1 when it reloads a file.
    merged = fix_glb_for_bevy(out)
    print(f"  merged 8 -> 4 bone influences on {merged} primitives")
    with open(os.path.join(a.out, "ratchet.json"), "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"wrote {out} ({os.path.getsize(out) / 1e6:.1f} MB), {len(g.meshes)} meshes, {len(g.materials)} materials, {len(g.textures)} textures")


if __name__ == "__main__":
    main()
