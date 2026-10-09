# SPDX-License-Identifier: GPL-3.0-or-later
# Extracts the game sounds the sandbox uses from YOUR OWN Rift Apart install into
# riftapart-assets/sounds/. Uses ALERT (GPLv3) through riftapart_wwise.py. The output is game
# data: keep it local (riftapart-assets/ is gitignored), never commit or share it.
#
# Each gameplay moment below lists the Wwise events the game plays for it (found in the animation
# clips' Sound events, the *_aud conduits and the soundbanks' event names). An event is followed
# through the banks' hierarchy with the switch values chosen here (Ratchet in his default armour
# with Clank on his back, on concrete) and flattened into layers that play together; each layer
# plays one of its variation files.
#
# Wwise Vorbis files are repacked losslessly into Ogg Vorbis, using the codebook library built into
# the game's sound engine (found by scanning RiftApart.exe). Opus files are converted to Ogg Vorbis
# with the ffmpeg on PATH (default; kept as .opus if ffmpeg is missing), kept as .opus
# (--opus keep) or left out (--opus skip).
#
# Output: sounds/<moment>/<wem id>.ogg and sounds/sounds.json:
#   {"moments": {"<moment>": {"event": Wwise event, "trigger": "clip" | "footstep" | "gameplay",
#                             "level_db", optional "chance" (0..1, the chance it plays at all),
#                             "events" (if several), "layers": [{"files": [...], "volume": linear
#                             gain, "loop": bool, "event": source event, optional "chance",
#                             "delay" s, "interval" s (continuous containers: next variation
#                             after this), "sequence", "pitch" [lo, hi] cents, "volumes" (per
#                             file)}]}},
#    "events": {"<Wwise event name>": "<moment>"},
#    "silent_events": [clip events that exist in no soundbank]}
# Layers play together; each plays one variation (a random file, in order for "sequence").
# Volumes come from the sounds' Wwise hierarchy (volume + make-up gain) relative to the moment's
# loudest file (1.0); "level_db" is that file's hierarchy level. The game's buses, HDR mixing and
# game-parameter curves aren't modelled, so balance moments against each other by ear.
#
# Usage:
#   python tools/extract_riftapart_sounds.py --game "<Rift Apart folder>" --alert "<ALERT clone>"
#          [--out riftapart-assets] [--opus ffmpeg|keep|skip] [--moments a,b,...] [--lang us]
import argparse
import concurrent.futures
import json
import os
import shutil
import subprocess
import sys

BANK = "sound/banks/{}.soundbank"

# Switch picks, group -> value (names; Wwise ids are their FNV-1 hashes).
SWITCHES = {
    "VAN_attire_legs": "Ratchet_Default",
    "VAN_attire_torso": "Ratchet_Default",
    "ClankBackpack_Type": "FullClank",  # Clank rides on Ratchet's back
    "HERO_CHARACTER": "Ratchet",
    "Footfell_Speed": "run",
    "ground_material": "kConcrete",
    "material": "kConcrete",
    "melee_weapon": "Wrench",
    0x3FBD3098: "RATC",  # the voice-effort character (group name unknown)
}

# Voice efforts: the event bank only points into per-character media banks (localized).
VOICE = {"banks": ["vfx_switch_ratc_media"], "trigger": "clip"}

# moment -> events as (bank, event), how the sandbox triggers it ("clip": an animation clip's
# Sound event of that name; "footstep": the clips' Footstep events; "gameplay": code), optional
# switch overrides and extra banks the events point into (looked up in the --lang archive).
# Clip-fired events get a moment each, so firing them from clips never doubles a sound.
MOMENTS = {
    "run_foley": {"events": [("ratchet", "Play_FOL_ratchet_run")], "trigger": "clip"},
    "walk_foley": {"events": [("ratchet", "Play_FOL_ratchet_walk")], "trigger": "clip"},
    "jump": {"events": [("ratchet", "Play_FOL_ratchet_jump")], "trigger": "clip"},
    "jump_swish": {"events": [("ratchet", "Play_FOL_ratchet_jump_swish")], "trigger": "clip"},
    "double_jump": {"events": [("ratchet", "Play_FOL_ratchet_jump_double_swish")], "trigger": "clip"},
    "land": {"events": [("ratchet", "Play_FOL_ratchet_land")], "trigger": "clip"},
    "swing_foley": {"events": [("ratchet", "Play_FOL_ratchet_swing")], "trigger": "clip"},
    "boot_tap": {"events": [("ratchet", "Play_FOL_ratchet_boot_tap")], "trigger": "clip"},
    "voice_jump": {"events": [("vfx_switch_master_heroes", "HERO_VFX_SWITCH_JUMP")], **VOICE},
    "voice_land": {"events": [("vfx_switch_master_heroes", "HERO_VFX_SWITCH_LAND")], **VOICE},
    "voice_melee": {"events": [("vfx_switch_master_heroes", "HERO_VFX_SWITCH_MELEE")], **VOICE},
    "voice_melee_heavy": {"events": [("vfx_switch_master_heroes", "HERO_VFX_SWITCH_MELEE_HEAVY")], **VOICE},
    "voice_throw": {"events": [("vfx_switch_master_heroes", "HERO_VFX_SWITCH_THROW")], **VOICE},
    # The wrench's own clips (swings, smash) fire these.
    "wrench_swing": {"events": [("wpn_wrench", "Play_WPN_Wrench_Swing")], "trigger": "clip"},
    "wrench_slam_base": {"events": [("wpn_wrench", "Play_WPN_MeleeWeapon_Slam_Impact_Base")], "trigger": "clip"},
    "wrench_slam_sweetener": {"events": [("cp11_sweeteners", "Play_WPN_gen_sweetener_solid_transient")], "trigger": "clip"},
    "clank_arm_unfold": {"events": [("hero_clank", "Play_FOL_clank_arm_unfold")], "trigger": "clip"},
    "clank_arm_fold": {"events": [("hero_clank", "Play_FOL_clank_arm_fold")], "trigger": "clip"},
    # The test dummy's idle and taunt clips fire a civilian chatter event.
    "dummy_chatter": {"events": [("chr_npc_nefity_civ", "Play_CHR_npc_nefcity_civ_chatter_sometimes")], "trigger": "clip"},
    # Footsteps on concrete; the game plays the same event with another speed for jumps/landings.
    "footstep": {"events": [("ratchet", "Play_FOOT_Ratchet_Master_Player")], "trigger": "footstep"},
    "footstep_jump": {"events": [("ratchet", "Play_FOOT_Ratchet_Master_Player")], "trigger": "gameplay",
                      "switches": {"Footfell_Speed": "Jump"}},
    "footstep_land": {"events": [("ratchet", "Play_FOOT_Ratchet_Master_Player")], "trigger": "gameplay",
                      "switches": {"Footfell_Speed": "LandMed"}},
    "heli_extend": {"events": [("gad_helicopter", "Play_GAD_Helicopter_v3_extend")], "trigger": "gameplay"},
    "heli_loop": {"events": [("gad_helicopter", "Play_GAD_Helicopter_top_blade_LP")], "trigger": "gameplay"},
    "heli_retract": {"events": [("gad_helicopter", "Play_GAD_Helicopter_v3_retract")], "trigger": "gameplay"},
    "phantom_dash": {"events": [("gad_phasedash", "Play_GAD_Phase_Dash_Move")], "trigger": "gameplay"},
    "phantom_dash_end": {"events": [("gad_phasedash", "Play_GAD_Phase_Dash_Move_End")], "trigger": "gameplay"},
    "phantom_dash_pass": {"events": [("gad_phasedash", "Play_GAD_Phase_Dash_Pass")], "trigger": "gameplay"},
    "air_dash": {"events": [("gad_airdash", "Play_AirDash")], "trigger": "gameplay"},
    # A bot hit (the test dummy is a bot): the wrench's mechanical impact + the enemy hit confirm.
    "wrench_hit": {"events": [("wpn_wrench", "Play_WPN_MeleeWeapon_Melee_Hit_Mechanical"),
                              ("wpn_global", "Play_WPN_Global_Hit_Melee_Enemy")], "trigger": "gameplay"},
    "wrench_slam": {"events": [("wpn_wrench", "Play_WPN_MeleeWeapon_Slam_Impact")], "trigger": "gameplay"},
    "wrench_throw": {"events": [("wpn_wrench", "Play_WPN_MeleeWeapon_Toss")], "trigger": "gameplay"},
    "wrench_throw_loop": {"events": [("wpn_wrench", "Play_WPN_MeleeWeapon_Toss_Loop")], "trigger": "gameplay"},
    "wrench_catch": {"events": [("wpn_wrench", "Play_WPN_MeleeWeapon_Catch")], "trigger": "gameplay"},
    "pistol_shot": {"events": [("wpn_burst_pistol", "Play_WPN_Burst_Pistol_shot")], "trigger": "gameplay"},
    "bolt_impact": {"events": [("wpn_burst_pistol", "Play_WPN_Burst_Pistol_projectile_hit")], "trigger": "gameplay"},
    "grenade_throw": {"events": [("wpn_protogrenade", "Play_WPN_ProroGrenade_throw")], "trigger": "gameplay"},
    "grenade_explode": {"events": [("wpn_protogrenade", "Play_WPN_ProtoGrenade_Explo")], "trigger": "gameplay"},
    # Bolts, the currency (the pup_bolt_aud conduit): collecting one (its PickupBoltEvent with the
    # default BoltId; the other ids are the unlockable coin, orb, ruby and overcharge looks), and the
    # sound of one flying to Ratchet (FlewByStartEvent; FlewByStopEvent's Stop event has no actions).
    # The conduit has no event for a bolt dropping or bouncing.
    "bolt_collect": {"events": [("pup_bolts", "Play_PUP_bolt_collect")], "trigger": "gameplay"},
    "bolt_approach": {"events": [("pup_bolts", "Play_OBJ_Pickup_Bolt_Approach")], "trigger": "gameplay"},
}

# Sound events the sandbox's clips fire that exist in no soundbank (silent in the game too).
SILENT_EVENTS = ["Play_cha_ratchet_voc_jump", "Play_cha_test_dummy_beeps", "Play_cha_test_dummy_death_impact",
                 "Play_cha_test_dummy_servo", "Play_cha_test_dummy_swipe", "Play_cha_test_dummy_vocalizations"]


def switch_ids(picks):
    from riftapart_wwise import fnv1
    return {(g if isinstance(g, int) else fnv1(g)): fnv1(v) for g, v in picks.items()}


def ffmpeg_to_vorbis(ffmpeg, opus_bytes, tmp_dir, name):
    src = os.path.join(tmp_dir, name + ".opus")
    dst = os.path.join(tmp_dir, name + ".ogg")
    with open(src, "wb") as f:
        f.write(opus_bytes)
    r = subprocess.run([ffmpeg, "-y", "-loglevel", "error", "-i", src, "-c:a", "libvorbis", "-q:a", "6", dst],
                       capture_output=True, text=True)
    os.remove(src)
    if r.returncode != 0 or not os.path.exists(dst):
        raise RuntimeError("ffmpeg failed: " + (r.stderr.strip() or "no output"))
    with open(dst, "rb") as f:
        out = f.read()
    os.remove(dst)
    return out


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    sys.path.insert(0, here)
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True)
    ap.add_argument("--alert", required=True)
    ap.add_argument("--out", default="riftapart-assets")
    ap.add_argument("--opus", default="ffmpeg", choices=["ffmpeg", "keep", "skip"],
                    help="Opus sounds: convert to Ogg Vorbis with ffmpeg, keep as .opus, or leave out")
    ap.add_argument("--moments", help="comma-separated subset (merged into an existing sounds.json)")
    ap.add_argument("--lang", default="us", help="language archive for voice lines (us, de, fr, ...)")
    ap.add_argument("--exe", help="game executable to read the Vorbis codebooks from (default: <game>/RiftApart.exe)")
    a = ap.parse_args()

    import riftapart_wwise as ww

    names = list(MOMENTS) if not a.moments else [m.strip() for m in a.moments.split(",")]
    unknown = [m for m in names if m not in MOMENTS]
    if unknown:
        sys.exit("unknown moments: " + ", ".join(unknown))
    opus_mode = a.opus
    ffmpeg = shutil.which("ffmpeg") if opus_mode == "ffmpeg" else None
    if opus_mode == "ffmpeg" and not ffmpeg:
        print("[!] ffmpeg not found on PATH: keeping Opus sounds as .opus (Bevy can't play them)")
        opus_mode = "keep"

    game = ww.GameFiles(a.game, a.alert)
    books = ww.find_vorbis_codebooks(a.exe or os.path.join(a.game, "RiftApart.exe"))
    print(f"  Vorbis codebooks: {len(books)} found in the game executable")
    sounds_dir = os.path.join(a.out, "sounds")
    tmp_dir = os.path.join(sounds_dir, ".tmp")
    os.makedirs(tmp_dir, exist_ok=True)

    bank_cache = {}

    def bank(name):
        if name not in bank_cache:
            bank_cache[name] = game.soundbank(BANK.format(name), a.lang)
        return bank_cache[name]

    # 1. Follow each moment's events to layers of sounds.
    plans = {}
    for moment in names:
        spec = MOMENTS[moment]
        picks = dict(SWITCHES)
        picks.update(spec.get("switches", {}))
        layers = []
        unresolved = set()
        for bank_name, event in spec["events"]:
            banks = [bank(bank_name)] + [bank(b) for b in spec.get("banks", [])]
            hier = ww.Hierarchy(banks, switch_ids(picks))
            try:
                found = hier.event(event)
            except KeyError:
                print(f"[!] {moment}: event {event} not found in {bank_name}")
                continue
            for layer in found:
                layer["event"] = event
                layer["hier"] = hier
            layers += found
            unresolved.update(hier.assumed)
        plans[moment] = (layers, sorted(unresolved))

    # 2. Fetch and convert every media file once.
    media = {}  # wem id -> (codec name, bytes or None)
    needed = {}
    for layers, _ in plans.values():
        for layer in layers:
            for bk, ob in layer["sounds"]:
                if ob["codec"] != ww.CODEC_SILENCE:
                    needed.setdefault(ob["source"], (bk, ob, layer["hier"]))
    to_ffmpeg = {}
    for wem_id, (bk, ob, hier) in needed.items():
        w = game.streamed_wem(wem_id, a.lang) if ob["stream"] else None
        if w is None:
            w = next((b.wem(wem_id) for b in hier.banks if b.wem(wem_id) is not None), None)
        if w is None:
            w = game.streamed_wem(wem_id, a.lang)
        if w is None:
            print(f"[!] wem {wem_id} not found")
            continue
        try:
            fmt = ww.wem_format(w)
            if fmt == 0xFFFF:
                media[wem_id] = ("vorbis", ww.wem_vorbis_to_ogg(w, books, serial=wem_id)[0])
            elif fmt == 0x3040:
                opus = ww.wem_opus(w)
                if opus_mode == "keep":
                    media[wem_id] = ("opus", opus)
                elif opus_mode == "ffmpeg":
                    to_ffmpeg[wem_id] = opus
            else:
                print(f"[!] wem {wem_id}: unsupported format {fmt:#06x}")
        except (ValueError, KeyError, IndexError) as e:
            print(f"[!] wem {wem_id}: {e}")
    if to_ffmpeg:
        with concurrent.futures.ThreadPoolExecutor(max_workers=os.cpu_count() or 4) as pool:
            jobs = {pool.submit(ffmpeg_to_vorbis, ffmpeg, data, tmp_dir, str(i)): i for i, data in to_ffmpeg.items()}
            for job in concurrent.futures.as_completed(jobs):
                try:
                    media[jobs[job]] = ("opus->vorbis", job.result())
                except RuntimeError as e:
                    print(f"[!] wem {jobs[job]}: {e}")
    shutil.rmtree(tmp_dir, ignore_errors=True)

    # 3. Write the files and sounds.json.
    json_path = os.path.join(sounds_dir, "sounds.json")
    manifest = {"moments": {}, "events": {}}
    if a.moments and os.path.exists(json_path):
        with open(json_path) as f:
            manifest["moments"] = {k: v for k, v in json.load(f).get("moments", {}).items() if k in MOMENTS}
    totals = {"files": 0, "bytes": 0}
    for moment, (layers, unresolved) in plans.items():
        mdir = os.path.join(sounds_dir, moment)
        os.makedirs(mdir, exist_ok=True)
        written = set()
        out_layers = []
        for layer in layers:
            hier = layer["hier"]
            files, gains, first = [], [], None
            total = len(layer["sounds"])
            silent = 0
            for bk, ob in layer["sounds"]:
                if ob["codec"] == ww.CODEC_SILENCE:
                    silent += 1
                    continue
                got = media.get(ob["source"])
                if got is None:
                    continue
                first = first or ob
                ext = "opus" if got[0] == "opus" else "ogg"
                rel = f"{moment}/{ob['source']}.{ext}"
                if rel in files:
                    continue
                if rel not in written:
                    with open(os.path.join(sounds_dir, rel), "wb") as f:
                        f.write(got[1])
                    written.add(rel)
                    totals["files"] += 1
                    totals["bytes"] += len(got[1])
                files.append(rel)
                gains.append(hier.gain_db(ob["id"]))
            if not files:
                continue
            entry = {"files": files, "gains": gains, "loop": bool(layer["loop"]), "event": layer["event"]}
            # The Play action's probability, times the share of non-silent variations.
            entry["chance"] = round(layer["probability"] * (total - silent) / total, 3)
            delay = layer["delay"] + hier.initial_delay(first["id"])
            if delay > 0.0005:
                entry["delay"] = round(delay, 4)
            if layer.get("interval"):
                entry["interval"] = layer["interval"]
            if layer.get("sequence"):
                entry["sequence"] = True
            lo, hi = hier.pitch_range(first["id"])
            if lo or hi:
                entry["pitch"] = [round(lo, 1), round(hi, 1)]
            out_layers.append(entry)
        # Drop files from an earlier run that this one didn't write (e.g. .opus after --opus ffmpeg).
        for fn in os.listdir(mdir):
            if f"{moment}/{fn}" not in written and fn.endswith((".ogg", ".opus")):
                os.remove(os.path.join(mdir, fn))
        # Volumes: the game's mix relies on its buses and HDR, so the hierarchy's volume + make-up gain
        # is only meaningful relative to the moment's other sounds: scale so the loudest file is 1.0.
        level = max((g for layer in out_layers for g in layer["gains"]), default=0.0)
        for i, layer in enumerate(out_layers):
            gains = layer.pop("gains")
            mean = sum(gains) / len(gains)
            layer = {"files": layer.pop("files"), "volume": round(10 ** ((mean - level) / 20), 4), **layer}
            if max(gains) - min(gains) > 1.0:
                layer["volumes"] = [round(10 ** ((g - level) / 20), 4) for g in gains]
            out_layers[i] = layer
        # A chance shared by every layer is the moment's ("chance"); otherwise it stays per layer.
        chances = {layer["chance"] for layer in out_layers}
        for layer in out_layers:
            if len(chances) == 1 or layer["chance"] >= 0.999:
                del layer["chance"]
        spec = MOMENTS[moment]
        info = {"event": spec["events"][0][1], "trigger": spec["trigger"], "level_db": round(level, 1)}
        if len(chances) == 1 and min(chances) < 0.999:
            info["chance"] = min(chances)
        if len(spec["events"]) > 1:
            info["events"] = [e for _, e in spec["events"]]
        if unresolved:
            info["unresolved_switch_groups"] = unresolved
        info["layers"] = out_layers
        manifest["moments"][moment] = info
        print(f"  {moment:22} {len(out_layers):2} layers {len(written):4} files"
              + (f"  chance {info['chance']}" if "chance" in info else ""))
    manifest["moments"] = {m: manifest["moments"][m] for m in MOMENTS if m in manifest["moments"]}
    # Event name -> moment, in table order (the footstep event maps to "footstep").
    for moment in MOMENTS:
        if moment in manifest["moments"]:
            for _, event in MOMENTS[moment]["events"]:
                manifest["events"].setdefault(event, moment)
    manifest["silent_events"] = SILENT_EVENTS
    with open(json_path, "w") as f:
        json.dump(manifest, f, indent=1)

    kinds = {}
    for kind, _ in media.values():
        kinds[kind] = kinds.get(kind, 0) + 1
    skipped = len(needed) - len(media)
    print(f"wrote {totals['files']} files ({totals['bytes'] / 1e6:.1f} MB) for {len(plans)} moments to {sounds_dir}")
    parts = [f"{v} {k}" for k, v in sorted(kinds.items())] + ([f"{skipped} left out"] if skipped else [])
    print("  media: " + ", ".join(parts))


if __name__ == "__main__":
    main()
