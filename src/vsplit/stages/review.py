from __future__ import annotations
import json
from vsplit.core.atomic import atomic_write_text
from vsplit.core.hash import content_hash
from vsplit.core.types import Stage, StageContext, StageResult


def _format_tc(seconds: float) -> str:
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = seconds - h * 3600 - m * 60
    return f"{h:02d}:{m:02d}:{s:06.3f}"


class ReviewStage(Stage):
    name = "review"
    depends_on = ["preview"]
    output_subdir = "decisions"

    def run(self, ctx: StageContext) -> StageResult:
        decisions_dir = ctx.workspace_dir / "decisions"
        decisions_dir.mkdir(parents=True, exist_ok=True)
        cuts_path = decisions_dir / "cuts.json"

        scenes_data = json.loads(
            (ctx.workspace_dir / "detect" / "scenes.json").read_text()
        )
        default_keep = ctx.config.review.default_keep
        cuts = {
            "schema_version": 1,
            "scenes": [
                {
                    "id": s["id"],
                    "start": _format_tc(s["start_time"]),
                    "end": _format_tc(s["end_time"]),
                    "keep": default_keep,
                    "label": None,
                    "merge_with_next": False,
                }
                for s in scenes_data["scenes"]
            ],
        }
        atomic_write_text(cuts_path, json.dumps(cuts, indent=2) + "\n")
        h = content_hash(cuts_path)
        return StageResult(
            status="awaiting_user",
            artifacts={
                "cuts_file_hash": h,
                "scenes_total": len(cuts["scenes"]),
                "scenes_marked_keep_initial": sum(1 for s in cuts["scenes"] if s["keep"]),
            },
        )
