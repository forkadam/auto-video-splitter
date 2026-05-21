from __future__ import annotations
from typing import Literal
from pydantic import BaseModel, ConfigDict, Field


class DetectConfig(BaseModel):
    model_config = ConfigDict(extra="forbid")
    threshold: float = 27.0
    min_scene_length: float = 0.6
    luma_only: bool = False


class PreviewConfig(BaseModel):
    model_config = ConfigDict(extra="forbid")
    images_per_scene: int = 3
    image_width: int = 480
    image_quality: int = 85


class ReviewConfig(BaseModel):
    model_config = ConfigDict(extra="forbid")
    default_keep: bool = True
    auto_apply_template: bool = True


class PlanConfig(BaseModel):
    model_config = ConfigDict(extra="forbid")
    keyframe_snap: Literal["nearest_preceding"] = "nearest_preceding"
    min_segment_length: float = 1.0


class SplitConfig(BaseModel):
    model_config = ConfigDict(extra="forbid")
    output_container: str = "mp4"


class VerifyConfig(BaseModel):
    model_config = ConfigDict(extra="forbid")
    duration_tolerance_seconds: float = 0.5


class ScanConfig(BaseModel):
    model_config = ConfigDict(extra="forbid")
    extensions: list[str] = Field(
        default_factory=lambda: ["mp4", "mkv", "mov", "avi", "m4v", "webm"]
    )
    recursive: bool = False


class Config(BaseModel):
    model_config = ConfigDict(extra="forbid")
    detect: DetectConfig = Field(default_factory=DetectConfig)
    preview: PreviewConfig = Field(default_factory=PreviewConfig)
    review: ReviewConfig = Field(default_factory=ReviewConfig)
    plan: PlanConfig = Field(default_factory=PlanConfig)
    split: SplitConfig = Field(default_factory=SplitConfig)
    verify: VerifyConfig = Field(default_factory=VerifyConfig)
    concurrency: dict[str, int] = Field(
        default_factory=lambda: {
            "probe": 4, "detect": 2, "preview": 4,
            "plan": 8, "split": 1, "verify": 4,
        }
    )
    scan: ScanConfig = Field(default_factory=ScanConfig)
