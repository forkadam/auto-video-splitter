from __future__ import annotations
import json
import subprocess
from pathlib import Path
from jinja2 import Environment, FileSystemLoader, select_autoescape
from vsplit.core.atomic import atomic_write_text
from vsplit.core.types import Stage, StageContext, StageResult

_TEMPLATE_DIR = Path(__file__).parent.parent / "templates"


def _format_tc(seconds: float) -> str:
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = seconds - h * 3600 - m * 60
    return f"{h:02d}:{m:02d}:{s:06.3f}"


class PreviewStage(Stage):
    name = "preview"
    depends_on = ["detect"]
    output_subdir = "preview"

    def run(self, ctx: StageContext) -> StageResult:
        outdir = ctx.workspace_dir / "preview"
        imgs = outdir / "images"
        imgs.mkdir(parents=True, exist_ok=True)

        scenes_data = json.loads(
            (ctx.workspace_dir / "detect" / "scenes.json").read_text()
        )
        n = ctx.config.preview.images_per_scene
        w = ctx.config.preview.image_width
        q = ctx.config.preview.image_quality

        scenes_for_template = []
        for scene in scenes_data["scenes"]:
            duration = scene["end_time"] - scene["start_time"]
            sample_ts = [
                scene["start_time"] + duration * (i / max(n - 1, 1))
                for i in range(n)
            ] if n > 1 else [scene["start_time"] + duration / 2]
            img_names = []
            for k, t in enumerate(sample_ts, start=1):
                name = f"scene-{scene['id']:03d}-{k:02d}.jpg"
                subprocess.run(
                    ["ffmpeg", "-y", "-loglevel", "error", "-ss", f"{t:.3f}",
                     "-i", str(ctx.source_path), "-frames:v", "1",
                     "-vf", f"scale={w}:-1",
                     "-q:v", str(max(2, min(31, 31 - int(q / 100 * 29)))),
                     str(imgs / name)],
                    check=True,
                )
                img_names.append(name)
            scenes_for_template.append({
                "id": scene["id"],
                "start_tc": _format_tc(scene["start_time"]),
                "end_tc": _format_tc(scene["end_time"]),
                "duration": duration,
                "images": img_names,
            })

        env = Environment(
            loader=FileSystemLoader(str(_TEMPLATE_DIR)),
            autoescape=select_autoescape(["html"]),
        )
        tmpl = env.get_template("preview_index.html.j2")
        rendered = tmpl.render(
            slug=ctx.manifest.id,
            source_rel=ctx.manifest.source.relative_path,
            scenes=scenes_for_template,
        )
        atomic_write_text(outdir / "index.html", rendered)

        return StageResult(artifacts={
            "images_generated": sum(len(s["images"]) for s in scenes_for_template),
        })
