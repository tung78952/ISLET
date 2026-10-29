"""Stacks zoomed crops of the same region from two captures for close comparison.

Usage: python -I zoom.py <reference.png> <islet.png> <out.png> x0,y0,x1,y1 [zoom]
"""
import sys

from PIL import Image

ref, isl, out = Image.open(sys.argv[1]).convert("RGB"), Image.open(sys.argv[2]).convert("RGB"), sys.argv[3]
x0, y0, x1, y1 = (int(v) for v in sys.argv[4].split(","))
z = int(sys.argv[5]) if len(sys.argv) > 5 else 3
a = ref.crop((x0, y0, x1, y1)).resize(((x1 - x0) * z, (y1 - y0) * z), Image.NEAREST)
b = isl.crop((x0, y0, x1, y1)).resize(((x1 - x0) * z, (y1 - y0) * z), Image.NEAREST)
canvas = Image.new("RGB", (a.width, a.height * 2 + 4), (255, 0, 0))
canvas.paste(a, (0, 0))
canvas.paste(b, (0, a.height + 4))
canvas.save(out)
