#!/usr/bin/env python3
"""Drive a Ghostty window from outside it: launch, locate, resize, screenshot.

The spike runs inside a terminal, so nothing inside it can see what the screen
actually looks like. This module is the outside eye. It launches a *separate*
Ghostty process (so its CPU is ours alone and not the operator's other
windows), finds the window by title through the Accessibility API, and takes
screenshots of exactly that rectangle.
"""
from __future__ import annotations

import json
import os
import shlex
import subprocess
import time

GHOSTTY = "/Applications/Ghostty.app/Contents/MacOS/ghostty"


def _osa(script: str) -> str:
    p = subprocess.run(["osascript", "-e", script], capture_output=True, text=True)
    return (p.stdout or p.stderr).strip()


def launch(title: str, command: str, cols: int = 110, rows: int = 34,
           x: int = 40, y: int = 60, extra: list | None = None) -> subprocess.Popen:
    """Start a new Ghostty process running `command` in a shell."""
    argv = [
        GHOSTTY,
        f"--title={title}",
        f"--window-width={cols}",
        f"--window-height={rows}",
        f"--window-position-x={x}",
        f"--window-position-y={y}",
        "--window-save-state=never",
        "--confirm-close-surface=false",
        "--window-padding-x=0",
        "--window-padding-y=0",
        "--window-padding-balance=false",
        "--shell-integration=none",
        "--font-size=13",
        "--background=#11161d",
        "--cursor-style=block",
    ] + (extra or []) + ["-e", "/bin/sh", "-c", command]
    return subprocess.Popen(argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


BOUNDS_SCRIPT = '''
tell application "System Events"
  set out to ""
  repeat with p in (every process whose name is "ghostty")
    repeat with w in (every window of p)
      try
        if name of w contains "%s" then
          set pos to position of w
          set sz to size of w
          set out to (item 1 of pos as text) & "," & (item 2 of pos as text) & "," & (item 1 of sz as text) & "," & (item 2 of sz as text)
        end if
      end try
    end repeat
  end repeat
  return out
end tell
'''


def bounds(title: str, timeout: float = 12.0):
    """Return (x, y, w, h) in points for the window whose title contains `title`."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        out = _osa(BOUNDS_SCRIPT % title)
        if out and "," in out and "error" not in out.lower():
            try:
                return tuple(int(float(v)) for v in out.split(","))
            except ValueError:
                pass
        time.sleep(0.3)
    raise RuntimeError(f"no ghostty window titled ~{title!r}: {out!r}")


SET_SIZE = '''
tell application "System Events"
  repeat with p in (every process whose name is "ghostty")
    repeat with w in (every window of p)
      try
        if name of w contains "%s" then
          set size of w to {%d, %d}
        end if
      end try
    end repeat
  end repeat
end tell
'''


def resize(title: str, w: int, h: int):
    _osa(SET_SIZE % (title, w, h))


def shot(title: str, path: str, pad: int = 0):
    """Screenshot just that window's rectangle."""
    x, y, w, h = bounds(title)
    subprocess.run(
        ["screencapture", "-x", "-o", "-R", f"{x-pad},{y-pad},{w+2*pad},{h+2*pad}", path],
        check=True,
    )
    return (x, y, w, h)


def shot_window_id(title: str, path: str):
    """Screenshot by window id: no shadow, no neighbours, exact content."""
    x, y, w, h = bounds(title)
    subprocess.run(["screencapture", "-x", "-o", "-R", f"{x},{y},{w},{h}", path], check=True)
    return (x, y, w, h)


def cputime(pid: int) -> float:
    """Total CPU seconds burned by a pid, from ps. The only honest CPU number
    here: a % from `top` samples an instant, this is an integral."""
    out = subprocess.run(["ps", "-o", "time=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    if not out:
        return float("nan")
    # Formats: MM:SS.ss or HH:MM:SS
    parts = out.split(":")
    secs = 0.0
    for part in parts:
        secs = secs * 60 + float(part)
    return secs


def descendants(pid: int) -> list:
    """Every pid under `pid`, so a window's helper processes are counted too."""
    out = subprocess.run(["ps", "-eo", "pid=,ppid="], capture_output=True, text=True).stdout
    kids = {}
    for line in out.splitlines():
        try:
            c, p = (int(v) for v in line.split())
        except ValueError:
            continue
        kids.setdefault(p, []).append(c)
    seen, stack = [], [pid]
    while stack:
        cur = stack.pop()
        seen.append(cur)
        stack.extend(kids.get(cur, []))
    return seen


def cputime_tree(pid: int) -> float:
    return sum(v for v in (cputime(p) for p in descendants(pid)) if v == v)


if __name__ == "__main__":
    import sys

    print(json.dumps({"bounds": bounds(sys.argv[1])}))
