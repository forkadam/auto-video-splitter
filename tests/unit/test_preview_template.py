"""Fast, ffmpeg-free render of the preview HTML template.

Guards the CSS contract that the standalone preview page (also embedded in the
dashboard's bounded preview pane via an iframe) must keep the <video> inside the
viewport. A tall portrait proxy — the common iPhone case — otherwise renders at
its intrinsic height and its bottom (plus the native control bar) falls below the
visible area, so it "looks different from the thumbnail."
"""
from jinja2 import Environment, FileSystemLoader, select_autoescape

from vsplit.stages.preview import _TEMPLATE_DIR


def _render(**overrides) -> str:
    env = Environment(
        loader=FileSystemLoader(str(_TEMPLATE_DIR)),
        autoescape=select_autoescape(["html"]),
    )
    ctx = dict(
        slug="demo",
        source_rel="demo.mov",
        scenes=[],
        has_proxy=True,
        proxy_name="source-proxy.mp4",
        duration_label=None,
    )
    ctx.update(overrides)
    return env.get_template("preview_index.html.j2").render(**ctx)


def test_preview_video_is_height_capped() -> None:
    html = _render()
    # The <video> must be height-constrained; without a cap a portrait proxy
    # overflows the bounded preview pane and hides the bottom of the frame.
    assert "max-height" in html
