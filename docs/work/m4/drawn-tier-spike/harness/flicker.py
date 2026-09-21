#!/usr/bin/env python3
"""Measure flicker, rather than squint at it.

"Does it flicker" is not a matter of opinion if you record the window
losslessly at the display's refresh rate and look at the pixels. The trick is
choosing where to look: a band of the window that contains *only* backdrop --
card stroke and grid, no text, no cursor -- is pixel-identical in every frame
unless the terminal drops, redraws or tears the image. So:

  quiet band   pixel-exact across frames  -> the image never flickers
  quiet band   goes flat background       -> the image blinked out
  quiet band   differs on some frames     -> the image tore or was rescaled

A second, busy band over the live text is measured the same way, as a control:
it *must* change, or the recording caught nothing and the whole measurement is
vacuous.

Usage: see cpu_and_flicker.py, which drives this.
"""
from __future__ import annotations

import json
import subprocess
import sys

import numpy as np


def record(rect, seconds, out, fps=60):
    """Record a screen rectangle (device pixels) losslessly."""
    x, y, w, h = rect
    # avfoundation wants even dimensions for most codecs; ffv1 is fine, but
    # keep it even anyway so the crop maths is boring.
    w -= w % 2
    h -= h % 2
    cmd = [
        "ffmpeg", "-hide_banner", "-loglevel", "error",
        "-f", "avfoundation", "-capture_cursor", "0", "-pixel_format", "bgr0",
        "-framerate", str(fps), "-i", "2",
        "-t", str(seconds),
        "-vf", f"crop={w}:{h}:{x}:{y}",
        "-c:v", "ffv1", "-y", out,
    ]
    return subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def frames(path, band=None):
    """Yield RGB frames as numpy arrays, optionally cropped to `band`."""
    probe = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0",
         "-show_entries", "stream=width,height", "-of", "json", path],
        capture_output=True, text=True).stdout
    meta = json.loads(probe)["streams"][0]
    w, h = meta["width"], meta["height"]
    vf = []
    if band:
        bx, by, bw, bh = band
        vf = ["-vf", f"crop={bw}:{bh}:{bx}:{by}"]
        w, h = bw, bh
    p = subprocess.Popen(
        ["ffmpeg", "-v", "error", "-i", path] + vf + ["-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        stdout=subprocess.PIPE)
    size = w * h * 3
    while True:
        buf = p.stdout.read(size)
        if len(buf) < size:
            break
        yield np.frombuffer(buf, dtype=np.uint8).reshape(h, w, 3)
    p.stdout.close()
    p.wait()


def analyse(path, quiet_band, busy_band, tol=6):
    """Compare every frame with the modal frame of each band."""
    out = {}
    for name, band in (("quiet", quiet_band), ("busy", busy_band)):
        fs = list(frames(path, band))
        if len(fs) < 5:
            out[name] = {"frames": len(fs), "error": "too few frames recorded"}
            continue
        arr = np.stack(fs).astype(np.int16)
        # The reference is the per-pixel median: robust to a few odd frames.
        ref = np.median(arr, axis=0)
        per_frame_max = np.abs(arr - ref).max(axis=(1, 2, 3))
        per_frame_mean = np.abs(arr - ref).mean(axis=(1, 2, 3))
        changed = int((per_frame_max > tol).sum())
        out[name] = {
            "frames": len(fs),
            "band": band,
            "frames_differing_from_median": changed,
            "fraction_differing": round(changed / len(fs), 4),
            "max_abs_pixel_delta": int(per_frame_max.max()),
            "mean_abs_delta_p99": round(float(np.percentile(per_frame_mean, 99)), 3),
            "identical_frame_pairs": int((np.abs(np.diff(arr, axis=0)).max(axis=(1, 2, 3)) == 0).sum()),
            "consecutive_pairs": len(fs) - 1,
        }
    return out


if __name__ == "__main__":
    print(json.dumps(analyse(sys.argv[1],
                             tuple(int(v) for v in sys.argv[2].split(",")),
                             tuple(int(v) for v in sys.argv[3].split(","))), indent=2))
