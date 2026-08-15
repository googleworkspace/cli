"""``python -m labancalib`` launches the graphical application."""

from __future__ import annotations

import sys


def main() -> int:
    try:
        from .gui import main as gui_main
    except ImportError as error:  # pragma: no cover - depends on the environment
        print(
            f"interface graphique indisponible ({error}).\n"
            "Installez les dépendances : pip install PySide6 matplotlib\n"
            "Ou utilisez la version en ligne de commande : python -m labancalib.cli --help",
            file=sys.stderr,
        )
        return 1
    return gui_main()


if __name__ == "__main__":
    sys.exit(main())
