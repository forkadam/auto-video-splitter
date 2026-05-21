from __future__ import annotations
import webbrowser
from pathlib import Path
import click
from rich.console import Console
from vsplit.cli.run import run_cmd
from vsplit.cli.approve import approve_cmd
from vsplit.core.adoption import adopt_or_find
from vsplit.core.paths import workspace_dir
from vsplit.schemas.manifest import STAGE_NAMES

console = Console()


@click.command(name="one")
@click.argument("file_path", type=click.Path(exists=True, dir_okay=False))
@click.option("--through", "through_stage", default=None,
              type=click.Choice(list(STAGE_NAMES)))
@click.option("--auto-approve", is_flag=True)
@click.pass_context
def one_cmd(ctx, file_path, through_stage, auto_approve):
    """Process a single file end-to-end. Does not scan the whole folder."""
    file_path = Path(file_path).resolve()
    parent = file_path.parent
    ctx.obj["working_dir"] = parent

    # Register just this one file (no folder walk, no batch adoption).
    try:
        slug = adopt_or_find(parent, file_path)
    except ValueError as e:
        raise click.ClickException(str(e)) from e
    console.print(f"[green]target[/]: {file_path.name} -> {slug}")

    target_through = through_stage or "preview"
    ctx.invoke(run_cmd, slug_glob=slug, through_stage=target_through)

    preview_html = workspace_dir(parent, slug) / "preview" / "index.html"
    if preview_html.exists():
        webbrowser.open(preview_html.as_uri())
        console.print(f"opened {preview_html}")

    if auto_approve:
        ctx.invoke(run_cmd, slug_glob=slug, through_stage="review")
        ctx.invoke(approve_cmd, slug_glob=slug)
        ctx.invoke(run_cmd, slug_glob=slug)
