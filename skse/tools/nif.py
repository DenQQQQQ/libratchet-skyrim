# SPDX-License-Identifier: GPL-3.0-or-later
"""A small reader and writer for Skyrim SE NIF files (version 20.2.0.7, BS version 100).

It covers the blocks a skinned character model needs: NiNode, BSFadeNode, BSTriShape, NiSkinInstance,
BSDismemberSkinInstance, NiSkinData, NiSkinPartition, BSLightingShaderProperty, BSShaderTextureSet,
NiAlphaProperty and the simple extra data blocks. Other blocks are kept as raw bytes, so a file reads
and writes back unchanged.

Written from the NIF format as documented by the NifTools project (nif.xml). `python nif.py FILE`
reads a file, checks every block against the size the header records for it, and writes it back
to check the round trip byte for byte.
"""
import struct
import sys

SSE_VERSION = 0x14020007
SSE_USER_VERSION = 12
SSE_BS_VERSION = 100

# BSVertexDesc attribute flags (bits 44.. of the descriptor).
VF_VERTEX = 0x001
VF_UV = 0x002
VF_UV_2 = 0x004
VF_NORMAL = 0x008
VF_TANGENT = 0x010
VF_COLORS = 0x020
VF_SKINNED = 0x040
VF_LANDDATA = 0x080
VF_EYEDATA = 0x100
VF_FULLPREC = 0x400


class Reader:
    def __init__(self, data, pos=0):
        self.data = data
        self.pos = pos

    def take(self, n):
        b = self.data[self.pos:self.pos + n]
        if len(b) != n:
            raise EOFError(f"wanted {n} bytes at {self.pos}")
        self.pos += n
        return b

    def unpack(self, fmt):
        size = struct.calcsize("<" + fmt)
        v = struct.unpack_from("<" + fmt, self.data, self.pos)
        self.pos += size
        return v

    def u8(self): return self.unpack("B")[0]
    def u16(self): return self.unpack("H")[0]
    def u32(self): return self.unpack("I")[0]
    def i32(self): return self.unpack("i")[0]
    def u64(self): return self.unpack("Q")[0]
    def f32(self): return self.unpack("f")[0]
    def vec3(self): return list(self.unpack("3f"))
    def mat33(self): return list(self.unpack("9f"))
    def sized_string(self): return self.take(self.u32()).decode("latin-1")
    def short_string(self):
        n = self.u8()
        return self.take(n).rstrip(b"\0").decode("latin-1")


class Writer:
    def __init__(self):
        self.parts = []

    def raw(self, b): self.parts.append(bytes(b))
    def pack(self, fmt, *v): self.parts.append(struct.pack("<" + fmt, *v))
    def u8(self, v): self.pack("B", v)
    def u16(self, v): self.pack("H", v)
    def u32(self, v): self.pack("I", v)
    def i32(self, v): self.pack("i", v)
    def u64(self, v): self.pack("Q", v)
    def f32(self, v): self.pack("f", v)
    def vec3(self, v): self.pack("3f", *v)
    def mat33(self, m): self.pack("9f", *m)
    def sized_string(self, s):
        b = s.encode("latin-1")
        self.u32(len(b))
        self.raw(b)
    def short_string(self, s):
        b = s.encode("latin-1") + b"\0"
        self.u8(len(b))
        self.raw(b)
    def bytes(self): return b"".join(self.parts)


# --- vertex data (BSVertexDataSSE)

def vertex_attrs(desc):
    return desc >> 44


def vertex_size(desc):
    return (desc & 0xF) * 4


def make_vertex_desc(attrs):
    """The BSVertexDesc for these attribute flags (SSE layout: the position is always 3 floats,
    plus the bitangent's x as a fourth)."""
    off = 0
    offsets = {}
    if attrs & VF_VERTEX:
        offsets["pos"] = off
        off += 16
    if attrs & VF_UV:
        offsets["uv"] = off
        off += 4
    if attrs & VF_UV_2:
        offsets["uv2"] = off
        off += 4
    if attrs & VF_NORMAL:
        offsets["normal"] = off
        off += 4
    if attrs & VF_TANGENT:
        offsets["tangent"] = off
        off += 4
    if attrs & VF_COLORS:
        offsets["color"] = off
        off += 4
    if attrs & VF_SKINNED:
        offsets["skin"] = off
        off += 12
    if attrs & VF_EYEDATA:
        offsets["eye"] = off
        off += 4
    desc = (off // 4) & 0xF
    desc |= ((offsets.get("uv", 0) // 4) & 0xF) << 8
    desc |= ((offsets.get("uv2", 0) // 4) & 0xF) << 12
    desc |= ((offsets.get("normal", 0) // 4) & 0xF) << 16
    desc |= ((offsets.get("tangent", 0) // 4) & 0xF) << 20
    desc |= ((offsets.get("color", 0) // 4) & 0xF) << 24
    desc |= ((offsets.get("skin", 0) // 4) & 0xF) << 28
    desc |= ((offsets.get("eye", 0) // 4) & 0xF) << 36
    desc |= attrs << 44
    return desc


# Vertex data stays packed bytes in a block (it's only ever copied); the converter packs it.


# --- blocks

def read_object_net(r, b):
    b["name"] = r.i32()
    b["extra_data"] = [r.i32() for _ in range(r.u32())]
    b["controller"] = r.i32()


def write_object_net(w, b):
    w.i32(b["name"])
    w.u32(len(b["extra_data"]))
    for x in b["extra_data"]:
        w.i32(x)
    w.i32(b["controller"])


def read_av_object(r, b):
    read_object_net(r, b)
    b["flags"] = r.u32()
    b["translation"] = r.vec3()
    b["rotation"] = r.mat33()
    b["scale"] = r.f32()
    b["collision"] = r.i32()


def write_av_object(w, b):
    write_object_net(w, b)
    w.u32(b["flags"])
    w.vec3(b["translation"])
    w.mat33(b["rotation"])
    w.f32(b["scale"])
    w.i32(b["collision"])


def read_node(r, b):
    read_av_object(r, b)
    b["children"] = [r.i32() for _ in range(r.u32())]
    b["effects"] = [r.i32() for _ in range(r.u32())]


def write_node(w, b):
    write_av_object(w, b)
    w.u32(len(b["children"]))
    for x in b["children"]:
        w.i32(x)
    w.u32(len(b["effects"]))
    for x in b["effects"]:
        w.i32(x)


def read_tri_shape(r, b):
    read_av_object(r, b)
    b["bound"] = r.vec3() + [r.f32()]
    b["skin"] = r.i32()
    b["shader"] = r.i32()
    b["alpha"] = r.i32()
    b["vertex_desc"] = r.u64()
    b["num_triangles"] = r.u16()
    b["num_vertices"] = r.u16()
    b["data_size"] = r.u32()
    if b["data_size"] > 0:
        b["vertex_data"] = r.take(b["num_vertices"] * vertex_size(b["vertex_desc"]))
        b["triangles"] = r.take(b["num_triangles"] * 6)
    b["particle_data_size"] = r.u32()
    if b["particle_data_size"] > 0:
        b["particle_data"] = r.take(b["num_vertices"] * 12 + b["num_triangles"] * 6)


def write_tri_shape(w, b):
    write_av_object(w, b)
    w.pack("4f", *b["bound"])
    w.i32(b["skin"])
    w.i32(b["shader"])
    w.i32(b["alpha"])
    w.u64(b["vertex_desc"])
    w.u16(b["num_triangles"])
    w.u16(b["num_vertices"])
    w.u32(b["data_size"])
    if b["data_size"] > 0:
        w.raw(b["vertex_data"])
        w.raw(b["triangles"])
    w.u32(b["particle_data_size"])
    if b["particle_data_size"] > 0:
        w.raw(b["particle_data"])


def read_skin_instance(r, b, dismember):
    b["data"] = r.i32()
    b["partition"] = r.i32()
    b["skeleton_root"] = r.i32()
    b["bones"] = [r.i32() for _ in range(r.u32())]
    if dismember:
        b["body_parts"] = [r.unpack("2H") for _ in range(r.i32())]


def write_skin_instance(w, b, dismember):
    w.i32(b["data"])
    w.i32(b["partition"])
    w.i32(b["skeleton_root"])
    w.u32(len(b["bones"]))
    for x in b["bones"]:
        w.i32(x)
    if dismember:
        w.i32(len(b["body_parts"]))
        for flag, part in b["body_parts"]:
            w.pack("2H", flag, part)


def read_transform(r):
    return {"rotation": r.mat33(), "translation": r.vec3(), "scale": r.f32()}


def write_transform(w, t):
    w.mat33(t["rotation"])
    w.vec3(t["translation"])
    w.f32(t["scale"])


def read_skin_data(r, b):
    b["transform"] = read_transform(r)
    n = r.u32()
    b["has_vertex_weights"] = r.u8()
    bones = []
    for _ in range(n):
        bone = {"transform": read_transform(r), "bound": r.vec3() + [r.f32()]}
        nv = r.u16()
        bone["weights"] = [r.unpack("Hf") for _ in range(nv)] if b["has_vertex_weights"] else []
        bone["num_vertices"] = nv
        bones.append(bone)
    b["bones"] = bones


def write_skin_data(w, b):
    write_transform(w, b["transform"])
    w.u32(len(b["bones"]))
    w.u8(b["has_vertex_weights"])
    for bone in b["bones"]:
        write_transform(w, bone["transform"])
        w.pack("4f", *bone["bound"])
        if b["has_vertex_weights"]:
            w.u16(len(bone["weights"]))
            for idx, wt in bone["weights"]:
                w.pack("Hf", idx, wt)
        else:
            w.u16(bone["num_vertices"])


def read_skin_partition(r, b):
    n = r.u32()
    b["data_size"] = r.u32()
    b["vertex_size"] = r.u32()
    b["vertex_desc"] = r.u64()
    b["vertex_data"] = r.take(b["data_size"]) if b["data_size"] > 0 else b""
    parts = []
    for _ in range(n):
        p = {}
        nv, nt, nb, ns, nw = r.unpack("5H")
        p["num_vertices"], p["num_triangles"], p["num_weights_per_vertex"] = nv, nt, nw
        p["bones"] = list(r.unpack(f"{nb}H")) if nb else []
        p["has_vertex_map"] = r.u8()
        p["vertex_map"] = list(r.unpack(f"{nv}H")) if p["has_vertex_map"] and nv else []
        p["has_vertex_weights"] = r.u8()
        if p["has_vertex_weights"]:
            p["vertex_weights"] = list(r.unpack(f"{nv * nw}f")) if nv * nw else []
        p["strip_lengths"] = list(r.unpack(f"{ns}H")) if ns else []
        p["has_faces"] = r.u8()
        if p["has_faces"]:
            if ns:
                p["strips"] = list(r.unpack(f"{sum(p['strip_lengths'])}H"))
            else:
                p["triangles"] = list(r.unpack(f"{nt * 3}H")) if nt else []
        p["has_bone_indices"] = r.u8()
        if p["has_bone_indices"]:
            p["bone_indices"] = list(r.take(nv * nw))
        p["lod_level"] = r.u8()
        p["global_vb"] = r.u8()
        p["vertex_desc"] = r.u64()
        p["triangles_copy"] = list(r.unpack(f"{nt * 3}H")) if nt else []
        parts.append(p)
    b["partitions"] = parts


def write_skin_partition(w, b):
    w.u32(len(b["partitions"]))
    w.u32(b["data_size"])
    w.u32(b["vertex_size"])
    w.u64(b["vertex_desc"])
    w.raw(b["vertex_data"])
    for p in b["partitions"]:
        nv, nt, nw = p["num_vertices"], p["num_triangles"], p["num_weights_per_vertex"]
        w.pack("5H", nv, nt, len(p["bones"]), len(p["strip_lengths"]), nw)
        if p["bones"]:
            w.pack(f"{len(p['bones'])}H", *p["bones"])
        w.u8(p["has_vertex_map"])
        if p["has_vertex_map"] and nv:
            w.pack(f"{nv}H", *p["vertex_map"])
        w.u8(p["has_vertex_weights"])
        if p["has_vertex_weights"] and nv * nw:
            w.pack(f"{nv * nw}f", *p["vertex_weights"])
        if p["strip_lengths"]:
            w.pack(f"{len(p['strip_lengths'])}H", *p["strip_lengths"])
        w.u8(p["has_faces"])
        if p["has_faces"]:
            if p["strip_lengths"]:
                w.pack(f"{len(p['strips'])}H", *p["strips"])
            elif nt:
                w.pack(f"{nt * 3}H", *p["triangles"])
        w.u8(p["has_bone_indices"])
        if p["has_bone_indices"]:
            w.raw(bytes(p["bone_indices"]))
        w.u8(p["lod_level"])
        w.u8(p["global_vb"])
        w.u64(p["vertex_desc"])
        if nt:
            w.pack(f"{nt * 3}H", *p["triangles_copy"])


# BSLightingShaderProperty: the extra fields of each shader type (Skyrim).
SHADER_TYPE_EXTRA = {
    1: "f",        # environment map: env map scale
    5: "3f",       # skin tint: colour
    6: "3f",       # hair tint: colour
    7: "2f",       # parallax occlusion: max passes, scale
    11: "f2f2f4f", # multilayer parallax: inner thickness, refraction scale, inner texture scale, envmap strength
    14: "4f",      # sparkle snow
    16: "f3f3f",   # eye envmap: scale, left and right reflection centres
}


def read_lighting_shader(r, b):
    b["shader_type"] = r.u32()
    read_object_net(r, b)
    b["flags1"] = r.u32()
    b["flags2"] = r.u32()
    b["uv_offset"] = list(r.unpack("2f"))
    b["uv_scale"] = list(r.unpack("2f"))
    b["texture_set"] = r.i32()
    b["emissive_color"] = r.vec3()
    b["emissive_multiple"] = r.f32()
    b["texture_clamp_mode"] = r.u32()
    b["alpha"] = r.f32()
    b["refraction_strength"] = r.f32()
    b["glossiness"] = r.f32()
    b["specular_color"] = r.vec3()
    b["specular_strength"] = r.f32()
    b["lighting_effect1"] = r.f32()
    b["lighting_effect2"] = r.f32()
    fmt = SHADER_TYPE_EXTRA.get(b["shader_type"], "")
    b["type_extra"] = list(r.unpack(fmt)) if fmt else []


def write_lighting_shader(w, b):
    w.u32(b["shader_type"])
    write_object_net(w, b)
    w.u32(b["flags1"])
    w.u32(b["flags2"])
    w.pack("2f", *b["uv_offset"])
    w.pack("2f", *b["uv_scale"])
    w.i32(b["texture_set"])
    w.vec3(b["emissive_color"])
    w.f32(b["emissive_multiple"])
    w.u32(b["texture_clamp_mode"])
    w.f32(b["alpha"])
    w.f32(b["refraction_strength"])
    w.f32(b["glossiness"])
    w.vec3(b["specular_color"])
    w.f32(b["specular_strength"])
    w.f32(b["lighting_effect1"])
    w.f32(b["lighting_effect2"])
    fmt = SHADER_TYPE_EXTRA.get(b["shader_type"], "")
    if fmt:
        w.pack(fmt, *b["type_extra"])


def read_texture_set(r, b):
    b["textures"] = [r.sized_string() for _ in range(r.i32())]


def write_texture_set(w, b):
    w.i32(len(b["textures"]))
    for t in b["textures"]:
        w.sized_string(t)


def read_alpha(r, b):
    read_object_net(r, b)
    b["flags"] = r.u16()
    b["threshold"] = r.u8()


def write_alpha(w, b):
    write_object_net(w, b)
    w.u16(b["flags"])
    w.u8(b["threshold"])


def read_int_extra(r, b):
    b["name"] = r.i32()
    b["value"] = r.u32()


def write_int_extra(w, b):
    w.i32(b["name"])
    w.u32(b["value"])


def read_string_extra(r, b):
    b["name"] = r.i32()
    b["value"] = r.i32()


def write_string_extra(w, b):
    w.i32(b["name"])
    w.i32(b["value"])


BLOCKS = {
    "NiNode": (read_node, write_node),
    "BSFadeNode": (read_node, write_node),
    "BSLeafAnimNode": (read_node, write_node),
    "BSTriShape": (read_tri_shape, write_tri_shape),
    "BSSubIndexTriShape": None,
    "NiSkinInstance": (lambda r, b: read_skin_instance(r, b, False), lambda w, b: write_skin_instance(w, b, False)),
    "BSDismemberSkinInstance": (lambda r, b: read_skin_instance(r, b, True), lambda w, b: write_skin_instance(w, b, True)),
    "NiSkinData": (read_skin_data, write_skin_data),
    "NiSkinPartition": (read_skin_partition, write_skin_partition),
    "BSLightingShaderProperty": (read_lighting_shader, write_lighting_shader),
    "BSShaderTextureSet": (read_texture_set, write_texture_set),
    "NiAlphaProperty": (read_alpha, write_alpha),
    "BSXFlags": (read_int_extra, write_int_extra),
    "NiIntegerExtraData": (read_int_extra, write_int_extra),
    "NiStringExtraData": (read_string_extra, write_string_extra),
}


class Nif:
    """A NIF file: header fields, strings, blocks (dicts with a "type"), roots."""

    def __init__(self):
        self.version = SSE_VERSION
        self.user_version = SSE_USER_VERSION
        self.bs_version = SSE_BS_VERSION
        self.author = ""
        self.process_script = ""
        self.export_script = ""
        self.strings = []
        self.blocks = []
        self.roots = [0]
        self.groups = []

    def string(self, s):
        """The index of string s (added if new); -1 for None."""
        if s is None:
            return -1
        if s not in self.strings:
            self.strings.append(s)
        return self.strings.index(s)

    def add(self, block):
        self.blocks.append(block)
        return len(self.blocks) - 1

    @staticmethod
    def read(data, check=True):
        r = Reader(data)
        line_end = data.index(b"\n")
        header = data[:line_end].decode("latin-1")
        r.pos = line_end + 1
        nif = Nif()
        nif.header_string = header
        nif.version = r.u32()
        if nif.version != SSE_VERSION:
            raise ValueError(f"not a Skyrim SE NIF ({header}, version {nif.version:#x})")
        r.u8()  # endian
        nif.user_version = r.u32()
        num_blocks = r.u32()
        nif.bs_version = r.u32()
        nif.author = r.short_string()
        nif.process_script = r.short_string()
        nif.export_script = r.short_string()
        types = [r.sized_string() for _ in range(r.u16())]
        nif.types = types
        type_index = [r.u16() for _ in range(num_blocks)]
        sizes = [r.u32() for _ in range(num_blocks)]
        nstrings = r.u32()
        r.u32()  # max string length
        nif.strings = [r.sized_string() for _ in range(nstrings)]
        nif.groups = [r.u32() for _ in range(r.u32())]
        for i in range(num_blocks):
            t = types[type_index[i] & 0x7FFF]
            start = r.pos
            block = {"type": t}
            codec = BLOCKS.get(t)
            if codec:
                sub = Reader(data, start)
                codec[0](sub, block)
                if check and sub.pos - start != sizes[i]:
                    raise ValueError(f"block {i} ({t}): read {sub.pos - start} bytes, the header says {sizes[i]}")
            else:
                block["raw"] = data[start:start + sizes[i]]
            r.pos = start + sizes[i]
            nif.blocks.append(block)
        nif.roots = [r.i32() for _ in range(r.u32())]
        if r.pos != len(data):
            raise ValueError(f"{len(data) - r.pos} bytes left after the footer")
        return nif

    def write(self):
        bodies = []
        for i, block in enumerate(self.blocks):
            w = Writer()
            if "raw" in block:
                w.raw(block["raw"])
            else:
                BLOCKS[block["type"]][1](w, block)
            bodies.append(w.bytes())
        # A file that was read keeps its type list's order.
        types = [t for t in getattr(self, "types", []) if any(b["type"] == t for b in self.blocks)]
        for block in self.blocks:
            if block["type"] not in types:
                types.append(block["type"])
        w = Writer()
        w.raw(b"Gamebryo File Format, Version 20.2.0.7\n")
        w.u32(self.version)
        w.u8(1)
        w.u32(self.user_version)
        w.u32(len(self.blocks))
        w.u32(self.bs_version)
        w.short_string(self.author)
        w.short_string(self.process_script)
        w.short_string(self.export_script)
        w.u16(len(types))
        for t in types:
            w.sized_string(t)
        for block in self.blocks:
            w.u16(types.index(block["type"]))
        for body in bodies:
            w.u32(len(body))
        w.u32(len(self.strings))
        w.u32(max((len(s) for s in self.strings), default=0))
        for s in self.strings:
            w.sized_string(s)
        w.u32(len(self.groups))
        for g in self.groups:
            w.u32(g)
        for body in bodies:
            w.raw(body)
        w.u32(len(self.roots))
        for root in self.roots:
            w.i32(root)
        return w.bytes()


if __name__ == "__main__":
    for path in sys.argv[1:]:
        data = open(path, "rb").read()
        nif = Nif.read(data)
        counts = {}
        for b in nif.blocks:
            counts[b["type"]] = counts.get(b["type"], 0) + 1
        out = nif.write()
        same = out == data
        print(f"{path}: {len(nif.blocks)} blocks {counts}, round trip {'identical' if same else 'DIFFERENT'}")
        if not same:
            n = next((i for i in range(min(len(out), len(data))) if out[i] != data[i]), min(len(out), len(data)))
            print(f"  first difference at byte {n} (sizes {len(data)} vs {len(out)})")
