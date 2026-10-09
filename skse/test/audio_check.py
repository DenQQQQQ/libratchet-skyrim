# SPDX-License-Identifier: GPL-3.0-or-later
"""Checks a recording of Skyrim's own audio (test/audio_capture.cpp) for what sounds like crackling:
samples over full scale (the mix clipping), the loudest moments, and dropouts (silence in the
middle of a loud passage). Each moment is lined up with the plugin's log.

    uv run --with numpy python skse/test/audio_check.py <audio.wav> [RatchetMovement.log] [--after <regex> [seconds]]

--after: the loudness every 50 ms for that long (default 2 s) after the first log line matching
the regex, and the strongest rate it pulses at (the glide's rotor chops: about 11 a second).
--each <regex> [seconds]: for every log line matching, the loudness of each channel over that long
(default 1 s) from 0.25 s after it, and in dB from the first (the sound lab's steps).
"""
import re
import struct
import sys
from datetime import datetime, timedelta

import numpy as np


def read_wav(path):
    data = open(path, "rb").read()
    assert data[:4] == b"RIFF" and data[8:12] == b"WAVE", "not a WAV"
    pos, fmt, samples = 12, None, None
    while pos + 8 <= len(data):
        kind, size = data[pos:pos + 4], struct.unpack_from("<I", data, pos + 4)[0]
        body = data[pos + 8:pos + 8 + size]
        if kind == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", body)
        elif kind == b"data":
            samples = body
        pos += 8 + size + (size & 1)
    tag, channels, rate, _, _, bits = fmt
    assert tag == 3 and bits == 32, "expected 32-bit float"
    return np.frombuffer(samples, dtype="<f4").reshape(-1, channels), rate


def read_log(path, day):
    events = []
    if not path:
        return events
    for line in open(path, encoding="utf-8", errors="replace"):
        m = re.match(r"\[(\d\d):(\d\d):(\d\d)\.(\d{3})\] \[\d+\] \[\w\] (.*)", line)
        if m and re.match(r"(autotest: |combat: |sounds: )", m.group(5)):
            h, mi, s, ms = (int(x) for x in m.groups()[:4])
            events.append((day.replace(hour=h, minute=mi, second=s, microsecond=ms * 1000), m.group(5).strip()))
    return events


def main():
    # <wav> [log] then the options, each a flag, a regex and maybe seconds.
    args, opts, rest = [], {}, sys.argv[1:]
    while rest:
        a = rest.pop(0)
        if a in ("--after", "--each"):
            pattern = rest.pop(0)
            span = float(rest.pop(0)) if rest and not rest[0].startswith("--") else (2.0 if a == "--after" else 1.0)
            opts[a] = (pattern, span)
        else:
            args.append(a)
    after, each = opts.get("--after"), opts.get("--each")
    wav = args[0]
    log = args[1] if len(args) > 1 else None
    x, rate = read_wav(wav)
    try:
        start = open(wav + ".start").read().strip()
        start = datetime.combine(datetime.today(), datetime.strptime(start, "%H:%M:%S.%f").time())
    except OSError:
        start = None
    events = read_log(log, start) if start else []

    def when(t):
        """The recording's second t as the wall clock, and the log line just before it."""
        if not start:
            return f"{t:7.2f} s"
        at = start + timedelta(seconds=t)
        before = [e for e in events if e[0] <= at]
        what = f"  <- {before[-1][1][:90]} ({(at - before[-1][0]).total_seconds():+.2f} s)" if before else ""
        return f"{at:%H:%M:%S}.{at.microsecond // 1000:03d}{what}"

    peak = float(np.abs(x).max()) if len(x) else 0.0
    over = int((np.abs(x) > 1.0).sum())
    full = int((np.abs(x) >= 0.999).sum())
    print(f"{wav}: {len(x) / rate:.1f} s, peak {peak:.3f} ({20 * np.log10(max(peak, 1e-9)):+.1f} dBFS), "
          f"{over} samples over full scale, {full} at or over 0.999")

    # The loudest 20 ms windows (one per 0.5 s, so a long blast counts once).
    win = rate // 50
    n = len(x) // win
    if n:
        w = np.abs(x[: n * win]).reshape(n, win, -1).max(axis=(1, 2))
        rms = np.sqrt((x[: n * win] ** 2).reshape(n, win, -1).mean(axis=(1, 2)))
        order = np.argsort(-w)
        shown, taken = 0, []
        print("loudest moments:")
        for i in order:
            if shown >= 12 or w[i] < 0.05:
                break
            if any(abs(i - j) < 25 for j in taken):
                continue
            taken.append(i)
            shown += 1
            clipped = int((np.abs(x[i * win:(i + 1) * win]) > 1.0).sum())
            print(f"  peak {w[i]:.3f} rms {rms[i]:.3f}{f' CLIPPED x{clipped}' if clipped else ''} at {when(i * win / rate)}")

        # Dropouts: a window of near silence between loud ones (the mix starving, not a sound ending).
        quiet = rms < 1e-4
        loud = rms > 0.05
        drops = [i for i in range(1, n - 1) if quiet[i] and loud[i - 1] and loud[i + 1]]
        print(f"dropouts (a silent 20 ms between loud ones): {len(drops)}")
        for i in drops[:10]:
            print(f"  at {when(i * win / rate)}")

    if each and start:
        pattern, span = each
        rows = []
        for when_, text in events:
            if not re.search(pattern, text):
                continue
            t0 = (when_ - start).total_seconds() + 0.25
            a, b = int(t0 * rate), int((t0 + span) * rate)
            seg = x[max(a, 0):min(b, len(x))]
            if len(seg) == 0:
                continue
            rms = np.sqrt((seg ** 2).mean(axis=0))
            rows.append((text, rms))
        if rows:
            ref = float(np.sqrt((rows[0][1] ** 2).mean()))
            print(f"each '{pattern}' ({span:.1f} s from 0.25 s after it): rms left, right, dB from the first")
            for text, rms in rows:
                both = float(np.sqrt((rms ** 2).mean()))
                db = 20 * np.log10(max(both, 1e-9) / max(ref, 1e-9))
                print(f"  {rms[0]:.4f} {rms[-1]:.4f} {db:+6.1f} dB  {text[:90]}")

    if after and start:
        pattern, span = after
        hits = [e for e in events if re.search(pattern, e[1])]
        if not hits:
            print(f"no log line matches {pattern!r}")
            return
        t0 = (hits[0][0] - start).total_seconds()
        a, b = int(t0 * rate), int((t0 + span) * rate)
        seg = x[max(a, 0):min(b, len(x))].mean(axis=1)
        step = rate // 20
        env = [float(np.sqrt((seg[i:i + step] ** 2).mean())) for i in range(0, len(seg) - step + 1, step)]
        print(f"after '{hits[0][1][:80]}': rms every 50 ms over {span:.1f} s:")
        print("  " + " ".join(f"{v:.3f}" for v in env))
        # How it pulses: the envelope's strongest rate between 2 and 40 Hz (5 ms windows).
        fine = rate // 200
        e = np.array([np.sqrt((seg[i:i + fine] ** 2).mean()) for i in range(0, len(seg) - fine + 1, fine)])
        if len(e) > 40:
            spec = np.abs(np.fft.rfft(e - e.mean()))
            freqs = np.fft.rfftfreq(len(e), 1 / 200)
            band = (freqs >= 2) & (freqs <= 40)
            if band.any():
                f = freqs[band][np.argmax(spec[band])]
                print(f"  pulses strongest at {f:.1f} Hz (every {1000 / f:.0f} ms)")


if __name__ == "__main__":
    main()
