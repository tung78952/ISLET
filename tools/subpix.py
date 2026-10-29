"""Reports the sub-pixel offset between two captures for each region, from ink centroids.

Usage: python -I subpix.py <reference.png> <islet.png> name:x0,y0,x1,y1 [...]
Positive dx/dy mean Islet must move right/down (pixels and layout units at scale 1.25).
"""
import sys

import numpy as np
from PIL import Image

ref = np.asarray(Image.open(sys.argv[1]).convert("L")).astype(float)
isl = np.asarray(Image.open(sys.argv[2]).convert("L")).astype(float)


def centroid(img, x0, y0, x1, y1):
    a = img[y0:y1, x0:x1]
    ys, xs = np.mgrid[y0:y1, x0:x1]
    s = a.sum() or 1.0
    return (a * xs).sum() / s, (a * ys).sum() / s


for spec in sys.argv[3:]:
    name, box = spec.split(":")
    b = [int(v) for v in box.split(",")]
    (rx, ry), (ix, iy) = centroid(ref, *b), centroid(isl, *b)
    print(f"{name:10s} dx={rx - ix:+.2f}px ({(rx - ix) / 1.25:+.2f}u) dy={ry - iy:+.2f}px ({(ry - iy) / 1.25:+.2f}u)")
