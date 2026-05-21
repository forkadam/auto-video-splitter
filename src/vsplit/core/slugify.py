from __future__ import annotations
import re
from pathlib import Path

_RESERVED_NAMES = {
    "con", "prn", "aux", "nul",
    *(f"com{i}" for i in range(1, 10)),
    *(f"lpt{i}" for i in range(1, 10)),
}
_CONTROL_CHARS = re.compile(r"[\x00-\x08\x0e-\x1f\x7f]")
_RESERVED_CHARS = re.compile(r'[<>:"/\\|?*]')
_WHITESPACE_UNDERSCORE = re.compile(r"[\s_]+")
_REPEATED_HYPHEN = re.compile(r"-{2,}")
_MAX_LEN = 64


def slugify(filename: str) -> str:
    stem = Path(filename).stem.lower()
    stem = _CONTROL_CHARS.sub("", stem)
    stem = _RESERVED_CHARS.sub("", stem)
    stem = _WHITESPACE_UNDERSCORE.sub("-", stem)
    stem = _REPEATED_HYPHEN.sub("-", stem)
    stem = stem.strip(". -")
    if stem in _RESERVED_NAMES:
        stem = f"_{stem}"
    stem = stem[:_MAX_LEN].strip(". -")
    if not stem:
        stem = "_unnamed"
    return stem


def resolve_collision(
    slug: str, live: set[str], retired: set[str]
) -> str:
    taken_lower = {s.lower() for s in live} | {s.lower() for s in retired}
    if slug.lower() not in taken_lower:
        return slug
    base = slug[:_MAX_LEN - 4].rstrip(". -")
    n = 2
    while f"{base}-{n}".lower() in taken_lower:
        n += 1
    return f"{base}-{n}"
