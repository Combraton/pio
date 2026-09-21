# M4 spike: the drawn tier in Ghostty

**Verdict: not clearly good. The character tier ships alone and nothing else
changes.** The mechanism itself works better than expected — in a bare Ghostty
window the backdrop is pixel-identical across 900 consecutive captured frames
while the text updates underneath it, it survives every resize to within six
device pixels, and it costs *less* CPU than drawing the same grid and card
outlines in characters. It fails on everything around it. Inside tmux it either
draws nothing at all (the default) or, with `allow-passthrough on`, paints PIO's
backdrop across other panes and other tmux windows, where tmux cannot remove it
because tmux does not know it is there. Three ordinary terminal operations
silently destroy the placement, one of them (`CSI 2 J`) being the first thing
most TUIs write. Getting it right needs a vector rasteriser, a PNG encoder, a
per-terminal capability probe and a second layout path in the binary, in
exchange for an appearance improvement and about half a percentage point of one
CPU core. That is not the "clearly good" the design asked for.

Everything below was measured on this machine, on 2026-09-21, against Ghostty
1.3.1 (Metal renderer, CoreText, macOS, Retina display at 2x device pixels), and
tmux 3.6a. Nothing was tested in Kitty or WezTerm: the design time-boxed this to
Ghostty and so did I, so the report says nothing about them.

---

## The four answers

### 1. Flicker — no, none, when the image is transmitted once

The window was recorded losslessly (ffv1) at 60 fps while the spike repainted a
full text frame at ~18.6 fps for 15 seconds. Two bands of that recording were
compared against their own per-pixel median:

- the **quiet band**, rows carrying backdrop and no text at all, and
- the **busy band**, the status bar, which changes on every frame and exists
  only to prove the recording caught something. A still quiet band means
  nothing if the busy band is still too.

| variant | quiet band frames differing | max pixel delta | busy band frames differing |
|---|---|---|---|
| no backdrop | 0 / 900 | 0 | 900 / 900 |
| character tier | 0 / 900 | 0 | 900 / 900 |
| **drawn, transmitted once** | **0 / 900** | **0** | 900 / 900 |
| drawn, once, with DECSET 2026 | 0 / 900 | 0 | 900 / 900 |
| drawn, deleted and re-sent every frame | **123 / 900** | 238 | 900 / 900 |
| drawn, every frame, via a temp file | **58 / 900** | 238 | 900 / 900 |

900 consecutive captured frames, and every consecutive pair of them, are
byte-identical in the quiet band. The image is not redrawn when the text is
redrawn; it is a placement the terminal already holds, and repainting cells over
it does not touch it. Synchronised output (DECSET 2026, which Ghostty supports)
changes nothing, because there is nothing to tear.

The flicker is real in the one case where the program creates it: deleting and
re-transmitting the image each frame blanks the backdrop on 14% of frames. The
worst case is worth naming because **a TUI can fall into it by accident** — see
the `CSI 2 J` trap below. The file-transfer medium halves the flicker but does
not remove it, which says the blink is the delete-and-replace cycle, not the
bandwidth.

Raw numbers: `data/measurements-20fps.json`.

### 2. Resize — survives, to within six device pixels; scroll is a different story

Resize was measured without heuristics. The spike was run with `--no-text`, so
the screenshot *is* the backdrop, the same backdrop was rendered offline from
the same layout code, and one was slid over the other; the shift with the
smallest mean absolute difference is the misalignment.

| window | grid | cell (device px) | best shift (dy, dx) | mean abs error |
|---|---|---|---|---|
| 880x610 | 110x34 | 16.00 x 34.00 | **(0, 0)** | 0.67 |
| 640x470 | 80x25 | 16.00 x 35.04 | (-2, 0) | 2.19 |
| 1180x760 | 147x42 | 16.05 x 34.67 | (-1, -4) | 1.89 |
| 560x360 | 70x19 | 16.00 x 34.53 | (-6, 0) | 2.14 |
| 900x500 | 112x27 | 16.07 x 34.67 | (-1, -4) | 2.38 |
| 880x610 (back) | 110x34 | 16.00 x 34.00 | **(0, 0)** | 0.67 |

Perfect — literally zero — whenever the cell is a whole number of device pixels.
Off by at most six device pixels, a fifth of a cell, when it is not, because
Ghostty scales the transmitted image into the `c`x`r` cell box it was given and
quantises differently from the renderer. Never off by a whole cell. Nothing was
dropped and nothing had to be re-placed by hand.

**During** a resize, before the program can react: the placement keeps its cell
dimensions. Freezing the spike with `SIGSTOP`, enlarging the window and
photographing it shows the backdrop still crisp — Ghostty re-rasterises at the
new cell size rather than stretching bitmap pixels — but still covering only the
original 110x34 cells of what is now a 147x42 grid
(`evidence/07-resize-before-the-app-catches-up.png`). So a drag shows a correct
picture in the wrong extent, never a flash or garbage. The catch-up cost is the
program's own: re-rendering the PNG took 22–104 ms (median 59 ms) for a 49–178 KB
image, transmission 1.1–3.5 ms. Under a continuous drag the backdrop would trail
the window by roughly one to three frames.

**Scroll is where it gets sharp**, and the answer differs by screen:

| operation | result |
|---|---|
| repaint every cell, no erase (what a TUI does) | image untouched |
| `CSI 3 S`, scroll the region up, in the alt screen | image **does not move**; it stays anchored to the viewport while the text slides under it |
| newline on the last row, in the alt screen | image does not move |
| 40 lines printed in the **primary** screen | image scrolls up with the text and out of the viewport, gone |

In the alternate screen — where a TUI lives — the image stays put across a
scroll, which is convenient but means any pane that scrolls its own text must
re-place the backdrop itself. In the primary screen the placement travels with
the text into scrollback, as the protocol says it should.

Raw numbers: `data/resize-alignment.json`.

### 3. tmux — broken by default, and worse than broken when "fixed"

Four cases, each a fresh 110x34 tmux session in its own Ghostty window.

**Bare tmux (`allow-passthrough off`, the default): nothing renders.** The text
is perfect, the backdrop is completely absent, and no garbage appears on screen
— tmux swallows the APC command whole. It fails cleanly and silently
(`evidence/03-tmux-bare-nothing-renders.png`).

**With `allow-passthrough on` and the graphics wrapped in tmux's own DCS, the
backdrop appears** — correctly positioned, correctly aligned with the text. This
was a surprise and it is the one genuinely unexpected positive of the spike.
It does not survive contact with tmux:

- **Split the window** and the backdrop is drawn *in the other pane*, at its
  original full-terminal size and position, while the pane that owns it is left
  bare. tmux cannot clip it, move it or delete it
  (`evidence/04-tmux-split-leaks-into-other-pane.png`).
- **Switch to a different tmux window** and PIO's grid, card outlines and arrow
  are still on screen, over somebody else's work, in a window PIO has nothing to
  do with (`evidence/05-tmux-leaks-onto-another-window.png`).
- Switching back leaves it visible, because tmux never removed it; it is still
  there only by accident.

This is not a cosmetic shortfall. With passthrough on — which is exactly the
setting a user turns on to get images working in tmux — PIO would paint over
other people's terminals with no way to clean up after itself.

`$TMUX` is set in the environment, so PIO could of course refuse the drawn tier
under tmux and fall back to characters. That is a real mitigation and it does
close this particular hole. It also means the drawn tier is, by construction,
unavailable to every tmux user while a second rendering path is maintained for
everyone else.

Raw numbers: `data/tmux.json`.

### 4. CPU — cheaper than the character tier, which is the opposite of the worry

CPU was measured as **CPU time, not CPU percent**: each variant ran in its own
Ghostty process — so the measurement contains that window and nothing else on
the machine — and `ps -o time=` was read at the start and at the end. That is an
integral over the interval, not a sample of an instant. The Python side is
reported separately from the terminal.

15 s at ~18.6 fps, 110x34, one full text frame per tick:

| variant | CPU seconds | % of one core | Ghostty | Python | bytes/s to the tty | image transmits |
|---|---|---|---|---|---|---|
| no backdrop | 0.35 | 2.2 | 0.31 | 0.04 | 37 KB | 0 |
| character tier | 0.44 | 2.7 | 0.39 | 0.05 | **134 KB** | 0 |
| **drawn, transmitted once** | **0.36** | **2.2** | 0.31 | 0.05 | 44 KB | 1 |
| drawn + DECSET 2026 | 0.40 | 2.5 | 0.36 | 0.04 | 45 KB | 1 |
| drawn, re-sent every frame | 2.63 | 16.4 | 2.38 | 0.25 | 2.6 MB | 371 |
| drawn, every frame, temp file | 1.72 | 10.7 | 1.58 | 0.14 | 39 KB | 371 |

Near idle (20 s at 1 fps): no backdrop 0.15 s (0.7%), character tier 0.23 s
(1.1%), drawn tier 0.22 s (1.0%).

The drawn tier costs the same as having no backdrop at all and about half a
percentage point of one core *less* than the character tier, because the
character tier re-sends a screenful of grid dots and box-drawing glyphs on every
frame — three times the bytes — while the drawn tier sends 106 KB once and then
nothing. One-off costs: 22–104 ms to render the PNG, 1.1–3.5 ms to transmit it,
paid again only on resize.

Two limits on this number, stated rather than buried. **GPU time is not
included**: `ps` measures CPU, and Ghostty composites through Metal, so the one
extra full-screen textured quad per frame is not in these figures. There is no
unprivileged way to measure it here and it was not estimated. And the figures
are for one window size on one machine; they are a comparison between variants
measured identically, not an absolute.

Raw numbers: `data/measurements-20fps.json`, `data/measurements-1fps.json`.

---

## What Ghostty actually supports, and three traps

`probe.py` asks the terminal and records its answers rather than assuming
(`data/ghostty-capability-probe.json`). Ghostty 1.3.1 answers `OK` to the
graphics query, to PNG payloads (`f=100`), to file and temp-file transmission,
to negative `z`, to `z` below `INT32_MIN/2`, to unicode placeholders (`U=1`),
and reports DECSET 2026 and the kitty keyboard protocol. It identifies itself as
`ghostty 1.3.1` via XTVERSION and reports `TIOCGWINSZ` pixel dimensions in
*device* pixels on a Retina display, which is what an image wants.

**Trap 1 — `z=-1` is the wrong z, and it looks right until it doesn't.**
Ghostty implements three distinct layers, and `zorder_test.py` photographs all
three (`evidence/02-z-index-layers.png`):

| z | where the image lands |
|---|---|
| `z >= 0` | over the text; the screen is just the picture |
| `-1073741824 <= z < 0` | over **every cell background**, under the glyphs |
| `z < -1073741824` | under cell backgrounds, over the default background |

At `z=-1` the text is readable — and every explicitly coloured cell background
is gone: the selected row, the `auth-fix` chip, reverse video, the amber sticky
note. Only glyph shapes survive. The M4 screens depend on cell backgrounds, so
the drawn tier must use `z < -1073741824`. Worth saying plainly because `z=-1`
is the obvious thing to write and it produces a screen that looks plausible in a
screenshot and is wrong in use.

**Trap 2 — `CSI 2 J` deletes the placement; `CSI J` does not.** Five erase
operations, each with the image freshly placed (`erase_test.py`):

| operation | backdrop afterwards |
|---|---|
| nothing erased | present |
| **`CSI 2 J`, erase whole display** | **gone** |
| `CSI H` then `CSI J`, erase to end of display | present |
| `CSI 2 K` on every row | present |
| overwrite every cell with a space | present |

`CSI 2 J` and `CSI H CSI J` erase exactly the same cells and are treated
differently. A TUI that opens each frame with a clear-screen would therefore
delete and re-transmit its backdrop every frame and land squarely in the 14%
flicker case above — which is precisely how this spike first failed, before the
clear was moved ahead of the transmit.

**Trap 3 — a query cannot tell you any of this.** `a=q` answered `OK` to every
placement key, including `z` and `U`, whether or not the terminal can honour it
in a query. Capability detection for the drawn tier has to be empirical or
version-based; it cannot be a handshake.

---

## Why "not clearly good"

The mechanism passes on its own terms. The judgement is about what shipping it
would cost against what it buys.

It buys: hand-drawn card outlines and arrows instead of box-drawing characters,
in Ghostty, Kitty and WezTerm, for users not running tmux — plus about half a
percentage point of one core.

It costs: a vector rasteriser and a PNG encoder in the PIO binary; a second
layout path that produces pixels where the other produces cells, kept in
agreement with it forever; per-terminal capability detection that cannot be done
by handshake; a hard exclusion of tmux users, enforced by PIO itself; and three
behaviours that destroy the image silently, two of which (`CSI 2 J`, a scroll in
the primary screen) a maintainer would have to remember not to do. The character
tier still has to exist for everyone else regardless, so none of this replaces
work — it adds to it.

There is no performance case either way: the difference between the two tiers is
inside the noise of a terminal already using 2% of one core.

So: **the character tier ships alone.** This report is here so that if the
question is reopened, the answer does not have to be found twice.

---

## What is in this directory

| file | what it is |
|---|---|
| `canvas.py` | renders the backdrop — dotted grid, hand-drawn rounded card outlines, arrow — as one RGBA PNG |
| `drawn_tier.py` | the spike: places that PNG behind the text and repaints a live PIO board over it |
| `probe.py` | asks the terminal what it supports and records its answers |
| `zorder_test.py` | the three z layers, for photographing |
| `erase_test.py` | which erase operations destroy a placement |
| `scroll_test.py` | what scrolling does, in the alt screen and the primary screen |
| `harness/window.py` | launches an isolated Ghostty, finds it, resizes it, photographs it, reads its CPU time |
| `harness/flicker.py` | lossless 60 fps recording and the quiet-band / busy-band analysis |
| `harness/measure.py` | runs every variant and writes the CPU and flicker table |
| `harness/resize_test.py` | resize with the live board on screen |
| `harness/align_test.py` | the pixel-exact alignment measurement |
| `harness/tmux_test.py` | the four tmux cases |
| `data/` | the raw measurements every table above is drawn from |
| `evidence/` | the screenshots |

To run it yourself, in Ghostty on macOS, from this directory:

```sh
python3 drawn_tier.py --tier drawn --seconds 20 --fps 20      # watch it
python3 harness/measure.py --out /tmp/r.json --scratch /tmp --seconds 15 --record
python3 harness/tmux_test.py --scratch /tmp --out /tmp/tmux.json
```

It needs Pillow, numpy, ffmpeg and macOS screen-recording permission. It is a
spike: it is not wired into the PIO binary and should not be.

## Things noticed and not fixed

- **Ghostty 1.3.1**: `CSI 2 J` deletes graphics placements while `CSI H CSI J`,
  which erases the same cells, does not. Recorded here, not filed and not fixed.
- **Ghostty 1.3.1**: `a=q` returns `OK` for placement keys a query cannot
  honour, so the query is not a capability handshake. Arguably conformant;
  noted because it changes how detection has to be written.
- **tmux 3.6a**: with `allow-passthrough on`, graphics escape the pane and the
  window that produced them. A known tmux limitation, not a PIO defect, and the
  reason for the verdict.
- **This spike's demo text** is not responsive below about 80 columns: at 53x15
  the board's text overruns its cards. That is the spike's placeholder layout,
  not the drawn tier, and it has no bearing on any measurement above.
- Nothing in the PIO source was touched, and nothing in it looked wrong from
  here.
