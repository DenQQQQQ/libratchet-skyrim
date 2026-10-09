# SPDX-License-Identifier: GPL-3.0-or-later
"""Rift Apart's title card as Skyrim's startup video (Data/Video/BGS_Logo.bik), from
riftapart-assets/menu (tools/extract_riftapart_menu.py makes it from YOUR OWN Rift Apart install).

Skyrim SE plays a Bink 1 video at launch (BGS_Logo.bik: 1920 x 1080 at 23.976 frames a second);
this one is the game's own title card (menu/title_card.png: its logo on a rift nebula) coming
out of black on a soft blue flash, pulling back slowly, and fading to black again; its sound is
the game's press-start sting (menu/intro_sting.wav) as it appears. Without the card, its logo
alone grows in out of a drawn rift field. The frames are made here; RAD's own encoder (RAD Video
Tools' RADVideo64.exe, the owner's download) makes the Bink file.

The output is game data: it stays on this PC (the build installs it into the MO2 mod), never in
the repository.

  uv run --with numpy --with pillow --with soundfile skse/tools/ratchet_intro_to_skyrim.py --rad <RADVideo64.exe> --out <dir>
"""
import argparse
import os
import shutil
import subprocess
import sys

import numpy as np
from PIL import Image, ImageFilter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ratchet_menu_to_skyrim as M  # noqa: E402

WIDTH, HEIGHT = 1920, 1080
FPS = 24000 / 1001  # the vanilla video's
SECONDS = 5.5
# Ours: the logo's width on screen, and the card's timing (seconds).
LOGO_WIDTH = 0.56
OPEN = (0.0, 0.7)          # the rift field fades in
LOGO_IN = (0.45, 1.35)     # the logo grows and fades in, on a flash
SWEEP = (2.0, 2.8)         # a light across it
FADE_OUT = (4.5, 5.5)


def ramp(t, span):
    a, b = span
    return float(np.clip((t - a) / (b - a), 0.0, 1.0))


def smooth(x):
    return x * x * (3.0 - 2.0 * x)


def background(t, grid):
    """The rift field: violet to near black from the middle, a blue core breathing slowly."""
    x, y = grid
    r = np.sqrt((x * 1.6) ** 2 + y ** 2)
    core = np.exp(-(r / (0.35 + 0.03 * np.sin(t * 2.2))) ** 2)
    halo = np.exp(-(r / 1.1) ** 2)
    # Streaks round the middle, turning slowly (the rift's swirl).
    angle = np.arctan2(y, x * 1.6)
    swirl = 0.5 + 0.5 * np.sin(angle * 7.0 + r * 9.0 - t * 1.4)
    swirl *= np.exp(-((r - 0.45) / 0.25) ** 2)
    img = np.zeros((HEIGHT, WIDTH, 3))
    img += halo[..., None] * np.array([0.13, 0.04, 0.22])
    img += swirl[..., None] * np.array([0.10, 0.05, 0.25]) * 0.8
    img += core[..., None] * np.array([0.10, 0.35, 0.75]) * 0.55
    return img


def card_frames(card, out_dir):
    """The title card: out of black on a soft blue flash, pulling back from 1.08 to 1.0 the whole
    time, into black at the end."""
    os.makedirs(out_dir, exist_ok=True)
    card = card.convert("RGB").resize((WIDTH, HEIGHT), Image.LANCZOS)
    yy, xx = np.mgrid[0:HEIGHT, 0:WIDTH]
    glow = np.exp(-((((xx - WIDTH / 2) / (HEIGHT / 2)) * 1.2) ** 2 + ((yy - HEIGHT / 2) / (HEIGHT / 2)) ** 2) / 0.25)[..., None] * np.array([0.3, 0.6, 1.0])
    count = int(round(SECONDS * FPS))
    for i in range(count):
        t = i / FPS
        zoom = 1.08 - 0.08 * smooth(t / SECONDS)
        w, h = int(round(WIDTH * zoom)), int(round(HEIGHT * zoom))
        x0, y0 = (w - WIDTH) // 2, (h - HEIGHT) // 2
        img = np.asarray(card.resize((w, h), Image.BICUBIC).crop((x0, y0, x0 + WIDTH, y0 + HEIGHT))).astype(np.float64) / 255.0
        light = smooth(ramp(t, OPEN)) * (1.0 - smooth(ramp(t, FADE_OUT)))
        flash = max(0.0, 1.0 - abs(t - 0.55) / 0.35) * 0.35
        img = img * light + glow * flash
        Image.fromarray((np.clip(img, 0.0, 1.0) * 255).astype(np.uint8), "RGB").save(os.path.join(out_dir, f"intro{i + 1:04}.png"))
    return count


def frames(logo, out_dir):
    os.makedirs(out_dir, exist_ok=True)
    yy, xx = np.mgrid[0:HEIGHT, 0:WIDTH]
    grid = ((xx - WIDTH / 2) / (HEIGHT / 2), (yy - HEIGHT / 2) / (HEIGHT / 2))
    count = int(round(SECONDS * FPS))
    base_w = int(WIDTH * LOGO_WIDTH)
    for i in range(count):
        t = i / FPS
        opened = smooth(ramp(t, OPEN))
        img = background(t, grid) * opened
        appear = smooth(ramp(t, LOGO_IN))
        # The flash as it comes in: a blue bloom peaking a third of the way.
        flash = max(0.0, 1.0 - abs(ramp(t, LOGO_IN) - 0.33) / 0.33) if LOGO_IN[0] <= t <= LOGO_IN[1] else 0.0
        img += np.exp(-((grid[0] * 1.2) ** 2 + grid[1] ** 2) / 0.12)[..., None] * np.array([0.3, 0.6, 1.0]) * flash * 0.9
        frame = Image.fromarray((np.clip(img, 0.0, 1.0) * 255).astype(np.uint8), "RGB")
        if appear > 0.0:
            scale = (1.18 - 0.18 * appear) * (1.0 + 0.025 * ramp(t, (LOGO_IN[1], FADE_OUT[1])))
            w = int(base_w * scale)
            h = int(w * logo.size[1] / logo.size[0])
            sized = logo.resize((w, h), Image.LANCZOS)
            # A light sweeping across it (brightened along a moving diagonal band, its alpha kept).
            s = ramp(t, SWEEP)
            if 0.0 < s < 1.0:
                a = np.asarray(sized).astype(np.float64)
                u = np.arange(w)[None, :] / w + np.arange(h)[:, None] / h * 0.35
                band = np.exp(-((u - (s * 1.6 - 0.3)) / 0.06) ** 2)
                a[..., :3] = np.clip(a[..., :3] + band[..., None] * 140.0, 0, 255)
                sized = Image.fromarray(a.astype(np.uint8), "RGBA")
            alpha = sized.getchannel("A").point(lambda v, k=appear: int(v * k))
            sized.putalpha(alpha)
            # Its glow behind it.
            glow = Image.new("RGBA", sized.size, (90, 170, 255, 0))
            glow.putalpha(alpha.filter(ImageFilter.GaussianBlur(18)).point(lambda v: int(v * 0.55)))
            at = ((WIDTH - w) // 2, (HEIGHT - h) // 2 - int(HEIGHT * 0.02))
            frame.paste(glow, at, glow)
            frame.paste(sized, at, sized)
        fade = 1.0 - smooth(ramp(t, FADE_OUT))
        if fade < 1.0:
            frame = Image.fromarray((np.asarray(frame).astype(np.float64) * fade).astype(np.uint8), "RGB")
        frame.save(os.path.join(out_dir, f"intro{i + 1:04}.png"))
    return count


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--menu", default=None, help="the extracted menu art (default: riftapart-assets\\menu)")
    ap.add_argument("--rad", required=True, help="RAD Video Tools' RADVideo64.exe")
    ap.add_argument("--work", default=None, help="folder for the frames (default: <out>\\..\\intro-frames)")
    ap.add_argument("--out", required=True, help="folder laid out like Skyrim's Data folder")
    a = ap.parse_args()
    src = a.menu or M.default_menu_dir()
    card_png = os.path.join(src, "title_card.png")
    logo_png = os.path.join(src, "logo.png")
    work = a.work or os.path.join(os.path.dirname(os.path.abspath(a.out)), "intro-frames")
    shutil.rmtree(work, ignore_errors=True)
    if os.path.exists(card_png):
        count = card_frames(Image.open(card_png), work)
    elif os.path.exists(logo_png):
        logo = Image.open(logo_png).convert("RGBA")
        logo = logo.crop(logo.getchannel("A").point(lambda v: 255 if v > 8 else 0).getbbox() or (0, 0, *logo.size))
        count = frames(logo, work)
    else:
        sys.exit(f"no {card_png} or {logo_png} (tools/extract_riftapart_menu.py makes them)")
    # (RAD's tools want whole Windows paths: a relative one, or forward slashes, fail.)
    def full(p):
        return os.path.normpath(os.path.abspath(p))

    video_dir = full(os.path.join(a.out, "Video"))
    os.makedirs(video_dir, exist_ok=True)
    silent = full(os.path.join(work, "intro_silent.bik"))
    out = os.path.join(video_dir, "BGS_Logo.bik")
    # Bink 1 (/V100), at the vanilla video's rate, overwriting, no questions or windows (/#).
    cmd = [full(a.rad), "Bink", full(os.path.join(work, f"intro????.png")) + f"*1-{count}", silent, "/V100", f"/F{FPS:.3f}", "/O", "/#"]
    subprocess.run(cmd, check=True)
    sting = full(os.path.join(src, "intro_sting.wav"))
    if os.path.exists(sting):
        # As the card appears (frame 7, 0.3 s in), perceptibly lossless.
        subprocess.run([full(a.rad), "BinkMix", silent, sting, out, "/L4", "/N7", "/O", "/#"], check=True)
    else:
        shutil.copyfile(silent, out)
    with open(out, "rb") as f:
        head = f.read(4)
    assert head[:3] == b"BIK", f"{out}: not a Bink 1 file ({head!r})"
    print(f"{out}: {count} frames, {os.path.getsize(out) / 1e6:.1f} MB{' with the sting' if os.path.exists(sting) else ', silent'}")


if __name__ == "__main__":
    main()
