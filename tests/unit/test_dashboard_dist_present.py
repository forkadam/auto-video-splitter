from pathlib import Path

DIST = Path(__file__).parents[2] / "src" / "vsplit" / "dashboard" / "dist"


def test_dist_index_html_exists() -> None:
    assert (DIST / "index.html").is_file(), (
        "Run `cd src/vsplit/dashboard/src && bun run build` to regenerate dist/"
    )


def test_dist_index_references_existing_asset() -> None:
    html = (DIST / "index.html").read_text()
    # Vite emits <script type="module" crossorigin src="/assets/index-XXXX.js"></script>
    import re
    m = re.search(r'src="(/assets/[^"]+\.js)"', html)
    assert m, "index.html missing assets/* script tag"
    asset = DIST / m.group(1).lstrip("/")
    assert asset.is_file(), f"asset {m.group(1)} referenced but missing in dist/"
