from __future__ import annotations
import re
from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

_TS_RE = re.compile(r"^\d{2}:\d{2}:\d{2}\.\d{3}$")


class Scene(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: int
    start: str        # HH:MM:SS.fff
    end: str
    keep: bool
    label: str | None = None
    merge_with_next: bool = False

    @field_validator("start", "end")
    @classmethod
    def _validate_timestamp(cls, v: str) -> str:
        if not _TS_RE.fullmatch(v):
            raise ValueError(f"expected HH:MM:SS.fff, got {v!r}")
        return v


class CutsJson(BaseModel):
    model_config = ConfigDict(extra="forbid")
    schema_version: int = 1
    scenes: list[Scene] = Field(default_factory=list)

    @model_validator(mode="after")
    def _validate_unique_ids(self) -> "CutsJson":
        ids = [s.id for s in self.scenes]
        if len(set(ids)) != len(ids):
            from collections import Counter
            dups = [i for i, c in Counter(ids).items() if c > 1]
            raise ValueError(f"duplicate scene ids: {sorted(dups)}")
        return self
