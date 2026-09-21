#!/usr/bin/env python3
"""What happens inside tmux. Four cases, four screenshots, one number each.

tmux owns the screen: it parses what the program writes, keeps its own model of
every cell, and repaints from that model. An APC graphics command is not a cell,
so tmux has nowhere to put it. The question is only *how* it fails, and whether
`allow-passthrough on` rescues it.

  bare          spike in tmux, graphics written straight out
  passthrough   spike in tmux, graphics wrapped in tmux's own DCS
  redraw        as passthrough, then force a tmux full redraw
  other-pane    as passthrough, then split the window

The measurement is "how much backdrop ink is on screen": the count of pixels
matching a card-stroke colour. Outside tmux that number is in the thousands.
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
import window  # noqa: E402

SESSION = "piom4spike"


def tmux(*a, check=False):
    return subprocess.run(["tmux"] + list(a), capture_output=True, text=True, check=check)


def ink(png_path, tol=60):
    """Pixels matching any card-stroke colour: the backdrop's signature."""
    a = np.asarray(Image.open(png_path).convert("RGB"), dtype=np.int16)
    total = 0
    per = {}
    for name, rgb in (("id1", canvas.ID1), ("id2", canvas.ID2), ("id3", canvas.ID3)):
        d = np.abs(a - np.array(rgb[:3], dtype=np.int16)).sum(axis=2)
        n = int((d < tol).sum())
        per[name] = n
        total += n
    per["total"] = total
    return per


def case(name, args, spike_flags, after=None, split=False):
    tmux("kill-session", "-t", SESSION)
    time.sleep(0.5)
    tmux("new-session", "-d", "-s", SESSION, "-x", "110", "-y", "34")
    tmux("set-option", "-t", SESSION, "-g", "allow-passthrough",
         "on" if "--passthrough" in spike_flags else "off")
    tmux("set-option", "-t", SESSION, "-g", "status", "off")

    log = os.path.join(args.scratch, f"log_tmux_{name}.jsonl")
    inner = (f"cd '{ROOT}' && python3 drawn_tier.py --tier drawn --seconds 50 "
             f"--fps 20 --log {log} " + " ".join(spike_flags))
    title = f"PIOM4-tmux-{name}"
    proc = window.launch(title, f"tmux attach -t {SESSION}", cols=110, rows=36, x=30, y=60)
    out = {"case": name, "flags": spike_flags}
    try:
        window.bounds(title)
        time.sleep(2.5)
        tmux("send-keys", "-t", SESSION, inner, "Enter")
        time.sleep(4.0)
        if split:
            tmux("split-window", "-t", SESSION, "-h", "sh -c 'printf hello; sleep 40'")
            time.sleep(2.0)
        if after:
            after()
            time.sleep(2.0)
        shot = os.path.join(args.scratch, f"tmux_{name}.png")
        window.shot(title, shot)
        out["shot"] = shot
        out["ink"] = ink(shot)
        pane = tmux("capture-pane", "-p", "-t", f"{SESSION}.0").stdout
        out["pane_text_lines"] = len([ln for ln in pane.splitlines() if ln.strip()])
        out["pane_head"] = pane.splitlines()[:3]
        out["tmux_allow_passthrough"] = tmux(
            "show-options", "-g", "allow-passthrough").stdout.strip()
    finally:
        tmux("kill-session", "-t", SESSION)
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=6)
            except Exception:
                proc.kill()
        time.sleep(1.0)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scratch", default="/tmp")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    results = []
    results.append(case("bare", args, []))
    results.append(case("passthrough", args, ["--passthrough"]))
    results.append(case("passthrough-redraw", args, ["--passthrough"],
                        after=lambda: tmux("refresh-client", "-t", SESSION, "-S")))
    results.append(case("passthrough-split", args, ["--passthrough"], split=True))

    for r in results:
        print(json.dumps({k: v for k, v in r.items() if k != "pane_head"}, indent=2), flush=True)
    with open(args.out, "w") as fh:
        json.dump(results, fh, indent=2)
    print("wrote", args.out)


if __name__ == "__main__":
    main()
