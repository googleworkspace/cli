"""Qt front-end (PySide6). Importing this package requires PySide6 and matplotlib."""

from __future__ import annotations

import sys


def main(argv: list[str] | None = None) -> int:
    """Launch the application; an optional argument opens a project file."""
    from PySide6.QtWidgets import QApplication

    from ..project import Project
    from .main_window import MainWindow

    argv = list(sys.argv if argv is None else argv)
    app = QApplication(argv)
    app.setApplicationName("Calib Laban")

    project = None
    if len(argv) > 1 and argv[1].endswith(".lcalib"):
        project = Project.load(argv[1])

    window = MainWindow(project)
    window.show()
    return app.exec()


__all__ = ["main"]
