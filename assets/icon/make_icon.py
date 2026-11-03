#!/usr/bin/env python3
"""Draws the app icon (original pixel art, no game data) and writes every format the builds need into this folder:

  icon.png          512x512, for the README
  icon.ico          16/32/48/256, embedded into giana-app.exe (crates/giana-app/build.rs)
  icon.icns         macOS app bundle (.github/scripts/package.sh); needs `iconutil`, so only written on macOS
  window-icon.rgba  64x64 raw RGBA, the window icon (crates/giana-app/src/main.rs)

Run it again after changing the drawing: python3 assets/icon/make_icon.py
"""
import os, shutil, struct, subprocess, sys, tempfile, zlib

HERE = os.path.dirname(os.path.abspath(__file__))

# C64 palette (Pepto)
BLACK = (0, 0, 0); WHITE = (255, 255, 255); BLUE = (53, 40, 121); LBLUE = (108, 94, 181)
CYAN = (112, 164, 178); PURPLE = (111, 61, 134); GREY3 = (149, 149, 149); YELLOW = (184, 199, 111)
DARK = (20, 14, 52)

N = 32
px = [[None] * N for _ in range(N)]
ORANGE = (111, 79, 37); BROWN = (67, 57, 0); LRED = (154, 103, 89)

def inside(x, y, r=6):
    cx = min(max(x, r), N - 1 - r); cy = min(max(y, r), N - 1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r + r

# Night sky, a little lighter towards the bottom (dithered band)
for y in range(N):
    for x in range(N):
        if inside(x, y):
            px[y][x] = BLUE

# A row of bricks to stand on
for y in range(25, N):
    for x in range(N):
        if not inside(x, y):
            continue
        row = (y - 25) // 3
        mortar = (y - 25) % 3 == 2 or (x + (4 if row % 2 else 0)) % 8 == 7
        px[y][x] = BROWN if mortar else (LRED if (y - 25) % 3 == 0 else ORANGE)

# Gem: a crown of 4 rows, a pavilion of 10 rows down to a point; facets split by slanted lines
top, left, w = 7, 6, 20
rows = [(4, 16), (2, 18), (1, 19), (0, 20)] + [(i, w - i) for i in range(0, 10)]
for i, (a, b) in enumerate(rows):
    y = top + i
    for x in range(left + a, left + b):
        r = x - left
        if i < 4:
            c = WHITE if r < 7 - i else (CYAN if r < 13 + i // 2 else LBLUE)
        else:
            k = i - 4
            c = CYAN if r < 7 + k // 2 else (LBLUE if r < 13 - k // 2 + 1 else PURPLE)
        px[y][x] = c
    px[y][left + a] = DARK
    px[y][left + b - 1] = DARK
for x in range(left + 4, left + 16):
    px[top - 1][x] = DARK
px[top + len(rows)][left + 9] = DARK; px[top + len(rows)][left + 10] = DARK
for x in range(left + 1, left + 19):      # girdle: the line between crown and pavilion
    px[top + 4][x] = LBLUE if px[top + 4][x] in (CYAN, WHITE) else px[top + 4][x]
px[top + 1][left + 6] = WHITE; px[top + 2][left + 4] = WHITE  # highlight

# Sparkles
for (sx, sy) in [(26, 6), (4, 19)]:
    for dx, dy in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)]:
        px[sy + dy][sx + dx] = WHITE if (dx, dy) == (0, 0) else YELLOW
for (sx, sy) in [(28, 15), (3, 9), (25, 21)]:
    px[sy][sx] = GREY3

def rgba(size):
    # nearest neighbour: whole pixels from 32 up, every other pixel at 16
    rows = []
    for y in range(size):
        for x in range(size):
            c = px[y * N // size][x * N // size]
            rows.append(bytes(c + (255,)) if c else b"\0\0\0\0")
    return b"".join(rows)


def png(size):
    data = rgba(size)
    raw = b"".join(b"\0" + data[y * size * 4:(y + 1) * size * 4] for y in range(size))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def ico(sizes):
    # ICO with PNG images inside (Windows Vista and later)
    images = [png(s) for s in sizes]
    out = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    for s, im in zip(sizes, images):
        out += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(im), offset)
        offset += len(im)
    return out + b"".join(images)


def write(name, data):
    with open(os.path.join(HERE, name), "wb") as f:
        f.write(data)


write("icon.png", png(512))
write("icon.ico", ico([16, 32, 48, 256]))
write("window-icon.rgba", rgba(64))
if shutil.which("iconutil"):
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, "icon.iconset")
        os.mkdir(iconset)
        for s in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                suffix = "@2x" if scale == 2 else ""
                with open(os.path.join(iconset, f"icon_{s}x{s}{suffix}.png"), "wb") as f:
                    f.write(png(s * scale))
        subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(HERE, "icon.icns")], check=True)
else:
    print("iconutil not found (macOS only): icon.icns left unchanged", file=sys.stderr)
