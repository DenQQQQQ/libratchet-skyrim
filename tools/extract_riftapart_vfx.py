# SPDX-License-Identifier: GPL-3.0-or-later
# Exports the visual-effect art the sandbox uses (textures as PNG, meshes as GLB) from YOUR OWN
# Rift Apart install into riftapart-assets/vfx/. Uses ALERT (GPLv3) through
# extract_riftapart_ratchet.py. The output is game data: keep it local, never commit it.
#
# The game's particle systems (.visualeffect) aren't decoded; the sandbox builds its effects from
# the same textures and meshes the game's effects reference, with its own timing. The colours come
# from the game too (vfx.json "palette"): material parameters (a decoded layout) and colour keys
# found in the effects' particle data (not decoded; picked by offset, checked to look like colours).
#
# Usage:
#   python tools/extract_riftapart_vfx.py --game "<Rift Apart folder>" --alert "<ALERT clone>"
import argparse
import json
import os
import struct
import sys
from types import SimpleNamespace

import numpy as np
from PIL import Image

# name -> texture asset (or (diffuse, alpha) pair merged into one RGBA image)
TEXTURES = {
    # Burst Pistol: tracer, muzzle flash, impact flare, sparks, bullet scorch, smoke.
    "tracer": "textures/visualeffect/beam/tracer_starwars_grey.texture",
    "muzzle_flash": "textures/visualeffect/muzzle_flash/muzzflash_01_rgb_sm.texture",
    "glow": "textures/effects/generic/fx_explosion_glow.texture",
    "impact_flare": "textures/visualeffect/lens_flare/impact_flare_grey_sm.texture",
    "spark": "textures/visualeffect/debris/ig_i20_debris_spark_standard.texture",
    "bullet_decal": "textures/effects/decal/bulletimpacts/metal/dec_bulletimpact_metal_color.texture",
    "smoke_puff": ("textures/visualeffect/flipbooks/gbl_smoke_sim_puff_00_30d_diffuse.texture",
                   "textures/visualeffect/flipbooks/gbl_smoke_sim_puff_00_30d_alpha.texture"),
    # The bolt's smoke trail (its smk_trail_01_unlit_bpistol material).
    "smoke_trail": ("textures/visualeffect/flipbooks/gbl_smoke_sim_trail_00_diffuse.texture",
                    "textures/visualeffect/flipbooks/gbl_smoke_sim_trail_00_alpha.texture"),
    # Shatterbomb: shockwave, flares, debris, scorch mark, fire colours.
    "shockwave_mask": "textures/visualeffect/noise/fx_noise_holes_01.texture",
    "flare_radial": "textures/visualeffect/flares/fx_flare_radial_01.texture",
    "glow_simple": "textures/visualeffect/global/flares/glow_simple.texture",
    "debris_chunk": ("textures/visualeffect/debris/debris_chunk_3d.texture", "textures/visualeffect/debris/debris_chunk_3d_alpha.texture"),
    "scorch_mask": "textures/visualeffect/decal/explosion_scorchmark/decal_explosion_scorchmark_mask.texture",
    "scorch_glow": "textures/visualeffect/decal/explosion_scorchmark/decal_explosion_scorchmark_emissive.texture",
    "fire_ramp": "textures/visualeffect/gradients/fx_ramp_explosion_fire_01.texture",
}

# name -> model asset (exported as plain meshes; the sandbox gives them effect materials)
MODELS = {
    "tracer_mesh": "visualeffects/burst_pistol/burst_pistol_tracer_wiggle.model",
    "projectile_shape": "visualeffect/global/gbl_projectile_shape_vfx/gbl_projectile_shape_vfx.model",
    "muzzle_flash_mesh": "visualeffect/equipment/weapon/wpn_blarg_trooper_blaster/wpn_blarg_trooper_muzzle_flash.model",
    "shockwave_hemisphere": "visualeffect/global/gbl_explosion_shapes/gbl_shockwave_hemisphere_01.model",
    "impact_blast": "visualeffect/global/gbl_impact_blast/gbl_impact_blast.model",
    "shatter_shards": "equipment/projectile/proj_protogrenade_shard/proj_protogrenade_shards.model",
    # The Burst Pistol's ejected shell (its WeaponConfig ShellCasingData.ShellAsset actor's model).
    "shell": "visualeffects/burst_pistol/burst_pistol_shell_01.model",
}

BOLT_MESH = "material/visualeffect/equipment/weapon/wpn_burst_pistol_projectile_mesh/wpn_burst_pistol_projectile_mesh.material"
WIGGLE = "material/visualeffect/equipment/projectile/proj_burst_pistol_tracer_wiggle/proj_burst_pistol_tracer_wiggle.material"
DECAL = "material/visualeffect/global/gbl_blaster_scorch_mark/burst_pistol_impact_decal_green.material"
SHELL_GLASS = "materials/jn_glass_01/burst_shell_simple_exterior.material"
SCORCH = "material/visualeffect/equipment/weapon/wpn_explosion_scorchmark/wpn_protogrenade_scorchmark.material"

# Palette entries from material parameters: name -> (material, parameter name hash).
MATERIAL_COLOURS = {
    "bolt": (BOLT_MESH, 0x7F058428),
    "bolt_intensity": (BOLT_MESH, 0x0FE74A17),
    "tracer": (WIGGLE, 0xCD6CE032),
    "tracer_dark": (WIGGLE, 0x425A90CC),
    "decal_glow": (DECAL, 0x342C191B),
    "decal": (DECAL, 0x0DCDF5B4),
    "shell_glass": (SHELL_GLASS, 0x251A9332),
    "scorch_glow": (SCORCH, 0x2E2142FB),
    "scorch_dark": (SCORCH, 0x5001A382),
}

BOLT_FX = "visualeffect/character/enemy/enm_blarg_blaster_projectile_fx/burst_pistol_gun_projectile_fast.visualeffect"
IMPACT_FX = "visualeffect/character/enemy/enm_blarg_v1_trooper_projectile_impact/burst_pistol_projectile_impact.visualeffect"
BLAST_FX = "visualeffect/weapon/wpn_protogrenade/wpn_protogrenade_explosion_sub.visualeffect"
GRENADE_FX = "visualeffect/weapon/wpn_protogrenade/wpn_protogrenade_projectile.visualeffect"

# Palette entries from the particle data: name -> (visualeffect, byte offset of a key in its
# half-float value table).
EFFECT_COLOURS = {
    "bolt_glow": (BOLT_FX, 0x180),
    "bolt_glow_end": (BOLT_FX, 0x190),
    "impact_flash": (IMPACT_FX, 0x230),
    "impact_spark": (IMPACT_FX, 0x240),
    "impact_glow": (IMPACT_FX, 0x7B0),
    "impact_glow_end": (IMPACT_FX, 0x7D0),
    "impact_smoke": (IMPACT_FX, 0x110),
    "blast_hot": (BLAST_FX, 0x1490),
    "blast_fire": (BLAST_FX, 0x14B0),
    "blast_fire_end": (BLAST_FX, 0x15A0),
    "blast_energy": (BLAST_FX, 0x13A0),
    "blast_shards": (BLAST_FX, 0x1700),
    "blast_smoke": (BLAST_FX, 0x150),
    "grenade_glow": (GRENADE_FX, 0x180),
}


# (tag, absolute offset, size) of each section of a DAT1 asset (36-byte asset header first).
def dat1_sections(b):
    count = struct.unpack_from("<I", b, 0x30)[0]
    for i in range(count):
        tag, off, size = struct.unpack_from("<III", b, 0x34 + 12 * i)
        yield tag, off + 0x24, size


# A material's parameter overrides: {name hash: values}. Section F5260180 holds a count, the
# data offset, then (u16 offset, u16 size, u32 hash) entries right before the data.
def material_params(b):
    for tag, off, _ in dat1_sections(b):
        if tag == 0xF5260180:
            _, count, data = struct.unpack_from("<III", b, off)
            first = off + data - 8 * count
            out = {}
            for i in range(count):
                o, size, h = struct.unpack_from("<HHI", b, first + 8 * i)
                out[h] = list(struct.unpack_from(f"<{size // 4}f", b, off + data + o))
            return out
    return {}


# RGB of a colour-like key (first three halfs of a 16-byte row) in a visualeffect's value table
# (section B1F4C248), or None if it doesn't look like a colour.
def effect_key(b, offset):
    for tag, off, size in dat1_sections(b):
        if tag == 0xB1F4C248 and offset + 16 <= size:
            rgb = np.frombuffer(b, np.float16, 3, off + offset).astype(np.float32)
            if np.all(np.isfinite(rgb)) and rgb.min() >= 0.0 and 0.05 < rgb.max() < 4.0:
                return [round(float(v), 4) for v in rgb]
    return None


def is_mask(rgb):
    """A one-channel texture decoded into red only."""
    return rgb[..., 1].max() == 0 and rgb[..., 2].max() == 0


def gray_if_mask(rgb):
    return np.repeat(rgb[..., :1], 3, axis=2) if is_mask(rgb) else rgb


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    sys.path.insert(0, here)
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True)
    ap.add_argument("--alert", required=True)
    ap.add_argument("--out", default="riftapart-assets")
    ap.add_argument("--gdeflate-dll", default=os.path.join(here, "..", "target", "release", "gdeflate_ffi.dll"))
    a = ap.parse_args()

    from extract_riftapart_ratchet import Game, convert

    game = Game(a.game, a.alert, a.gdeflate_dll)
    out_dir = os.path.join(a.out, "vfx")
    os.makedirs(out_dir, exist_ok=True)
    manifest = {"textures": {}, "models": {}}

    for name, src in TEXTURES.items():
        if isinstance(src, tuple):
            color, alpha = (game.texture(p, 1024) for p in src)
            if color is None or alpha is None:
                print(f"[!] {name}: missing {src}")
                continue
            rgb = gray_if_mask(np.asarray(color[0].convert("RGB")))
            a_img = alpha[0].convert("RGB").resize(color[0].size)
            img = Image.fromarray(np.dstack([rgb, np.asarray(a_img)[..., 0]]), "RGBA")
        else:
            got = game.texture(src, 1024)
            if got is None:
                print(f"[!] {name}: missing {src}")
                continue
            px = np.asarray(got[0].convert("RGBA"))
            if is_mask(px[..., :3]):
                # One-channel masks (BC4): white, with the mask as alpha, so the sandbox tints
                # them the way the game's shaders do.
                img = Image.fromarray(np.dstack([np.full(px.shape[:2] + (3,), 255, np.uint8), px[..., 0]]), "RGBA")
            elif name == "muzzle_flash":
                # Three masks packed in RGB (core, body, edge): one white shape, alpha = brightest,
                # for the sandbox to tint.
                img = Image.fromarray(np.dstack([np.full(px.shape[:2] + (3,), 255, np.uint8), px[..., :3].max(axis=2)]), "RGBA")
            else:
                img = got[0].convert("RGBA")
        path = os.path.join(out_dir, name + ".png")
        img.save(path, optimize=True)
        manifest["textures"][name] = {"source": src, "size": img.size}
        print(f"  texture {name:16} {img.size[0]}x{img.size[1]}")

    args = SimpleNamespace(out=a.out, look=0, lod=0, tex_size=1024, no_anims=True)
    for name, model in MODELS.items():
        preset = {"model": model, "out": f"vfx/{name}.glb", "clips": {}, "clip_dir": "", "vfx": True}
        try:
            convert(game, args, preset)
            manifest["models"][name] = {"source": model}
        except Exception as e:  # noqa: BLE001 - report and carry on with the rest
            print(f"[!] {name}: {e}")

    palette = {}
    for name, (material, h) in MATERIAL_COLOURS.items():
        b = game.read(material)
        values = material_params(b).get(h) if b is not None else None
        if values is None:
            print(f"[!] palette {name}: no parameter {h:08X} in {material}")
            continue
        palette[name] = [round(v, 4) for v in values]
    for name, (effect, offset) in EFFECT_COLOURS.items():
        b = game.read(effect)
        rgb = effect_key(b, offset) if b is not None else None
        if rgb is None:
            print(f"[!] palette {name}: no colour at {offset:#x} in {effect}")
            continue
        palette[name] = rgb
    manifest["palette"] = palette
    print(f"  palette: {len(palette)} colours")

    with open(os.path.join(out_dir, "vfx.json"), "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"wrote {len(manifest['textures'])} textures and {len(manifest['models'])} meshes to {out_dir}")


if __name__ == "__main__":
    main()
