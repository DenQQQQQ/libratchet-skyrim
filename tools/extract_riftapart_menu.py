# SPDX-License-Identifier: GPL-3.0-or-later
"""Exports Rift Apart's title logo, title card and menu music from YOUR OWN install into
riftapart-assets/menu/ for the SKSE build (skse/tools/ratchet_menu_to_skyrim.py and
ratchet_intro_to_skyrim.py read it). Uses ALERT (https://github.com/Tkachov/ALERT, GPLv3) through
extract_riftapart_ratchet.py and riftapart_wwise.py, so this script is GPLv3 too. The output is
game data: keep it local (riftapart-assets/ is gitignored), never commit or share it.

Writes:
  logo.png         the title logo the start screen shows: configs/uiconfig/uiicons.config's
                   LogoImage (English, and every language without a CustomLogos entry of its own),
                   trimmed to its drawn part (alpha > 8) with a small margin. The texture's colour is
                   premultiplied by its alpha; it's divided out here (straight alpha).
  title_card.png   the logo on the violet rift field the game shows as its act 1 title card,
                   1920 x 1080 (from 3840 x 2160), opaque.
  title_intro.wav  the menu music as the game starts it: the playlist's first variant's intro
  title_loop.wav   segment (the file from a point in, with a fade-in from its clip automation),
                   then its loop segment (the whole file, seamless) for as long as the menu is up.
                   Stereo (FL + BL and FR + BR at -3 dB, normalised: what ffmpeg's plain -ac 2
                   does), 48 kHz, 16-bit PCM, at the file's own level: the game's playback level
                   is in menu.json, not applied.
  intro_sting.wav  the press-start sound (Play_UI_HUD_MENU_start_game): its two audio layers mixed
                   as the game sets them against each other (the louder at its file's level), its
                   DualSense rumble layers left out; stereo, 48 kHz, 16-bit PCM.
  menu.json        where each came from (asset paths, Wwise event and object ids), lengths, the
                   loop's seamless length and the game's levels.

The music is the pause menu's: conduit/ui_sounds.conduit answers PauseMenuShowEvent with
Play_UI_PauseMenu_Pause_menu_open, which plays an interactive-music playlist (a random pick of
two intro + loop pairs). No front-end event plays music (UIFrontEndOpenEvent's only plays a UI
sound, Play_i29_Music_Menu has no actions, and the gameplay music switch has no menu branch), so
whether the main menu plays this too isn't confirmed. The playlist, segments and tracks are Wwise
interactive-music objects (bank version 135), parsed here as far as needed.

Needs: pip install numpy pillow texture2ddecoder lz4 pygltflib; ffmpeg on PATH (the music is Opus);
       cargo build -p gdeflate-ffi --release  (found in this checkout or the main one)
Usage:
  python tools/extract_riftapart_menu.py --game "<Rift Apart folder>" --alert "<ALERT clone>" [--out <dir>]
"""
import argparse
import contextlib
import io
import json
import math
import os
import shutil
import struct
import subprocess
import sys
import wave

import numpy as np
from PIL import Image

REPO = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

UI_ICONS = "configs/uiconfig/uiicons.config"  # its LogoImage: the title logo for English
TITLE_CARD = "textures/ui/act1_rc_logo_black_screen.texture"
CARD_SIZE = (1920, 1080)
LOGO_ALPHA_MIN = 8  # what counts as drawn when trimming
LOGO_MARGIN = 4  # px kept round the drawn part

UI_BANK = "sound/banks/ui_hud.soundbank"  # the event's bank
MUSIC_BANK = "sound/banks/music.soundbank"  # has the same music objects too
MENU_EVENT = "Play_UI_PauseMenu_Pause_menu_open"  # answers PauseMenuShowEvent (conduit/ui_sounds.conduit)
# Events that stop it (the same conduit): leaving the pause menu, closing it from the map.
STOP_EVENTS = ["Play_UI_MENU_pause_exit", "Stop_i29_Music_Menu", "Play_UI_PauseMenu_Pause_map_menu_close"]
STING_EVENT = "Play_UI_HUD_MENU_start_game"  # answers UIFrontEndPressStartEvent / PressContinueEvent (same conduit)
RATE = 48000

# HIRC object types and ids used here (riftapart_wwise has the sound-side ones).
SEGMENT, TRACK, MSWITCH, MRANSEQ = 10, 11, 12, 13
PLAY = 0x0403
FADE_IN = 3  # AkClipAutomationType
CURVES = {0: "log3", 1: "sine", 2: "log1", 3: "inv_s_curve", 4: "linear", 5: "s_curve", 6: "exp1", 7: "sine_recip",
          8: "exp3", 9: "constant"}  # AkCurveInterpolation


def main_checkout_path(*parts):
    """A path in this checkout if it exists, else in the main checkout (when this is a worktree)."""
    here = os.path.join(REPO, *parts)
    if os.path.exists(here):
        return here
    try:
        common = subprocess.run(["git", "-C", REPO, "rev-parse", "--path-format=absolute", "--git-common-dir"],
                                capture_output=True, text=True, check=True).stdout.strip()
        main = os.path.normpath(os.path.join(os.path.dirname(common), *parts))
        if os.path.exists(main):
            return main
    except (OSError, subprocess.CalledProcessError):
        pass
    # A worktree under .claude/worktrees/<name>.
    return os.path.normpath(os.path.join(REPO, "..", "..", "..", *parts))


def default_out_dir():
    """riftapart-assets/menu in this checkout, else in the main checkout's riftapart-assets."""
    return os.path.join(main_checkout_path("riftapart-assets"), "menu")


# ---------------------------------------------------------------------------------------------
# Art

def config_root(game, path):
    import dat1lib.types.sections.config.serialized as ser
    cfg = game.dat1lib.read(io.BytesIO(game.read(path)))
    return cfg.dat1.get_section(ser.ConfigContentSection.TAG).root


def export_logo(game, out_dir):
    path = config_root(game, UI_ICONS)["LogoImage"]
    got = game.texture(path, 1 << 16)
    if got is None:
        sys.exit(f"missing {path}")
    px = np.asarray(got[0].convert("RGBA")).astype(np.float64)
    alpha = px[..., 3:4]
    # Premultiplied colour -> straight: divide by alpha (fully clear pixels stay black).
    rgb = np.where(alpha > 0, np.clip(px[..., :3] * 255.0 / np.maximum(alpha, 1.0), 0, 255), 0.0)
    img = np.concatenate([rgb, alpha], axis=2).round().astype(np.uint8)
    ys, xs = np.nonzero(img[..., 3] > LOGO_ALPHA_MIN)
    h, w = img.shape[:2]
    box = (max(0, int(xs.min()) - LOGO_MARGIN), max(0, int(ys.min()) - LOGO_MARGIN),
           min(w, int(xs.max()) + 1 + LOGO_MARGIN), min(h, int(ys.max()) + 1 + LOGO_MARGIN))
    logo = Image.fromarray(img, "RGBA").crop(box)
    logo.save(os.path.join(out_dir, "logo.png"), optimize=True)
    return {"file": "logo.png", "source": path, "chosen_by": f"{UI_ICONS} LogoImage", "texture_size": [w, h],
            "crop": list(box), "size": list(logo.size), "alpha": "straight (the texture's colour is premultiplied; divided out)"}


def export_title_card(game, out_dir):
    got = game.texture(TITLE_CARD, 1 << 16)
    if got is None:
        sys.exit(f"missing {TITLE_CARD}")
    src = got[0]
    card = src.convert("RGB").resize(CARD_SIZE, Image.LANCZOS)  # its alpha is 251..255: opaque
    card.save(os.path.join(out_dir, "title_card.png"), optimize=True)
    return {"file": "title_card.png", "source": TITLE_CARD, "texture_size": list(src.size), "size": list(card.size)}


# ---------------------------------------------------------------------------------------------
# Interactive music (bank version 135 / Wwise 2019.2), on riftapart_wwise's parsed objects.
#   MusicNodeParams: u8 flags, NodeBaseParams, children (u32 count, sorted ids), meter (f64 grid,
#     f64 grid offset, f32 tempo, u8 beats, u8 beat value), u8 meter flag, u32 stingers (24 bytes each).
#   Segment: MusicNodeParams, f64 duration (ms), u32 markers (u32 id, f64 position ms, u32 name size, name).
#   RanSeq: MusicNodeParams, transition rules, u32 playlist item count, then the playlist tree
#     (u32 segment, u32 item id, u32 children, i32 type, i16 loop (0 = forever), i16 loop min/max,
#     u32 weight, u16 avoid repeat, u8 use weight, u8 shuffle; children follow their parent).
#   Track: u8 flags, u32 sources (u32 plugin, u8 stream, u32 source id, u32 prefetch size, u8 bits),
#     u32 clips (u32 subtrack, u32 source, u32 event, f64 play at, begin trim, end trim, source
#     duration; ms), u32 subtracks, u32 automations (u32 clip, u32 type, u32 points: f32 time s,
#     f32 value, u32 curve), then NodeBaseParams.

def _u32(d, p):
    return struct.unpack_from("<I", d, p)[0]


def _track(d):
    p = 5  # past the id and the flags
    n = _u32(d, p)
    p += 4
    sources = []
    for _ in range(n):
        plugin, stream, sid, _size, _bits = struct.unpack_from("<IBIIB", d, p)
        p += 14
        if plugin & 0xF == 2:  # a source plugin: its parameters follow
            p += 4 + _u32(d, p)
        sources.append({"codec": plugin >> 16, "stream": stream, "source": sid})
    count = _u32(d, p)
    p += 4
    clips = []
    for _ in range(count):
        sub, sid, _event = struct.unpack_from("<III", d, p)
        play_at, begin, end, dur = struct.unpack_from("<4d", d, p + 12)
        clips.append({"subtrack": sub, "source": sid, "play_at_ms": play_at, "begin_trim_ms": begin, "end_trim_ms": end,
                      "source_ms": dur})
        p += 44
    if count:
        p += 4  # subtracks
    n = _u32(d, p)
    p += 4
    autos = []
    for _ in range(n):
        clip, kind, npts = struct.unpack_from("<III", d, p)
        pts = [struct.unpack_from("<ffI", d, p + 12 + 12 * k) for k in range(npts)]
        autos.append({"clip": clip, "type": kind, "points": pts})
        p += 12 + 12 * npts
    return {"sources": sources, "clips": clips, "automation": autos, "node_base": p}


class Music:
    """The music objects of a set of banks (first bank wins), with parents, children and props."""

    def __init__(self, banks):
        import riftapart_wwise as ww
        self.ww = ww
        self.objs = {}
        for bk in banks:
            for oid, ob in bk.objs.items():
                if ob["type"] in (SEGMENT, TRACK, MSWITCH, MRANSEQ, ww.EVENT, ww.ACTION) and oid not in self.objs:
                    self.objs[oid] = ob
        for ob in self.objs.values():
            d = ob["raw"]
            if ob["type"] == TRACK:
                ob["track"] = _track(d)
                ww.Bank._node_base(ob, d, ob["track"]["node_base"])
            elif ob["type"] in (SEGMENT, MSWITCH, MRANSEQ):
                ww.Bank._node_base(ob, d, 5)
        self.kids = {}
        for ob in self.objs.values():
            if ob["type"] in (SEGMENT, TRACK, MSWITCH, MRANSEQ) and ob.get("parent"):
                self.kids.setdefault(ob["parent"], []).append(ob["id"])

    def _after_node(self, ob):
        """Offset just past MusicNodeParams (children, meter, stingers)."""
        d = ob["raw"]
        kids = sorted(self.kids.get(ob["id"], []))
        pat = struct.pack("<I%dI" % len(kids), len(kids), *kids)
        pos = d.find(pat, ob.get("after_props", 4))
        if pos < 0:
            raise ValueError(f"music object {ob['id']}: children list not found")
        q = pos + len(pat) + 22 + 1  # children, meter, meter flag
        return q + 4 + 24 * _u32(d, q)  # stingers

    def segment(self, oid):
        ob = self.objs[oid]
        d = ob["raw"]
        q = self._after_node(ob)
        duration = struct.unpack_from("<d", d, q)[0]
        n = _u32(d, q + 8)
        q += 12
        markers = []
        for _ in range(n):
            markers.append(struct.unpack_from("<d", d, q + 4)[0])
            q += 16 + _u32(d, q + 12)  # id, position, name size, name
        return {"duration_ms": duration, "markers_ms": markers, "tracks": sorted(self.kids.get(oid, []))}

    def playlist(self, oid):
        ob = self.objs[oid]
        d = ob["raw"]
        q = self._after_node(ob)
        rules = _u32(d, q)
        q += 4
        for _ in range(rules):  # transition rules: skipped
            q += 4 + 4 * _u32(d, q)  # sources
            q += 4 + 4 * _u32(d, q)  # destinations
            q += 21 + 26  # the source and destination rules
            q += 1 + (30 if d[q] else 0)  # a transition segment
        q += 4  # the playlist's item count (each item says how many children follow it)

        def item(q):
            seg, _iid, n, kind = struct.unpack_from("<IIIi", d, q)
            loop = struct.unpack_from("<h", d, q + 16)[0]
            node = {"segment": seg, "type": kind, "loop": loop, "children": []}
            q += 30
            for _ in range(n):
                child, q = item(q)
                node["children"].append(child)
            return node, q

        return item(q)[0]

    def gain_db(self, oid):
        """Volume + make-up gain over the object and its music parents (buses not included)."""
        total = 0.0
        while oid in self.objs:
            props = self.objs[oid].get("props", {})
            total += self.ww.as_float(props.get(self.ww.P_VOLUME, 0)) + self.ww.as_float(props.get(self.ww.P_MAKEUP_GAIN, 0))
            oid = self.objs[oid].get("parent")
        return total


def find_playlist(music, event):
    ev = music.objs.get(music.ww.fnv1(event))
    if ev is None or ev["type"] != music.ww.EVENT:
        sys.exit(f"event {event} not found")
    for aid in ev["actions"]:
        act = music.objs.get(aid)
        if act and act.get("atype") == PLAY and music.objs.get(act["target"], {}).get("type") == MRANSEQ:
            return act["target"]
    sys.exit(f"{event} plays no music playlist")


def first_variant(node):
    """The playlist's first intro + loop pair: [(segment, loop count)] in play order."""
    while node["children"] and not any(c["segment"] for c in node["children"]):
        node = node["children"][0]  # random/sequence groups: take the first
    return [(c["segment"], c["loop"]) for c in node["children"] if c["segment"]]


def clip_of(music, seg_id):
    seg = music.segment(seg_id)
    if len(seg["tracks"]) != 1:
        raise ValueError(f"segment {seg_id}: {len(seg['tracks'])} tracks (one expected)")
    tr = music.objs[seg["tracks"][0]]["track"]
    if len(tr["clips"]) != 1:
        raise ValueError(f"segment {seg_id}: {len(tr['clips'])} clips (one expected)")
    clip = tr["clips"][0]
    fade = next((a for a in tr["automation"] if a["type"] == FADE_IN), None)
    return seg, tr, clip, fade


# ---------------------------------------------------------------------------------------------
# Audio

def decode_ogg(ffmpeg, ogg):
    """Ogg Opus or Vorbis bytes -> float32 samples (n, channels) at 48 kHz."""
    head = ogg.find(b"OpusHead")
    if head >= 0:
        channels = ogg[head + 9]
    else:
        head = ogg.find(b"\x01vorbis")
        if head < 0:
            raise ValueError("neither Opus nor Vorbis")
        channels = ogg[head + 11]
    r = subprocess.run([ffmpeg, "-v", "error", "-i", "pipe:0", "-f", "f32le", "-acodec", "pcm_f32le", "-ar", str(RATE), "pipe:1"],
                       input=ogg, capture_output=True, check=False)
    if r.returncode:
        raise RuntimeError("ffmpeg: " + r.stderr.decode(errors="replace").strip())
    return np.frombuffer(r.stdout, "<f4").reshape(-1, channels), channels


class Wems:
    """A .wem's samples, from a bank or streamed: Opus as is, Wwise Vorbis repacked (codebooks from
    the game's executable, read only if needed)."""

    def __init__(self, gf, game_dir, ffmpeg):
        self.gf, self.game_dir, self.ffmpeg = gf, game_dir, ffmpeg
        self.books = None
        self.cache = {}

    def samples(self, wem_id, banks=()):
        if wem_id not in self.cache:
            w = next((b.wem(wem_id) for b in banks if b.wem(wem_id) is not None), None) or self.gf.streamed_wem(wem_id)
            if w is None:
                sys.exit(f".wem {wem_id} not found")
            ww = sys.modules["riftapart_wwise"]
            fmt = ww.wem_format(w)
            if fmt == 0x3040:
                ogg = ww.wem_opus(w)
            elif fmt == 0xFFFF:
                if self.books is None:
                    self.books = ww.find_vorbis_codebooks(os.path.join(self.game_dir, "RiftApart.exe"))
                ogg = ww.wem_vorbis_to_ogg(w, self.books)[0]
            else:
                sys.exit(f".wem {wem_id}: format {fmt:#x} not handled")
            self.cache[wem_id] = decode_ogg(self.ffmpeg, ogg)
        return self.cache[wem_id]


def downmix(x):
    """ffmpeg's plain -ac 2 (16-bit output): back channels at -3 dB into the fronts, normalised."""
    n = x.shape[1]
    if n == 1:
        return np.repeat(x, 2, axis=1)
    if n == 2:
        return x
    if n == 4:  # quad: FL FR BL BR
        c = math.sqrt(0.5)
        return np.stack([x[:, 0] + c * x[:, 2], x[:, 1] + c * x[:, 3]], axis=1) / (1.0 + c)
    raise ValueError(f"{n} channels: no downmix here")


def fade_in(x, seconds, curve):
    n = min(len(x), int(round(seconds * RATE)))
    t = np.arange(n) / max(1, n)
    g = {"sine": np.sin(t * math.pi / 2), "linear": t}.get(curve)
    if g is None:
        raise ValueError(f"fade curve {curve} not handled")
    y = x.copy()
    y[:n] *= g[:, None]
    return y


def write_wav(path, x):
    pcm = np.clip(np.round(x * 32767.0), -32768, 32767).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(x.shape[1])
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())
    return {"samples": len(x), "seconds": round(len(x) / RATE, 3), "peak_dbfs": round(20 * math.log10(max(1e-9, np.abs(x).max())), 1)}


def export_music(gf, wems, ui_bank, out_dir):
    music = Music([ui_bank, gf.soundbank(MUSIC_BANK)])
    playlist = find_playlist(music, MENU_EVENT)
    pair = first_variant(music.playlist(playlist))
    intro = [s for s, loop in pair if loop != 0]
    looped = [s for s, loop in pair if loop == 0]
    if len(intro) != 1 or len(looped) != 1:
        sys.exit(f"playlist {playlist}: expected one intro and one looping segment, got {pair}")

    def source(wem_id):
        return wems.samples(wem_id)

    info = {"event": MENU_EVENT, "bank": UI_BANK, "trigger": "PauseMenuShowEvent (conduit/ui_sounds.conduit)",
            "stop_events": STOP_EVENTS, "playlist": playlist,
            "variant": "the playlist's first intro + loop pair (the game picks one of its pairs at random)",
            "note": "the pause menu's music; that the main menu plays it too isn't confirmed"}
    out = {}
    for name, seg_id in (("intro", intro[0]), ("loop", looped[0])):
        seg, tr, clip, fade = clip_of(music, seg_id)
        x, channels = source(clip["source"])
        # The segment's timeline: the source sits at play_at (negative: it started before the
        # segment), and is heard from begin_trim on (a hair after the segment starts).
        offset = int(round(-clip["play_at_ms"] * RATE / 1000.0))
        head = max(0, int(round((clip["begin_trim_ms"] + clip["play_at_ms"]) * RATE / 1000.0)))
        length = int(round(seg["duration_ms"] * RATE / 1000.0))
        y = downmix(x[offset:offset + length])
        y[:head] = 0.0
        entry = {"file": f"title_{name}.wav", "segment": seg_id, "wem": clip["source"], "source_channels": channels,
                 "starts_at_s": round((offset + head) / RATE, 4), "starts_at_sample": offset + head,
                 "segment_s": round(seg["duration_ms"] / 1000.0, 3), "markers_s": [round(m / 1000.0, 3) for m in seg["markers_ms"]]}
        if fade:  # its times are from the clip's start
            secs = fade["points"][-1][0]
            curve = CURVES.get(fade["points"][0][2], fade["points"][0][2])
            y[head:] = fade_in(y[head:], secs, curve)
            entry["fade_in"] = {"seconds": round(secs, 3), "curve": curve}
        entry.update(write_wav(os.path.join(out_dir, entry["file"]), y))
        if name == "loop":
            # It repeats from its entry marker to its exit marker; with them at 0 and its end (no
            # pre-entry or post-exit), the whole file loops with no gap.
            marks = seg["markers_ms"]
            span = (marks[-1] - marks[0]) if len(marks) >= 2 else seg["duration_ms"]
            entry["seamless_loop_s"] = round(span / 1000.0, 3)
            entry["seamless_loop_samples"] = int(round(span * RATE / 1000.0))
            entry["seamless"] = bool(marks and marks[0] == 0 and entry["samples"] == entry["seamless_loop_samples"])
            level_from = seg["tracks"][0]
        out[name] = entry
    info["game_level_db"] = round(music.gain_db(level_from), 1)
    info["level_note"] = "volume + make-up gain of the track, segment, playlist and its parents (buses not included); not applied to the files"
    info["format"] = {"rate": RATE, "channels": 2, "bits": 16,
                      "downmix": "L = (FL + 0.7071 BL) / 1.7071, R = (FR + 0.7071 BR) / 1.7071 (ffmpeg's plain -ac 2)"}
    info["play"] = "title_intro.wav once, then title_loop.wav over and over (entry 0, exit at its end: no gap)"
    info.update(out)
    return info


def export_sting(wems, ui_bank, out_dir):
    """The press-start sound: its audio layers mixed as the game sets them against each other (the
    loudest at its file's level, the rest by their hierarchy volumes and delays); the DualSense
    rumble layers left out."""
    ww = sys.modules["riftapart_wwise"]
    hier = ww.Hierarchy([ui_bank], {})
    ev = ui_bank.objs[ww.fnv1(STING_EVENT)]
    rumble = sum(1 for aid in ev["actions"]
                 if ui_bank.objs.get(ui_bank.objs.get(aid, {}).get("target"), {}).get("codec") == ww.CODEC_HAPTICS)
    layers = []
    for layer in hier.event(STING_EVENT):
        bk, ob = next(((bk, ob) for bk, ob in layer["sounds"] if ob["codec"] != ww.CODEC_SILENCE), (None, None))
        if ob is None:
            continue
        x, channels = wems.samples(ob["source"], [bk])
        layers.append({"wem": ob["source"], "x": downmix(x), "channels": channels, "level_db": hier.gain_db(ob["id"]),
                       "delay_s": layer["delay"] + hier.initial_delay(ob["id"]), "variations": len(layer["sounds"])})
    if not layers:
        sys.exit(f"{STING_EVENT}: no audio layers")
    top = max(lay["level_db"] for lay in layers)
    n = max(int(round(lay["delay_s"] * RATE)) + len(lay["x"]) for lay in layers)
    mix = np.zeros((n, 2), np.float32)
    for lay in layers:
        at = int(round(lay["delay_s"] * RATE))
        mix[at:at + len(lay["x"])] += lay["x"] * 10 ** ((lay["level_db"] - top) / 20)
    info = {"file": "intro_sting.wav", "event": STING_EVENT, "bank": UI_BANK,
            "trigger": "UIFrontEndPressStartEvent, UIFrontEndPressContinueEvent (conduit/ui_sounds.conduit)",
            "layers": [{"wem": lay["wem"], "source_channels": lay["channels"], "seconds": round(len(lay["x"]) / RATE, 3),
                        "level_db": round(lay["level_db"], 1), "in_mix_db": round(lay["level_db"] - top, 1),
                        "delay_s": round(lay["delay_s"], 3), "variations": lay["variations"]} for lay in layers],
            "left_out": f"{rumble} DualSense rumble layers",
            "game_level_db": round(top, 1),
            "level_note": "the loudest layer's hierarchy volume (buses not included); not applied to the file"}
    info.update(write_wav(os.path.join(out_dir, info["file"]), mix))
    return info


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    sys.path.insert(0, here)
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--game", required=True, help="Rift Apart install folder (has 'toc' and 'd')")
    ap.add_argument("--alert", required=True, help="folder of a clone of github.com/Tkachov/ALERT")
    ap.add_argument("--out", default=None, help="output folder (default: riftapart-assets/menu, the main checkout's from a worktree)")
    ap.add_argument("--gdeflate-dll", default=None, help="build it with: cargo build -p gdeflate-ffi --release")
    a = ap.parse_args()
    out_dir = a.out or default_out_dir()
    dll = a.gdeflate_dll or main_checkout_path("target", "release", "gdeflate_ffi.dll")
    if not os.path.exists(dll):
        sys.exit(f"no GDeflate decoder at {dll} (cargo build -p gdeflate-ffi --release)")
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        sys.exit("ffmpeg not found on PATH (the menu music is Opus)")
    os.makedirs(out_dir, exist_ok=True)

    from extract_riftapart_ratchet import Game

    with contextlib.redirect_stdout(io.StringIO()):  # the toc reader's size note
        game = Game(a.game, a.alert, dll)
    manifest = {"logo": export_logo(game, out_dir), "title_card": export_title_card(game, out_dir)}
    print(f"  logo        {manifest['logo']['size'][0]} x {manifest['logo']['size'][1]}  ({manifest['logo']['source']})")
    print(f"  title card  {CARD_SIZE[0]} x {CARD_SIZE[1]}  ({TITLE_CARD})")
    import riftapart_wwise as ww

    gf = ww.GameFiles(a.game, a.alert)
    ui_bank = gf.soundbank(UI_BANK)
    wems = Wems(gf, a.game, ffmpeg)
    manifest["music"] = export_music(gf, wems, ui_bank, out_dir)
    m = manifest["music"]
    for k in ("intro", "loop"):
        print(f"  title_{k:5}  {m[k]['seconds']:8.3f} s  (segment {m[k]['segment']}, .wem {m[k]['wem']}, from {m[k]['starts_at_s']} s)")
    print(f"  music level in the game: {m['game_level_db']} dB (not applied)")
    s = manifest["sting"] = export_sting(wems, ui_bank, out_dir)
    print(f"  intro_sting  {s['seconds']:8.3f} s  peak {s['peak_dbfs']} dBFS  ({len(s['layers'])} layers mixed, {s['left_out']}; "
          f"game level {s['game_level_db']} dB, not applied)")
    with open(os.path.join(out_dir, "menu.json"), "w") as f:
        json.dump(manifest, f, indent=1)
    print(f"wrote {out_dir}")


if __name__ == "__main__":
    main()
