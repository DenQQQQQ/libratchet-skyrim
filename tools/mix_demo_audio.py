# SPDX-License-Identifier: MIT
# Lays a recorded sandbox demo's soundtrack under its video: reads the recording's sounds.log
# (every sound the demo played, with its time and volume; loops with their start and stop) and
# mixes those files with ffmpeg into one track, muxed with the video.
#
# Usage:
#   python tools/mix_demo_audio.py <recording dir> <video.mp4> <out.mp4> [--assets riftapart-assets]
import argparse
import os
import subprocess
import tempfile


def video_length(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path],
                         capture_output=True, text=True, check=True).stdout
    return float(out.strip())


def read_log(path, length):
    """(start, file, volume, speed, end or None) per sound; a loop ends at its stop line or the
    end. Lines: seconds, file (or moment for stop), volume, speed, once|loop|stop[, moment]."""
    sounds, open_loops = [], {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 5:
                continue
            t, what, volume, speed, kind = float(parts[0]), parts[1], float(parts[2]), float(parts[3]), parts[4]
            if kind == "once":
                sounds.append([t, what, volume, speed, None])
            elif kind == "loop" and len(parts) > 5:
                sounds.append([t, what, volume, speed, length])
                open_loops.setdefault(parts[5], []).append(len(sounds) - 1)
            elif kind == "stop":
                for i in open_loops.pop(what, []):
                    sounds[i][4] = t
    return [s for s in sounds if s[0] < length]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("recording")
    ap.add_argument("video")
    ap.add_argument("out")
    ap.add_argument("--assets", default="riftapart-assets")
    a = ap.parse_args()

    length = video_length(a.video)
    sounds = read_log(os.path.join(a.recording, "sounds.log"), length)
    if not sounds:
        raise SystemExit("no sounds in the log")
    args = ["ffmpeg", "-y", "-loglevel", "error", "-i", a.video]
    graph = []
    for i, (start, file, volume, speed, end) in enumerate(sounds, start=1):
        # Each sound is read inside the filter graph (too many for the command line); paths
        # use forward slashes, with the colons and quotes filters care about escaped.
        path = os.path.join(a.assets, file).replace("\\", "/").replace(":", "\:").replace("'", "\'")
        source = f"amovie={path}"
        if end is not None:
            source += f":loop=0,atrim=0:{max(end - start, 0.01):.3f},asetpts=PTS-STARTPTS"
        delay = int(round(start * 1000))
        # Played faster or slower for its pitch, as the sandbox does.
        pitch = f"asetrate={48000 * speed:.0f},aresample=48000," if abs(speed - 1.0) > 1e-3 else ""
        graph.append(f"{source},aformat=sample_rates=48000:channel_layouts=stereo,{pitch}volume={volume:.4f},"
                     f"adelay=delays={delay}:all=1[a{i}]")
    inputs = "".join(f"[a{i}]" for i in range(1, len(sounds) + 1))
    graph.append(f"{inputs}amix=inputs={len(sounds)}:normalize=0:dropout_transition=0,alimiter=limit=0.95[mix]")
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as f:
        f.write(";\n".join(graph))
        script = f.name
    try:
        args += ["-filter_complex_script", script, "-map", "0:v", "-map", "[mix]", "-c:v", "copy", "-c:a", "aac",
                 "-b:a", "192k", "-t", f"{length:.3f}", a.out]
        subprocess.run(args, check=True)
    finally:
        os.remove(script)
    print(f"{a.out}: {len(sounds)} sounds over {length:.1f} s")


if __name__ == "__main__":
    main()
