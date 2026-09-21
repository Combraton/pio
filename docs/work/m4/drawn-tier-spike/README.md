# Drawn-tier spike (M4, time-boxed to one day)

The question, from `../design-input/M4-DESIGN-INPUT.md`: could PIO render the
Canvas grid, the hand-drawn card outlines and the arrows as **one image under
the text cells** instead of drawing them with characters?

**The answer is in [`REPORT.md`](REPORT.md), and it is no — not clearly good.
The character tier ships alone.**

This directory is the evidence for that sentence: a working spike, the
measurement harness that produced every number, the raw data and the
screenshots. It is not production code, it is not wired into the PIO binary,
and it should not be.
