#!/usr/bin/env python3
"""Where exactly does the picture land, to the pixel, at each window size?

Colour matching finds the pink card edge and the pink status word equally well,
so it is no way to measure alignment. This does it without heuristics: run the
spike with no text at all, so the screenshot *is* the backdrop, render the same
backdrop offline from the same layout code, and slide one over the other. The
shift with the smallest difference is the misalignment, in pixels.

A perfectly placed image scores (0, 0).
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, ROOT)

import canvas  # noqa: E402
import window  # noqa: E402

BG = (17, 22, 29)


def reference(cols, rows, xpix, ypix):
    cards, arrows = canvas.demo_layout(cols, rows)
    png = canvas.render(xpix, ypix, xpix / cols, ypix / rows, cards, arrows)
    import io
    img = Image.open(io.BytesIO(png)).convert("RGBA")
    flat = Image.new("RGBA", img.size, BG + (255,))
    flat.alpha_composite(img)
    return np.asarray(flat.convert("RGB"), dtype=np.int16)


def best_shift(shot_path, ref, ypix, span=6):
    """Slide the reference over the screenshot's text area; return the best shift."""
    shot = np.asarray(Image.open(shot_path).convert("RGB"), dtype=np.int16)
    title_h = shot.shape[0] - ypix
    area = shot[title_h:title_h + ypix, :ref.shape[1]]
    if area.shape != ref.shape:
        return {"error": f"shape mismatch {area.shape} vs {ref.shape}"}
    best = None
    scores = {}
    for dy in range(-span, span + 1):
        for dx in range(-span, span + 1):
            a = area[max(0, dy):area.shape[0] + min(0, dy),
                     max(0, dx):area.shape[1] + min(0, dx)]
            b = ref[max(0, -dy):ref.shape[0] + min(0, -dy),
                    max(0, -dx):ref.shape[1] + min(0, -dx)]
            score = float(np.abs(a - b).mean())
            scores[(dy, dx)] = score
            if best is None or score < best[0]:
                best = (score, dy, dx)
    return {
        "title_h": int(title_h),
        "best_shift_dy_dx": [best[1], best[2]],
        "mean_abs_error_at_best": round(best[0], 3),
        "mean_abs_error_at_zero": round(scores[(0, 0)], 3),
        "mean_abs_error_at_one_cell_off": round(
            scores.get((min(span, int(round(ypix / 34))), 0), float("nan")), 3),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", default="/tmp")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    title = "PIOM4-align"
    log = os.path.join(args.scratch, "log_align.jsonl")
    cmd = (f"cd '{ROOT}' && python3 drawn_tier.py --tier drawn --no-text "
           f"--seconds 80 --fps 10 --log {log}")
    proc = window.launch(title, cmd, cols=110, rows=34, x=30, y=60)
    steps = []
    try:
        window.bounds(title)
        time.sleep(3)
        for i, (tw, th) in enumerate([(880, 610), (640, 470), (1180, 760),
                                      (560, 360), (900, 500), (880, 610)]):
            window.resize(title, tw, th)
            time.sleep(2.5)
            shot = os.path.join(args.scratch, f"align_{i}_{tw}x{th}.png")
            window.shot(title, shot)
            entries = [json.loads(l) for l in open(log)]
            last = [e for e in entries if e.get("ev") in ("start", "render_png")][-1]
            cols, rows = last["cols"], last["rows"]
            xpix, ypix = last["px"]
            r = best_shift(shot, reference(cols, rows, xpix, ypix), ypix)
            r.update({"requested_points": [tw, th], "grid": [cols, rows],
                      "text_area_px": [xpix, ypix], "shot": shot,
                      "cell_px": [round(xpix / cols, 2), round(ypix / rows, 2)]})
            steps.append(r)
            print(json.dumps(r), flush=True)
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=6)
            except Exception:
                proc.kill()

    entries = [json.loads(l) for l in open(log)]
    out = {"steps": steps,
           "renders": [e for e in entries if e.get("ev") == "render_png"],
           "transmits": [e for e in entries if e.get("ev") == "transmit"],
           "resizes": [e for e in entries if e.get("ev") == "resize"]}
    with open(args.out, "w") as fh:
        json.dump(out, fh, indent=2)
    print("wrote", args.out)


if __name__ == "__main__":
    main()
