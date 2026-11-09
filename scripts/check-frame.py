#!/usr/bin/env python3
"""Fail when a screenshot is a blank screen.

The app renders a dark scene (0x0E131A) with a lit floor grid, so "dark"
alone is not a signal. A blank frame shows as (nearly) every pixel at
luminance 0 with only one or two distinct colours — SwiftUI's material
background plus the loading text would still produce several colours and
text pixels.

Usage: check-frame.py <screenshot.png> [--min-lit 0.0005]
Exit code 0 = the frame has content, 1 = blank.
"""
import sys
import zlib


def read_png(path):
    with open(path, "rb") as handle:
        data = handle.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    pos = 8
    width = height = depth = color = None
    idat = bytearray()
    while pos < len(data):
        length = int.from_bytes(data[pos:pos + 4], "big")
        kind = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width = int.from_bytes(body[0:4], "big")
            height = int.from_bytes(body[4:8], "big")
            depth = body[8]
            color = body[9]
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    if width is None:
        raise ValueError("no IHDR")
    if depth != 8:
        raise ValueError(f"unsupported bit depth {depth}")
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}.get(color)
    if channels is None:
        raise ValueError(f"unsupported colour type {color}")
    return width, height, channels, zlib.decompress(bytes(idat))


def unfilter(raw, width, height, channels):
    """Undo the PNG per-scanline filters, yielding rows of bytes."""
    stride = width * channels
    out = bytearray()
    previous = bytearray(stride)
    pos = 0
    for _ in range(height):
        filter_type = raw[pos]
        pos += 1
        line = bytearray(raw[pos:pos + stride])
        pos += stride
        if filter_type == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif filter_type == 2:
            for i in range(stride):
                line[i] = (line[i] + previous[i]) & 0xFF
        elif filter_type == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + previous[i]) >> 1)) & 0xFF
        elif filter_type == 4:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                up = previous[i]
                upper_left = previous[i - channels] if i >= channels else 0
                estimate = left + up - upper_left
                da, db, dc = abs(estimate - left), abs(estimate - up), abs(estimate - upper_left)
                if da <= db and da <= dc:
                    predictor = left
                elif db <= dc:
                    predictor = up
                else:
                    predictor = upper_left
                line[i] = (line[i] + predictor) & 0xFF
        elif filter_type != 0:
            raise ValueError(f"unknown filter {filter_type}")
        out += line
        previous = line
    return out


def main(argv):
    if len(argv) < 2:
        print("usage: check-frame.py <screenshot.png>")
        return 2
    path = argv[1]
    min_lit = 0.0005
    if "--min-lit" in argv:
        min_lit = float(argv[argv.index("--min-lit") + 1])
    width, height, channels, raw = read_png(path)
    pixels = unfilter(raw, width, height, channels)
    total = width * height
    lit = 0
    colours = set()
    for index in range(0, len(pixels), channels):
        r = pixels[index]
        g = pixels[index + 1] if channels >= 3 else r
        b = pixels[index + 2] if channels >= 3 else r
        if 0.2126 * r + 0.7152 * g + 0.0722 * b > 24:
            lit += 1
        if len(colours) < 64:
            colours.add((r >> 3, g >> 3, b >> 3))
    ratio = lit / total if total else 0
    print(
        f"{path}: {width}x{height} lit={ratio:.4%} colours={len(colours)}"
    )
    if ratio < min_lit:
        print(
            f"::error::frame is blank (only {ratio:.4%} lit pixels, "
            f"{len(colours)} colours) — the app is showing a black screen"
        )
        return 1
    if len(colours) < 3:
        print(
            f"::error::frame has only {len(colours)} distinct colours — "
            "the app is showing an empty scene"
        )
        return 1
    print("frame has content")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
