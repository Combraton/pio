#!/usr/bin/env python3
"""Render the Canvas backdrop as one RGBA image.

The drawn tier's whole premise is that the grid, the card outlines and the
arrows are *one picture*, not characters. This module is that picture: given a
pixel size and a cell size it paints a dotted grid, some hand-drawn rounded
card outlines and an arrow, on a transparent ground so the terminal's own
background shows through.

The jitter is deliberate and deterministic. `rough.js` in the HTML mockups gets
its hand-drawn look from sampling a wobbly path twice; the same trick works
here, and seeding it from the layout means the same layout always draws the
same wobble, so a repaint is pixel-identical and a screenshot diff means
something.
"""
from __future__ import annotations

import io
import math
import random
from dataclasses import dataclass

from PIL import Image, ImageDraw

# The Canvas palette, from the M4 design input's PIO Dark column.
GRID_DOT = (82, 94, 104, 255)
EDGE = (65, 84, 95, 255)
ID1 = (111, 168, 255, 255)
ID2 = (240, 143, 180, 255)
ID3 = (127, 214, 194, 255)


@dataclass(frozen=True)
class Card:
    """A card outline in *cell* coordinates."""

    col: int
    row: int
    cols: int
    rows: int
    color: tuple
    label: str = ""


def _wobble(rng, pts, amount):
    """Displace each point by up to `amount` pixels."""
    return [(x + rng.uniform(-amount, amount), y + rng.uniform(-amount, amount)) for x, y in pts]


def _rounded_path(x0, y0, x1, y1, radius, steps=6):
    """A rounded rectangle as a list of points, corners sampled as arcs."""
    pts = []
    corners = [
        (x1 - radius, y0 + radius, -90, 0),
        (x1 - radius, y1 - radius, 0, 90),
        (x0 + radius, y1 - radius, 90, 180),
        (x0 + radius, y0 + radius, 180, 270),
    ]
    pts.append((x0 + radius, y0))
    for cx, cy, a0, a1 in corners:
        for i in range(steps + 1):
            a = math.radians(a0 + (a1 - a0) * i / steps)
            pts.append((cx + radius * math.cos(a), cy + radius * math.sin(a)))
    pts.append((x0 + radius, y0))
    return pts


def _sketch(draw, rng, pts, color, width, passes=2, amount=1.1):
    """Draw a path two or three times with fresh jitter: the hand-drawn look."""
    for _ in range(passes):
        draw.line(_wobble(rng, pts, amount), fill=color, width=width, joint="curve")


def _arrow(draw, rng, start, end, color, width=2):
    """A slightly bowed arrow with a two-stroke head."""
    (x0, y0), (x1, y1) = start, end
    mx, my = (x0 + x1) / 2, (y0 + y1) / 2
    bow = 10
    ctrl = (mx, my - bow)
    pts = []
    for i in range(25):
        t = i / 24
        a = (1 - t) ** 2
        b = 2 * (1 - t) * t
        c = t * t
        pts.append((a * x0 + b * ctrl[0] + c * x1, a * y0 + b * ctrl[1] + c * y1))
    _sketch(draw, rng, pts, color, width, passes=2, amount=0.9)

    ang = math.atan2(pts[-1][1] - pts[-2][1], pts[-1][0] - pts[-2][0])
    for off in (2.5, -2.5):
        tip = (x1, y1)
        back = (x1 - 11 * math.cos(ang + off / 6), y1 - 11 * math.sin(ang + off / 6))
        _sketch(draw, rng, [back, tip], color, width, passes=2, amount=0.7)


def render(
    width_px: int,
    height_px: int,
    cell_w: int,
    cell_h: int,
    cards: list,
    arrows: list,
    scale: int = 2,
    seed: int = 7,
) -> bytes:
    """Return PNG bytes for the whole backdrop.

    `scale` supersamples: Ghostty scales the placement down to the cell box we
    ask for, so drawing at 2x and letting it downsample is what keeps a 2px
    stroke from looking chewed.
    """
    rng = random.Random(seed)
    W, H = int(round(width_px * scale)), int(round(height_px * scale))
    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    # Cell size stays fractional. Flooring it to whole pixels is what makes the
    # image a few pixels shorter than the text area, and Ghostty then stretches
    # it to fill the cells it was asked to cover -- a slow drift that shows up
    # as a card outline sitting half a row off at some window heights.
    cw, ch = cell_w * scale, cell_h * scale

    # The dotted grid: one dot every other column, every other row.
    r = max(1, scale)
    nrows = int(H / (ch * 2)) + 1
    ncols = int(W / (cw * 2)) + 1
    for ri in range(nrows):
        for ci in range(ncols):
            x, y = ci * cw * 2 + cw / 2, ri * ch * 2 + ch / 2
            draw.ellipse((x - r, y - r, x + r, y + r), fill=GRID_DOT)

    for card in cards:
        x0 = card.col * cw
        y0 = card.row * ch
        x1 = (card.col + card.cols) * cw
        y1 = (card.row + card.rows) * ch
        pts = _rounded_path(x0 + 2, y0 + 2, x1 - 2, y1 - 2, radius=ch)
        _sketch(draw, rng, pts, card.color, width=max(1, scale), passes=2, amount=1.0 * scale / 2)

    for (a, b, color) in arrows:
        _arrow(
            draw,
            rng,
            (a[0] * cw, a[1] * ch),
            (b[0] * cw, b[1] * ch),
            color,
            width=max(1, scale),
        )

    if scale != 1:
        img = img.resize((int(round(width_px)), int(round(height_px))), Image.LANCZOS)
    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=False, compress_level=1)
    return buf.getvalue()


def demo_layout(cols: int, rows: int):
    """Three cards and an arrow, sized to whatever terminal we are handed."""
    body_top = 2
    body_bottom = rows - 3
    left_w = max(20, cols // 2 - 4)
    right_col = left_w + 6
    right_w = max(18, cols - right_col - 1)
    split = body_top + max(6, (body_bottom - body_top) * 2 // 3)

    cards = [
        Card(1, body_top, left_w, split - body_top, ID1, "PROJECTS"),
        Card(right_col, body_top, right_w, split - body_top, ID3, "auth-fix"),
        Card(1, split + 1, cols - 3, body_bottom - split - 1, ID2, "2 need you"),
    ]
    arrows = [
        ((left_w + 1, body_top + (split - body_top) // 2), (right_col, body_top + (split - body_top) // 2), ID3),
    ]
    return cards, arrows


if __name__ == "__main__":
    import sys

    cols, rows, cw, ch = 100, 30, 9, 19
    cards, arrows = demo_layout(cols, rows)
    data = render(cols * cw, rows * ch, cw, ch, cards, arrows)
    out = sys.argv[1] if len(sys.argv) > 1 else "canvas.png"
    with open(out, "wb") as fh:
        fh.write(data)
    print(f"{out}: {len(data)} bytes, {cols*cw}x{rows*ch}")
