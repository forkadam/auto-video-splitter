from __future__ import annotations
import json
from vsplit.core.atomic import atomic_write_json
from vsplit.core.keyframes import load_keyframes, snap_preceding
from vsplit.core.types import Stage, StageContext, StageResult
from vsplit.schemas.cuts import CutsJson, Scene


def _parse_tc(tc: str) -> float:
    h, m, s = tc.split(":")
    return int(h) * 3600 + int(m) * 60 + float(s)


def _sanitize_label(label: str | None) -> str | None:
    if not label:
        return None
    safe = "".join(c if c.isalnum() or c in "-_" else "-" for c in label.lower())
    safe = "-".join(filter(None, safe.split("-")))
    return safe or None


class PlanStage(Stage):
    name = "plan"
    depends_on = ["review", "probe"]
    output_subdir = "plan"

    def run(self, ctx: StageContext) -> StageResult:
        outdir = ctx.workspace_dir / "plan"
        outdir.mkdir(parents=True, exist_ok=True)

        cuts = CutsJson.model_validate_json(
            (ctx.workspace_dir / "decisions" / "cuts.json").read_text()
        )
        keyframes = load_keyframes(ctx.workspace_dir / "probe" / "keyframes.csv")
        kept = [s for s in cuts.scenes if s.keep]

        # Merge adjacent kept scenes via merge_with_next. Only join when the
        # next kept scene is the immediate timeline neighbor (id N → N+1). A
        # merge flag whose following scene was dropped must NOT bridge the gap:
        # a single stream-copy clip spans first.start → last.end, so bridging
        # would swallow the footage of the dropped scenes in between.
        merged: list[list[Scene]] = []
        for s in kept:
            prev = merged[-1][-1] if merged else None
            if prev is not None and prev.merge_with_next and s.id == prev.id + 1:
                merged[-1].append(s)
            else:
                merged.append([s])

        resolved = []
        dropped_degenerate: list[int] = []
        out_index = 1
        for group in merged:
            start_req = _parse_tc(group[0].start)
            end_req = _parse_tc(group[-1].end)
            # Honor explicit keeps: every scene here was already chosen during
            # review, so we do NOT drop short segments — only guard against
            # degenerate (zero/negative-length) spans that ffmpeg cannot cut.
            if end_req <= start_req:
                dropped_degenerate.append(group[0].id)
                ctx.logger.warning(
                    "plan: dropping scene %s — non-positive length (%.3fs → %.3fs)",
                    group[0].id, start_req, end_req,
                )
                continue
            actual_start = snap_preceding(start_req, keyframes)
            actual_end = end_req
            label = _sanitize_label(group[0].label)
            base = f"{ctx.manifest.id}-highlight-{out_index:02d}"
            fname = f"{base}-{label}.{ctx.config.split.output_container}" if label \
                else f"{base}.{ctx.config.split.output_container}"
            resolved.append({
                "id": group[0].id,
                "label": label,
                "requested_start": start_req,
                "requested_end": end_req,
                "actual_start": actual_start,
                "actual_end": actual_end,
                "drift_seconds": round(start_req - actual_start, 6),
                "strategy": "stream_copy",
                "output_filename": fname,
            })
            out_index += 1

        atomic_write_json(outdir / "cuts-resolved.json", {"scenes": resolved})
        return StageResult(artifacts={
            "segments": len(resolved),
            "dropped_degenerate": dropped_degenerate,
        })
