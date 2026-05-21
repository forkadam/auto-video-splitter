from __future__ import annotations
import json
import subprocess
from vsplit.core.atomic import atomic_write_text, atomic_write_json
from vsplit.core.types import Stage, StageContext, StageResult


class ProbeStage(Stage):
    name = "probe"
    depends_on: list[str] = []
    output_subdir = "probe"

    def run(self, ctx: StageContext) -> StageResult:
        outdir = ctx.workspace_dir / "probe"
        outdir.mkdir(parents=True, exist_ok=True)
        src = str(ctx.source_path)

        meta_proc = subprocess.run(
            ["ffprobe", "-v", "error", "-print_format", "json",
             "-show_streams", "-show_format", src],
            check=True, capture_output=True, text=True,
        )
        meta = json.loads(meta_proc.stdout)
        atomic_write_json(outdir / "ffprobe.json", meta)

        kf_proc = subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "v:0",
             "-skip_frame", "nokey", "-show_frames",
             "-show_entries", "frame=pts_time",
             "-of", "csv=p=0", src],
            check=True, capture_output=True, text=True,
        )
        atomic_write_text(outdir / "keyframes.csv", kf_proc.stdout)

        v_stream = next(
            (s for s in meta.get("streams", []) if s.get("codec_type") == "video"),
            None,
        )
        a_stream = next(
            (s for s in meta.get("streams", []) if s.get("codec_type") == "audio"),
            None,
        )
        fps = None
        if v_stream:
            num, _, den = v_stream.get("avg_frame_rate", "0/0").partition("/")
            try:
                if int(den) != 0:
                    fps = int(num) / int(den)
            except ValueError:
                fps = None

        keyframe_count = sum(1 for line in kf_proc.stdout.splitlines() if line.strip())

        ctx.manifest.metadata.duration_seconds = (
            float(meta.get("format", {}).get("duration", 0.0)) or None
        )
        ctx.manifest.metadata.container = meta.get("format", {}).get("format_name")
        ctx.manifest.metadata.video_codec = v_stream and v_stream.get("codec_name")
        ctx.manifest.metadata.audio_codec = a_stream and a_stream.get("codec_name")
        ctx.manifest.metadata.fps = fps
        ctx.manifest.metadata.width = v_stream and v_stream.get("width")
        ctx.manifest.metadata.height = v_stream and v_stream.get("height")
        ctx.manifest.metadata.keyframe_count = keyframe_count

        return StageResult(artifacts={"keyframe_count": keyframe_count})
