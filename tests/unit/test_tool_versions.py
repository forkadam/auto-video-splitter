from vsplit.core.tool_versions import probe_versions

def test_probe_versions_returns_strings():
    v = probe_versions()
    assert isinstance(v["vsplit"], str)
    # ffmpeg/ffprobe/scenedetect may be None if not installed, but the keys must exist
    assert "ffmpeg" in v
    assert "ffprobe" in v
    assert "scenedetect" in v
