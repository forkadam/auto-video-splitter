from __future__ import annotations
from datetime import datetime
from pydantic import BaseModel, ConfigDict, Field


class WorkspaceEntry(BaseModel):
    model_config = ConfigDict(extra="forbid")
    source_relative_path: str
    size_bytes: int
    partial_hash: str
    adopted_at: datetime


class Index(BaseModel):
    model_config = ConfigDict(extra="forbid")
    schema_version: int = 1
    working_root: str = "."
    workspaces: dict[str, WorkspaceEntry] = Field(default_factory=dict)
    retired_slugs: list[str] = Field(default_factory=list)
