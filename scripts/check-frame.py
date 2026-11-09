#!/usr/bin/env python3
"""Fail when a screenshot is a blank screen.

The app renders a dark scene (0x0E131A) with a lit floor grid, so "dark"
alone is not a signal. A blank frame shows as (nearly) every pixel at
luminance 0 with only one or two distinct colours - SwiftUI's material
background plus the loading text would still produce several colours and
text pixels.

With two files the frames are also compared: a renderer that never came up
draws the same image every time, and a frozen scene never changes either.

Usage:
    check-frame.py <screenshot.png> [--min-lit 0.0005]
    check-frame.py <a.png> <b.png> [--min-diff 0.0002]
Exit code 0 = the frame has content, 1 = blank (or frozen, in compare mode).
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


def load(path):
    width, height, channels, raw = read_png(path)
    pixels = unfilter(raw, width, height, channels)
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
    total = width * height
    return {
        "path": path,
        "width": width,
        "height": height,
        "channels": channels,
        "pixels": pixels,
        "lit": lit / total if total else 0,
        "colours": colours,
    }


def region_stats(frame, region):
    """Lit ratio and colour count inside a fractional (x0, y0, x1, y1) box."""
    x0, y0, x1, y1 = region
    width = frame["width"]
    height = frame["height"]
    channels = frame["channels"]
    pixels = frame["pixels"]
    left, right = int(width * x0), int(width * x1)
    top, bottom = int(height * y0), int(height * y1)
    lit = 0
    total = 0
    colours = set()
    for y in range(top, bottom):
        row = y * width
        for x in range(left, right):
            index = (row + x) * channels
            r = pixels[index]
            g = pixels[index + 1] if channels >= 3 else r
            b = pixels[index + 2] if channels >= 3 else r
            if 0.2126 * r + 0.7152 * g + 0.0722 * b > 24:
                lit += 1
            total += 1
            if len(colours) < 64:
                colours.add((r >> 3, g >> 3, b >> 3))
    return (lit / total if total else 0.0), len(colours)


def count_near(frame, target, tolerance, region=None):
    """Pixels within `tolerance` of `target` RGB, optionally inside a box."""
    width = frame["width"]
    height = frame["height"]
    channels = frame["channels"]
    pixels = frame["pixels"]
    if region:
        left, right = int(width * region[0]), int(width * region[2])
        top, bottom = int(height * region[1]), int(height * region[3])
    else:
        left, right, top, bottom = 0, width, 0, height
    tr, tg, tb = target
    found = 0
    for y in range(top, bottom):
        row = y * width
        for x in range(left, right):
            index = (row + x) * channels
            r = pixels[index]
            g = pixels[index + 1] if channels >= 3 else r
            b = pixels[index + 2] if channels >= 3 else r
            if abs(r - tr) <= tolerance and abs(g - tg) <= tolerance and abs(b - tb) <= tolerance:
                found += 1
    return found


def check_single(frame, min_lit):
    print(
        f"{frame['path']}: {frame['width']}x{frame['height']} "
        f"lit={frame['lit']:.4%} colours={len(frame['colours'])}"
    )
    if frame["lit"] < min_lit:
        print(
            f"::error::frame is blank (only {frame['lit']:.4%} lit pixels, "
            f"{len(frame['colours'])} colours) - the app is showing a black screen"
        )
        return False
    if len(frame["colours"]) < 3:
        print(
            f"::error::frame has only {len(frame['colours'])} distinct colours - "
            "the app is showing an empty scene"
        )
        return False
    print("frame has content")
    return True


def compare(first, second, min_diff):
    """Fraction of pixels that differ between two frames."""
    if (first["width"], first["height"]) != (second["width"], second["height"]):
        print(
            f"::error::frame sizes differ: {first['width']}x{first['height']} "
            f"vs {second['width']}x{second['height']}"
        )
        return False
    channels = first["channels"]
    a = first["pixels"]
    b = second["pixels"]
    total = first["width"] * first["height"]
    differing = 0
    for index in range(0, len(a), channels):
        # Ignore tiny differences: the simulator can dither.
        if (
            abs(a[index] - b[index]) > 8
            or abs(a[index + 1] - b[index + 1]) > 8
            or abs(a[index + 2] - b[index + 2]) > 8
        ):
            differing += 1
    ratio = differing / total if total else 0.0
    print(
        f"{first['path']} vs {second['path']}: "
        f"{differing}/{total} pixels differ ({ratio:.4%})"
    )
    if ratio < min_diff:
        print(
            f"::error::the two frames are the same ({ratio:.4%} differ, "
            f"want {min_diff:.4%}) - the renderer is frozen or never started"
        )
        return False
    print("the frames differ - the scene is live")
    return True


def main(argv):
    if len(argv) < 2:
        print(
            "usage: check-frame.py <screenshot.png> [<other.png>] "
            "[--min-lit N] [--min-diff N] [--region x0,y0,x1,y1] [--region-lit N]"
        )
        return 2
    min_lit = 0.0005
    min_diff = 0.0002
    region = None
    region_lit = 0.02
    expectations = []
    flags = []
    rest = list(argv[1:])
    while rest:
        arg = rest.pop(0)
        if arg == "--min-lit":
            min_lit = float(rest.pop(0))
        elif arg == "--min-diff":
            min_diff = float(rest.pop(0))
        elif arg == "--region":
            region = tuple(float(part) for part in rest.pop(0).split(","))
            if len(region) != 4:
                print("::error::--region wants x0,y0,x1,y1 as fractions")
                return 2
        elif arg == "--region-lit":
            region_lit = float(rest.pop(0))
        elif arg == "--expect-colour":
            # r,g,b,tolerance,min_pixels - the duck's own palette: light
            # shells and orange feet. A mis-assembled model renders grey
            # internals and loses both.
            parts = [int(value) for value in rest.pop(0).split(",")]
            if len(parts) != 5:
                print("::error::--expect-colour wants r,g,b,tolerance,min_pixels")
                return 2
            expectations.append(parts)
        else:
            flags.append(arg)
    paths = [arg for arg in flags if not arg.startswith("--")]

    frames = [load(path) for path in paths]
    for frame in frames:
        if not check_single(frame, min_lit):
            return 1
    if region:
        # The whole-frame check above passes on an app whose SwiftUI chrome
        # draws over a black 3D viewport: the buttons light up enough
        # pixels. Check the scene region instead - a black Metal view puts
        # 0% lit pixels there.
        for frame in frames:
            ratio, colours = region_stats(frame, region)
            print(
                f"{frame['path']}: region {region} "
                f"lit={ratio:.4%} colours={colours}"
            )
            if ratio < region_lit:
                print(
                    f"::error::the 3D viewport is black (only {ratio:.4%} lit "
                    f"pixels inside {region}, want {region_lit:.4%}) - the "
                    "renderer is not drawing the scene"
                )
                return 1
    for target in expectations:
        r, g, b, tolerance, want = target
        for frame in frames:
            got = count_near(frame, (r, g, b), tolerance, region)
            print(
                f"{frame['path']}: colour ({r},{g},{b}) +/-{tolerance} "
                f"-> {got} px (want >= {want})"
            )
            if got < want:
                print(
                    f"::error::only {got} pixels match the duck's colour "
                    f"({r},{g},{b}) - the model is not rendering its materials"
                )
                return 1
    if len(frames) >= 2:
        for first, second in zip(frames, frames[1:]):
            if not compare(first, second, min_diff):
                return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
