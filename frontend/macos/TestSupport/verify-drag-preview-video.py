#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Verify actual OS drag feedback in XCTest's retained screen recording.

Requires NumPy and PyAV (15.1.0 was used for the current macOS receipt).
The source and independent fixture use a 1920x1080 desktop. Decoder pixels are
scaled to that desktop; Core and AppKit use CSS/point sizes. The source's static
green tile is at x=40 and excluded; two moving copies must be observed.
"""
import argparse
from collections import deque
import hashlib
import json
from pathlib import Path
import re
import struct
import sys
import zlib

parser = argparse.ArgumentParser()
parser.add_argument("video", type=Path)
parser.add_argument("image", type=Path)
parser.add_argument("--decoder-path", type=Path)
parser.add_argument("--native-log", type=Path, required=True)
args = parser.parse_args()
native_log = args.native_log.read_text()
starts = re.findall(r't =\s+([0-9.]+)s Click and drag from "Custom preview source"', native_log)
if len(starts) != 1:
    raise SystemExit("Expected exactly one actual custom-preview drag in the native log")
event_start = float(starts[0])
if args.decoder_path:
    sys.path.insert(0, str(args.decoder_path))
import av
import numpy as np


def png(rgb):
    height, width, _ = rgb.shape
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))
    rows = b"".join(b"\0" + row.tobytes() for row in rgb)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def preview_bounds(rgb):
    height, width, _ = rgb.shape
    sx, sy = width / 1920, height / 1080
    left, right = round(200 * sx), round(1420 * sx)
    top, bottom = round(150 * sy), round(394 * sy)
    region = rgb[top:bottom, left:right].astype(np.int16)
    red, green, blue = (region[:, :, i] for i in range(3))
    # Core's exact #00cc66 bytes are checked before NSImage creation. The
    # screen recording has its own HDR/display color space and OS opacity.
    mask = (green - red > 40) & (green - blue > 40)
    ys, xs = np.nonzero(mask)
    for y, x in zip(ys, xs):
        if not mask[y, x]:
            continue
        queue = deque([(int(x), int(y))])
        mask[y, x] = False
        min_x = max_x = int(x)
        min_y = max_y = int(y)
        count = 0
        while queue:
            px, py = queue.popleft()
            count += 1
            min_x, max_x = min(min_x, px), max(max_x, px)
            min_y, max_y = min(min_y, py), max(max_y, py)
            for nx, ny in [(px - 1, py), (px + 1, py), (px, py - 1), (px, py + 1)]:
                if 0 <= nx < mask.shape[1] and 0 <= ny < mask.shape[0] and mask[ny, nx]:
                    mask[ny, nx] = False
                    queue.append((nx, ny))
        observed_width, observed_height = (max_x - min_x + 1) / sx, (max_y - min_y + 1) / sy
        if abs(observed_width - 100) <= 3 and abs(observed_height - 48) <= 3 and count / (sx * sy) > 3000:
            return {"x": (left + min_x) / sx, "y": (top + min_y) / sy,
                    "width": observed_width, "height": observed_height, "green_pixels": count}
    return None


with av.open(str(args.video)) as media:
    media.streams.video[0].codec_context.thread_count = 2
    observations = []
    accepted = False
    for frame in media.decode(video=0):
        if frame.time is None or frame.time < max(0, event_start-0.5):
            continue
        if frame.time > event_start+10:
            break
        rgb = frame.to_ndarray(format="rgb24")
        bounds = preview_bounds(rgb)
        if bounds:
            if not observations:
                observations.append({"time_seconds": frame.time, "bounds": bounds})
                continue
            first = observations[0]
            if frame.time-first["time_seconds"] < 0.1 or abs(bounds["x"]-first["bounds"]["x"]) < 100:
                continue
            observations.append({"time_seconds": frame.time, "bounds": bounds})
            args.image.write_bytes(png(rgb))
            receipt = {"accepted": True, "decoder": av.__version__, "libraries": av.library_versions,
                       "video": str(args.video), "video_sha256": hashlib.sha256(args.video.read_bytes()).hexdigest(),
                       "time_seconds": frame.time, "pixel_width": frame.width, "pixel_height": frame.height,
                       "desktop_width": 1920, "desktop_height": 1080, "bounds_in_points": bounds,
                       "moving_preview_observations": observations,
                       "native_log": str(args.native_log), "native_log_sha256": hashlib.sha256(args.native_log.read_bytes()).hexdigest(),
                       "event_start_seconds": event_start,
                       "image": str(args.image), "image_sha256": hashlib.sha256(args.image.read_bytes()).hexdigest()}
            args.image.with_suffix(".json").write_text(json.dumps(receipt, indent=2) + "\n")
            print(json.dumps(receipt))
            accepted = True
            break
    if not accepted:
        raise SystemExit("No actual 100x48 green OS preview in the independent destination window")
