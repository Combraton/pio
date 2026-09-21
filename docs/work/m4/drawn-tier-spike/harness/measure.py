#!/usr/bin/env python3
"""Run every variant of the spike and write down what it cost.

For each variant this launches its own Ghostty process -- so the CPU measured
belongs to that window and nothing else -- records the window losslessly at the
display refresh rate, and reads CPU *time* (an integral) rather than CPU
percent (a sample) from `ps` at the start and end.

    python3 harness/measure.py --out /tmp/results.json --seconds 20
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, ROOT)

import canvas  # noqa: E402
import flicker  # noqa: E402
import window  # noqa: E402

COLS, ROWS = 110, 34


def python_child(pid):
    for p in window.descendants(pid):
        comm = subprocess.run(["ps", "-o", "comm=", "-p", str(p)],
                              capture_output=True, text=True).stdout.strip()
        if "python" in comm.lower():
            return p
    return None


def bands(win_w_dev, win_h_dev, cell_w, cell_h):
    """Two rectangles inside the window crop, in device pixels.

    quiet: rows carrying backdrop and no text at all, between the sticky note's
           last line and the status bar. If the image ever blinks, tears or is
           re-scaled, these pixels change.
    busy:  the status bar, which changes on every single frame. It is the
           control: if it does *not* change, the recording caught nothing and
           the quiet band's stillness means nothing either.
    """
    title_h = win_h_dev - ROWS * cell_h
    cards, _ = canvas.demo_layout(COLS, ROWS)
    sticky = cards[2]
    quiet_top_row = sticky.row + 5          # below the sticky note's three lines
    quiet_bot_row = ROWS - 2                # above the status bar
    quiet = (8, title_h + quiet_top_row * cell_h, win_w_dev - 16,
             max(cell_h, (quiet_bot_row - quiet_top_row) * cell_h))
    busy = (8, title_h + (ROWS - 2) * cell_h, win_w_dev - 16, cell_h * 2)
    return quiet, busy, title_h


def wait_for_cell(log, timeout=15):
    """The spike logs its own cell size; take it from there rather than guess."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        if os.path.exists(log):
            for line in open(log):
                r = json.loads(line)
                if r.get("ev") in ("start", "render_png"):
                    return r["px"][0] // r["cols"], r["px"][1] // r["rows"]
        time.sleep(0.25)
    raise RuntimeError("spike never logged a render")


VARIANTS = [
    ("none", ["--tier", "none", "--retransmit", "never"]),
    ("char", ["--tier", "char", "--retransmit", "never"]),
    ("drawn-once", ["--tier", "drawn", "--retransmit", "resize"]),
    ("drawn-once-sync", ["--tier", "drawn", "--retransmit", "resize", "--sync"]),
    ("drawn-every-frame", ["--tier", "drawn", "--retransmit", "frame"]),
    ("drawn-every-frame-file", ["--tier", "drawn", "--retransmit", "frame", "--medium", "file"]),
]


def run_one(name, extra, args, scratch):
    title = f"PIOM4-{name}"
    log = os.path.join(scratch, f"log_{name}.jsonl")
    vid = os.path.join(scratch, f"cap_{name}.mkv")
    cmd = (f"cd '{ROOT}' && python3 drawn_tier.py --seconds {args.seconds + 5} "
           f"--fps {args.fps} --log {log} " + " ".join(extra))
    proc = window.launch(title, cmd, cols=COLS, rows=ROWS, x=args.x, y=args.y)
    try:
        x, y, w, h = window.bounds(title)
        time.sleep(3.0)  # let the first frame and the image placement settle
        rect = (x * 2, y * 2, w * 2, h * 2)
        cell_w, cell_h = wait_for_cell(log)
        quiet, busy, title_h = bands(rect[2], rect[3], cell_w, cell_h)

        py = python_child(proc.pid)
        cpu0_tree = window.cputime_tree(proc.pid)
        cpu0_py = window.cputime(py) if py else float("nan")
        rec = flicker.record(rect, args.seconds, vid, fps=args.fps_capture) if args.record else None
        t0 = time.time()
        time.sleep(args.seconds + 1.0)
        elapsed = time.time() - t0
        cpu1_tree = window.cputime_tree(proc.pid)
        cpu1_py = window.cputime(py) if py else float("nan")
        shot = os.path.join(scratch, f"shot_{name}.png")
        try:
            window.shot(title, shot)
        except Exception:
            shot = None
        if rec:
            rec.wait()
        # Let the spike finish on its own so it writes its summary line.
        try:
            proc.wait(timeout=20)
        except Exception:
            pass
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=6)
            except Exception:
                proc.kill()
        time.sleep(1.0)

    result = {
        "variant": name,
        "args": extra,
        "window_rect_points": [x, y, w, h],
        "seconds_sampled": round(elapsed, 2),
        "cpu_seconds_total": round(cpu1_tree - cpu0_tree, 2),
        "cpu_seconds_python": (round(cpu1_py - cpu0_py, 2) if cpu1_py == cpu1_py else None),
        "cell_h_device_px": cell_h,
        "titlebar_device_px": title_h,
    }
    result["cpu_seconds_ghostty"] = round(
        result["cpu_seconds_total"] - (result["cpu_seconds_python"] or 0), 2)
    for key in ("cpu_seconds_total", "cpu_seconds_ghostty", "cpu_seconds_python"):
        if result[key] is not None:
            result[key.replace("cpu_seconds", "cpu_percent_of_one_core")] = round(
                100.0 * result[key] / elapsed, 1)

    if os.path.exists(log):
        for line in open(log):
            rec_ = json.loads(line)
            if rec_.get("ev") == "summary":
                result["spike"] = rec_
    if args.record and os.path.exists(vid):
        result["flicker"] = flicker.analyse(vid, quiet, busy)
        result["video_bytes"] = os.path.getsize(vid)
        if args.keep_video:
            result["video"] = vid
        else:
            os.remove(vid)
    result["shot"] = shot
    return result


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--scratch", default="/tmp")
    ap.add_argument("--seconds", type=float, default=20)
    ap.add_argument("--fps", type=float, default=20)
    ap.add_argument("--fps-capture", type=int, default=60)
    ap.add_argument("--x", type=int, default=30)
    ap.add_argument("--y", type=int, default=60)
    ap.add_argument("--record", action="store_true")
    ap.add_argument("--only", default="")
    ap.add_argument("--keep-video", action="store_true")
    args = ap.parse_args()

    results = []
    for name, extra in VARIANTS:
        if args.only and name not in args.only.split(","):
            continue
        print(f"== {name}", flush=True)
        try:
            r = run_one(name, extra, args, args.scratch)
        except Exception as exc:  # a failed variant is data too
            r = {"variant": name, "error": repr(exc)}
        results.append(r)
        print(json.dumps({k: v for k, v in r.items() if k != "flicker"}, indent=2), flush=True)
        with open(args.out, "w") as fh:
            json.dump(results, fh, indent=2)
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
