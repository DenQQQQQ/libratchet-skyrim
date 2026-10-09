# SPDX-License-Identifier: GPL-3.0-or-later
"""Converts the weapon effects' art (riftapart-assets/vfx, made by tools/extract_riftapart_vfx.py from
YOUR OWN Rift Apart install) into the templates the SKSE plugin builds its effect pieces from:

  textures/RatchetMovement/fx/<texture>.dds             every texture, and white.dds
  meshes/RatchetMovement/fx/quad_<texture>_add.nif      a unit quad per texture (and white), added
  meshes/RatchetMovement/fx/quad_<texture>_blend.nif    ... and blended
  meshes/RatchetMovement/fx/<mesh>_<i>.nif               the effect meshes, as the sandbox shades them
  meshes/RatchetMovement/fx/crosshair_blend.nif         the sandbox's "+" (made here), drawn over everything
  SKSE/Plugins/RatchetMovement/fx.ini                   the colour palette ([Palette] name=r,g,b)

The output is game data: it stays on this PC (the build installs it into the MO2 mod), never in
the repository.

Every NIF is a root NiNode named after its file with one static BSTriShape "fx" (opaque white
vertex colours), shaded by a BSEffectShaderProperty with the texture and a NiAlphaProperty that
adds or blends it. The meshes keep the glTF's own space (metres, y up) and the quad is the
sandbox's (Bevy's Rectangle::new(1.0, 1.0): the XY plane, facing +Z): the plugin turns and scales
every piece.

  uv run --with numpy --with pillow skse/tools/ratchet_vfx_to_skyrim.py [--vfx DIR] [--out DIR]
"""
import argparse
import glob
import json
import os
import shutil
import subprocess
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nif as N  # noqa: E402
import ratchet_to_skyrim as R  # noqa: E402

REPO = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
MESH_DIR = "RatchetMovement\\fx"      # under Data\meshes
TEXTURE_DIR = "RatchetMovement\\fx"   # under Data\textures
INI_FILE = "fx.ini"                    # under Data\SKSE\Plugins\RatchetMovement

# The sandbox's effect meshes (apps/sandbox/src/vfx.rs: MESHES, each loaded as the glTF's mesh i,
# primitive 0), shaded as it shades them: (glb, mesh, NIF, add or blend, texture, writes depth).
MESHES = [
    ("muzzle_flash_mesh", 0, "muzzle_flash_mesh_0", "blend", "muzzle_flash", False),
    ("tracer_mesh", 0, "tracer_mesh_0", "blend", "tracer", False),
    ("projectile_shape", 0, "projectile_shape_0", "add", "glow_simple", False),
    # The dome: added for the Hyper-Strike's ring, blended for the Shatterbomb's blast.
    ("shockwave_hemisphere", 0, "shockwave_hemisphere_0_add", "add", "shockwave_mask", False),
    ("shockwave_hemisphere", 0, "shockwave_hemisphere_0_blend", "blend", "shockwave_mask", False),
    *[("impact_blast", i, f"impact_blast_{i}", "add", "white", False) for i in range(4)],
    *[("shatter_shards", i, f"shatter_shards_{i}", "blend", "white", False) for i in range(5)],
    # The Burst Pistol's shell: the glass, the bolt's energy inside it, and the metal ends, which
    # are opaque (they stay at full alpha and write depth, so the glass sorts against them).
    ("shell", 0, "shell_0", "blend", "white", False),
    ("shell", 1, "shell_1", "add", "white", False),
    ("shell", 2, "shell_2", "blend", "white", True),
]

# BSEffectShaderProperty: Skyrim's shader flags 1 and 2.
SLSF1_ZBUFFER_TEST = 0x80000000
SLSF2_ZBUFFER_WRITE = 0x1
SLSF2_DOUBLE_SIDED = 0x10
SLSF2_VERTEX_COLORS = 0x20
TEXTURE_WRAP_S_WRAP_T = 3
# NiAlphaProperty: blending on, source SRC_ALPHA, destination ONE (added light); or destination
# INV_SRC_ALPHA with the alpha test (greater than the threshold) on as well (blended).
ALPHA_FLAGS = {"add": 0x100D, "blend": 0x12ED}

# Static vertices: float32 position, half-float UV, normal, tangent space, vertex colour, in Skyrim
# SE's layout (nif.xml BSVertexDataSSE: the bitangent's x after the position, its y and z in the
# normal's and tangent's fourth bytes). The effect shader doesn't use the tangents, but Skyrim's
# shader package (Skyrim - Shaders.bsa) has its plain effect permutations (no falloff, lighting or
# membrane) only without the Normals bit or with Normals and BinormalTangent both, never Normals
# alone. Assumption: the game sets those bits from the shape's vertices, so normals go with
# tangents here, as on the game's own effect shapes (this layout, 0x3b, is 5328 of its 6633; the
# only ones with UVs and normals but no tangents are 13 untextured helper meshes).
VERTEX_ATTRS = N.VF_VERTEX | N.VF_UV | N.VF_NORMAL | N.VF_TANGENT | N.VF_COLORS | N.VF_FULLPREC
VERTEX_DESC = N.make_vertex_desc(VERTEX_ATTRS)
VERTEX = np.dtype([("pos", "<f4", 3), ("bitangent_x", "<f4"), ("uv", "<f2", 2), ("normal", "u1", 3), ("bitangent_y", "u1"),
                   ("tangent", "u1", 3), ("bitangent_z", "u1"), ("color", "u1", 4)])
assert VERTEX.itemsize == N.vertex_size(VERTEX_DESC)
assert [VERTEX.fields[f][1] for f in ("uv", "normal", "tangent", "color")] == [((VERTEX_DESC >> s) & 0xF) * 4 for s in (8, 16, 20, 24)]

IDENTITY = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]

# Bevy's Rectangle::new(1.0, 1.0), as the sandbox draws its sprites.
QUAD_POS = np.array([[0.5, 0.5, 0.0], [-0.5, 0.5, 0.0], [-0.5, -0.5, 0.0], [0.5, -0.5, 0.0]])
QUAD_UV = np.array([[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]])
QUAD_NORMAL = np.array([[0.0, 0.0, 1.0]] * 4)
QUAD_TRIS = np.array([[0, 1, 2], [0, 2, 3]])


def default_vfx_dir():
    """riftapart-assets/vfx in this checkout, else in the main checkout (when this is a worktree)."""
    here = os.path.join(REPO, "riftapart-assets", "vfx")
    if os.path.isdir(here):
        return here
    try:
        common = subprocess.run(["git", "-C", REPO, "rev-parse", "--path-format=absolute", "--git-common-dir"],
                                capture_output=True, text=True, check=True).stdout.strip()
        main = os.path.join(os.path.dirname(common), "riftapart-assets", "vfx")
        if os.path.isdir(main):
            return main
    except (OSError, subprocess.CalledProcessError):
        pass
    # A worktree under .claude/worktrees/<name>.
    return os.path.normpath(os.path.join(REPO, "..", "..", "..", "riftapart-assets", "vfx"))


# --- shapes

def write_shape_nif(path, name, pos, normal, uv, tris, texture, blend, zwrite, ztest=True):
    """A NIF of one static shape: root NiNode `name`, BSTriShape "fx", its shader and alpha
    (ztest off: drawn over everything, the crosshair)."""
    nv, nt = len(pos), len(tris)
    if nv > 65535 or nt > 65535:
        raise ValueError(f"{name}: too big for one shape ({nv} vertices, {nt} triangles)")
    tangent, bitangent = R.tangent_space(pos, normal, uv, tris)
    bb = R.unorm8(bitangent)
    verts = np.zeros(nv, VERTEX)
    verts["pos"] = pos
    verts["bitangent_x"] = bitangent[:, 0]
    verts["uv"] = uv
    verts["normal"] = R.unorm8(normal)
    verts["bitangent_y"] = bb[:, 1]
    verts["tangent"] = R.unorm8(tangent)
    verts["bitangent_z"] = bb[:, 2]
    verts["color"] = 255
    vertex_data = verts.tobytes()
    triangles = np.ascontiguousarray(tris, dtype="<u2").tobytes()

    nf = N.Nif()
    nf.process_script = "riftapart-skyrim ratchet_vfx_to_skyrim.py"
    root = nf.add({"type": "NiNode", "name": nf.string(name), "extra_data": [], "controller": -1, "flags": 0xE,
                   "translation": [0.0, 0.0, 0.0], "rotation": IDENTITY, "scale": 1.0, "collision": -1, "children": [], "effects": []})
    shape = nf.add({
        "type": "BSTriShape", "name": nf.string("fx"), "extra_data": [], "controller": -1, "flags": 0xE,
        "translation": [0.0, 0.0, 0.0], "rotation": IDENTITY, "scale": 1.0, "collision": -1,
        "bound": R.bounding_sphere(pos), "skin": -1, "shader": -1, "alpha": -1,
        "vertex_desc": VERTEX_DESC, "num_triangles": nt, "num_vertices": nv,
        "data_size": len(vertex_data) + len(triangles), "vertex_data": vertex_data, "triangles": triangles,
        "particle_data_size": 0,
    })
    nf.blocks[root]["children"].append(shape)
    nf.blocks[shape]["shader"] = nf.add({
        "type": "BSEffectShaderProperty", "name": -1, "extra_data": [], "controller": -1,
        "flags1": SLSF1_ZBUFFER_TEST if ztest else 0,
        "flags2": SLSF2_DOUBLE_SIDED | SLSF2_VERTEX_COLORS | (SLSF2_ZBUFFER_WRITE if zwrite else 0),
        "uv_offset": [0.0, 0.0], "uv_scale": [1.0, 1.0],
        "source_texture": f"textures\\{TEXTURE_DIR}\\{texture}.dds",
        "texture_clamp_mode": TEXTURE_WRAP_S_WRAP_T, "lighting_influence": 0, "env_map_min_lod": 0, "unused_byte": 0,
        # Falloff off: full opacity at every angle.
        "falloff_start_angle": 1.0, "falloff_stop_angle": 1.0, "falloff_start_opacity": 1.0, "falloff_stop_opacity": 1.0,
        "base_color": [1.0, 1.0, 1.0, 1.0], "base_color_scale": 1.0, "soft_falloff_depth": 0.0, "greyscale_texture": "",
    })
    nf.blocks[shape]["alpha"] = nf.add({"type": "NiAlphaProperty", "name": -1, "extra_data": [], "controller": -1,
                                        "flags": ALPHA_FLAGS[blend], "threshold": 0})
    out = nf.write()
    # The writer must read its own file back exactly.
    assert N.Nif.read(out).write() == out, f"{name}: the NIF doesn't read back the same"
    with open(path, "wb") as f:
        f.write(out)
    return nv, nt


def crosshair_image(size=64):
    """A "+" 44/64 of the image across, its arms 6/64 thick, with its shadow 4/64 down and right:
    drawn 4 times larger and scaled down (smooth edges)."""
    k = 4
    n = size * k
    a = np.zeros((n, n, 4), np.float64)

    def plus(dx, dy, rgb, alpha):
        c = n // 2
        half, arm = 22 * k, 3 * k
        m = np.zeros((n, n), bool)
        m[c - arm + dy:c + arm + dy, c - half + dx:c + half + dx] = True
        m[c - half + dy:c + half + dy, c - arm + dx:c + arm + dx] = True
        # Over what's there (straight alpha).
        out_a = alpha + a[..., 3] * (1 - alpha)
        for i in range(3):
            a[..., i] = np.where(m, (rgb[i] * alpha + a[..., i] * a[..., 3] * (1 - alpha)) / np.maximum(out_a, 1e-9), a[..., i])
        a[..., 3] = np.where(m, out_a, a[..., 3])

    plus(4 * k, 4 * k, (0.0, 0.0, 0.0), 0.8)
    plus(0, 0, (1.0, 1.0, 1.0), 0.9)
    big = Image.fromarray(np.round(a * 255).astype(np.uint8), "RGBA")
    return big.resize((size, size), Image.LANCZOS)


def glb_primitive(gl, mesh, path):
    """Mesh `mesh`'s primitive 0, as the sandbox loads it: positions, normals, UVs, triangles."""
    meshes = gl.g.get("meshes", [])
    if mesh >= len(meshes) or not meshes[mesh].get("primitives"):
        raise ValueError(f"{path} has no mesh {mesh} with a primitive (it has {len(meshes)} meshes): the extraction changed")
    prim = meshes[mesh]["primitives"][0]
    if prim.get("mode", 4) != 4:
        raise ValueError(f"{path} mesh {mesh}: not a triangle list (mode {prim['mode']})")
    at = prim["attributes"]
    pos = gl.accessor(at["POSITION"]).astype(np.float64)
    tris = gl.accessor(prim["indices"]).astype(np.int64).reshape(-1, 3) if "indices" in prim else np.arange(len(pos)).reshape(-1, 3)
    uv = gl.accessor(at["TEXCOORD_0"]).astype(np.float64) if "TEXCOORD_0" in at else np.zeros((len(pos), 2))
    if "NORMAL" in at:
        normal = gl.accessor(at["NORMAL"]).astype(np.float64)
    else:
        # Smooth normals from the faces.
        normal = np.zeros_like(pos)
        face = np.cross(pos[tris[:, 1]] - pos[tris[:, 0]], pos[tris[:, 2]] - pos[tris[:, 0]])
        for k in range(3):
            np.add.at(normal, tris[:, k], face)
    normal /= np.maximum(np.linalg.norm(normal, axis=1, keepdims=True), 1e-12)
    return pos, normal, uv, tris


# --- the palette

def number(v):
    """Up to 4 decimals, no trailing zeros."""
    s = f"{v:.4f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def write_palette(path, vfx_json):
    palette = {}
    if os.path.exists(vfx_json):
        palette = json.load(open(vfx_json)).get("palette") or {}
    lines = ["; The weapon effects' colours (linear RGB, past 1 to glow) and intensities from the game's",
             "; effect data (riftapart-assets\\vfx\\vfx.json), written by skse\\tools\\ratchet_vfx_to_skyrim.py.",
             "[Palette]"]
    colours = values = 0
    for name, v in palette.items():
        vals = v if isinstance(v, list) else [v]
        if not vals or not all(isinstance(x, (int, float)) and not isinstance(x, bool) for x in vals):
            print(f"warning: palette entry {name} isn't numbers ({v!r}): left out", file=sys.stderr)
            continue
        lines.append(f"{name}={','.join(number(float(x)) for x in vals)}")
        if len(vals) >= 3:
            colours += 1
        else:
            values += 1
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="ascii", newline="\r\n") as f:
        f.write("\n".join(lines) + "\n")
    return colours, values


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--vfx", default=None, help="the extracted effect art (default: riftapart-assets\\vfx of this checkout or the main one)")
    ap.add_argument("--out", default=os.path.join(REPO, "target", "skse", "vfx"), help="folder laid out like Skyrim's Data folder")
    a = ap.parse_args()
    vfx = a.vfx or default_vfx_dir()
    if not os.path.isdir(vfx):
        sys.exit(f"no effect art at {vfx} (tools/extract_riftapart_vfx.py makes it)")

    tex_dir = os.path.join(a.out, "textures", *TEXTURE_DIR.split("\\"))
    mesh_dir = os.path.join(a.out, "meshes", *MESH_DIR.split("\\"))
    # Both folders are ours alone: nothing stale from an older conversion stays.
    for d in (tex_dir, mesh_dir):
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d)

    # Textures: every PNG, uncompressed with mipmaps, and plain white for the untextured meshes.
    textures = sorted(os.path.splitext(os.path.basename(p))[0] for p in glob.glob(os.path.join(vfx, "*.png")))
    for t in textures:
        R.write_dds(os.path.join(tex_dir, f"{t}.dds"), Image.open(os.path.join(vfx, f"{t}.png")))
    R.write_dds(os.path.join(tex_dir, "white.dds"), Image.new("RGBA", (4, 4), (255, 255, 255, 255)))
    textures.append("white")

    # A quad per texture, added and blended.
    quads = 0
    for t in textures:
        for blend in ("add", "blend"):
            name = f"quad_{t}_{blend}"
            write_shape_nif(os.path.join(mesh_dir, f"{name}.nif"), name, QUAD_POS, QUAD_NORMAL, QUAD_UV, QUAD_TRIS, t, blend, False)
            quads += 1

    # The sandbox's crosshair (main.rs: a "+" at 28 px, white at 90%, its shadow 1.5 px down and
    # right, black at 80%), made here (no game art): its own quad, drawn over everything.
    R.write_dds(os.path.join(tex_dir, "crosshair.dds"), crosshair_image())
    write_shape_nif(os.path.join(mesh_dir, "crosshair_blend.nif"), "crosshair_blend", QUAD_POS, QUAD_NORMAL, QUAD_UV, QUAD_TRIS,
                    "crosshair", "blend", False, ztest=False)

    # The effect meshes.
    meshes = 0
    loaded = {}
    for glb, mesh, name, blend, texture, zwrite in MESHES:
        path = os.path.join(vfx, f"{glb}.glb")
        if not os.path.exists(path):
            print(f"warning: no {path}: no {name}.nif", file=sys.stderr)
            continue
        if texture not in textures:
            print(f"warning: no {texture}.png for {name}.nif: it's drawn white", file=sys.stderr)
            texture = "white"
        if glb not in loaded:
            loaded[glb] = R.Gltf(path)
        pos, normal, uv, tris = glb_primitive(loaded[glb], mesh, path)
        write_shape_nif(os.path.join(mesh_dir, f"{name}.nif"), name, pos, normal, uv, tris, texture, blend, zwrite)
        meshes += 1

    # The palette last: the build takes fx.ini's time as the conversion's.
    colours, values = write_palette(os.path.join(a.out, "SKSE", "Plugins", "RatchetMovement", INI_FILE), os.path.join(vfx, "vfx.json"))
    print(f"{a.out}: {len(textures)} textures, {quads} quads, {meshes} meshes, {colours} palette colours"
          + (f" (and {values} intensities)" if values else ""))


if __name__ == "__main__":
    main()
