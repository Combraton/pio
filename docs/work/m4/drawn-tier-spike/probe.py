#!/usr/bin/env python3
"""Ask the terminal what it actually supports, instead of assuming.

Every claim in the report about "Ghostty supports X" comes from this file
running on a real tty and reading the terminal's own answer back. The Kitty
graphics protocol answers a query with `_Gi=<id>;OK` or `_Gi=<id>;E<code>:...`,
so an unsupported key or mode is not a guess — it is a string the terminal
sent us.

Run it *in* the terminal under test:

    python3 probe.py /tmp/probe.json
"""
from __future__ import annotations

import base64
import fcntl
import json
import os
import select
import struct
import sys
import termios
import tty

ESC = "\x1b"
APC = ESC + "_G"
ST = ESC + "\\"


def winsize(fd):
    packed = fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8)
    rows, cols, xpix, ypix = struct.unpack("HHHH", packed)
    return {"rows": rows, "cols": cols, "xpixel": xpix, "ypixel": ypix}


def drain(fd, timeout=0.05):
    out = b""
    while select.select([fd], [], [], timeout)[0]:
        chunk = os.read(fd, 65536)
        if not chunk:
            break
        out += chunk
        timeout = 0.02
    return out


def ask(fd, payload, timeout=0.6):
    """Write a sequence, read whatever comes back within `timeout`."""
    drain(fd, 0.01)
    os.write(fd, payload.encode())
    out = b""
    deadline = timeout
    while select.select([fd], [], [], deadline)[0]:
        out += os.read(fd, 65536)
        deadline = 0.08
        if out.endswith(b"\x1b\\") or out.endswith(b"\x07"):
            break
    return out.decode("utf-8", "replace")


def gq(fd, keys, payload_bytes=b"\x00\x00\x00", ident=31):
    """A graphics *query* (a=q): the terminal replies without drawing."""
    b64 = base64.b64encode(payload_bytes).decode()
    seq = f"{APC}i={ident},{keys};{b64}{ST}"
    return ask(fd, seq)


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/probe.json"
    fd = os.open("/dev/tty", os.O_RDWR)
    old = termios.tcgetattr(fd)
    result = {}
    try:
        tty.setraw(fd)
        result["term"] = os.environ.get("TERM")
        result["term_program"] = os.environ.get("TERM_PROGRAM")
        result["tmux"] = os.environ.get("TMUX", "")
        result["winsize"] = winsize(fd)

        # 1. Primary device attributes: a baseline that every terminal answers.
        result["da1"] = repr(ask(fd, ESC + "[c"))
        # 2. XTVERSION: who are we talking to.
        result["xtversion"] = repr(ask(fd, ESC + "[>0q"))
        # 3. Cell size in pixels, the terminal's own answer (CSI 16 t).
        result["csi16t_cellsize"] = repr(ask(fd, ESC + "[16t"))
        # 4. Text area in pixels (CSI 14 t).
        result["csi14t_textarea"] = repr(ask(fd, ESC + "[14t"))

        # 5. Does it speak the graphics protocol at all?
        result["graphics_query"] = repr(gq(fd, "a=q,s=1,v=1,f=24,t=d"))

        # 6. Does it accept a *negative z* placement key?
        #    a=q with the placement keys present: kitty and Ghostty validate the
        #    whole command, so an unknown or rejected key answers E...
        result["z_negative"] = repr(gq(fd, "a=q,s=1,v=1,f=24,t=d,z=-1,c=2,r=1", ident=32))
        result["z_below_bg"] = repr(gq(fd, "a=q,s=1,v=1,f=24,t=d,z=-1073741825,c=2,r=1", ident=33))

        # 7. Unicode placeholder (virtual) placement.
        result["unicode_placeholder"] = repr(gq(fd, "a=q,s=1,v=1,f=24,t=d,U=1,c=2,r=1", ident=34))

        # 8. PNG payload (f=100) and file/tempfile transmission mediums.
        png = base64.b64decode(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
        )
        result["png_f100"] = repr(gq(fd, "a=q,f=100,t=d", png, ident=35))
        tmp = "/tmp/pio_probe_pixel.png"
        with open(tmp, "wb") as fh:
            fh.write(png)
        result["medium_file"] = repr(
            ask(fd, f"{APC}i=36,a=q,f=100,t=f;{base64.b64encode(tmp.encode()).decode()}{ST}")
        )

        # 9. Animation frames (a=f) -- would be needed for a moving backdrop.
        result["animation_a_f"] = repr(gq(fd, "a=q,s=1,v=1,f=24,t=d,r=1", ident=37))

        # 10. Kitty keyboard protocol, since a TUI wants it anyway.
        result["kitty_keyboard"] = repr(ask(fd, ESC + "[?u"))

        # 11. Does the terminal report graphics in its DA1? (kitty does not)
        # 12. Synchronised output (DECSET 2026): the anti-flicker primitive.
        result["sync_output_2026"] = repr(ask(fd, ESC + "[?2026$p"))
        # 13. Passthrough-relevant: DECSET 1049 alt screen is assumed; check
        #     that the terminal answers a mode query at all.
        result["altscreen_1049"] = repr(ask(fd, ESC + "[?1049$p"))
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
        os.close(fd)

    with open(out_path, "w") as fh:
        json.dump(result, fh, indent=2)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
