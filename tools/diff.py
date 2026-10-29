"""Compares an Islet capture with a reference capture inside the notch.

Usage: python -I diff.py <background.png> <reference.png> <islet.png> <out_prefix>
The notch mask comes from the reference frame (pixels differing from the
background, plus their dark fill). Prints the share of mask pixels whose
channels differ by at most TOL, and writes a side-by-side image with a heatmap.
"""
import sys

import numpy as np
from PIL import Image

TOL = 24


def load(p):
    return np.asarray(Image.open(p).convert("RGB")).astype(int)


def main():
    bg, ref, isl, out = load(sys.argv[1]), load(sys.argv[2]), load(sys.argv[3]), sys.argv[4]
    h = min(ref.shape[0], isl.shape[0])
    bg, ref, isl = bg[:h], ref[:h], isl[:h]
    if len(sys.argv) > 5:
        # Explicit notch box in pixels: x0,y0,x1,y1,radius.
        x0, y0, x1, y1, r = (int(v) for v in sys.argv[5].split(","))
        yy, xx = np.mgrid[0:h, 0:ref.shape[1]]
        mask = (xx >= x0) & (xx < x1) & (yy >= y0) & (yy < y1)
        for cx in (x0 + r, x1 - r):
            cy = y1 - r
            corner = ((xx < x0 + r) if cx == x0 + r else (xx >= x1 - r)) & (yy >= cy)
            mask &= ~(corner & ((xx - cx) ** 2 + (yy - cy) ** 2 > r * r))
    else:
        changed = np.abs(ref - bg).max(axis=2) > 24
        # Fill the silhouette row by row between its outermost changed pixels.
        mask = np.zeros_like(changed)
        for y in range(h):
            xs = np.nonzero(changed[y])[0]
            if len(xs) > 8:
                mask[y, xs.min():xs.max() + 1] = True
    diff = np.abs(ref - isl).max(axis=2)
    ok = (diff <= TOL) & mask
    total = mask.sum()
    score = ok.sum() / total * 100 if total else 0.0
    heat = np.zeros_like(ref)
    heat[..., 0] = np.where(mask, np.clip(diff * 3, 0, 255), 0)
    heat[..., 1] = np.where(mask & (diff <= TOL), 60, 0)
    side = np.concatenate([ref, isl, heat], axis=0).astype(np.uint8)
    Image.fromarray(side).save(out + "_cmp.png")
    print(f"match {score:.2f}% of {total} notch pixels (tolerance {TOL})")


main()
