"""Build icon/file-buddy.ico from icon/final_rounded.png with the vendored FFmpeg."""

import struct
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FFMPEG = ROOT / "ffmpeg" / "bin" / "ffmpeg.exe"
SOURCE = ROOT / "icon" / "final_rounded.png"
TARGET = ROOT / "icon" / "file-buddy.ico"
SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
TILE = bytes((11, 13, 16))


def ffmpeg(args, data=None):
    return subprocess.run(
        [FFMPEG, "-v", "error", *args], input=data, capture_output=True, check=True
    ).stdout


def main():
    rgba = bytearray(ffmpeg(["-i", SOURCE, "-pix_fmt", "rgba", "-f", "rawvideo", "-"]))
    side = int((len(rgba) // 4) ** 0.5)
    # Clear pixels carry white, which bleeds into the corners when scaled down.
    for i in range(0, len(rgba), 4):
        if rgba[i + 3] == 0:
            rgba[i : i + 3] = TILE

    images = []
    for size in SIZES:
        png = ffmpeg(
            [
                "-f", "rawvideo", "-pix_fmt", "rgba", "-s", f"{side}x{side}", "-i", "-",
                "-vf", f"scale={size}:{size}:flags=lanczos",
                "-pix_fmt", "rgba", "-f", "image2pipe", "-c:v", "png", "-",
            ],
            bytes(rgba),
        )
        images.append((size, png))

    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries, blobs = b"", b""
    for size, png in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), offset)
        blobs += png
        offset += len(png)
    TARGET.write_bytes(header + entries + blobs)
    print(f"{TARGET.relative_to(ROOT)}: {', '.join(str(s) for s in SIZES)}")


if __name__ == "__main__":
    main()
