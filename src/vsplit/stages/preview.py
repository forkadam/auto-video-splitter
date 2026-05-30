from __future__ import annotations
import json
import subprocess
from pathlib import Path
from jinja2 import Environment, FileSystemLoader, select_autoescape
from vsplit.core.atomic import atomic_write_text
from vsplit.core.keyframes import load_keyframes, snap_preceding
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

        # Keyframes drive the "where the cut will actually land" info we show
        # alongside the detector's boundary. Probe is a hard dependency of detect,
        # so keyframes.csv is always present by the time preview runs.
        keyframes = load_keyframes(ctx.workspace_dir / "probe" / "keyframes.csv")

        scenes_for_template = []
        for scene in scenes_data["scenes"]:
            duration = scene["end_time"] - scene["start_time"]
            _safe_end = scene["end_time"] - min(0.1, duration * 0.05)
            # Very short scenes (< ~1s) don't have room for N evenly-spaced
            # frames; collapse to a single midpoint thumb instead.
            n_eff = 1 if duration < 1.0 else n
            sample_ts = [
                min(scene["start_time"] + duration * (i / max(n_eff - 1, 1)), _safe_end)
                for i in range(n_eff)
            ] if n_eff > 1 else [scene["start_time"] + duration / 2]
            img_names = []
            for k, t in enumerate(sample_ts, start=1):
                name = f"scene-{scene['id']:03d}-{k:02d}.jpg"
                # `format=yuvj420p` downsamples 10-bit HEVC (iPhone default)
                # to JPEG's expected 8-bit full-range YUV. Without it MJPEG
                # encoder fails with "ff_frame_thread_encoder_init failed".
                subprocess.run(
                    ["ffmpeg", "-y", "-loglevel", "error", "-ss", f"{t:.3f}",
                     "-i", str(ctx.source_path), "-frames:v", "1",
                     "-vf", f"scale={w}:-1,format=yuvj420p",
                     "-q:v", str(max(2, min(31, 31 - int(q / 100 * 29)))),
                     str(imgs / name)],
                    check=True,
                )
                # ffmpeg can exit 0 yet produce no file when the seek lands
                # past the last decodable frame near EOF. Only list thumbs
                # that actually made it to disk so the HTML <img>s aren't broken.
                out_path = imgs / name
                if out_path.exists() and out_path.stat().st_size > 0:
                    img_names.append(name)
            actual_start = snap_preceding(scene["start_time"], keyframes)
            drift = scene["start_time"] - actual_start
            scenes_for_template.append({
                "id": scene["id"],
                "start_tc": _format_tc(scene["start_time"]),
                "end_tc": _format_tc(scene["end_time"]),
                "actual_start_tc": _format_tc(actual_start),
                "drift_seconds": drift,
                "duration": duration,
                "images": img_names,
            })

        # Optional H.264 proxy so the <video> element plays in any browser
        # (iPhone HEVC clips don't play in Chrome/Edge/Firefox).
        proxy_height = ctx.config.preview.proxy_height
        proxy_name = "source-proxy.mp4"
        has_proxy = False
        if proxy_height and proxy_height > 0:
            proxy_path = outdir / proxy_name
            # `format=yuv420p` downconverts 10-bit HEVC to 8-bit so libx264
            # (default 8-bit build) accepts the input.
            subprocess.run(
                ["ffmpeg", "-y", "-loglevel", "error",
                 "-i", str(ctx.source_path),
                 "-vf", f"scale=-2:{proxy_height},format=yuv420p",
                 "-c:v", "libx264", "-preset", "veryfast", "-crf", "28",
                 "-c:a", "aac", "-b:a", "96k",
                 "-movflags", "+faststart",
                 str(proxy_path)],
                check=True,
            )
            has_proxy = True

        # Format the source duration as M:SS (or H:MM:SS past an hour) for the
        # preview header. None until probe has populated metadata.
        _dur = ctx.manifest.metadata.duration_seconds
        duration_label = None
        if _dur is not None:
            total = round(_dur)
            hours, rem = divmod(total, 3600)
            minutes, secs = divmod(rem, 60)
            duration_label = (
                f"{hours}:{minutes:02d}:{secs:02d}" if hours else f"{minutes}:{secs:02d}"
            )

        env = Environment(
            loader=FileSystemLoader(str(_TEMPLATE_DIR)),
            autoescape=select_autoescape(["html"]),
        )
        tmpl = env.get_template("preview_index.html.j2")
        rendered = tmpl.render(
            slug=ctx.manifest.id,
            source_rel=ctx.manifest.source.relative_path,
            scenes=scenes_for_template,
            has_proxy=has_proxy,
            proxy_name=proxy_name,
            duration_label=duration_label,
        )
        atomic_write_text(outdir / "index.html", rendered)

        return StageResult(artifacts={
            "images_generated": sum(len(s["images"]) for s in scenes_for_template),
            "proxy_generated": has_proxy,
        })
