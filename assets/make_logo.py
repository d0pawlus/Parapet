"""Generates the Parapet pixel-art logo: a wooden window with a sill, each pane showing different weather.

Usage: python assets/make_logo.py   (writes assets/logo-*.png)
"""
from pathlib import Path
from PIL import Image

N = 32
C = {
    "out": (59, 36, 24), "wood": (176, 112, 64), "woodL": (214, 152, 96), "woodD": (128, 74, 40),
    "sun_sky": (143, 211, 244), "sun_skyL": (182, 228, 250), "sun": (255, 212, 71), "sunL": (255, 236, 150),
    "rain_sky": (104, 136, 166), "cloud": (196, 208, 222), "cloudD": (150, 166, 186), "drop": (214, 236, 255),
    "snow_sky": (47, 63, 110), "flake": (255, 255, 255), "flakeD": (170, 190, 230),
    "storm_sky": (64, 58, 92), "scloud": (128, 122, 152), "scloudD": (96, 90, 122), "bolt": (255, 230, 107),
    "orange": (240, 138, 36), "red": (217, 72, 43), "yellow": (242, 194, 48), "stem": (110, 60, 30),
    "snow": (255, 255, 255), "snowS": (200, 220, 240), "glint": (255, 255, 255),
}
img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
px = img.load()


def put(x, y, c):
    if 0 <= x < N and 0 <= y < N:
        px[x, y] = C[c] + (255,) if isinstance(c, str) else c


def rect(x0, y0, x1, y1, c):
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            put(x, y, c)


def pts(c, *xy):
    for x, y in xy:
        put(x, y, c)


# --- frame: outline, wood, bevel -------------------------------------------------
FX0, FY0, FX1, FY1 = 4, 2, 27, 25
rect(FX0, FY0, FX1, FY1, "out")
rect(FX0 + 1, FY0 + 1, FX1 - 1, FY1 - 1, "wood")
rect(FX0 + 1, FY0 + 1, FX1 - 1, FY0 + 1, "woodL")      # top highlight
rect(FX0 + 1, FY0 + 1, FX0 + 1, FY1 - 1, "woodL")      # left highlight
rect(FX1 - 1, FY0 + 2, FX1 - 1, FY1 - 1, "woodD")      # right shadow

# panes (inner glass areas), separated by a 2px cross
PANES = {
    "tl": (7, 5, 14, 13), "tr": (17, 5, 24, 13),
    "bl": (7, 16, 14, 23), "br": (17, 16, 24, 23),
}
for (x0, y0, x1, y1) in PANES.values():
    rect(x0 - 1, y0 - 1, x1 + 1, y1 + 1, "out")         # dark inset around glass

# top-left: sunny autumn with falling leaves
x0, y0, x1, y1 = PANES["tl"]
rect(x0, y0, x1, y1, "sun_sky")
rect(x0, y0, x1, y0 + 1, "sun_skyL")
rect(x0, y0, x0 + 2, y0 + 2, "sun"); pts("sun", (x0 + 3, y0), (x0, y0 + 3)); put(x0, y0, "sunL")
pts("orange", (x0 + 5, y0 + 3), (x0 + 6, y0 + 3), (x0 + 5, y0 + 4)); put(x0 + 4, y0 + 2, "stem")
pts("red", (x0 + 2, y0 + 6), (x0 + 3, y0 + 6), (x0 + 3, y0 + 7))
pts("yellow", (x0 + 6, y0 + 7))

# top-right: rain
x0, y0, x1, y1 = PANES["tr"]
rect(x0, y0, x1, y1, "rain_sky")
rect(x0 + 1, y0 + 1, x1 - 1, y0 + 2, "cloud"); rect(x0 + 2, y0, x1 - 2, y0, "cloud")
rect(x0 + 1, y0 + 2, x1 - 1, y0 + 2, "cloudD")
for (dx, dy) in [(1, 4), (4, 5), (7, 4), (2, 7), (5, 7), (7, 8)]:
    pts("drop", (x0 + dx, y0 + dy), (x0 + dx, y0 + dy + 1))

# bottom-left: snow
x0, y0, x1, y1 = PANES["bl"]
rect(x0, y0, x1, y1, "snow_sky")
for (dx, dy) in [(1, 1), (5, 2), (3, 4), (7, 5), (1, 6)]:
    put(x0 + dx, y0 + dy, "flake")
cx, cy = x0 + 5, y0 + 6                                      # one bigger flake
pts("flake", (cx, cy), (cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1))
rect(x0, y1, x1, y1, "flake"); pts("flake", (x0 + 1, y1 - 1), (x0 + 2, y1 - 1), (x1 - 1, y1 - 1))

# bottom-right: storm
x0, y0, x1, y1 = PANES["br"]
rect(x0, y0, x1, y1, "storm_sky")
rect(x0, y0, x1, y0 + 1, "scloud"); rect(x0 + 1, y0 + 2, x1 - 1, y0 + 2, "scloudD")
pts("bolt", (x0 + 4, y0 + 3), (x0 + 3, y0 + 4), (x0 + 3, y0 + 5), (x0 + 4, y0 + 5),
    (x0 + 5, y0 + 5), (x0 + 4, y0 + 6), (x0 + 3, y0 + 7))
pts("drop", (x0 + 1, y0 + 4), (x0 + 1, y0 + 5), (x0 + 7, y0 + 5), (x0 + 7, y0 + 6))

# --- sill --------------------------------------------------------------------------
SX0, SY0, SX1, SY1 = 1, 25, 30, 29
rect(SX0, SY0, SX1, SY1, "out")
rect(SX0 + 1, SY0 + 1, SX1 - 1, SY1 - 1, "wood")
rect(SX0 + 1, SY0 + 1, SX1 - 1, SY0 + 1, "woodL")
rect(SX0 + 1, SY1 - 1, SX1 - 1, SY1 - 1, "woodD")
rect(SX0 + 2, SY1 + 1, SX1 - 2, SY1 + 1, (40, 24, 16, 90))   # soft shadow under the sill

# snow pile resting on the left of the sill (sits on the sill's top outline at y=25)
rows = {24: (2, 8), 23: (3, 7), 22: (4, 6)}
for y, (a, b) in rows.items():
    rect(a - 1, y - 1, b + 1, y, "out")
for y, (a, b) in rows.items():
    rect(a, y, b, y, "snow")
pts("snowS", (7, 24), (8, 24), (7, 23))

# leaves resting on the right of the sill
pts("orange", (21, 24), (22, 24), (22, 23)); put(23, 23, "stem")
pts("red", (25, 24), (26, 24), (26, 23))
pts("yellow", (28, 24))

out = Path(__file__).parent
img.save(out / "logo-32.png")
for s in (64, 128, 256, 512, 1024):
    img.resize((s, s), Image.NEAREST).save(out / f"logo-{s}.png")

# --- hand-drawn 16x16 variant for the small tray icon ---------------------------------
KEY = {
    "o": "out", "l": "woodL", "w": "wood", "d": "woodD", "y": "sun", "s": "sun_sky", "g": "orange",
    "c": "cloud", "r": "rain_sky", "b": "drop", "n": "snow_sky", "f": "flake", "W": "snow",
    "t": "storm_sky", "z": "bolt",
}
TL = ["yyss", "yssg", "ssss", "sgss"]
TR = ["cccc", "rbrr", "rrrb", "rbrr"]
BL = ["nfnn", "nnnf", "fnnn", "WWWW"]
BR = ["tttt", "ttzt", "tzzt", "tztt"]
rows16 = [".oooooooooooooo.", ".o" + "l" * 11 + "do."]
rows16 += [".ol" + TL[i] + "ww" + TR[i] + "do." for i in range(4)]
rows16 += [".ol" + "w" * 10 + "do."] * 2
rows16 += [".ol" + BL[i] + "ww" + BR[i] + "do." for i in range(4)]
rows16 += ["o" * 16, "o" + "l" * 14 + "o", "o" + "d" * 14 + "o", "o" * 16]
small = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
for y, row in enumerate(rows16):
    assert len(row) == 16, (y, row)
    for x, ch in enumerate(row):
        if ch != ".":
            small.putpixel((x, y), C[KEY[ch]] + (255,))
small.save(out / "icon-16.png")

# --- Windows icon: pixel-exact frames where the scale is whole, smooth ones in between --
frames = {16: small, 32: img}
for s in (64, 128, 256):
    frames[s] = img.resize((s, s), Image.NEAREST)
big = img.resize((256, 256), Image.NEAREST)
for s in (20, 24):                       # tray at 125%/150% DPI: keep the 16 px drawing crisp, centred
    frame = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    frame.alpha_composite(small, ((s - 16) // 2, (s - 16) // 2))
    frames[s] = frame
for s in (40, 48):
    frames[s] = big.resize((s, s), Image.LANCZOS)
sizes = sorted(frames)
frames[256].save(out / "parapet.ico", format="ICO", sizes=[(s, s) for s in sizes],
                 append_images=[frames[s] for s in sizes if s != 256])
print("ok")
