from __future__ import annotations
import json
import subprocess
from vsplit.core.types import Stage, StageContext, StageResult


def _probe_duration(path) -> float:
    result = subprocess.run(
        ["ffprobe", "-v", "error", "-print_format", "json",
         "-show_format", str(path)],
        check=True, capture_output=True, text=True,
    )
    data = json.loads(result.stdout)
    return float(data["format"]["duration"])


class VerifyStage(Stage):
    name = "verify"
    depends_on = ["split"]
    output_subdir = None      # writes to manifest only

    def run(self, ctx: StageContext) -> StageResult:
        plan = json.loads(
            (ctx.workspace_dir / "plan" / "cuts-resolved.json").read_text()
        )
        tol = ctx.config.verify.duration_tolerance_seconds
        outdir = ctx.workspace_dir / "output"
        report = {"all_ok": True, "files": []}
        for seg in plan["scenes"]:
            f = outdir / seg["output_filename"]
            expected = seg["actual_end"] - seg["actual_start"]
            entry: dict = {"file": seg["output_filename"], "expected_duration": expected}
            if not f.exists():
                entry["status"] = "missing"
                report["all_ok"] = False
            else:
                try:
                    actual = _probe_duration(f)
                    entry["actual_duration"] = actual
                    if abs(actual - expected) > tol:
                        entry["status"] = "duration_mismatch"
                        report["all_ok"] = False
                    else:
                        entry["status"] = "ok"
                except subprocess.CalledProcessError:
                    entry["status"] = "unreadable"
                    report["all_ok"] = False
            report["files"].append(entry)
        return StageResult(artifacts={"report": report})

    def recover(self, ctx: StageContext) -> None:
        """No on-disk state to clean; reset status."""
        block = ctx.manifest.stages[self.name]
        block.status = "pending"
        block.artifacts = {}
        block.error = None
        block.started_at = None
        block.completed_at = None
