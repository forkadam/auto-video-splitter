"""Generate tiny test video fixtures using ffmpeg."""
from __future__ import annotations
import subprocess
import sys
from pathlib import Path

FIXTURES = Path(__file__).parent


def run(args: list[str]) -> None:
    print(" ".join(args))
    subprocess.run(args, check=True)


def main() -> None:
    FIXTURES.mkdir(exist_ok=True)
    # 5-second mp4 with two color changes -> scene boundaries
    out = FIXTURES / "short.mp4"
    run([
        "ffmpeg", "-y",
        "-f", "lavfi", "-i",
        "color=red:size=320x240:duration=2:rate=24[a];"
        "color=green:size=320x240:duration=2:rate=24[b];"
        "color=blue:size=320x240:duration=1:rate=24[c];"
        "[a][b][c]concat=n=3:v=1:a=0",
        "-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "24",
        str(out),
    ])
    # 3-second mkv (different container)
    out = FIXTURES / "short.mkv"
    run([
        "ffmpeg", "-y",
        "-f", "lavfi", "-i", "color=white:size=320x240:duration=3:rate=24",
        "-c:v", "libx264", "-pix_fmt", "yuv420p",
        str(out),
    ])


if __name__ == "__main__":
    sys.exit(main())
