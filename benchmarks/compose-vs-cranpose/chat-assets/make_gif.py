#!/usr/bin/env python3
"""Writes `spin.gif`, the animated picture of the patchwork's chat: a wheel
of the palette's eight colors, 64 by 64 pixels, that turns by one color a
frame through 16 frames 40 ms apart, forever. Run it again to change the
picture; every app loads the file it wrote."""
import math
import struct
from pathlib import Path

SIZE = 64
FRAMES = 16
DELAY_CS = 4
PALETTE = [0xEF4444, 0xF97316, 0xEAB308, 0x22C55E, 0x14B8A6, 0x3B82F6, 0x8B5CF6, 0xEC4899]
BACKGROUND = 0x0F1220


def pixels(frame):
    """The palette index of every pixel of `frame`: index 8 is the background."""
    middle = (SIZE - 1) / 2
    out = []
    for y in range(SIZE):
        for x in range(SIZE):
            dx, dy = x - middle, y - middle
            radius = math.hypot(dx, dy)
            if radius > SIZE / 2 - 1 or radius < SIZE / 8:
                out.append(8)
            else:
                turn = (math.atan2(dy, dx) / math.tau) % 1.0
                out.append((int(turn * 16) + frame) // 2 % 8)
    return out


def lzw(indices):
    """`indices` as GIF image data at 9 bits a code: a clear code every 250
    pixels keeps the decoder's table from growing past 9-bit codes."""
    clear, end = 256, 257
    bits, count, data = 0, 0, bytearray()
    codes = []
    for start in range(0, len(indices), 250):
        codes.append(clear)
        codes.extend(indices[start:start + 250])
    codes.append(end)
    for code in codes:
        bits |= code << count
        count += 9
        while count >= 8:
            data.append(bits & 0xFF)
            bits >>= 8
            count -= 8
    if count:
        data.append(bits & 0xFF)
    blocks = bytearray([8])
    for start in range(0, len(data), 255):
        chunk = data[start:start + 255]
        blocks.append(len(chunk))
        blocks.extend(chunk)
    blocks.append(0)
    return bytes(blocks)


def main():
    colors = PALETTE + [BACKGROUND] + [0] * 247
    out = bytearray(b'GIF89a')
    out += struct.pack('<HHBBB', SIZE, SIZE, 0xF7, 8, 0)
    for color in colors:
        out += bytes([(color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF])
    out += b'\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00'
    for frame in range(FRAMES):
        out += b'\x21\xF9\x04\x04' + struct.pack('<H', DELAY_CS) + b'\x00\x00'
        out += b'\x2C' + struct.pack('<HHHHB', 0, 0, SIZE, SIZE, 0)
        out += lzw(pixels(frame))
    out += b'\x3B'
    Path(__file__).with_name('spin.gif').write_bytes(out)


if __name__ == '__main__':
    main()
