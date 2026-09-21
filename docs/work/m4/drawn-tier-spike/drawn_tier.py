#!/usr/bin/env python3
"""The drawn tier, as small as it can be and still tell the truth.

One image — grid, card outlines, arrow — placed once at a negative z index so
it sits *under* the text cells, with a text frame repainted over it many times
a second. Everything the report claims is measured here or measured against
this.

    python3 drawn_tier.py --tier drawn --seconds 20 --fps 20

Tiers
  drawn   the backdrop is one Kitty-graphics image at z=-1
  char    the backdrop is box-drawing characters and dots, repainted each frame
  none    no backdrop at all: the floor for the CPU comparison

Retransmit policy (the thing that decides whether it flickers)
  never   transmit once, never again
  resize  transmit again only on SIGWINCH          <- what a real TUI would do
  frame   transmit again every single frame        <- the deliberate worst case

Everything it does is logged as JSON lines to --log, so the numbers in the
report are not impressions.
"""
from __future__ import annotations

import argparse
import base64
import fcntl
import json
import os
import signal
import struct
import sys
import termios
import time
import tty

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import canvas  # noqa: E402

ESC = "\x1b"
ST = ESC + "\\"
IMAGE_ID = 1717

SYNC_BEGIN = ESC + "[?2026h"
SYNC_END = ESC + "[?2026l"

# 256-colour approximations of the PIO Dark tokens, so the text tier needs no
# truecolour and the comparison is fair on any terminal.
DIM = ESC + "[38;5;238m"
TEXT = ESC + "[38;5;252m"
MUTED = ESC + "[38;5;245m"
BLUE = ESC + "[38;5;111m"
PINK = ESC + "[38;5;211m"
TEAL = ESC + "[38;5;115m"
AMBER = ESC + "[38;5;179m"
RESET = ESC + "[0m"


def winsize(fd):
    rows, cols, xpix, ypix = struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8))
    return rows, cols, xpix, ypix


class Screen:
    def __init__(self, args):
        self.args = args
        self.fd = os.open("/dev/tty", os.O_RDWR)
        self.old = termios.tcgetattr(self.fd)
        self.log = open(args.log, "w") if args.log else None
        self.frame = 0
        self.bytes_written = 0
        self.image_bytes = 0
        self.transmits = 0
        self.resizes = 0
        self.png = b""
        self.dirty = True
        self.measure()

    # ---- plumbing -------------------------------------------------------
    def measure(self):
        self.rows, self.cols, self.xpix, self.ypix = winsize(self.fd)
        # Ghostty reports device pixels here on a Retina display, which is what
        # we want: the image is rendered at native resolution. Keep the cell
        # size fractional -- see canvas.render for why flooring it drifts.
        self.cell_w = self.xpix / max(1, self.cols)
        self.cell_h = self.ypix / max(1, self.rows)

    def w(self, s: str):
        data = s.encode()
        self.bytes_written += len(data)
        while data:
            data = data[os.write(self.fd, data):]

    def g(self, s: str):
        """Write a graphics sequence, wrapping it for tmux when asked.

        tmux only forwards an unknown escape if `allow-passthrough` is on and
        the sequence is wrapped in its own DCS, with every ESC doubled."""
        if self.args.passthrough:
            self.w(f"{ESC}Ptmux;" + s.replace(ESC, ESC + ESC) + ST)
        else:
            self.w(s)

    def record(self, **kw):
        if self.log:
            kw["t"] = time.time()
            self.log.write(json.dumps(kw) + "\n")
            self.log.flush()

    # ---- the image ------------------------------------------------------
    def build_png(self):
        cards, arrows = canvas.demo_layout(self.cols, self.rows)
        t0 = time.perf_counter()
        self.png = canvas.render(
            self.xpix,
            self.ypix,
            self.cell_w,
            self.cell_h,
            cards,
            arrows,
            scale=self.args.scale,
        )
        self.render_ms = (time.perf_counter() - t0) * 1000
        self.record(ev="render_png", ms=round(self.render_ms, 2), bytes=len(self.png),
                    cols=self.cols, rows=self.rows, px=[self.xpix, self.ypix])

    def delete_image(self):
        self.g(f"{ESC}_Ga=d,d=I,i={IMAGE_ID},q=2{ST}")

    def transmit(self):
        """Transmit and place in one command, anchored at the home cell."""
        t0 = time.perf_counter()
        keys = f"a=T,f=100,i={IMAGE_ID},q=2,c={self.cols},r={self.rows},z={self.args.z},C=1"
        if self.args.medium == "file":
            path = os.path.join(self.args.tmpdir, f"pio_canvas_{os.getpid()}.png")
            with open(path, "wb") as fh:
                fh.write(self.png)
            b64 = base64.standard_b64encode(path.encode()).decode()
            self.w(f"{ESC}[H")
            self.g(f"{ESC}_G{keys},t=f;{b64}{ST}")
        else:
            b64 = base64.standard_b64encode(self.png).decode()
            first = True
            i = 0
            while i < len(b64):
                chunk = b64[i:i + 4096]
                i += 4096
                more = 1 if i < len(b64) else 0
                if first:
                    self.w(f"{ESC}[H")
                    self.g(f"{ESC}_G{keys},t=d,m={more};{chunk}{ST}")
                    first = False
                else:
                    self.g(f"{ESC}_Gm={more};{chunk}{ST}")
        self.transmits += 1
        self.image_bytes += len(self.png)
        self.record(ev="transmit", ms=round((time.perf_counter() - t0) * 1000, 2),
                    bytes=len(self.png), medium=self.args.medium, z=self.args.z)

    # ---- the text -------------------------------------------------------
    def char_backdrop(self, out):
        """What the character tier costs: dots and box drawing, every frame."""
        cards, _ = canvas.demo_layout(self.cols, self.rows)
        for r in range(0, self.rows, 2):
            out.append(f"{ESC}[{r+1};1H{DIM}" + ("· " * (self.cols // 2)))
        for c in cards:
            x, y, w_, h_ = c.col + 1, c.row + 1, c.cols, c.rows
            top = "╭" + "─" * (w_ - 2) + "╮"
            bot = "╰" + "─" * (w_ - 2) + "╯"
            out.append(f"{ESC}[{y};{x}H{MUTED}{top}")
            for i in range(1, h_ - 1):
                out.append(f"{ESC}[{y+i};{x}H│{ESC}[{y+i};{x+w_-1}H│")
            out.append(f"{ESC}[{y+h_-1};{x}H{bot}")
        out.append(RESET)

    FACES = ["(•_•)", "(◑_◑)", "(•ᴗ•)", "(◕‿◕)", "(°_°)"]
    VERBS = ["untangling things", "reading the room", "counting spoons", "chasing a pointer"]

    def text_frame(self, out):
        n = self.frame
        spin = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"[n % 10]
        face = self.FACES[(n // 7) % len(self.FACES)]
        verb = self.VERBS[(n // 23) % len(self.VERBS)]
        cards, _ = canvas.demo_layout(self.cols, self.rows)
        left, right, sticky = cards[0], cards[1], cards[2]

        def at(row, col, s):
            out.append(f"{ESC}[{row};{col}H{s}")

        at(1, 2, f"{BLUE}pio{RESET}{MUTED} · local service · 6 runs in 4 projects{RESET}")
        at(1, max(2, self.cols - 34), f"{MUTED}codex 0.155.1 ✓ · claude 2.1.278 ✓{RESET}")

        rows_left = [
            (f"{MUTED}PROJECTS{RESET}", f"{MUTED}all · ●2 ◐2 ◇1 ○1{RESET}"),
            (f"{TEXT}▾ Billing revamp{RESET}", f"{MUTED}~/src/api{RESET}"),
            (f"{AMBER}  ◐ auth-fix{RESET}   {BLUE}claude{RESET}  {MUTED}wt-2{RESET}", f"{TEXT}{(n // 10) % 60:2d}m{RESET}"),
            (f"{MUTED}  ○ lint      codex   wt-5{RESET}", f"{MUTED}3h{RESET}"),
            (f"{TEXT}▾ Data migration{RESET}", f"{MUTED}~/src/db{RESET}"),
            (f"{AMBER}  ◐ seed-data{RESET}  {TEAL}codex{RESET}   {MUTED}wt-6{RESET}", f"{TEXT}{40 + n % 20}s{RESET}"),
            (f"{PINK}  ◇ migrate{RESET}    {TEAL}codex{RESET}   {MUTED}wt-4{RESET}", f"{MUTED}1h{RESET}"),
        ]
        for i, (a, b) in enumerate(rows_left):
            r = left.row + 2 + i
            if r >= left.row + left.rows - 1:
                break
            at(r, left.col + 3, a)
            at(r, left.col + left.cols - 12, b)

        at(right.row + 2, right.col + 3,
           f"{ESC}[48;5;111m{ESC}[38;5;235m auth-fix {RESET} {AMBER}◐ needs approval{RESET}")
        at(right.row + 3, right.col + 3,
           f"{MUTED}claude 2.1.278 · ~/src/api · wt-2 · {64300 + n * 37:,} tokens{RESET}")
        body = [
            f"{MUTED}truth  {TEAL}delivered ✓{RESET} · {TEAL}acknowledged ✓{RESET} · {PINK}no sandbox{RESET}",
            f"{MUTED}⋮ 4 more blocks above{RESET}",
            f"{TEXT}● Edit  auth/session.rs{RESET}",
            f"{MUTED}  └ +14 −3 · inside workspace{RESET}",
            f"{TEXT}● Bash  cargo test -p auth{RESET}",
            f"{MUTED}  └ 23 passed · pre-approved by your rules{RESET}",
            f"{BLUE}● claude{RESET}",
            f"{TEXT}  The race is fixed. Tagging the release.{RESET}",
            f"{TEXT}● Bash  git tag v1.2{RESET}",
            f"{MUTED}  └ {AMBER}◐ WAITING FOR YOU{RESET}{MUTED} · 01:{59 - (n // 10) % 60:02d} left{RESET}",
        ]
        for i, line in enumerate(body):
            r = right.row + 5 + i
            if r >= right.row + right.rows - 1:
                break
            at(r, right.col + 3, line)

        at(sticky.row + 1, sticky.col + 3, f"{AMBER}◐ 2 need you{RESET} {MUTED}· press a{RESET}")
        at(sticky.row + 2, sticky.col + 3,
           f"{TEXT}auth-fix   git tag v1.2      01:{59 - (n // 10) % 60:02d} left{RESET}")
        at(sticky.row + 3, sticky.col + 3,
           f"{TEXT}seed-data  psql -f seed.sql  01:{52 - (n // 13) % 52:02d} left{RESET}")

        at(self.rows - 1, 2,
           f"{TEAL}● 2 running{RESET} {MUTED}·{RESET} {AMBER}◐ 2 waiting{RESET} {MUTED}·{RESET} "
           f"{PINK}◇ 1 uncertain{RESET} {MUTED}· ○ 1 done   {face} {spin} calc-add is {verb}…{RESET}")
        at(self.rows, 2,
           f"{MUTED}↵ open · space fold · ⌄ split · o orchestrate · a approvals · "
           f"u uncertain · n new · / filter · q detach{RESET}")
        at(self.rows - 1, max(2, self.cols - 24), f"{MUTED}{137 + n // 40}k today · frame {n}{RESET}")

    # ---- the loop -------------------------------------------------------
    def on_winch(self, *_):
        self.dirty = True

    def run(self):
        args = self.args
        signal.signal(signal.SIGWINCH, self.on_winch)
        tty.setraw(self.fd)
        self.record(ev="start", tier=args.tier, cols=self.cols, rows=self.rows,
                    cell=[self.cell_w, self.cell_h], px=[self.xpix, self.ypix],
                    pid=os.getpid())
        self.w(f"{ESC}[?1049h{ESC}[?25l{ESC}[?7l{ESC}[2J")
        started = time.time()
        frame_ms = []
        try:
            while time.time() - started < args.seconds:
                loop_t0 = time.perf_counter()
                if self.dirty:
                    prev = (self.rows, self.cols)
                    self.measure()
                    if prev != (self.rows, self.cols):
                        self.resizes += 1
                        self.record(ev="resize", cols=self.cols, rows=self.rows,
                                    xpix=self.xpix, ypix=self.ypix)
                    # Clear *first*. Erasing the display after a placement
                    # destroys it: the placement is anchored to a cell, and
                    # erasing that cell takes the image with it.
                    self.w(f"{ESC}[2J")
                    if args.tier == "drawn":
                        self.delete_image()
                        self.build_png()
                        self.transmit()
                    self.dirty = False
                elif args.tier == "drawn" and args.retransmit == "frame":
                    self.delete_image()
                    self.transmit()

                out = []
                if args.sync:
                    out.append(SYNC_BEGIN)
                if args.tier == "char":
                    self.char_backdrop(out)
                if args.tier != "blank" and not args.no_text:
                    self.text_frame(out)
                if args.sync:
                    out.append(SYNC_END)
                self.w("".join(out))

                self.frame += 1
                dt = time.perf_counter() - loop_t0
                frame_ms.append(dt * 1000)
                sleep = max(0.0, 1.0 / args.fps - dt)
                time.sleep(sleep)
        finally:
            if args.tier == "drawn":
                self.delete_image()
            self.w(f"{ESC}[?7h{ESC}[?25h{ESC}[?1049l{RESET}")
            termios.tcsetattr(self.fd, termios.TCSADRAIN, self.old)
            elapsed = time.time() - started
            frame_ms.sort()
            summary = {
                "ev": "summary",
                "tier": args.tier,
                "retransmit": args.retransmit,
                "medium": args.medium,
                "z": args.z,
                "sync": args.sync,
                "passthrough": args.passthrough,
                "tmux": bool(os.environ.get("TMUX")),
                "seconds": round(elapsed, 3),
                "frames": self.frame,
                "fps_actual": round(self.frame / elapsed, 2) if elapsed else 0,
                "bytes_to_tty": self.bytes_written,
                "bytes_per_second": int(self.bytes_written / elapsed) if elapsed else 0,
                "image_transmits": self.transmits,
                "image_bytes_total": self.image_bytes,
                "resizes": self.resizes,
                "frame_ms_p50": round(frame_ms[len(frame_ms) // 2], 3) if frame_ms else None,
                "frame_ms_p99": round(frame_ms[int(len(frame_ms) * 0.99)], 3) if frame_ms else None,
                "cols": self.cols, "rows": self.rows,
                "cell": [round(self.cell_w, 3), round(self.cell_h, 3)],
                "text_area_px": [self.xpix, self.ypix],
            }
            self.record(**summary)
            if self.log:
                self.log.close()
            os.close(self.fd)
            print(json.dumps(summary))


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--tier", choices=["drawn", "char", "none", "blank"], default="drawn")
    p.add_argument("--seconds", type=float, default=15)
    p.add_argument("--fps", type=float, default=20)
    p.add_argument("--retransmit", choices=["never", "resize", "frame"], default="resize")
    p.add_argument("--medium", choices=["direct", "file"], default="direct")
    # -1 is *not* the right z. At -1 the image covers every explicitly set cell
    # background -- selections, chips, the amber sticky note -- and only the
    # glyphs survive. Below INT32_MIN/2 the image goes under cell backgrounds
    # and over the default background, which is what "behind the text" means
    # for a TUI. zorder_test.py has the screenshots.
    p.add_argument("--z", type=int, default=-1073741825)
    p.add_argument("--scale", type=int, default=2)
    p.add_argument("--sync", action="store_true", help="wrap each frame in DECSET 2026")
    p.add_argument("--log", default="")
    p.add_argument("--no-text", action="store_true",
                   help="backdrop only: lets a screenshot be diffed against the reference")
    p.add_argument("--passthrough", action="store_true",
                   help="wrap graphics escapes in tmux DCS passthrough")
    p.add_argument("--tmpdir", default="/tmp")
    args = p.parse_args()
    Screen(args).run()


if __name__ == "__main__":
    main()
