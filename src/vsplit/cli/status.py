from __future__ import annotations
import json
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._output import render_status_table
from vsplit.core.index import load_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

console = Console()


@click.command(name="status")
@click.argument("slug", required=False)
@click.option("--awaiting-user", is_flag=True)
@click.option("--failed", is_flag=True)
@click.pass_context
def status_cmd(ctx, slug, awaiting_user, failed):
    """Show pipeline status."""
    working_root = Path(ctx.obj["working_dir"])
    idx = load_index(working_root)
    manifests = {}
    for s in idx.workspaces:
        if slug and s != slug:
            continue
        try:
            m = load_manifest(workspace_dir(working_root, s))
        except FileNotFoundError:
            continue
        if awaiting_user and not any(b.status == "awaiting_user" for b in m.stages.values()):
            continue
        if failed and not any(b.status == "failed" for b in m.stages.values()):
            continue
        manifests[s] = m

    if ctx.obj["json_output"]:
        click.echo(json.dumps({
            "workspaces": {k: v.model_dump(mode="json") for k, v in manifests.items()},
        }, indent=2, default=str))
        return
    render_status_table(manifests, console)
