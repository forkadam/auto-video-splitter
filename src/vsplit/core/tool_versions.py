from __future__ import annotations
import subprocess
import vsplit


def _first_line(cmd: list[str]) -> str | None:
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, check=True)
        return r.stdout.splitlines()[0].strip() if r.stdout else None
    except (FileNotFoundError, subprocess.CalledProcessError, IndexError):
        return None


def probe_versions() -> dict[str, str | None]:
    ffmpeg = _first_line(["ffmpeg", "-version"])
    ffprobe = _first_line(["ffprobe", "-version"])
    try:
        import scenedetect
        sd = scenedetect.__version__
    except Exception:
        sd = None
    return {
        "vsplit": vsplit.__version__,
        "ffmpeg": ffmpeg.split()[2] if ffmpeg else None,
        "ffprobe": ffprobe.split()[2] if ffprobe else None,
        "scenedetect": sd,
    }
