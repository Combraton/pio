#!/usr/bin/env python3
"""Does the backdrop survive a resize? Measure it, do not eyeball it.

A resize is the one moment where an image placed in cell coordinates can go
wrong in three different ways, and all three look similar at a glance:

  1. the image is left at its old pixel size and Ghostty rescales it,
  2. the image is dropped and never comes back,
  3. the image comes back but no longer lines up with the text.

The check here is geometric: after each resize, find the sticky note's bottom
stroke in the screenshot by looking for the row with the most pixels of its
colour, and compare that row with the row the *layout* says it should be on.
If the two agree to within a cell, the backdrop and the text agree.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, ROOT)

import canvas  # noqa: E402
import flicker  # noqa: E402
import window  # noqa: E402

STICKY_RGB = np.array(canvas.ID2[:3], dtype=np.int16)


def find_stroke_rows(png_path, rgb=STICKY_RGB, tol=45, min_run_frac=0.5):
    """The topmost and bottom-most image rows that are a *horizontal stroke*.

    Pink text is pink too -- the status line's `1 uncertain` is within a few
    units of the sticky note's colour -- so colour alone finds glyphs and calls
    them card edges. A stroke is distinguished by length: it runs across most
    of the window, where a word runs across a dozen cells. Requiring half the
    window's width makes the measurement mean what it says.
    """
    a = np.asarray(Image.open(png_path).convert("RGB"), dtype=np.int16)
    dist = np.abs(a - rgb).sum(axis=2)
    hits = (dist < tol).sum(axis=1)
    strong = np.flatnonzero(hits >= a.shape[1] * min_run_frac)
    if strong.size == 0:
        return None, None, int(hits.max())
    return int(strong[0]), int(strong[-1]), int(hits.max())


def read_log(path):
    out = []
    if os.path.exists(path):
        for line in open(path):
            try:
                out.append(json.loads(line))
            except ValueError:
                pass
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", default="/tmp")
    ap.add_argument("--out", required=True)
    ap.add_argument("--record", action="store_true")
    args = ap.parse_args()

    title = "PIOM4-resize"
    log = os.path.join(args.scratch, "log_resize.jsonl")
    cmd = f"cd '{ROOT}' && python3 drawn_tier.py --tier drawn --seconds 70 --fps 20 --log {log}"
    proc = window.launch(title, cmd, cols=110, rows=34, x=30, y=60)
    steps = []
    rec = None
    try:
        x, y, w, h = window.bounds(title)
        time.sleep(3)
        if args.record:
            rec = flicker.record((x * 2, y * 2, w * 2, h * 2), 46,
                                 os.path.join(args.scratch, "cap_resize.mkv"))
            time.sleep(1)

        sizes = [(880, 610), (640, 470), (1180, 760), (560, 360), (880, 610)]
        for i, (tw, th) in enumerate(sizes):
            window.resize(title, tw, th)
            time.sleep(2.5)  # SIGWINCH, re-render, re-transmit, repaint
            shot = os.path.join(args.scratch, f"resize_{i}_{tw}x{th}.png")
            bx, by, bw, bh = window.shot(title, shot)
            entries = read_log(log)
            last_start = [e for e in entries if e.get("ev") in ("start", "render_png")][-1]
            cols, rows = last_start["cols"], last_start["rows"]
            cell_w = last_start["px"][0] / cols
            cell_h = last_start["px"][1] / rows
            title_h = bh * 2 - last_start["px"][1]
            cards, _ = canvas.demo_layout(cols, rows)
            sticky = cards[2]
            exp_top = title_h + sticky.row * cell_h
            exp_bot = title_h + (sticky.row + sticky.rows) * cell_h
            top, bot, strength = find_stroke_rows(shot)
            steps.append({
                "requested_points": [tw, th],
                "actual_points": [bw, bh],
                "grid": [cols, rows],
                "text_area_device_px": last_start["px"],
                "cell_device_px": [round(cell_w, 2), round(cell_h, 2)],
                "titlebar_device_px": title_h,
                "sticky_top_expected_px": round(exp_top, 1),
                "sticky_top_found_px": top,
                "sticky_bottom_expected_px": round(exp_bot, 1),
                "sticky_bottom_found_px": bot,
                "top_off_by_cells": (None if top is None else round((top - exp_top) / cell_h, 2)),
                "bottom_off_by_cells": (None if bot is None else round((bot - exp_bot) / cell_h, 2)),
                "stroke_pixels_on_strongest_row": strength,
                "shot": shot,
            })
            print(json.dumps(steps[-1], indent=2), flush=True)

        if rec:
            rec.wait()
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=6)
            except Exception:
                proc.kill()

    entries = read_log(log)
    result = {
        "steps": steps,
        "resize_events": [e for e in entries if e.get("ev") == "resize"],
        "transmits": [e for e in entries if e.get("ev") == "transmit"],
        "renders": [e for e in entries if e.get("ev") == "render_png"],
        "summary": [e for e in entries if e.get("ev") == "summary"],
    }
    with open(args.out, "w") as fh:
        json.dump(result, fh, indent=2)
    print("wrote", args.out)


if __name__ == "__main__":
    main()
