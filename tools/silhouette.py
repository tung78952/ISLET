"""Measures the notch silhouette (width, height in layout units) in every frame of a capture.

Usage: python -I silhouette.py <frames_dir> [first] [last]
Reads log.csv for timestamps (ms;region). The notch spans the outermost near-black pixels of each row (the desktop around it is
not pure black); height is the first row without such a span, width is taken mid-height.
"""
import os
import sys

import numpy as np
from PIL import Image

d = sys.argv[1]
times = [float(l.split(";")[0]) for l in open(os.path.join(d, "log.csv"))]
files = sorted(f for f in os.listdir(d) if f[:4].isdigit() and f.endswith(".png"))
lo = int(sys.argv[2]) if len(sys.argv) > 2 else 0
hi = int(sys.argv[3]) if len(sys.argv) > 3 else 10**9
prev = None
for f in files:
    i = int(f[:4])
    if i < lo or i > hi:
        continue
    a = np.asarray(Image.open(os.path.join(d, f)).convert("RGB")).astype(int)
    dark = a.max(axis=2) <= 6
    spans = []
    for y in range(dark.shape[0]):
        xs = np.nonzero(dark[y])[0]
        spans.append((xs.min(), xs.max()) if len(xs) > 8 else None)
    h = 0
    while h < len(spans) and spans[h] is not None:
        h += 1
    if h:
        l, r = spans[min(h - 1, max(h // 2, 0))]
        w = r - l + 1
    else:
        w = 0
    t = times[i] if i < len(times) else float("nan")
    cur = (round(w / 1.25, 1), round(h / 1.25, 1))
    if cur != prev:
        print(f"{i:4d} t={t:8.1f}ms  w={cur[0]:6.1f}u h={cur[1]:6.1f}u")
        prev = cur
