#!/usr/bin/env python3
"""Convert the PPM that `cargo run --example map` writes into a PNG.

    python3 tools/ppm_to_png.py map.ppm map.png
"""
import struct
import sys
import zlib


def main():
    source, target = sys.argv[1], sys.argv[2]
    data = open(source, 'rb').read()
    magic, width, height, depth, pixels = data.split(maxsplit=4)
    if magic != b'P6' or depth != b'255':
        raise SystemExit(f'{source}: not a binary 8-bit PPM')
    width, height = int(width), int(height)
    rows = b''.join(b'\x00' + pixels[y * width * 3:(y + 1) * width * 3] for y in range(height))

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))

    png = (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
           + chunk(b'IDAT', zlib.compress(rows, 6)) + chunk(b'IEND', b''))
    open(target, 'wb').write(png)


if __name__ == '__main__':
    main()
