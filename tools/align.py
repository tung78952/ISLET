"""Finds the pixel shift that best aligns a region of an Islet capture to the reference.

Usage: python -I align.py <reference.png> <islet.png> name:x0,y0,x1,y1 [...]
Prints, per region, the shift (dx, dy) in pixels and layout units (scale 1.25)
to apply to Islet's drawing so it lands on the reference.
"""
import sys

import numpy as np
from PIL import Image

ref = np.asarray(Image.open(sys.argv[1]).convert("L")).astype(float)
isl = np.asarray(Image.open(sys.argv[2]).convert("L")).astype(float)
R = 6
for spec in sys.argv[3:]:
    name, box = spec.split(":")
    x0, y0, x1, y1 = (int(v) for v in box.split(","))
    target = ref[y0:y1, x0:x1]
    best = None
    for dy in range(-R, R + 1):
        for dx in range(-R, R + 1):
            ys, xs = y0 - dy, x0 - dx
            if ys < 0 or xs < 0:
                continue
            cand = isl[ys:ys + (y1 - y0), xs:xs + (x1 - x0)]
            if cand.shape != target.shape:
                continue
            err = np.abs(cand - target).mean()
            if best is None or err < best[0]:
                best = (err, dx, dy)
    base = np.abs(isl[y0:y1, x0:x1] - target).mean()
    err, dx, dy = best
    print(f"{name:12s} shift dx={dx:+d}px ({dx / 1.25:+.1f}u) dy={dy:+d}px ({dy / 1.25:+.1f}u)  err {base:.1f} -> {err:.1f}")
