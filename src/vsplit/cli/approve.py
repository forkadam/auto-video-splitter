from __future__ import annotations
from datetime import datetime, timezone
from pathlib import Path
import click
import fnmatch
from rich.console import Console
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest, save_manifest
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

    now = datetime.now(timezone.utc).isoformat()
    count = 0
    for slug in targets:
        ws = workspace_dir(working_root, slug)
        m = load_manifest(ws)
        block = m.stages["review"]
        if block.status != "awaiting_user":
            continue
        block.artifacts["approved_at"] = now
        save_manifest(ws, m)
        console.print(f"[green]approved[/]: {slug}")
        count += 1
    console.print(f"\n{count} approved")
