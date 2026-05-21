from pathlib import Path
from vsplit.core.paths import workspace_dir, project_path_length_estimate

def test_workspace_dir():
    root = Path("/tmp/working")
    assert workspace_dir(root, "birthday") == root / ".vsplit" / "workspaces" / "birthday"

def test_path_length_estimate_scales_with_slug(tmp_path: Path):
    root = tmp_path / "working"
    n_short = project_path_length_estimate(root, "a")
    n_long = project_path_length_estimate(root, "a" * 64)
    assert n_long > n_short

def test_path_length_estimate_accounts_for_output_filename(tmp_path: Path):
    root = tmp_path / "working"
    slug = "a" * 30
    n = project_path_length_estimate(root, slug)
    # Output path is <root>/.vsplit/workspaces/<slug>/output/<slug>-highlight-NN-label.mp4
    # That length must be reachable by the estimate; old estimate (preview/images/...) underestimates by ~50 chars
    expected_min = len(str(workspace_dir(root, slug) / "output" / f"{slug}-highlight-99.mp4"))
    assert n >= expected_min
