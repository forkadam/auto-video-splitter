from __future__ import annotations
import shutil
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._filter import parse_filter, apply_filters, Predicate
from vsplit.core.index import load_index, save_index
from vsplit.core.manifest import load_manifest
from vsplit.core.paths import workspace_dir

console = Console()


@click.command(name="clean")
@click.argument("slug_glob", required=False)
@click.option("--all", "clean_all", is_flag=True)
@click.option("--filter", "filters", multiple=True)
@click.option("--force", is_flag=True)
@click.pass_context
def clean_cmd(ctx, slug_glob, clean_all, filters, force):
    """Delete workspaces entirely. Source files are never touched."""
    working_root = Path(ctx.obj["working_dir"])
    idx = load_index(working_root)
    slugs = list(idx.workspaces.keys())
    if clean_all:
        targets = slugs
    else:
        preds: list[Predicate] = []
        if slug_glob:
            preds.append(Predicate(key="slug", value=slug_glob))
        for f in filters:
            preds.append(parse_filter(f))

        def has_status(slug, val):
            m = load_manifest(workspace_dir(working_root, slug))
            return any(b.status == val for b in m.stages.values())
        targets = apply_filters(slugs, preds,
                                status_pred=has_status, stage_pred=lambda *a: True)
    if not targets:
        console.print("nothing to clean")
        return
    if not force and not click.confirm(
        f"delete {len(targets)} workspace(s)?", default=False
    ):
        return
    for slug in targets:
        ws = workspace_dir(working_root, slug)
        shutil.rmtree(ws, ignore_errors=True)
        idx.workspaces.pop(slug, None)
        if slug not in idx.retired_slugs:
            idx.retired_slugs.append(slug)
        console.print(f"[green]cleaned[/]: {slug}")
    save_index(working_root, idx)
