from __future__ import annotations
from datetime import datetime
from typing import Any, Literal
from pydantic import BaseModel, ConfigDict, Field, model_validator

StageStatus = Literal["pending", "running", "done", "failed", "awaiting_user", "skipped"]
STAGE_NAMES = ("probe", "detect", "preview", "review", "plan", "split", "verify")


class SourceInfo(BaseModel):
    model_config = ConfigDict(extra="forbid")
    relative_path: str
    size_bytes: int
    mtime: datetime
    partial_hash: str


class MetadataBlock(BaseModel):
    model_config = ConfigDict(extra="forbid")
    duration_seconds: float | None = None
    container: str | None = None
    video_codec: str | None = None
    audio_codec: str | None = None
    fps: float | None = None
    width: int | None = None
    height: int | None = None
    keyframe_count: int | None = None


class ToolVersions(BaseModel):
    model_config = ConfigDict(extra="forbid")
    vsplit: str | None = None
    ffmpeg: str | None = None
    ffprobe: str | None = None
    scenedetect: str | None = None


class StageError(BaseModel):
    model_config = ConfigDict(extra="forbid")
    message: str
    exit_code: int | None = None
    log_path: str | None = None


class StageBlock(BaseModel):
    model_config = ConfigDict(extra="forbid")
    status: StageStatus = "pending"
    started_at: datetime | None = None
    completed_at: datetime | None = None
    set_at: datetime | None = None
    artifacts: dict[str, Any] = Field(default_factory=dict)
    error: StageError | None = None


class Manifest(BaseModel):
    model_config = ConfigDict(extra="forbid")
    schema_version: int = 1
    id: str
    created_at: datetime
    updated_at: datetime
    source: SourceInfo
    ignored: bool = False
    ignored_at: datetime | None = None
    metadata: MetadataBlock = Field(default_factory=MetadataBlock)
    tool_versions: ToolVersions = Field(default_factory=ToolVersions)
    stages: dict[str, StageBlock]
    config_snapshot: dict[str, Any] = Field(default_factory=dict)

    @model_validator(mode="after")
    def _validate_stage_keys(self) -> "Manifest":
        expected = set(STAGE_NAMES)
        actual = set(self.stages.keys())
        missing = expected - actual
        extra = actual - expected
        if missing:
            raise ValueError(f"manifest missing stage keys: {sorted(missing)}")
        if extra:
            raise ValueError(f"manifest has unknown stage keys: {sorted(extra)}")
        return self
