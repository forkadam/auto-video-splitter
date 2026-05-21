from __future__ import annotations
from pathlib import Path

VSPLIT_DIR = ".vsplit"
WORKSPACES_DIR = "workspaces"

# Longest path the tool can produce inside a workspace:
#   <root>/.vsplit/workspaces/<slug>/output/<slug>-highlight-NN-<label>.mp4
# We size the synthetic filename to mirror split's output pattern (spec §2.3):
#   <slug>-highlight-NN[-label].mp4, with NN up to 99 (2 digits) and label up to 32 chars.
_OUTPUT_LABEL_MAX = 32  # generous upper bound for human-supplied scene labels


def working_state_dir(working_root: Path) -> Path:
    return working_root / VSPLIT_DIR


def workspaces_root(working_root: Path) -> Path:
    return working_state_dir(working_root) / WORKSPACES_DIR


def workspace_dir(working_root: Path, slug: str) -> Path:
    return workspaces_root(working_root) / slug


def project_path_length_estimate(working_root: Path, slug: str) -> int:
    longest_filename = f"{slug}-highlight-99-{'X' * _OUTPUT_LABEL_MAX}.mp4"
    p = workspace_dir(working_root, slug) / "output" / longest_filename
    return len(str(p))
