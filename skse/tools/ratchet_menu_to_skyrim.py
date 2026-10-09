# SPDX-License-Identifier: GPL-3.0-or-later
"""Rift Apart on Skyrim's main menu, from riftapart-assets/menu (tools/extract_riftapart_menu.py
makes it from YOUR OWN Rift Apart install) into a folder laid out like Skyrim's Data:

  meshes/interface/logo/logo.nif          its title logo where Skyrim's emblem is (also as
  meshes/interface/logo/logo01ae*.nif     the Anniversary Edition's two)
  textures/RatchetMovement/menu/logo.dds
  Sound/FX/RatchetMovement/menu/title_intro.wav, title_loop.wav
                                          its menu music, the intro and then the seamless loop
                                          (the SKSE plugin plays them while the main menu is up,
                                          Skyrim's music muted)

The output is game data: it stays on this PC (the build installs it into the MO2 mod), never in
the repository.

Skyrim's emblem (logo.nif) is one lit mesh, the same from both sides, under a BSFadeNode named
"Logo", about 39 x 73 units round (23.8, 16.8, 0) in the XZ plane; the main menu draws it in 3D.
The logo goes there as two single-sided quads, one facing each way, each reading the right way
round from its side (the file's front faces wind counter-clockwise round their normal), drawn
unlit with its alpha.

  uv run --with numpy --with pillow --with soundfile skse/tools/ratchet_menu_to_skyrim.py --out <dir>
"""
import argparse
import os
import shutil
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nif as N  # noqa: E402
import ratchet_to_skyrim as R  # noqa: E402
import ratchet_vfx_to_skyrim as V  # noqa: E402

TEXTURE_DIR = "RatchetMovement\\menu"  # under Data\textures
LOGOS = ["logo.nif", "logo01ae.nif", "logo01ae_animated.nif"]  # under Data\meshes\interface\logo
# Where Skyrim's emblem is in its file (its shape's place and bounds), and how tall the logo
# stands there (units; the emblem is 73 tall, the logo wider: ours).
EMBLEM_CENTER = (23.807, 16.8, 0.0)
LOGO_HEIGHT = 46.0
IDENTITY = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]


def default_menu_dir():
    """riftapart-assets/menu next to riftapart-assets/vfx (this checkout's or the main one's)."""
    return os.path.join(os.path.dirname(V.default_vfx_dir()), "menu")


def quad_shape(nf, name, corners, normal, texture):
    """One single-sided quad: corners top-left, top-right, bottom-right, bottom-left as seen from
    its front (normal toward the viewer), the texture right way round from there."""
    pos = np.array(corners, dtype=np.float64)
    uv = np.array([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
    nrm = np.array([normal] * 4, dtype=np.float64)
    # TL, BL, BR and TL, BR, TR: counter-clockwise round the normal (Skyrim's front faces).
    tris = np.array([[0, 3, 2], [0, 2, 1]])
    assert np.dot(np.cross(pos[3] - pos[0], pos[2] - pos[0]), normal) > 0, "a quad's winding"
    tangent, bitangent = R.tangent_space(pos, nrm, uv, tris)
    bb = R.unorm8(bitangent)
    verts = np.zeros(4, V.VERTEX)
    verts["pos"] = pos
    verts["bitangent_x"] = bitangent[:, 0]
    verts["uv"] = uv
    verts["normal"] = R.unorm8(nrm)
    verts["bitangent_y"] = bb[:, 1]
    verts["tangent"] = R.unorm8(tangent)
    verts["bitangent_z"] = bb[:, 2]
    verts["color"] = 255
    vertex_data = verts.tobytes()
    triangles = np.ascontiguousarray(tris, dtype="<u2").tobytes()
    shape = nf.add({
        "type": "BSTriShape", "name": nf.string(name), "extra_data": [], "controller": -1, "flags": 0xE,
        "translation": [0.0, 0.0, 0.0], "rotation": IDENTITY, "scale": 1.0, "collision": -1,
        "bound": R.bounding_sphere(pos), "skin": -1, "shader": -1, "alpha": -1,
        "vertex_desc": V.VERTEX_DESC, "num_triangles": len(tris), "num_vertices": 4,
        "data_size": len(vertex_data) + len(triangles), "vertex_data": vertex_data, "triangles": triangles, "particle_data_size": 0,
    })
    nf.blocks[shape]["shader"] = nf.add({
        "type": "BSEffectShaderProperty", "name": -1, "extra_data": [], "controller": -1,
        "flags1": V.SLSF1_ZBUFFER_TEST, "flags2": V.SLSF2_VERTEX_COLORS,
        "uv_offset": [0.0, 0.0], "uv_scale": [1.0, 1.0], "source_texture": f"textures\\{TEXTURE_DIR}\\{texture}.dds",
        "texture_clamp_mode": 0, "lighting_influence": 0, "env_map_min_lod": 0, "unused_byte": 0,
        "falloff_start_angle": 1.0, "falloff_stop_angle": 1.0, "falloff_start_opacity": 1.0, "falloff_stop_opacity": 1.0,
        "base_color": [1.0, 1.0, 1.0, 1.0], "base_color_scale": 1.0, "soft_falloff_depth": 0.0, "greyscale_texture": "",
    })
    nf.blocks[shape]["alpha"] = nf.add({"type": "NiAlphaProperty", "name": -1, "extra_data": [], "controller": -1,
                                        "flags": V.ALPHA_FLAGS["blend"], "threshold": 0})
    return shape


def write_logo_nif(path, aspect, texture):
    """Skyrim's main-menu logo file, with the logo where its emblem was."""
    cx, cy, cz = EMBLEM_CENTER
    h = LOGO_HEIGHT
    w = h * aspect
    x0, x1, z0, z1 = cx - w / 2, cx + w / 2, cz - h / 2, cz + h / 2
    nf = N.Nif()
    nf.process_script = "riftapart-skyrim ratchet_menu_to_skyrim.py"
    root = nf.add({"type": "BSFadeNode", "name": nf.string("Logo"), "extra_data": [], "controller": -1, "flags": 0xE,
                   "translation": [0.0, 0.0, 0.0], "rotation": IDENTITY, "scale": 1.0, "collision": -1, "children": [], "effects": []})
    # Seen from -y (screen right +x) and from +y (screen right -x), a hair apart.
    front = quad_shape(nf, "RatchetLogoFront", [(x0, cy - 0.05, z1), (x1, cy - 0.05, z1), (x1, cy - 0.05, z0), (x0, cy - 0.05, z0)], (0.0, -1.0, 0.0), texture)
    back = quad_shape(nf, "RatchetLogoBack", [(x1, cy + 0.05, z1), (x0, cy + 0.05, z1), (x0, cy + 0.05, z0), (x1, cy + 0.05, z0)], (0.0, 1.0, 0.0), texture)
    nf.blocks[root]["children"] += [front, back]
    out = nf.write()
    assert N.Nif.read(out).write() == out, f"{path}: the NIF doesn't read back the same"
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--menu", default=None, help="the extracted menu art and music (default: riftapart-assets\\menu)")
    ap.add_argument("--out", required=True, help="folder laid out like Skyrim's Data folder")
    a = ap.parse_args()
    src = a.menu or default_menu_dir()
    if not os.path.isdir(src):
        sys.exit(f"no menu art at {src} (tools/extract_riftapart_menu.py makes it)")
    made = []

    logo_png = os.path.join(src, "logo.png")
    if os.path.exists(logo_png):
        logo = Image.open(logo_png).convert("RGBA")
        # Its drawn part only (the art has room round it), and a power of two each way for Skyrim.
        box = logo.getchannel("A").point(lambda v: 255 if v > 8 else 0).getbbox() or (0, 0, *logo.size)
        logo = logo.crop(box)
        aspect = logo.size[0] / logo.size[1]
        size = (min(2048, 1 << (logo.size[0] - 1).bit_length()), min(1024, 1 << (logo.size[1] - 1).bit_length()))
        tex_dir = os.path.join(a.out, "textures", *TEXTURE_DIR.split("\\"))
        os.makedirs(tex_dir, exist_ok=True)
        R.write_dds(os.path.join(tex_dir, "logo.dds"), logo.resize(size, Image.LANCZOS))
        for name in LOGOS:
            write_logo_nif(os.path.join(a.out, "meshes", "interface", "logo", name), aspect, "logo")
        made.append(f"the logo ({logo.size[0]} x {logo.size[1]} drawn, {aspect:.2f} wide to tall, as {size[0]} x {size[1]})")
    else:
        print(f"warning: no {logo_png}: Skyrim's own emblem stays", file=sys.stderr)

    # The menu music: its intro, then its loop (the extractor's menu.json says which of the game's).
    parts = [os.path.join(src, f"title_{part}.wav") for part in ("intro", "loop")]
    if all(os.path.exists(p) for p in parts):
        import soundfile as sf
        lengths = []
        for p in parts:
            data, rate = sf.read(p, always_2d=True)
            out = os.path.join(a.out, "Sound", "FX", "RatchetMovement", "menu", os.path.basename(p))
            os.makedirs(os.path.dirname(out), exist_ok=True)
            # 16-bit PCM, its own channels and rate (what Skyrim plays from a file).
            sf.write(out, np.clip(data, -1.0, 1.0), rate, subtype="PCM_16")
            lengths.append(f"{len(data) / rate:.1f} s")
        made.append(f"the menu music ({' then '.join(lengths)} looped)")
    else:
        print(f"warning: no {' and '.join(parts)}: Skyrim's own menu music stays", file=sys.stderr)

    print(f"{a.out}: " + ("; ".join(made) if made else "nothing"))


if __name__ == "__main__":
    main()
