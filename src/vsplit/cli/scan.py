from __future__ import annotations
from datetime import datetime, timezone
from pathlib import Path
import click
from rich.console import Console

from vsplit.core.config import load_resolved_config
from vsplit.core.hash import partial_hash
from vsplit.core.index import load_index, save_index
from vsplit.core.manifest import (
    load_manifest, save_manifest, new_manifest, manifest_path
)
from vsplit.core.paths import workspace_dir, working_state_dir, project_path_length_estimate
from vsplit.core.slugify import slugify, resolve_collision
from vsplit.schemas.index import WorkspaceEntry
from vsplit.schemas.manifest import SourceInfo

console = Console()


@click.command(name="scan")
@click.argument("path", required=False, type=click.Path(exists=True, file_okay=False))
@click.option("--recursive/--no-recursive", default=None)
@click.option("--dry-run", is_flag=True)
@click.option("--auto-relocate", is_flag=True, help="Update pointer without prompting on rename.")
@click.pass_context
def scan_cmd(ctx, path, recursive, dry_run, auto_relocate):
    """Discover videos and adopt new ones into workspaces."""
    working_root = Path(path) if path else Path(ctx.obj["working_dir"])
    config = load_resolved_config(
        working_root=working_root,
        explicit_path=ctx.obj.get("config_path"),
    )
    recursive_eff = config.scan.recursive if recursive is None else recursive
    extensions = {e.lower().lstrip(".") for e in config.scan.extensions}

    idx = load_index(working_root)
    live_slugs = set(idx.workspaces.keys())
    retired_slugs = set(idx.retired_slugs)
    by_hash = {w.partial_hash: slug for slug, w in idx.workspaces.items()}
    by_path = {w.source_relative_path: slug for slug, w in idx.workspaces.items()}

    iterator = (working_root.rglob("*") if recursive_eff else working_root.iterdir())
    candidates = []
    for p in iterator:
        if not p.is_file():
            continue
        if p.suffix.lower().lstrip(".") not in extensions:
            continue
        # Skip anything inside .vsplit
        try:
            rel = p.relative_to(working_root)
        except ValueError:
            continue
        if rel.parts and rel.parts[0] == ".vsplit":
            continue
        candidates.append(p)

    adopted = 0
    for p in sorted(candidates):
        rel = str(p.relative_to(working_root))
        stat = p.stat()
        ph = partial_hash(p)
        if ph in by_hash:
            existing_slug = by_hash[ph]
            entry = idx.workspaces[existing_slug]
            if entry.source_relative_path != rel:
                if not (auto_relocate or click.confirm(
                    f"Rename detected: {entry.source_relative_path} -> {rel}. Update pointer?",
                    default=False,
                )):
                    continue
                if not dry_run:
                    entry.source_relative_path = rel
                    ws = workspace_dir(working_root, existing_slug)
                    m = load_manifest(ws)
                    m.source.relative_path = rel
                    save_manifest(ws, m)
            continue
        if rel in by_path:
            console.print(
                f"[yellow]warning[/]: source {rel} replaced (hash changed); "
                f"existing workspace state may be stale"
            )
            continue
        base = slugify(p.name)
        if not base:
            console.print(f"[yellow]skip[/]: cannot slugify {rel}")
            continue
        new_slug = resolve_collision(base, live_slugs, retired_slugs)
        live_slugs.add(new_slug)
        if project_path_length_estimate(working_root, new_slug) > 240:
            console.print(
                f"[yellow]warning[/]: path length warning — projected path for {new_slug} "
                f"exceeds 240 chars; consider a shorter working folder or filename"
            )
        if dry_run:
            console.print(f"[blue]would adopt[/]: {rel} -> {new_slug}")
            continue
        ws = workspace_dir(working_root, new_slug)
        ws.mkdir(parents=True, exist_ok=True)
        source = SourceInfo(
            relative_path=rel,
            size_bytes=stat.st_size,
            mtime=datetime.fromtimestamp(stat.st_mtime, tz=timezone.utc),
            partial_hash=ph,
        )
        m = new_manifest(slug=new_slug, source=source)
        save_manifest(ws, m)
        idx.workspaces[new_slug] = WorkspaceEntry(
            source_relative_path=rel,
            size_bytes=stat.st_size,
            partial_hash=ph,
            adopted_at=datetime.now(timezone.utc),
        )
        adopted += 1
        console.print(f"[green]adopted[/]: {rel} -> {new_slug}")

    if not dry_run:
        working_state_dir(working_root).mkdir(parents=True, exist_ok=True)
        save_index(working_root, idx)
    console.print(f"\n{adopted} adopted, {len(idx.workspaces)} total")
