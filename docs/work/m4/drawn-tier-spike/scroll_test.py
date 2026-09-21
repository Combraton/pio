#!/usr/bin/env python3
"""What scrolling does to a placement, in four timed phases.

A Kitty-protocol placement is anchored to a *cell*, not to the window. That is
the whole reason it survives a repaint -- and the whole reason a scroll is
dangerous: whatever moves the anchor cell moves the picture, and whatever
deletes the anchor cell deletes the picture. Each phase below holds long enough
for the harness outside to photograph it.

  0  alt screen, image placed, text on top      (the baseline)
  1  alt screen, CSI 3 S: scroll the region up  (does the picture move?)
  2  alt screen, a newline on the last row      (does it survive a 1-line scroll?)
  3  primary screen, image placed, 40 lines printed past it
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--hold", type=float, default=5.0)
    args = ap.parse_args()

    fd = os.open("/dev/tty", os.O_RDWR)
    old = termios.tcgetattr(fd)
    tty.setraw(fd)
    rows, cols, xp, yp = struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8))
    cw, ch = xp // cols, yp // rows

    def w(s):
        data = s.encode()
        while data:
            data = data[os.write(fd, data):]

    def place(image_id=7):
        cards, arrows = canvas.demo_layout(cols, rows)
        png = canvas.render(cols * cw, rows * ch, cw, ch, cards, arrows)
        b64 = base64.standard_b64encode(png).decode()
        keys = f"a=T,f=100,i={image_id},q=2,c={cols},r={rows},z={Z},C=1"
        first, i = True, 0
        while i < len(b64):
            chunk, i = b64[i:i + 4096], i + 4096
            more = 1 if i < len(b64) else 0
            if first:
                w(f"{ESC}[H{ESC}_G{keys},t=d,m={more};{chunk}{ST}")
                first = False
            else:
                w(f"{ESC}_Gm={more};{chunk}{ST}")

    def markers(phase, note):
        w(f"{ESC}[2;4H{ESC}[38;5;255mPHASE {phase}: {note}{RESET}")
        for r in range(4, rows - 1, 3):
            w(f"{ESC}[{r};6H{ESC}[38;5;250mrow {r:02d} marker{RESET}")

    try:
        # phase 0 ------------------------------------------------------
        w(f"{ESC}[?1049h{ESC}[?25l{ESC}[2J")
        place()
        markers(0, "alt screen, image placed, text over it")
        time.sleep(args.hold)

        # phase 1 ------------------------------------------------------
        # Re-place before every phase: a phase that inherits the previous
        # phase's deleted image measures nothing.
        w(f"{ESC}[2J")
        place()
        markers(1, "after CSI 3 S (scroll region up by 3)")
        w(f"{ESC}[3S")
        markers(1, "after CSI 3 S (scroll region up by 3)")
        time.sleep(args.hold)

        # phase 2 ------------------------------------------------------
        w(f"{ESC}[2J")
        place()
        markers(2, "after one newline on the last row")
        w(f"{ESC}[{rows};1H\r\n")
        markers(2, "after one newline on the last row")
        time.sleep(args.hold)

        # phase 3 ------------------------------------------------------
        w(f"{ESC}[?1049l{ESC}[2J{ESC}[H")
        place(image_id=8)
        markers(3, "primary screen, image placed")
        time.sleep(args.hold)
        w(f"{ESC}[{rows};1H")
        for i in range(40):
            w(f"scrolled line {i:02d}\r\n")
        time.sleep(args.hold)
    finally:
        w(f"{ESC}_Ga=d,d=A,q=2{ST}{ESC}[?25h{ESC}[?1049l{RESET}\r\n")
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        os.close(fd)


if __name__ == "__main__":
    main()
