from __future__ import annotations
import fnmatch
from dataclasses import dataclass
from typing import Callable


@dataclass(frozen=True)
class Predicate:
    key: str
    value: str


VALID_KEYS = {"status", "stage", "slug"}


def parse_filter(s: str) -> Predicate:
    if "=" not in s:
        raise ValueError(f"filter must be key=value, got {s!r}")
    k, v = s.split("=", 1)
    if k not in VALID_KEYS:
        raise ValueError(f"unknown filter key: {k!r} (allowed: {sorted(VALID_KEYS)})")
    return Predicate(key=k, value=v)


def apply_filters(
    slugs: list[str],
    predicates: list[Predicate],
    *,
    status_pred: Callable[[str, str], bool],
    stage_pred: Callable[[str, str], bool],
) -> list[str]:
    result = list(slugs)
    for p in predicates:
        if p.key == "slug":
            result = [s for s in result if fnmatch.fnmatch(s, p.value)]
        elif p.key == "status":
            result = [s for s in result if status_pred(s, p.value)]
        elif p.key == "stage":
            result = [s for s in result if stage_pred(s, p.value)]
    return result
