from __future__ import annotations
from pathlib import Path
import click
import fnmatch
from rich.console import Console
from vsplit.core.actions import approve_workspaces
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

console = Console()


@click.command(name="approve")
@click.argument("slug_glob", required=False)
@click.option("--filter", "filters", multiple=True)
@click.pass_context
def approve_cmd(ctx, slug_glob, filters):
    """Mark workspaces awaiting user review as approved."""
    working_root = Path(ctx.obj["working_dir"])
    idx = load_index(working_root)
    targets = list(idx.workspaces.keys())
    if slug_glob:
        targets = [s for s in targets if fnmatch.fnmatch(s, slug_glob)]
    for f in filters:
        if "=" not in f:
            raise click.ClickException(f"bad filter: {f}")
        k, v = f.split("=", 1)
        if k == "status":
            targets = [
                s for s in targets
                if any(b.status == v for b in load_manifest(workspace_dir(working_root, s)).stages.values())
            ]
        elif k == "slug":
            targets = [s for s in targets if fnmatch.fnmatch(s, v)]

    approved = approve_workspaces(working_root, targets)
    for slug in targets:
        m = load_manifest(workspace_dir(working_root, slug))
        if m.stages["review"].artifacts.get("approved_at"):
            console.print(f"[green]approved[/]: {slug}")
    console.print(f"\n{approved} approved")
