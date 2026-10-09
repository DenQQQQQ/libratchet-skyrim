# SPDX-License-Identifier: GPL-3.0-or-later
"""Converts Ratchet (riftapart-assets/ratchet.glb, made by tools/extract_riftapart_ratchet.py from
YOUR OWN Rift Apart install) into what the SKSE plugin draws in Skyrim SE:

  meshes/RatchetMovement/ratchet.nif                 his skinned meshes on his own skeleton
  textures/RatchetMovement/ratchet/*.dds             their textures
  SKSE/Plugins/RatchetMovement/ratchet.anim          his clips, for the plugin to play

The same for Clank (--name clank: clank.nif, clank.anim) and the weapons (the wrench's and the
pistol's with their clips; --no-clips for the Shatterbomb and its glove). A model with no skin
(the bolt pickups: bolt_<tier>_0<n>.glb) becomes rigid shapes holding their own vertices; models
sharing textures can share their folder (--texture-dir). For Skyrim's loading screens, --pose
bakes a clip's frame into the skeleton (he stands in it) and --loadscreen turns the model into
Skyrim's axes and units at its root, as the game's own loading-screen art is.

The output is game data: it stays on this PC (the build installs it into the MO2 mod), never in
the repository.

The model keeps glTF's space (metres, Y up, facing +Z); the plugin scales and turns it when it
attaches it to the player. Every node name gets the prefix RC_, so no lookup of Skyrim's own
nodes by name can land on his.

  uv run --with numpy --with pillow skse/tools/ratchet_to_skyrim.py --glb riftapart-assets/ratchet.glb --out <dir>
"""
import argparse
import base64
import io
import json
import math
import os
import re
import struct
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nif as N  # noqa: E402

# Set from --name and --prefix: Ratchet's are the defaults; his weapons get their own.
PREFIX = "RC_"
MESH_PATH = "RatchetMovement\\ratchet.nif"           # under Data\meshes
TEXTURE_DIR = "RatchetMovement\\ratchet"             # under Data\textures
ANIM_FILE = "ratchet.anim"                            # under Data\SKSE\Plugins\RatchetMovement
ROOT_NAME = "RatchetRoot"
MAX_BONES_PER_PARTITION = 60                          # Skyrim's shaders take up to 80
MAX_TEXTURE = 1024  # uncompressed for now: 4 MB each at 1024

# Every clip goes in, the wrench's and the guns' too (the plugin fights with them now). A pattern
# here would leave matching clips out.
SKIP_CLIPS = re.compile(r"^$")
# Set from --pose and --loadscreen: a clip's frame baked into the skeleton, and the root in
# Skyrim's axes (z up: glTF's (x, y, z) to (x, -z, y)) and game units (a metre is 1 / 0.0142875).
POSE = None  # (clip name, seconds into it)
LOADSCREEN = False
UNITS_PER_METRE = 1.0 / 0.0142875

COMPONENT = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
WIDTH = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


# --- glTF

class Gltf:
    def __init__(self, path):
        data = open(path, "rb").read()
        magic, _, _ = struct.unpack_from("<III", data, 0)
        if magic != 0x46546C67:
            raise ValueError(f"{path} isn't a GLB")
        off = 12
        self.json, self.bin = None, b""
        while off < len(data):
            n, kind = struct.unpack_from("<II", data, off)
            chunk = data[off + 8:off + 8 + n]
            if kind == 0x4E4F534A:
                self.json = json.loads(chunk)
            elif kind == 0x004E4942:
                self.bin = chunk
            off += 8 + n
        self.g = self.json

    def accessor(self, i):
        a = self.g["accessors"][i]
        view = self.g["bufferViews"][a["bufferView"]]
        dtype = np.dtype(COMPONENT[a["componentType"]])
        width = WIDTH[a["type"]]
        start = view.get("byteOffset", 0) + a.get("byteOffset", 0)
        stride = view.get("byteStride", 0)
        count = a["count"]
        if stride and stride != dtype.itemsize * width:
            rows = [np.frombuffer(self.bin, dtype, width, start + k * stride) for k in range(count)]
            out = np.array(rows)
        else:
            out = np.frombuffer(self.bin, dtype, count * width, start).reshape(count, width) if width > 1 else \
                np.frombuffer(self.bin, dtype, count, start)
        if a.get("normalized"):
            out = out.astype(np.float32) / np.iinfo(dtype).max
        return out

    def image(self, i):
        im = self.g["images"][i]
        if "uri" in im:
            head, b64 = im["uri"].split(",", 1)
            raw = base64.b64decode(b64)
        else:
            view = self.g["bufferViews"][im["bufferView"]]
            raw = self.bin[view.get("byteOffset", 0):view.get("byteOffset", 0) + view["byteLength"]]
        return Image.open(io.BytesIO(raw))


# --- maths

def quat_to_matrix(q):
    x, y, z, w = q
    return np.array([
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ], dtype=np.float64)


def node_local(node):
    """A glTF node's local (rotation 3x3, translation, uniform scale)."""
    if "matrix" in node:
        m = np.array(node["matrix"], dtype=np.float64).reshape(4, 4).T
        s = float(np.mean(np.linalg.norm(m[:3, :3], axis=0)))
        return m[:3, :3] / s, m[:3, 3], s
    r = quat_to_matrix(node.get("rotation", [0, 0, 0, 1]))
    t = np.array(node.get("translation", [0, 0, 0]), dtype=np.float64)
    s = float(np.mean(node.get("scale", [1, 1, 1])))
    return r, t, s


def transform(r, t, s):
    return {"rotation": [float(x) for x in r.reshape(9)], "translation": [float(x) for x in t], "scale": float(s)}


def bounding_sphere(points):
    if len(points) == 0:
        return [0.0, 0.0, 0.0, 0.0]
    lo, hi = points.min(axis=0), points.max(axis=0)
    c = (lo + hi) / 2
    r = float(np.max(np.linalg.norm(points - c, axis=1)))
    return [float(c[0]), float(c[1]), float(c[2]), r]


def tangent_space(pos, nrm, uv, tris):
    """Per-vertex tangent space the way Skyrim's tools write it: the "tangent" slot holds dP/dv
    and the "bitangent" slot dP/du (NifSkope's tangent space spell), both made orthogonal to the
    normal."""
    sdir = np.zeros_like(pos)
    tdir = np.zeros_like(pos)
    i0, i1, i2 = tris[:, 0], tris[:, 1], tris[:, 2]
    e1, e2 = pos[i1] - pos[i0], pos[i2] - pos[i0]
    d1, d2 = uv[i1] - uv[i0], uv[i2] - uv[i0]
    det = d1[:, 0] * d2[:, 1] - d2[:, 0] * d1[:, 1]
    r = np.where(np.abs(det) > 1e-12, 1.0 / np.where(det == 0, 1, det), 0.0)[:, None]
    s = (e1 * d2[:, 1:2] - e2 * d1[:, 1:2]) * r
    t = (e2 * d1[:, 0:1] - e1 * d2[:, 0:1]) * r
    for idx in (i0, i1, i2):
        np.add.at(sdir, idx, s)
        np.add.at(tdir, idx, t)

    def ortho(v):
        v = v - nrm * np.sum(nrm * v, axis=1, keepdims=True)
        n = np.linalg.norm(v, axis=1, keepdims=True)
        # Degenerate UVs: any vector across the normal.
        fallback = np.cross(nrm, np.where(np.abs(nrm[:, 0:1]) < 0.9, [[1.0, 0, 0]], [[0, 1.0, 0]]))
        return np.where(n > 1e-8, v / np.maximum(n, 1e-8), fallback / np.maximum(np.linalg.norm(fallback, axis=1, keepdims=True), 1e-8))

    return ortho(tdir), ortho(sdir)


def unorm8(v):
    return np.clip(np.round((v * 0.5 + 0.5) * 255), 0, 255).astype(np.uint8)


# --- DDS (uncompressed B8G8R8A8 with mipmaps)

def write_dds(path, img):
    img = img.convert("RGBA")
    w, h = img.size
    mips = []
    cur = img
    while True:
        r, g, b, a = cur.split()
        mips.append(Image.merge("RGBA", (b, g, r, a)).tobytes())
        if cur.size == (1, 1):
            break
        cur = cur.resize((max(1, cur.size[0] // 2), max(1, cur.size[1] // 2)), Image.LANCZOS)
    header = struct.pack("<4sIIIIIII44x", b"DDS ", 124, 0x1 | 0x2 | 0x4 | 0x1000 | 0x8 | 0x20000, h, w, w * 4, 0, len(mips))
    # DDS_PIXELFORMAT: RGB + alpha, 32 bits, masks for B8G8R8A8 in memory order.
    pf = struct.pack("<II4sIIIII", 32, 0x41, b"\0\0\0\0", 32, 0x00FF0000, 0x0000FF00, 0x000000FF, 0xFF000000)
    caps = struct.pack("<IIII4x", 0x1000 | 0x8 | 0x400000, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(header + pf + caps)
        for m in mips:
            f.write(m)


def fit(img):
    w, h = img.size
    k = max(w, h) / MAX_TEXTURE
    if k > 1:
        img = img.resize((max(1, round(w / k)), max(1, round(h / k))), Image.LANCZOS)
    return img


# --- the model

# A rigid shape's vertices (no skin: the shape holds them, not a skin partition): float32
# position, half-float UV, normal and tangent space, in Skyrim SE's layout (nif.xml
# BSVertexDataSSE: the bitangent's x after the position, its y and z in the normal's and tangent's
# fourth bytes).
RIGID_DESC = N.make_vertex_desc(N.VF_VERTEX | N.VF_UV | N.VF_NORMAL | N.VF_TANGENT | N.VF_FULLPREC)
RIGID_VERTEX = np.dtype([("pos", "<f4", 3), ("bitangent_x", "<f4"), ("uv", "<f2", 2), ("normal", "u1", 3), ("bitangent_y", "u1"),
                         ("tangent", "u1", 3), ("bitangent_z", "u1")])
assert RIGID_VERTEX.itemsize == N.vertex_size(RIGID_DESC)


def rigid_shape(nf, gl, node, ni, prim, shader, alpha):
    """A BSTriShape holding its own vertices and triangles, placed by its node's transform.
    Returns its block, and its vertex and triangle counts."""
    at = prim["attributes"]
    pos = gl.accessor(at["POSITION"]).astype(np.float64)
    nrm = gl.accessor(at["NORMAL"]).astype(np.float64)
    uv = gl.accessor(at["TEXCOORD_0"]).astype(np.float64) if "TEXCOORD_0" in at else np.zeros((len(pos), 2))
    tris = gl.accessor(prim["indices"]).astype(np.int64).reshape(-1, 3) if "indices" in prim else np.arange(len(pos)).reshape(-1, 3)
    nv, nt = len(pos), len(tris)
    if nv > 65535 or nt > 65535:
        raise ValueError(f"{node.get('name')}: too big for one shape ({nv} vertices, {nt} triangles)")
    nrm /= np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-12)
    tan, bit = tangent_space(pos, nrm, uv, tris)
    bb = unorm8(bit)
    verts = np.zeros(nv, RIGID_VERTEX)
    verts["pos"] = pos
    verts["bitangent_x"] = bit[:, 0]
    verts["uv"] = uv
    verts["normal"] = unorm8(nrm)
    verts["bitangent_y"] = bb[:, 1]
    verts["tangent"] = unorm8(tan)
    verts["bitangent_z"] = bb[:, 2]
    vertex_data = verts.tobytes()
    triangles = np.ascontiguousarray(tris, dtype="<u2").tobytes()
    r, t, s = node_local(node)
    shape = nf.add({
        "type": "BSTriShape", "name": nf.string(PREFIX + node.get("name", f"shape{ni}")), "extra_data": [], "controller": -1,
        "flags": 0xE, "translation": [float(x) for x in t], "rotation": [float(x) for x in r.reshape(9)], "scale": float(s),
        "collision": -1, "bound": bounding_sphere(pos), "skin": -1, "shader": shader, "alpha": alpha,
        "vertex_desc": RIGID_DESC, "num_triangles": nt, "num_vertices": nv,
        "data_size": len(vertex_data) + len(triangles), "vertex_data": vertex_data, "triangles": triangles, "particle_data_size": 0,
    })
    return shape, nv, nt


def clip_pose(gl, name, at):
    """Each joint's translation, rotation and scale in clip `name` `at` seconds in (the key at or
    before it), by node: what --pose bakes in."""
    clips = [a for a in gl.g.get("animations", []) if a.get("name") == name]
    if not clips:
        raise ValueError(f"no clip {name!r} (it has {', '.join(a.get('name', '?') for a in gl.g.get('animations', []))})")
    pose = {}
    for ch in clips[0]["channels"]:
        s = clips[0]["samplers"][ch["sampler"]]
        times = gl.accessor(s["input"])
        vals = gl.accessor(s["output"]).astype(np.float64)
        k = min(max(int(np.searchsorted(times, at, side="right")) - 1, 0), len(times) - 1)
        pose.setdefault(ch["target"]["node"], {})[ch["target"]["path"]] = [float(x) for x in vals[k]]
    return pose


def convert_model(gl, out_dir, manifest):
    g = gl.g
    nodes = g["nodes"]
    if POSE:
        # He stands in that frame: its keys over the rest pose (no matrix left to override them).
        pose = clip_pose(gl, *POSE)
        nodes = [dict(n) for n in nodes]
        for i, keys in pose.items():
            nodes[i].pop("matrix", None)
            nodes[i].update(keys)
    # No skin: rigid shapes (the bolt pickups).
    skinned = bool(g.get("skins"))
    if skinned:
        skin = g["skins"][0]
        joints = skin["joints"]
        ibms = gl.accessor(skin["inverseBindMatrices"]).reshape(-1, 4, 4).transpose(0, 2, 1).astype(np.float64)
    parent = {}
    for i, n in enumerate(nodes):
        for c in n.get("children", []):
            parent[c] = i
    scene_roots = g["scenes"][g.get("scene", 0)]["nodes"]
    rig_roots = [r for r in scene_roots if "mesh" not in nodes[r]]

    nf = N.Nif()
    nf.process_script = "riftapart-skyrim ratchet_to_skyrim.py"

    def node_block(name, r, t, s):
        return {"type": "NiNode", "name": nf.string(name), "extra_data": [], "controller": -1, "flags": 0xE,
                "translation": [float(x) for x in t], "rotation": [float(x) for x in r.reshape(9)], "scale": float(s),
                "collision": -1, "children": [], "effects": []}

    root = nf.add(node_block(ROOT_NAME, np.eye(3), np.zeros(3), 1.0))
    nf.blocks[root]["flags"] = 0x8000E
    # What the model hangs under: the root, or for a loading screen a node turning it into
    # Skyrim's axes and units ((x, y, z) to (x, -z, y), a metre in game units). Not the root
    # itself: the loading screen sets the root's own place, turn and size.
    holder = root
    if LOADSCREEN:
        up = np.array([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]])
        holder = nf.add(node_block(ROOT_NAME + "_Skyrim", up, np.zeros(3), UNITS_PER_METRE))
        nf.blocks[root]["children"].append(holder)
    block_of_node = {}

    def add_tree(i, parent_block):
        r, t, s = node_local(nodes[i])
        b = nf.add(node_block(PREFIX + nodes[i].get("name", f"node{i}"), r, t, s))
        block_of_node[i] = b
        nf.blocks[parent_block]["children"].append(b)
        for c in nodes[i].get("children", []):
            if "mesh" not in nodes[c]:
                add_tree(c, b)

    for r in rig_roots:
        add_tree(r, holder)

    # Materials: textures once each.
    tex_dir = os.path.join(out_dir, "textures", *TEXTURE_DIR.split("\\"))
    os.makedirs(tex_dir, exist_ok=True)
    written = {}

    def texture(index, kind, extra=None):
        key = (index, kind, extra)
        if key in written:
            return written[key]
        src = g["textures"][index]["source"]
        # The image's index first: some of his textures differ only in case (pants_c and Pants_c),
        # and Windows and Skyrim's paths don't tell those apart.
        name = f"{src:02}_" + re.sub(r"[^A-Za-z0-9_]+", "_", os.path.splitext(g["images"][src].get("name", f"image{src}"))[0])
        img = fit(gl.image(src)).convert("RGBA")
        if kind == "color" and extra is not None:
            # Times the material's colour factor (one file per factor).
            a = np.asarray(img).astype(np.float64) * np.array(extra)[None, None, :]
            img = Image.fromarray(np.clip(np.round(a), 0, 255).astype(np.uint8), "RGBA")
            name += "_x" + "".join(f"{round(c * 255):02x}" for c in extra)
        if kind == "normal":
            a = np.asarray(img).copy()
            a[..., 1] = 255 - a[..., 1]  # glTF's normal maps are OpenGL-style; Skyrim's are DirectX-style
            if extra is not None:
                gloss = fit(gl.image(g["textures"][extra]["source"])).convert("RGBA").resize(img.size, Image.LANCZOS)
                # Specular mask in the alpha: the gloss (the glb holds roughness in green).
                a[..., 3] = 255 - np.asarray(gloss)[..., 1]
            else:
                a[..., 3] = 96
            img = Image.fromarray(a, "RGBA")
            # Its specular mask is part of it: one file per normal + gloss pair.
            name += f"_s{g['textures'][extra]['source']:02}" if extra is not None else "_n"
        if kind == "glow":
            name += "_glow"
        file = name + ".dds"
        write_dds(os.path.join(tex_dir, file), img)
        written[key] = "textures\\" + TEXTURE_DIR + "\\" + file
        return written[key]

    def solid(rgba):
        """A small texture of one colour (materials with a colour factor and no texture: Clank's
        glass and glows, the wrench's glow strip)."""
        px = tuple(int(np.clip(round(c * 255), 0, 255)) for c in rgba)
        key = ("solid", px)
        if key not in written:
            file = "solid_" + "".join(f"{c:02x}" for c in px) + ".dds"
            write_dds(os.path.join(tex_dir, file), Image.new("RGBA", (4, 4), px))
            written[key] = "textures\\" + TEXTURE_DIR + "\\" + file
        return written[key]

    def gamma(rgb):
        # glTF's colour factors are linear; Skyrim's colours and textures are gamma (sRGB).
        return [float(c) ** (1 / 2.2) for c in rgb]

    # Every shape gets its own shader, texture set and alpha blocks: a shader property holds its
    # shape's render passes, and one shared by two shapes makes Skyrim's renderer loop forever
    # (found in game: 13 million bone-buffer maps in one frame with both gloves drawn).
    def material(mi):
        m = g["materials"][mi]
        pbr = m.get("pbrMetallicRoughness", {})
        factor = [float(x) for x in pbr.get("baseColorFactor", [1, 1, 1, 1])]
        factor = gamma(factor[:3]) + factor[3:]
        if "baseColorTexture" in pbr:
            plain = all(abs(c - 1.0) < 1e-6 for c in factor)
            diffuse = texture(pbr["baseColorTexture"]["index"], "color", None if plain else tuple(factor))
        else:
            diffuse = solid(factor)
        normal = ""
        if "normalTexture" in m:
            normal = texture(m["normalTexture"]["index"], "normal", pbr.get("metallicRoughnessTexture", {}).get("index"))
        emissive = gamma(m.get("emissiveFactor", [0, 0, 0]))
        glows = max(emissive) > 1e-3
        # An emissive texture glows where it's bright, times the factor (glTF's): Skyrim's glow
        # shader with it as the glow map (a bolt's little green light), not the whole shape.
        glow_map = texture(m["emissiveTexture"]["index"], "glow") if glows and "emissiveTexture" in m else ""
        tex_set = nf.add({"type": "BSShaderTextureSet", "textures": [diffuse, normal, glow_map, "", "", "", "", "", ""]})
        mode = m.get("alphaMode", "OPAQUE")
        flags1 = 0x1 | 0x100 | 0x200 | 0x2000000 | 0x80000000         # specular, shadows, remappable, z test
        flags1 |= 0x2 if skinned else 0                               # skinned
        flags1 |= 0x400000 if glows else 0                            # own emit
        flags2 = 0x8000 | (0x10 if m.get("doubleSided", True) else 0)  # envmap light fade, two sided
        flags2 |= 0 if mode == "BLEND" else 0x1                        # z write, but not see-through ones
        flags2 |= 0x40 if glow_map else 0                              # glow map
        shader = nf.add({
            "type": "BSLightingShaderProperty", "shader_type": 2 if glow_map else 0, "name": nf.string(m.get("name", "")), "extra_data": [], "controller": -1,
            "flags1": flags1, "flags2": flags2, "uv_offset": [0.0, 0.0], "uv_scale": [1.0, 1.0], "texture_set": tex_set,
            "emissive_color": emissive if glows else [0.0, 0.0, 0.0], "emissive_multiple": 1.0, "texture_clamp_mode": 3, "alpha": 1.0,
            "refraction_strength": 0.0, "glossiness": 40.0, "specular_color": [1.0, 1.0, 1.0], "specular_strength": 0.6,
            "lighting_effect1": 0.3, "lighting_effect2": 2.0, "type_extra": [],
        })
        alpha = -1
        if mode == "MASK":
            # Alpha test: drawn where alpha > the cutoff.
            alpha = nf.add({"type": "NiAlphaProperty", "name": -1, "extra_data": [], "controller": -1,
                            "flags": 0x12EC, "threshold": int(255 * m.get("alphaCutoff", 0.4))})
        elif mode == "BLEND":
            # Blended by its alpha (Clank's glass, his rotor blades), fully clear pixels dropped.
            alpha = nf.add({"type": "NiAlphaProperty", "name": -1, "extra_data": [], "controller": -1,
                            "flags": 0x12ED, "threshold": 0})
        return shader, alpha

    attrs = N.VF_VERTEX | N.VF_UV | N.VF_NORMAL | N.VF_TANGENT | N.VF_SKINNED
    desc = N.make_vertex_desc(attrs)
    vsize = N.vertex_size(desc)
    stats = []
    for ni in scene_roots:
        node = nodes[ni]
        if "mesh" not in node:
            continue
        for prim in g["meshes"][node["mesh"]]["primitives"]:
            if not skinned:
                shader, alpha = material(prim["material"]) if "material" in prim else (-1, -1)
                shape, nv, nt = rigid_shape(nf, gl, node, ni, prim, shader, alpha)
                nf.blocks[holder]["children"].append(shape)
                stats.append((PREFIX + node.get("name", f"shape{ni}"), nv, nt, 0, 0))
                continue
            at = prim["attributes"]
            pos = gl.accessor(at["POSITION"]).astype(np.float64)
            nrm = gl.accessor(at["NORMAL"]).astype(np.float64)
            uv = gl.accessor(at["TEXCOORD_0"]).astype(np.float64)
            jnt = gl.accessor(at["JOINTS_0"]).astype(np.int64)
            wts = gl.accessor(at["WEIGHTS_0"]).astype(np.float64)
            tris = gl.accessor(prim["indices"]).astype(np.int64).reshape(-1, 3)
            nv = len(pos)
            if nv > 65535 or len(tris) > 65535:
                raise ValueError(f"{node.get('name')}: too big for one shape ({nv} vertices, {len(tris)} triangles)")
            nrm /= np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-12)
            tan, bit = tangent_space(pos, nrm, uv, tris)
            # Weights: the four biggest, normalised; unused slots point at bone 0 with weight 0.
            wts = np.where(wts > 1e-4, wts, 0)
            wsum = wts.sum(axis=1, keepdims=True)
            wts = np.where(wsum > 0, wts / np.where(wsum > 0, wsum, 1), np.array([[1.0, 0, 0, 0]]))
            used = sorted(set(jnt[wts > 0].tolist()))
            bone_of_joint = {j: k for k, j in enumerate(used)}
            local_idx = np.vectorize(lambda j: bone_of_joint.get(j, 0))(jnt)
            local_idx = np.where(wts > 0, local_idx, 0)

            # Packed vertices (BSVertexDataSSE).
            buf = bytearray(nv * vsize)
            half_uv = uv.astype(np.float16)
            nb, tb = unorm8(nrm), unorm8(tan)
            bb = unorm8(bit)
            hw = wts.astype(np.float16)
            for v in range(nv):
                o = v * vsize
                struct.pack_into("<3ff", buf, o, pos[v, 0], pos[v, 1], pos[v, 2], float(bit[v, 0]))
                struct.pack_into("<2e", buf, o + 16, float(half_uv[v, 0]), float(half_uv[v, 1]))
                struct.pack_into("<4B", buf, o + 20, nb[v, 0], nb[v, 1], nb[v, 2], bb[v, 1])
                struct.pack_into("<4B", buf, o + 24, tb[v, 0], tb[v, 1], tb[v, 2], bb[v, 2])
                struct.pack_into("<4e4B", buf, o + 28, *[float(x) for x in hw[v]], *[int(x) for x in local_idx[v]])

            # Skin data: each bone's inverse bind and its vertices.
            bones_data = []
            for k, j in enumerate(used):
                ibm = ibms[j]
                s = float(np.mean(np.linalg.norm(ibm[:3, :3], axis=0)))
                verts = np.nonzero(np.any((jnt == j) & (wts > 0), axis=1))[0]
                weights = []
                for v in verts:
                    weights.append((int(v), float(np.sum(wts[v][jnt[v] == j]))))
                pts = (ibm[:3, :3] @ pos[verts].T).T + ibm[:3, 3] if len(verts) else np.zeros((0, 3))
                bones_data.append({"transform": transform(ibm[:3, :3] / s, ibm[:3, 3], s), "bound": bounding_sphere(pts),
                                   "weights": weights, "num_vertices": len(weights)})
            skin_data = nf.add({"type": "NiSkinData", "transform": transform(np.eye(3), np.zeros(3), 1.0),
                                "has_vertex_weights": 1, "bones": bones_data})

            # Partitions: triangles go to the first partition whose bone set stays small enough.
            parts = []
            for tri in tris:
                tb_set = set()
                for v in tri:
                    tb_set.update(local_idx[v][wts[v] > 0].tolist())
                for p in parts:
                    if len(p["bones"] | tb_set) <= MAX_BONES_PER_PARTITION:
                        p["bones"] |= tb_set
                        p["tris"].append(tri)
                        break
                else:
                    parts.append({"bones": set(tb_set), "tris": [tri]})
            partitions = []
            for p in parts:
                pbones = sorted(p["bones"])
                pidx = {b: k for k, b in enumerate(pbones)}
                ptris = np.array(p["tris"], dtype=np.int64)
                vmap = sorted(set(ptris.reshape(-1).tolist()))
                vweights, bidx = [], []
                for v in vmap:
                    vweights.extend(float(x) for x in hw[v].astype(np.float32))
                    bidx.extend(pidx.get(int(b), 0) if w > 0 else 0 for b, w in zip(local_idx[v], wts[v]))
                flat = [int(x) for x in ptris.reshape(-1)]
                partitions.append({
                    "num_vertices": len(vmap), "num_triangles": len(ptris), "num_weights_per_vertex": 4, "bones": pbones,
                    "has_vertex_map": 1, "vertex_map": vmap, "has_vertex_weights": 1, "vertex_weights": vweights,
                    "strip_lengths": [], "has_faces": 1, "triangles": flat, "has_bone_indices": 1, "bone_indices": bidx,
                    "lod_level": 0, "global_vb": 0, "vertex_desc": desc, "triangles_copy": flat,
                })
            partition = nf.add({"type": "NiSkinPartition", "data_size": len(buf), "vertex_size": vsize, "vertex_desc": desc,
                                "vertex_data": bytes(buf), "partitions": partitions})
            skin_inst = nf.add({"type": "BSDismemberSkinInstance", "data": skin_data, "partition": partition, "skeleton_root": holder,
                                "bones": [block_of_node[joints[j]] for j in used], "body_parts": [(257, 32)] * len(partitions)})
            shader, alpha = material(prim["material"]) if "material" in prim else (-1, -1)
            # A part the plugin shows only at times (Clank's heli-pack: "part": "heli") is in the
            # shape's name: RK_heli_sm119.
            part = node.get("extras", {}).get("part")
            shape_name = PREFIX + (f"{part}_" if part else "") + node.get("name", f"shape{ni}")
            shape = nf.add({
                "type": "BSTriShape", "name": nf.string(shape_name), "extra_data": [], "controller": -1,
                "flags": 0xE, "translation": [0.0, 0.0, 0.0], "rotation": [1.0, 0, 0, 0, 1.0, 0, 0, 0, 1.0], "scale": 1.0,
                # The bind pose's sphere, grown: animation swings arms and tail out of it.
                "collision": -1, "bound": bounding_sphere(pos)[:3] + [bounding_sphere(pos)[3] * 1.5], "skin": skin_inst,
                "shader": shader, "alpha": alpha,
                "vertex_desc": desc, "num_triangles": 0, "num_vertices": 0, "data_size": 0, "particle_data_size": 0,
            })
            nf.blocks[holder]["children"].append(shape)
            stats.append((shape_name, nv, len(tris), len(used), len(partitions)))

    mesh_dir = os.path.join(out_dir, "meshes", *MESH_PATH.split("\\")[:-1])
    os.makedirs(mesh_dir, exist_ok=True)
    out = nf.write()
    path = os.path.join(out_dir, "meshes", *MESH_PATH.split("\\"))
    with open(path, "wb") as f:
        f.write(out)
    # The writer must read its own file back exactly.
    again = N.Nif.read(out)
    assert again.write() == out, "the NIF doesn't read back the same"
    print(f"{path}: {len(nf.blocks)} blocks, {len(out) / 1e6:.1f} MB")
    for s in stats:
        print(f"  {s[0]}: {s[1]} vertices, {s[2]} triangles, {s[3]} bones, {s[4]} partitions")
    print(f"  {len(written)} textures in {tex_dir}")
    return block_of_node


# --- the clips

def convert_clips(gl, out_dir, manifest):
    """ratchet.anim (clank.anim): the skeleton (names, parents, rest pose) and every clip at its keys.

    Layout (little-endian): "RANM", u32 version 1, u32 joints, per joint (u16 name length, name,
    i32 parent, rest translation 3f, rotation 4f xyzw, scale f), u32 clips, per clip (u16 name
    length, name, f32 seconds, f32 frames per second, u32 frames, f32 root distance, u8 additive,
    u32 tracks, per track (u16 joint, u8 path 0 translation / 1 rotation / 2 scale, u8 constant,
    values: constant ? one : frames)). Values are 3f, 4f or 1f (uniform scale).
    """
    g = gl.g
    nodes = g["nodes"]
    parent = {}
    for i, n in enumerate(nodes):
        for c in n.get("children", []):
            parent[c] = i
    rig = [i for i, n in enumerate(nodes) if "mesh" not in n]
    index = {n: k for k, n in enumerate(rig)}
    w = N.Writer()
    w.raw(b"RANM")
    w.u32(1)
    w.u32(len(rig))
    for i in rig:
        n = nodes[i]
        name = (PREFIX + n.get("name", f"node{i}")).encode("latin-1")
        w.u16(len(name))
        w.raw(name)
        w.i32(index.get(parent.get(i, -1), -1))
        w.pack("3f", *n.get("translation", [0, 0, 0]))
        w.pack("4f", *n.get("rotation", [0, 0, 0, 1]))
        w.f32(float(np.mean(n.get("scale", [1, 1, 1]))))
    info = manifest.get("animations", {}) if manifest else {}
    clips = [a for a in g["animations"] if not SKIP_CLIPS.match(a.get("name", ""))]
    w.u32(len(clips))
    total = 0
    for a in clips:
        name = a.get("name", "")
        tracks = []
        frames = 0
        seconds = 0.0
        for ch in a["channels"]:
            node = ch["target"]["node"]
            if node not in index:
                continue
            s = a["samplers"][ch["sampler"]]
            times = gl.accessor(s["input"])
            vals = gl.accessor(s["output"]).astype(np.float32)
            frames = max(frames, len(times))
            seconds = max(seconds, float(times[-1]) if len(times) else 0.0)
            path = {"translation": 0, "rotation": 1, "scale": 2}[ch["target"]["path"]]
            if path == 2:
                vals = vals.mean(axis=1, keepdims=True)
            tracks.append((index[node], path, times, vals))
        rate = (frames - 1) / seconds if seconds > 0 else 30.0
        meta = info.get(name, {})
        nm = name.encode("latin-1")
        w.u16(len(nm))
        w.raw(nm)
        w.f32(seconds)
        w.f32(rate)
        w.u32(frames)
        w.f32(float(meta.get("root_distance", 0.0)))
        w.u8(1 if name.endswith("_additive") or meta.get("additive") else 0)
        w.u32(len(tracks))
        for joint, path, times, vals in tracks:
            if len(times) != frames:
                # Resample onto the clip's frames.
                grid = np.linspace(0, seconds, frames)
                vals = np.stack([np.interp(grid, times, vals[:, c]) for c in range(vals.shape[1])], axis=1).astype(np.float32)
            const = bool(np.all(np.abs(vals - vals[0]) < 1e-5))
            w.u16(joint)
            w.u8(path)
            w.u8(1 if const else 0)
            data = vals[:1] if const else vals
            w.raw(data.astype("<f4").tobytes())
            total += len(data)
    out = w.bytes()
    path = os.path.join(out_dir, "SKSE", "Plugins", "RatchetMovement", ANIM_FILE)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(out)
    print(f"{path}: {len(rig)} joints, {len(clips)} clips, {total} keys, {len(out) / 1e6:.1f} MB")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--glb", required=True)
    ap.add_argument("--json", help="the extractor's ratchet.json (clip speeds); default: next to the glb")
    ap.add_argument("--out", required=True, help="folder laid out like Skyrim's Data folder")
    ap.add_argument("--name", default="ratchet", help="meshes\\RatchetMovement\\<name>.nif, textures\\RatchetMovement\\<name>, <name>.anim")
    ap.add_argument("--prefix", default="RC_", help="put before every node name (keeps the models' nodes apart in one scene)")
    ap.add_argument("--no-clips", action="store_true", help="the model only (the Shatterbomb and its glove have no clips of their own)")
    ap.add_argument("--texture-dir", help="textures\\<this> instead of textures\\RatchetMovement\\<name> (models sharing their textures: the bolts)")
    ap.add_argument("--pose", help="CLIP[@SECONDS]: that clip's frame baked into the skeleton (a loading screen's still)")
    ap.add_argument("--loadscreen", action="store_true", help="the root in Skyrim's axes and game units, as loading-screen art is")
    a = ap.parse_args()
    global PREFIX, MESH_PATH, TEXTURE_DIR, ANIM_FILE, ROOT_NAME, POSE, LOADSCREEN
    PREFIX = a.prefix
    if a.pose:
        clip, _, at = a.pose.partition("@")
        POSE = (clip, float(at) if at else 0.0)
    LOADSCREEN = a.loadscreen
    MESH_PATH = f"RatchetMovement\\{a.name}.nif"
    TEXTURE_DIR = a.texture_dir or f"RatchetMovement\\{a.name}"
    ANIM_FILE = f"{a.name}.anim"
    ROOT_NAME = "RatchetRoot" if a.name == "ratchet" else f"RatchetRoot_{a.name}"
    manifest_path = a.json or os.path.splitext(a.glb)[0] + ".json"
    manifest = json.load(open(manifest_path)) if os.path.exists(manifest_path) else None
    gl = Gltf(a.glb)
    convert_model(gl, a.out, manifest)
    if not a.no_clips:
        convert_clips(gl, a.out, manifest)


if __name__ == "__main__":
    main()
