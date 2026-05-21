from __future__ import annotations
from scenedetect import detect, AdaptiveDetector
from vsplit.core.atomic import atomic_write_json
from vsplit.core.types import Stage, StageContext, StageResult


class DetectStage(Stage):
    name = "detect"
    depends_on = ["probe"]
    output_subdir = "detect"

    def run(self, ctx: StageContext) -> StageResult:
        outdir = ctx.workspace_dir / "detect"
        outdir.mkdir(parents=True, exist_ok=True)
        detector = AdaptiveDetector(
            adaptive_threshold=ctx.config.detect.threshold,
            min_scene_len=int(
                ctx.config.detect.min_scene_length * (ctx.manifest.metadata.fps or 24)
            ) or 1,
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
            "detector": "adaptive",
            "config": {"threshold": ctx.config.detect.threshold},
            "scenes": scenes_payload,
        })
        return StageResult(artifacts={
            "scenes_found": len(scenes_payload),
            "detector": "adaptive",
        })
