import click


@click.group()
@click.version_option()
def main() -> None:
    """vsplit — local scene-aware video splitter."""


if __name__ == "__main__":
    main()
