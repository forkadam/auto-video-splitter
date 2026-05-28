from __future__ import annotations
from pathlib import Path
import click
from rich.console import Console
from vsplit.core.collect import collect_outputs
from vsplit.core.config import load_resolved_config

console = Console()


@click.command(name="collect")
@click.argument("slugs", nargs=-1)
@click.option("--to", "dest", type=click.Path(file_okay=False), default=None,
              help="Destination directory. Defaults to config.collect.destination "
                   "(relative paths resolve under the working root).")
@click.option("--mode", type=click.Choice(["copy", "move", "link"]), default=None,
              help="copy (safe default) / move (removes source) / link "
                   "(hardlink, falls back to symlink across volumes).")
@click.option("--force", is_flag=True,
              help="Overwrite existing files at the destination.")
@click.pass_context
def collect_cmd(ctx, slugs, dest, mode, force):
    """Gather every split=done workspace's output clips into one folder.

    With no SLUGS, collects from every workspace whose split stage finished.
    Files are renamed nowhere — output names already include the workspace
    slug so they're globally unique. Re-running is safe: existing files are
    skipped unless --force.
    """
    working_root = Path(ctx.obj["working_dir"])
    cfg = load_resolved_config(working_root=working_root, explicit_path=ctx.obj["config_path"])

    if dest is None:
        configured = Path(cfg.collect.destination)
        dest_path = configured if configured.is_absolute() else (working_root / configured)
    else:
        dest_path = Path(dest)
    if mode is None:
        mode = cfg.collect.mode

    result = collect_outputs(
        working_root,
        slugs=list(slugs) if slugs else None,
        dest=dest_path,
        mode=mode,
        force=force,
    )

    console.print(f"destination: [bold]{dest_path}[/]")
    console.print(f"mode: {mode}")
    console.print(
        f"[green]{result['collected']}[/] file(s) from "
        f"{result['from_workspaces']} workspace(s)"
    )
    if result["skipped_not_done"]:
        console.print(f"[yellow]skipped[/] {result['skipped_not_done']} workspace(s): split not done")
    if result["skipped_no_output"]:
        console.print(f"[yellow]skipped[/] {result['skipped_no_output']} workspace(s): no output files")
    if result["skipped_existing"]:
        console.print(
            f"[yellow]skipped[/] {result['skipped_existing']} file(s) already at destination "
            f"(re-run with --force to overwrite)"
        )
