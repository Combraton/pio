#!/usr/bin/env python3
"""Which ways of clearing the screen take the picture with them.

This is the finding that decides whether the drawn tier can flicker at all. A
placement lives on a cell. Anything that *erases* that cell erases the
placement, and a TUI that opens each frame with a clear would therefore delete
and re-transmit its backdrop sixty times a second -- which is exactly the
flicker the question asks about, arriving through a door nobody opened on
purpose.

Five phases, one screenshot each. In every phase the image is placed fresh, so
each result is about that phase's operation and nothing else.
"""
from __future__ import annotations

import argparse
import base64
import fcntl
import os
import struct
import sys
import termios
import time
import tty

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import canvas  # noqa: E402

ESC = "\x1b"
ST = ESC + "\\"
RESET = ESC + "[0m"
Z = -1073741825

PHASES = [
    ("placed, nothing erased", lambda w, rows, cols: None),
    ("ESC[2J erase whole display", lambda w, rows, cols: w(f"{ESC}[2J")),
    ("ESC[H ESC[J erase to end", lambda w, rows, cols: w(f"{ESC}[H{ESC}[J")),
    ("ESC[2K on every row", lambda w, rows, cols: [w(f"{ESC}[{r};1H{ESC}[2K") for r in range(1, rows + 1)]),
    ("overwrite every cell with a space", lambda w, rows, cols: [w(f"{ESC}[{r};1H" + " " * cols) for r in range(1, rows + 1)]),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--hold", type=float, default=5.0)
    args = ap.parse_args()

    fd = os.open("/dev/tty", os.O_RDWR)
    old = termios.tcgetattr(fd)
    tty.setraw(fd)
    rows, cols, xp, yp = struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8))

    def w(s):
        data = s.encode()
        while data:
            data = data[os.write(fd, data):]

    cards, arrows = canvas.demo_layout(cols, rows)
    png = canvas.render(xp, yp, xp / cols, yp / rows, cards, arrows)
    b64 = base64.standard_b64encode(png).decode()

    def place():
        keys = f"a=T,f=100,i=11,q=2,c={cols},r={rows},z={Z},C=1"
        first, i = True, 0
        while i < len(b64):
            chunk, i = b64[i:i + 4096], i + 4096
            more = 1 if i < len(b64) else 0
            if first:
                w(f"{ESC}[H{ESC}_G{keys},t=d,m={more};{chunk}{ST}")
                first = False
            else:
                w(f"{ESC}_Gm={more};{chunk}{ST}")

    try:
        w(f"{ESC}[?1049h{ESC}[?25l{ESC}[?7l")
        for n, (label, op) in enumerate(PHASES):
            w(f"{ESC}_Ga=d,d=I,i=11,q=2{ST}{ESC}[2J")
            place()
            w(f"{ESC}[2;4H{ESC}[38;5;255mPHASE {n}: {label}{RESET}")
            op(w, rows, cols)
            w(f"{ESC}[2;4H{ESC}[38;5;255mPHASE {n}: {label}{RESET}")
            time.sleep(args.hold)
    finally:
        w(f"{ESC}_Ga=d,d=A,q=2{ST}{ESC}[?7h{ESC}[?25h{ESC}[?1049l{RESET}")
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        os.close(fd)


if __name__ == "__main__":
    main()
