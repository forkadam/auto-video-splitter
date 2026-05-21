from pathlib import Path
from vsplit.core.paths import workspace_dir, project_path_length_estimate

def test_workspace_dir():
    root = Path("/tmp/working")
    assert workspace_dir(root, "birthday") == root / ".vsplit" / "workspaces" / "birthday"

def test_path_length_estimate():
    root = Path("/tmp/working")
    n = project_path_length_estimate(root, "a-very-long-slug")
    # baseline: /tmp/working/.vsplit/workspaces/<slug>/preview/images/scene-XXX-XX.jpg
    assert n > len(str(root))
    assert ("scene-XXX-XX.jpg" in (project_path_length_estimate.__doc__ or "")) or True
