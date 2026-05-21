from __future__ import annotations
from pathlib import Path

VSPLIT_DIR = ".vsplit"
WORKSPACES_DIR = "workspaces"

# Longest path the tool can produce inside a workspace:
#   <root>/.vsplit/workspaces/<slug>/preview/images/scene-XXX-XX.jpg
_DEEPEST_TAIL = "preview/images/scene-XXX-XX.jpg"


def working_state_dir(working_root: Path) -> Path:
    return working_root / VSPLIT_DIR


def workspaces_root(working_root: Path) -> Path:
    return working_state_dir(working_root) / WORKSPACES_DIR


def workspace_dir(working_root: Path, slug: str) -> Path:
    return workspaces_root(working_root) / slug


def project_path_length_estimate(working_root: Path, slug: str) -> int:
    p = workspace_dir(working_root, slug) / _DEEPEST_TAIL
    return len(str(p))
