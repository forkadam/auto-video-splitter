from __future__ import annotations
import json
import subprocess
from vsplit.core.types import Stage, StageContext, StageResult


class SplitStage(Stage):
    name = "split"
    depends_on = ["plan"]
    output_subdir = "output"

    def run(self, ctx: StageContext) -> StageResult:
        outdir = ctx.workspace_dir / "output"
        outdir.mkdir(parents=True, exist_ok=True)
        plan = json.loads(
            (ctx.workspace_dir / "plan" / "cuts-resolved.json").read_text()
        )
        produced = []
        for seg in plan["scenes"]:
            if ctx.cancel_event.is_set():
                break
            out_path = outdir / seg["output_filename"]
            cmd = [
                "ffmpeg", "-y", "-loglevel", "error",
                "-ss", f"{seg['actual_start']:.3f}",
                "-to", f"{seg['actual_end']:.3f}",
                "-i", str(ctx.source_path),
                "-map", "0", "-c", "copy",
                "-avoid_negative_ts", "make_zero",
                str(out_path),
            ]
            ctx.logger.info("split: %s", " ".join(cmd))
            subprocess.run(cmd, check=True)
            produced.append(seg["output_filename"])
        return StageResult(artifacts={"outputs": produced})

    def recover(self, ctx: StageContext) -> None:
        """Keep validated outputs; delete incomplete ones; reset to pending."""
        outdir = ctx.workspace_dir / "output"
        plan_path = ctx.workspace_dir / "plan" / "cuts-resolved.json"
        if plan_path.exists() and outdir.exists():
            plan = json.loads(plan_path.read_text())
            expected = {s["output_filename"] for s in plan["scenes"]}
            for f in list(outdir.iterdir()):
                if f.name not in expected:
                    f.unlink(missing_ok=True)
                    continue
                try:
                    subprocess.run(
                        ["ffprobe", "-v", "error", "-show_format", str(f)],
                        check=True, capture_output=True,
                    )
                except subprocess.CalledProcessError:
                    f.unlink(missing_ok=True)
        block = ctx.manifest.stages[self.name]
        block.status = "pending"
        block.artifacts = {}
        block.error = None
        block.started_at = None
        block.completed_at = None
