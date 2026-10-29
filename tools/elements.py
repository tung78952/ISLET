"""Lists bright UI elements inside the notch of a captured frame.

Usage: python -I elements.py <frame.png> [left_px] [top_px]
Groups non-black pixels into boxes (merging nearby pixels) and prints each box
in panel-local layout units (scale 1.25, origin at the panel's left edge) with
its dominant color. The default origin is the expanded panel (x = 137.5 px).
"""
import sys

from collections import deque

import numpy as np
from PIL import Image

SCALE = 1.25


def dilate(m, it):
    for _ in range(it):
        g = m.copy()
        g[1:] |= m[:-1]
        g[:-1] |= m[1:]
        g[:, 1:] |= m[:, :-1]
        g[:, :-1] |= m[:, 1:]
        m = g
    return m


def label(m):
    h, w = m.shape
    lab = np.zeros((h, w), dtype=np.int32)
    n = 0
    for y, x in zip(*np.nonzero(m)):
        if lab[y, x]:
            continue
        n += 1
        lab[y, x] = n
        q = deque([(y, x)])
        while q:
            cy, cx = q.popleft()
            for ny, nx in ((cy + 1, cx), (cy - 1, cx), (cy, cx + 1), (cy, cx - 1)):
                if 0 <= ny < h and 0 <= nx < w and m[ny, nx] and not lab[ny, nx]:
                    lab[ny, nx] = n
                    q.append((ny, nx))
    boxes = []
    for i in range(1, n + 1):
        ys, xs = np.nonzero(lab == i)
        boxes.append((slice(ys.min(), ys.max() + 1), slice(xs.min(), xs.max() + 1)))
    return lab, n, boxes


def main():
    path = sys.argv[1]
    ox = float(sys.argv[2]) if len(sys.argv) > 2 else 137.5
    oy = float(sys.argv[3]) if len(sys.argv) > 3 else 0.0
    a = np.asarray(Image.open(path).convert("RGB")).astype(int)
    h, w, _ = a.shape
    x0, x1 = int(ox), int(w - ox)
    region = a[0:min(h, 178), x0:x1]
    lum = region.max(axis=2)
    on = lum > 40
    grown = dilate(on, 3)
    labels, n, boxes = label(grown)
    out = []
    for i, sl in enumerate(boxes):
        ys, xs = sl
        sub = on[sl] & (labels[sl] == i + 1)
        if sub.sum() < 6:
            continue
        px = region[sl][sub]
        bright = px[px.max(axis=1) >= np.percentile(px.max(axis=1), 70)]
        col = np.median(bright, axis=0).astype(int)
        sy, sx = np.nonzero(sub)
        bx, by = xs.start + sx.min() + x0 - ox, ys.start + sy.min() - oy
        bw, bh = sx.max() - sx.min() + 1, sy.max() - sy.min() + 1
        out.append((by, bx, bw, bh, col, sub.sum()))
    out.sort(key=lambda t: (round(t[0] / 6), t[1]))
    for by, bx, bw, bh, col, n in out:
        print(
            f"x={bx / SCALE:6.1f} y={by / SCALE:6.1f} w={bw / SCALE:6.1f} h={bh / SCALE:5.1f}u "
            f"color=#{col[0]:02X}{col[1]:02X}{col[2]:02X} px={n}"
        )


main()
