#!/usr/bin/env python3
"""Prove, with a screenshot, where the image lands relative to the text.

Three bands of solid colour are placed as one image at the z index under test,
and text is written over them: plain text, text with an explicit background
colour, and text on a cell whose background is the default. A photograph of
the result settles the question that `a=q` answers only with `OK`.

    python3 zorder_test.py --z -1 --hold 40
"""
from __future__ import annotations

import argparse
import base64
import fcntl
import io
import os
import struct
import sys
import termios
import time
import tty

from PIL import Image, ImageDraw

ESC = "\x1b"
ST = ESC + "\\"
RESET = ESC + "[0m"


def winsize(fd):
    rows, cols, xp, yp = struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8))
    return rows, cols, xp, yp


def build(width, height):
    img = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    third = height // 3
    d.rectangle((0, 0, width, third), fill=(200, 30, 120, 255))          # magenta
    d.rectangle((0, third, width, 2 * third), fill=(30, 140, 200, 255))  # blue
    d.rectangle((0, 2 * third, width, height), fill=(240, 190, 40, 255)) # amber
    for x in range(0, width, 40):
        d.line((x, 0, x, height), fill=(255, 255, 255, 90), width=1)
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--z", type=int, default=-1)
    ap.add_argument("--hold", type=float, default=30)
    ap.add_argument("--label", default="")
    args = ap.parse_args()

    fd = os.open("/dev/tty", os.O_RDWR)
    old = termios.tcgetattr(fd)
    tty.setraw(fd)
    rows, cols, xp, yp = winsize(fd)
    cw, ch = xp // cols, yp // rows
    png = build(cols * cw, rows * ch)

    def w(s):
        data = s.encode()
        while data:
            data = data[os.write(fd, data):]

    try:
        w(f"{ESC}[?1049h{ESC}[?25l{ESC}[2J{ESC}[H")
        b64 = base64.standard_b64encode(png).decode()
        keys = f"a=T,f=100,i=99,q=2,c={cols},r={rows},z={args.z},C=1"
        first, i = True, 0
        while i < len(b64):
            chunk, i = b64[i:i + 4096], i + 4096
            more = 1 if i < len(b64) else 0
            if first:
                w(f"{ESC}[H{ESC}_G{keys},t=d,m={more};{chunk}{ST}")
                first = False
            else:
                w(f"{ESC}_Gm={more};{chunk}{ST}")

        lines = [
            (2, f"{ESC}[1;37m Z={args.z} {args.label} default-bg white text {RESET}"),
            (4, f"{ESC}[38;5;15m default background, bright white: can you read this?{RESET}"),
            (6, f"{ESC}[38;5;16;48;5;255m EXPLICIT white cell background {RESET}"),
            (8, f"{ESC}[38;5;15;48;5;235m EXPLICIT dark cell background {RESET}"),
            (10, f"{ESC}[38;5;0m default background, black text over the image{RESET}"),
            (12, f"{ESC}[7m reverse video {RESET}"),
            (14, f"{ESC}[4m underlined text {RESET}   {ESC}[9mstruck{RESET}"),
            (16, "█" * 20 + "  <- solid blocks in the default foreground"),
        ]
        for r, s in lines:
            if r <= rows:
                w(f"{ESC}[{r};3H{s}")
        w(f"{ESC}[{rows};3H{ESC}[38;5;255mbottom row{RESET}")
        time.sleep(args.hold)
    finally:
        w(f"{ESC}_Ga=d,d=I,i=99,q=2{ST}{ESC}[?25h{ESC}[?1049l{RESET}")
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        os.close(fd)


if __name__ == "__main__":
    main()
