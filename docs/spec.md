# Islet visual and behavior spec

Measurements of the reference notch app, used as targets for Islet. Values come
from Ghidra pseudocode of the shipped binary (numbers only, no code reused) and
from screen captures. "u" is a layout unit; pixels = u × DPI scale (1.25 at 120 DPI).

## Window
- Class style: popup, ex-style TOPMOST | TOOLWINDOW | NOACTIVATE | NOREDIRECTIONBITMAP.
- Size 640 × 190 u, horizontally centered on the primary monitor, y = 0.
- Rendering: Direct2D on a DirectComposition target.
- Hit testing: `SetWindowRgn` with a rectangle `[x0, 0, x1, h]` rounded from the
  current animated notch rectangle, refreshed as the notch animates. Everything
  outside the rectangle is click-through.

## Typography (all "Segoe UI", normal style, stretch 5, locale en-us)
| Use (to confirm from captures) | Size u | Weight |
|---|---|---|
| body | 13 | 400 |
| emphasis | 15 | 600 |
| small | 11.5 | 400 |
| large | 17 | 400 |
| shelf labels | 10.5 | 600 |
| shelf secondary | 9.5 | 400 |
| lyrics/view title | 16 | 700 |
| lyrics/view secondary | 12.5 | 600 |

## Animation
- Each animated value is a damped spring `{x, v, k, c}` stepped with semi-implicit
  Euler: `v += (-(x - target)·k - c·v)·dt; x += v·dt`.
  It snaps to the target when `|v| < 0.05` and `|x - target| < 0.002`.
- Frame `dt` is clamped to 0.05 s and split into equal sub-steps.
- Notch width and height are separate springs. The initial width is 150 u.
- Spring constants by state transition (state byte 1–6):
  - default: k = 230, c = 19
  - from state 1 or 2 into state 4: k = 110, c = 15.5
  - into state 2 (with flag), or 1 → 2: k = 200, c = 29
  - secondary springs: k = 420, c = 41
- Visibility/progress springs (0 → 1) drive content fades and use the same model.
- List items slide with a 68 u stride; a press scale uses 0.92 (pressed) or
  1.08 (hover pop) as the spring target.

## Persistence
- `%APPDATA%\<app>\settings.txt`: `pet` (default 1), `minimal` (default 0).
- `water.txt` keys: bottle, goal, start, end, day, taken, inb, fired, vis, hours (24 values), on.

## States to measure from captures
Compact idle, hover, expanded per tab, toasts, lyrics. Fill in the sizes,
positions and colors below after Phase 2.
