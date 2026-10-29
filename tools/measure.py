"""Measures the notch silhouette in captured frames.

Usage: python -I measure.py <background.png> <frame.png>...
The background is a frame of the same screen without the notch. Pixels that
differ from it form the notch; prints its size in pixels and layout units
(scale 1.25), the bottom corner radius and the top ear flare.
"""
import sys

import numpy as np
from PIL import Image

SCALE = 1.25


def load(p):
    return np.asarray(Image.open(p).convert("RGB")).astype(int)


def measure(bg, path):
    a = load(path)
    diff = np.abs(a - bg).max(axis=2) > 24
    dark = a.max(axis=2) < 16
    mask = diff & dark
    h, w = mask.shape
    rows = np.where(mask.any(axis=1))[0]
    if len(rows) == 0:
        print(f"{path}: no notch")
        return
    # Rows belonging to the notch: contiguous from the top.
    nh = 0
    while nh < h and mask[nh].sum() > 3:
        nh += 1
    mid = mask[nh // 2]
    xs = np.where(mid)[0]
    l, r = xs.min(), xs.max()
    top = np.where(mask[0])[0]
    bottom = np.where(mask[nh - 1])[0]
    radius = (bottom.min() - l) if len(bottom) else 0
    ear = (l - top.min()) if len(top) else 0
    print(
        f"{path.split(chr(92))[-2]}/{path.split(chr(92))[-1]}: "
        f"{r - l + 1}x{nh}px = {(r - l + 1) / SCALE:.1f}x{nh / SCALE:.1f}u "
        f"x={l}..{r} radius~{radius / SCALE:.1f}u ear~{ear / SCALE:.1f}u"
    )


bg = load(sys.argv[1])
for p in sys.argv[2:]:
    measure(bg, p)
