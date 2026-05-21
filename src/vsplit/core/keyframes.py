"""Keyframe utilities shared by plan and preview stages."""
from __future__ import annotations
from pathlib import Path


def load_keyframes(path: Path) -> list[float]:
    """Parse ffprobe's `-of csv=p=0` output. Tolerates extra fields (e.g. SEI messages)."""
    out: list[float] = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        first_field = line.split(",")[0]
        try:
            out.append(float(first_field))
        except ValueError:
            pass
    return out


def snap_preceding(t: float, keyframes: list[float]) -> float:
    """Return the largest keyframe <= t, or 0.0 if none."""
    candidates = [k for k in keyframes if k <= t]
    return candidates[-1] if candidates else 0.0
