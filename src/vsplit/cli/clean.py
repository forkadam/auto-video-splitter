from __future__ import annotations
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli._filter import parse_filter, apply_filters, Predicate
from vsplit.core.actions import clean_workspaces
from vsplit.core.index import load_index
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
    removed = clean_workspaces(working_root, targets)
    # A target still in the index after cleaning couldn't be deleted (a file is
    # held open). Report it honestly instead of claiming it was cleaned.
    still_present = set(load_index(working_root).workspaces)
    for slug in targets:
        if slug in still_present:
            console.print(f"[yellow]kept[/] (in use — not deleted): {slug}")
        else:
            console.print(f"[green]cleaned[/]: {slug}")
    console.print(f"\n{removed} cleaned")
