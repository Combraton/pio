# M4 notes — decisions and material held for later

## The drawn tier: not in v0.1

The one-day spike ([issue #13](https://github.com/Combraton/pio/issues/13), draft [PR #14](https://github.com/Combraton/pio/pull/14), branch `codex/m4-drawn-tier`, report at `docs/work/m4/drawn-tier-spike/REPORT.md` on that branch) answered the question and the answer is **no**. **The character tier ships alone and nothing is wired into the binary.** The branch and its evidence are kept.

The mechanism itself worked better than expected — no flicker across 900 captured frames, survives resize to within six device pixels, and costs *less* CPU than drawing the same grid in characters. It failed on everything around it: inside tmux it either renders nothing or, with passthrough on, paints PIO's grid into other panes and other tmux windows, because tmux cannot clip or delete an image it does not know exists.

### Three keepers, post-v0.1

Recorded because they were expensive to find and will be needed if this is ever revisited.

1. **`z=-1` is the wrong z, and it looks right.** Ghostty has three layers. Between `-1073741824` and `0` the image covers **every explicitly set cell background**, so a chip, a selected row, reverse video and the amber sticky notes lose their backgrounds and only glyph shapes survive. The drawn tier needs `z < -1073741824`. That constant is the whole trick.
2. **Ghostty 1.3.1 erases graphics inconsistently.** `CSI 2 J` deletes graphics placements; `CSI H` followed by `CSI J`, which erases exactly the same cells, does not. This is how the spike first failed silently. Not filed upstream.
3. **`a=q` is not a capability handshake.** Ghostty answers `OK` to `z`, `U`, `c` and `r` regardless of whether it supports them, so any tier detection has to be empirical or version-based.

Only Ghostty was tested, as the time-box specified. The report claims nothing about Kitty or WezTerm.

## Held for later

- **The drawn tier**, above.
- **Orchestrate (M4b)** waits on two things the design input names: a Protocol proposal so a steering message can record its author, and the owner's dated approval of the lead's tool. The lead-tool mechanism per harness is posted for that approval before any live lead run.
