from __future__ import annotations
from pydantic import BaseModel, ConfigDict, Field


class Scene(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: int
    start: str        # HH:MM:SS.fff
    end: str
    keep: bool
    label: str | None = None
    merge_with_next: bool = False


class CutsJson(BaseModel):
    model_config = ConfigDict(extra="forbid")
    schema_version: int = 1
    scenes: list[Scene] = Field(default_factory=list)
