# PIO M4 — terminal screen design input

Status: **accepted by the owner, 2026-09-21.** Produced by the independent reviewer as the M4 design step the release plan requires. The builder implements against this and **records every deviation with its reason**.

## What is in this folder

| File | Use |
|---|---|
| `mockups.html` | The accepted mockups. Open in a browser. Skin and feel switches are at the top. |
| `screens/*-canvas.png` | One screenshot per screen, Canvas feel, PIO Dark. The reference look. |
| `screens/*-plain.png` | The same screens, Plain feel. The look when Canvas is switched off. |
| `M4-DESIGN-INPUT.md` | This document. |

The mockups are HTML. The hand-drawn outlines come from a browser library. **Nothing about the HTML is a requirement.** The requirements are the layout, the words on screen, the states, the keys, the mouse actions and the rules below.

Copy this folder into your own worktree, for example `docs/work/m4/design/`. The reviewer does not write to your checkout.

## Decisions the owner made

1. The screen opens on **the board**.
2. Expect **five or six runs** at once.
3. Approvals **never interrupt**. They wait on an attention note and are walked with `a`.
4. **Uncertainty has its own color** (violet) everywhere, plus its own glyph and word.
5. The transcript is **blocks** you step through, not a raw stream.
6. **Mouse and keyboard.** Every mouse action also has a key.
7. **Themed**, switchable live. First launch uses **PIO Dark**.
8. A folder is a **project the user names**. The working location is shown beside it.
9. The busy indicator is **playful**. It can be turned off.
10. Desktop notification when a run needs the user, **only when the terminal is not focused**. The user can change this.
11. On an uncertain run the one action is **open the workspace diff**.
12. The preferred feel is **Canvas**: cards on a grid, in the spirit of Excalidraw.
13. **Orchestrate at size Medium is in v0.1**: a harness can lead other runs. See the last section for what that costs.

## The shape of every screen

- **No divider lines.** Every pane is a **card**. The gap between cards is the divider. A small grip `⋮⋮` in the gap is the drag handle.
- Top line: product, service, counts on the left. Qualified harness versions on the right.
- Bottom: a **status bar** that is always true, then one dim **hint line** whose keys change with the focused card.
- A card for a run is outlined in that run's **identity color**. The same color is on its name chip and on the rule above its prompt.
- Attention lives on two **sticky notes**: amber for approvals, violet for uncertain runs.

## Screens

### 1. The board (`screens/1-board-*.png`)
- Left card **PROJECTS**: user-named projects, each with its working location and its own state counts. Rows: glyph, run name, harness in its color, worktree, age. `space` or a click folds a project. A folded project still shows who inside needs the user.
- Right card: **live preview of the selected run**. Title row: name chip, state, harness version, location, worktree, tokens. Then the truth line, then the latest blocks. It is the real session, not a summary.
- Below: the two sticky notes. `a` walks approvals. `u` walks uncertain runs. Clicking a note does the same.
- Resize: drag the grip, or `ctrl-←` / `ctrl-→`. `p` moves the preview underneath on a narrow terminal. Sizes persist.
- Keys: `↵` open, `space` fold, `v` split, `o` orchestrate, `n` new, `/` live filter, `:` command box, `?` keys for this screen, `q` detach.

### 2. Split view (`screens/2-split-view-*.png`)
- Two to four run cards side by side. Each has its own prompt line.
- `ctrl-arrows` resize, `tab` next card, `z` zoom the focused card and back, `=` equal sizes, `+` add a card, `x` close a card.
- **Closing a card never stops the run.**

### 3. Orchestrate (`screens/3-orchestrate-*.png`) — M4b
- Left: the **team diagram**. Lead card on top with its goal and what it is waiting for. The runs it started below. Labeled arrows for who talks to whom.
- Right: the **MESSAGES thread**. Each entry: from → to, kind (brief or note), time, delivery ticks, then the text.
- Ticks: `✓` released, `✓✓` delivered and acknowledged, violet `◇` uncertain. **An unproven message never shows two ticks.**
- Top right: the lead's budget meter. Everything the lead starts counts against it.
- `↑↓←→` move between cards, `↵` open that run, `m` send a message yourself, `t` thread only, `o` leave.

### 4. The approval walk (`screens/4-approval-walk-*.png`)
- A card over the board, one request at a time: run, action id, what it wants to do, where it lands, what the harness said.
- Shows **what the harness offered** and **what PIO will send**. "Always allow" is visible and visibly unavailable.
- Numbered answers: `1` allow once, `2` deny, `3` show the full request, `4` decide later. `tab` goes to the next request without answering this one.
- States the deadline and that **no answer means PIO denies it, recorded as PIO's decision**.
- Must hold: an answer aimed at the wrong run or a stale controller fails loudly and approves nothing.

### 5. The run view (`screens/5-run-view-*.png`)
- Header: back to board, run identity, state, tokens. A **truth line** that folds open into full evidence.
- Blocks: a `⏺` line for what happened, a `⎿` line for its result. The highlighted block is the cursor.
- Every tool use says **where it landed** (inside workspace, outside, not classifiable) and **who allowed it** (pre-approved by the user's rules, the user, PIO, the harness).
- `↑↓` blocks, `↵` open block, `y` copy, `d` diff, `[` `]` previous or next run, `c` cancel with a confirmation, `esc` board.

### 6. An uncertain run (`screens/6-uncertain-run-*.png`)
- Violet. Three plain sections: **what PIO knows, what PIO does not know, what PIO will not do**.
- A workspace check the user can verify themselves.
- One action: **open the workspace diff**. `e` shows the evidence.
- Usage unknown is never shown as zero.

### 7. Leaving and coming back (`screens/7-leaving-and-returning-*.png`)
- On `q`: how many runs keep working, how many approvals keep waiting, and when PIO will deny them if nobody answers.
- On reopen: **while you were away**, first thing. Anything PIO decided is marked as PIO's decision.

## One vocabulary

| Glyph | Word | Means | Color role |
|---|---|---|---|
| `●` | running | Working, with proof of delivery or waiting for it | green |
| `◐` | needs approval | A request is pending and has a deadline | amber, bold |
| `◇` | uncertain | Ambiguous delivery, lost host, or usage unknown | violet |
| `○` | finished | Exited, exit code shown | dim |
| `✕` | failed, refused, expired | Always followed by who decided: the user, PIO, or the harness | red |

**Meaning never rides on color alone.** Glyph and word are always present, so High Contrast and monochrome terminals work.

## Skins

A skin is a small token file. Tokens: `bg`, `card`, `fg`, `dim`, `head`, `run`, `wait`, `unknown`, `bad`, `accent`, `selection`, `edge`, `edge-strong`, `bar-bg`, `grid`, and three run identity colors `id1`, `id2`, `id3` (cycle for more runs).

| Token | PIO Dark | PIO Light | Catppuccin Mocha | Gruvbox Dark | High Contrast |
|---|---|---|---|---|---|
| bg | `#0B1115` | `#F7F9F9` | `#1E1E2E` | `#282828` | `#000000` |
| card | `#0F171C` | `#FFFFFF` | `#24243A` | `#2E2C2B` | `#0A0A0A` |
| fg | `#C9D6DB` | `#1B2730` | `#CDD6F4` | `#EBDBB2` | `#FFFFFF` |
| dim | `#6F8088` | `#7A8892` | `#7F849C` | `#928374` | `#B4B4B4` |
| head | `#8FA3AD` | `#4C5C66` | `#A6ADC8` | `#BDAE93` | `#E0E0E0` |
| run | `#6CCB8B` | `#1F7A45` | `#A6E3A1` | `#B8BB26` | `#3DFF9A` |
| wait | `#F0B354` | `#9A5B00` | `#F9E2AF` | `#FABD2F` | `#FFD83D` |
| unknown | `#B79CF0` | `#6B45B8` | `#CBA6F7` | `#D3869B` | `#E0B0FF` |
| bad | `#F07C6E` | `#B23A2E` | `#F38BA8` | `#FB4934` | `#FF7A7A` |
| accent | `#5CC8D6` | `#0C6B75` | `#74C7EC` | `#8EC07C` | `#3DE8FF` |
| selection | `#182A33` | `#E1EBEE` | `#34344C` | `#3C3836` | `#2B2B2B` |
| edge-strong | `#41545F` | `#8FA0A8` | `#585B70` | `#665C54` | `#9A9A9A` |
| bar-bg | `#111B21` | `#EEF3F4` | `#181825` | `#1D2021` | `#161616` |
| id1 / id2 / id3 | `#6FA8FF` `#F08FB4` `#7FD6C2` | `#2C5FC9` `#B8336A` `#0F7D6B` | `#89B4FA` `#F5C2E7` `#94E2D5` | `#83A598` `#FE8019` `#8EC07C` | `#7FB0FF` `#FF9AD0` `#7FFFE0` |

Also ship a **Terminal** skin that maps the tokens to the user's own 16 colors, so PIO matches whatever theme their terminal runs. Detect a light terminal automatically. `t` switches skins live.

The typeface is the terminal's, not PIO's. The mockups use Recursive Mono Casual. Do not depend on any font. Use only glyphs that render at one cell wide in common monospace fonts, and test the glyph set in Ghostty, a default macOS Terminal and a Linux terminal.

## Feel: Canvas and Plain

- **Plain**: cards with straight round-cornered borders drawn in characters, no grid.
- **Canvas, character tier** (works everywhere): the same cards on a dotted grid drawn in dim characters in the empty space, round corners, arrows made of characters.
- **Canvas, drawn tier** (optional, one-day experiment, **only if the owner says yes**): Ghostty, Kitty and WezTerm can place a picture **behind** the text. PIO would render the grid, the hand-drawn card outlines and the arrows as one image under the text cells. Time-box it to one day, in Ghostty. Report: does it redraw without flicker, does it survive resize and scroll, what happens inside tmux, what it costs in CPU. If it is not clearly good, the character tier ships alone and nothing else changes. It must never block M4.

## Behaviour rules

1. The screen reads and writes **only through the public API the command line uses**. No reading the store, no private shortcuts.
2. Every word of state comes from a recorded observation. If PIO does not know, the screen says **unknown** in violet. It never shows a guess.
3. Every refusal names its decider: the user, PIO, or the harness.
4. Quitting never stops work. Closing a card never stops a run. Cancel always asks for confirmation.
5. Mouse capture is a setting with three levels: wheel only, clicks, everything. Text selection with the terminal's own gesture must keep working.
6. The playful busy line rotates a small face and a whimsical verb. It is a setting, it can be off, and it never replaces a real state word.
7. Notifications: only when the terminal is not focused, by default. A setting with off, unfocused only, always.
8. Narrow terminals: at 80 columns the board drops workspace and delivery detail first. State, tokens and the attention notes never drop.
9. Keyboard-only operation, resize, and clean terminal restoration on exit are acceptance checks, as the plan already says.

## What M4 must prove

The terminal journey in `docs/JOURNEYS.md`: open the screen, run two real sessions in two workspaces, inspect distinct outputs, **deny then allow** a native permission request in the intended run, **detach during an approval wait**, reopen. Work survives closing the screen. The negative control: an answer for the wrong run or a stale controller fails without approving either.

A headless command cannot prove this. It needs a real terminal recording with the public API and journal records beside it.

## Suggested order

1. **M4a**: board, run view, approval walk, uncertain run, leaving and returning. This is the journey.
2. **M4a**: split view, skins, mouse levels, notifications, the playful busy line.
3. **Optional, one day**: the drawn tier experiment, only with the owner's yes.
4. **M4b**: orchestrate.

Two obligations from earlier milestones land before or with M4a: incremental changed-key commits, and the per-message usage reader for Claude Code.

## Orchestrate at size Medium: what it needs before any code

The owner chose Medium for v0.1. It is a scope change, so it needs these first, in this order.

1. **A Protocol proposal**, filed in the Protocol repository with demonstrating evidence, so that a message (a brief or a steering note) can record **which run wrote it**. Today only a person can be the author. No local widening.
2. **The owner's explicit approval of the lead's tool.** The leading harness needs a way to start runs and send messages through PIO's public API. For Claude Code that means giving one session a tool it would not otherwise have, which bends "exactly as the user configured it". Record it as a dated decision, the way the model exception was.
3. **Mid-turn messages for Claude Code.** Codex can take one. The Claude host cannot yet. Until it can, a message to a Claude run is delivered at its next turn and the thread says **queued**, never delivered.
4. **Rules that do not bend:** every relay carries delivery proof like any other delivery. A run can never answer another run's permission request; approvals always go to the user. The lead has a budget and everything it starts counts against it. A lead never edits files itself unless the user says so.
5. **A journey for it.** Propose a new journey for leading and relaying truthfully, with a negative control: a message whose delivery is unproven must show as uncertain in the thread, and a lead that tries to answer a permission request must be refused.

Write the M4b task packet only after items 1 and 2 are settled.
