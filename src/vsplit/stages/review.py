from __future__ import annotations
import json
from pydantic import ValidationError
from vsplit.core.atomic import atomic_write_text
from vsplit.core.hash import content_hash
from vsplit.core.types import Stage, StageContext, StageResult
from vsplit.schemas.cuts import CutsJson


def _format_tc(seconds: float) -> str:
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = seconds - h * 3600 - m * 60
    return f"{h:02d}:{m:02d}:{s:06.3f}"


def _initial_cuts(scenes: list[dict], default_keep: bool) -> dict:
    return {
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
            for s in scenes
        ],
    }


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
        detected_ids = {s["id"] for s in scenes_data["scenes"]}

        block = ctx.manifest.stages["review"]
        is_first_entry = not cuts_path.exists()
        template_path = ctx.working_root / "cuts.json.template"

        # Template auto-apply (only on first entry)
        if (
            is_first_entry
            and ctx.config.review.auto_apply_template
            and template_path.exists()
        ):
            atomic_write_text(cuts_path, template_path.read_text() + ("" if template_path.read_text().endswith("\n") else "\n"))
            cuts = CutsJson.model_validate_json(cuts_path.read_text())
            _validate_ids(cuts, detected_ids)
            return StageResult(
                status="done",
                artifacts={
                    "cuts_file_hash": content_hash(cuts_path),
                    "scenes_total": len(cuts.scenes),
                    "scenes_marked_keep_final": sum(1 for s in cuts.scenes if s.keep),
                    "applied_template": True,
                },
            )

        if is_first_entry:
            cuts = _initial_cuts(scenes_data["scenes"], ctx.config.review.default_keep)
            atomic_write_text(cuts_path, json.dumps(cuts, indent=2) + "\n")
            return StageResult(
                status="awaiting_user",
                artifacts={
                    "cuts_file_hash": content_hash(cuts_path),
                    "scenes_total": len(cuts["scenes"]),
                    "scenes_marked_keep_initial": sum(1 for s in cuts["scenes"] if s["keep"]),
                },
            )

        # Re-entry: decide if the gate has been passed
        current_hash = content_hash(cuts_path)
        recorded_hash = block.artifacts.get("cuts_file_hash")
        approved = block.artifacts.get("approved_at") is not None

        if current_hash == recorded_hash and not approved:
            return StageResult(
                status="awaiting_user",
                artifacts={**block.artifacts},
            )

        try:
            parsed = CutsJson.model_validate_json(cuts_path.read_text())
        except ValidationError as e:
            raise ValueError(f"cuts.json invalid: {e}") from e

        _validate_ids(parsed, detected_ids)

        return StageResult(
            status="done",
            artifacts={
                **block.artifacts,
                "cuts_file_hash": current_hash,
                "scenes_marked_keep_final": sum(1 for s in parsed.scenes if s.keep),
            },
        )


def _validate_ids(parsed: CutsJson, detected_ids: set[int]) -> None:
    # No scenes detected → the user authors cuts manually (whole-clip or manual
    # ranges), so any scene ids are accepted. With detected scenes present we
    # still keep cuts.json in sync (reject added/removed rows).
    if not detected_ids:
        return
    cuts_ids = {s.id for s in parsed.scenes}
    missing = detected_ids - cuts_ids
    extra = cuts_ids - detected_ids
    if missing:
        raise ValueError(
            f"scene IDs {sorted(missing)} from detect/scenes.json missing in cuts.json — "
            f"use keep:false to drop a scene, don't remove the row"
        )
    if extra:
        raise ValueError(
            f"scene IDs {sorted(extra)} in cuts.json not present in detect/scenes.json — "
            f"add new scenes by editing detect/scenes.json then `reset --from detect`"
        )
