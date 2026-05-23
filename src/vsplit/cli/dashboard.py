from __future__ import annotations
from pathlib import Path
import click
from vsplit.dashboard.server import run_server


@click.command(name="dashboard")
@click.option("--port", default=8765, type=int)
@click.option("--host", default="127.0.0.1")
@click.option("--no-open", "no_open", is_flag=True, help="Skip browser auto-open.")
@click.pass_context
def dashboard_cmd(ctx, port: int, host: str, no_open: bool) -> None:
    """Launch the local-only web dashboard."""
    working_root = Path(ctx.obj["working_dir"])
    run_server(working_root, host=host, port=port, open_browser=not no_open)
