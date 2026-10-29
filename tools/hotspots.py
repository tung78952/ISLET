"""Lists the blocks with the most mismatching pixels between two captures.

Usage: python -I hotspots.py <reference.png> <islet.png> x0,y0,x1,y1 [block]
Coordinates in pixels; block reports use layout units relative to x0 (scale 1.25).
"""
import sys

import numpy as np
from PIL import Image

ref = np.asarray(Image.open(sys.argv[1]).convert("RGB")).astype(int)
isl = np.asarray(Image.open(sys.argv[2]).convert("RGB")).astype(int)
x0, y0, x1, y1 = (int(v) for v in sys.argv[3].split(","))
b = int(sys.argv[4]) if len(sys.argv) > 4 else 20
bad = np.abs(ref - isl).max(axis=2) > 24
rows = []
for y in range(y0, y1, b):
    for x in range(x0, x1, b):
        n = bad[y:min(y + b, y1), x:min(x + b, x1)].sum()
        if n:
            rows.append((n, (x - x0) / 1.25, y / 1.25))
rows.sort(reverse=True)
total = bad[y0:y1, x0:x1].sum()
for n, ux, uy in rows[:12]:
    print(f"{n:5d} px ({n / total * 100:4.1f}%) at u x={ux:5.1f} y={uy:5.1f}")
