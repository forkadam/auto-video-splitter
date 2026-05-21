from __future__ import annotations
from scenedetect import detect
from scenedetect.detectors import (
    AdaptiveDetector,
    ContentDetector,
    HashDetector,
    HistogramDetector,
)
from vsplit.core.atomic import atomic_write_json
from vsplit.core.types import Stage, StageContext, StageResult


def _build_detector(name: str, threshold: float, min_scene_len: int, luma_only: bool):
    if name == "content":
        return ContentDetector(threshold=threshold, min_scene_len=min_scene_len, luma_only=luma_only)
    if name == "adaptive":
        return AdaptiveDetector(adaptive_threshold=threshold, min_scene_len=min_scene_len, luma_only=luma_only)
    if name == "histogram":
        return HistogramDetector(threshold=threshold, min_scene_len=min_scene_len)
    if name == "hash":
        return HashDetector(threshold=threshold, min_scene_len=min_scene_len)
    raise ValueError(f"unknown detector: {name!r}")


class DetectStage(Stage):
    name = "detect"
    depends_on = ["probe"]
    output_subdir = "detect"

    def run(self, ctx: StageContext) -> StageResult:
        outdir = ctx.workspace_dir / "detect"
        outdir.mkdir(parents=True, exist_ok=True)
        detector_name = ctx.config.detect.detector
        threshold = ctx.config.detect.threshold
        min_scene_len = int(
            ctx.config.detect.min_scene_length * (ctx.manifest.metadata.fps or 24)
        ) or 1
        detector = _build_detector(
            name=detector_name,
            threshold=threshold,
            min_scene_len=min_scene_len,
            luma_only=ctx.config.detect.luma_only,
        )
        scene_list = detect(str(ctx.source_path), detector)
        scenes_payload = []
        for i, (start, end) in enumerate(scene_list, start=1):
            scenes_payload.append({
                "id": i,
                "start_time": start.seconds,
                "end_time": end.seconds,
                "start_frame": start.frame_num,
                "end_frame": end.frame_num,
            })
        atomic_write_json(outdir / "scenes.json", {
            "detector": detector_name,
            "config": {"threshold": threshold},
            "scenes": scenes_payload,
        })
        return StageResult(artifacts={
            "scenes_found": len(scenes_payload),
            "detector": detector_name,
        })
