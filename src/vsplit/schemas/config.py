from __future__ import annotations
from typing import Literal
from pydantic import BaseModel, ConfigDict, Field, field_validator


class DetectConfig(BaseModel):
    model_config = ConfigDict(extra="forbid")
    # `threshold` is interpreted in the chosen detector's native scale:
    #   content   ~0–100 (default 27)
    #   adaptive  ~0–10  (default 3.0)
    #   histogram ~0–1   (default 0.05)
    #   hash      ~0–1   (default 0.395)
    # Switching detectors usually means picking a new threshold too.
    detector: Literal["content", "adaptive", "histogram", "hash"] = "content"
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


_VALID_CONCURRENCY_STAGES = frozenset({"probe", "detect", "preview", "plan", "split", "verify"})
# Note: "review" is excluded — review runs synchronously, no concurrency.


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

    @field_validator("concurrency")
    @classmethod
    def _validate_concurrency(cls, v: dict[str, int]) -> dict[str, int]:
        unknown = set(v) - _VALID_CONCURRENCY_STAGES
        if unknown:
            raise ValueError(f"unknown stage(s) in concurrency: {sorted(unknown)}")
        if any(n < 1 for n in v.values()):
            raise ValueError("concurrency values must be >= 1")
        return v
