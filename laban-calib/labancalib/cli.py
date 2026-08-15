"""Headless entry point — same engine as the GUI, for batch and cluster runs.

    python -m labancalib.cli board --kind charuco --out mire.pdf
    python -m labancalib.cli calibrate cam0/ cam1/ cam2/ --model fisheye --out rig.json
"""

from __future__ import annotations

import argparse
import os
import sys

from .board import ARUCO_DICTIONARIES, BOARD_KINDS, BoardSpec, render_board, save_board_pdf
from .bundle import BundleOptions
from .export import EXPORT_FORMATS, export_result, parameters_report
from .models import CAMERA_MODELS, PINHOLE
from .pipeline import run_calibration, run_detection
from .project import Project
from .sources import VideoExtractOptions, extract_frames, video_info


def _board_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--kind", choices=BOARD_KINDS, default="charuco", help="type de mire")
    parser.add_argument("--cols", type=int, default=8, help="colonnes (cases, ou cercles par rangée)")
    parser.add_argument("--rows", type=int, default=6, help="rangées")
    parser.add_argument("--square", type=float, default=30.0, help="côté de case / pas des cercles, en mm")
    parser.add_argument("--marker", type=float, default=22.0, help="côté du marqueur ArUco, en mm")
    parser.add_argument("--dict", dest="dictionary", choices=ARUCO_DICTIONARIES, default="DICT_5X5_100")


def _board_from_args(args) -> BoardSpec:
    return BoardSpec(
        kind=args.kind,
        cols=args.cols,
        rows=args.rows,
        square_size=args.square / 1000.0,
        marker_size=args.marker / 1000.0,
        dictionary=args.dictionary,
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="labancalib",
        description="Étalonnage multi-caméras pour la captation de mouvement (Laban-Notation).",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    board = subparsers.add_parser("board", help="générer une mire imprimable")
    _board_arguments(board)
    board.add_argument("--out", required=True, help="fichier .pdf ou .png à écrire")
    board.add_argument("--page", default="A4", choices=["A4", "A3", "LETTER"])

    frames = subparsers.add_parser("frames", help="extraire des images d'une vidéo")
    frames.add_argument("video")
    frames.add_argument("--out", required=True, help="dossier de destination")
    frames.add_argument("--stride", type=int, default=15)
    frames.add_argument("--max-frames", type=int, default=60)
    frames.add_argument("--min-sharpness", type=float, default=0.0)

    calibrate = subparsers.add_parser("calibrate", help="étalonner un dispositif multi-caméras")
    calibrate.add_argument("folders", nargs="+", help="un dossier d'images par caméra")
    _board_arguments(calibrate)
    calibrate.add_argument("--model", choices=CAMERA_MODELS, default=PINHOLE)
    calibrate.add_argument("--out", help="fichier de sortie (.json, .yml ou .txt)")
    calibrate.add_argument("--format", dest="fmt", choices=EXPORT_FORMATS, default="")
    calibrate.add_argument("--project", help="enregistrer aussi le projet (.lcalib)")
    calibrate.add_argument("--reference", type=int, default=0, help="indice de la caméra de référence")
    calibrate.add_argument("--reject-sigma", type=float, default=0.0, help="rejet des points aberrants (0 = désactivé)")
    calibrate.add_argument("--no-robust", action="store_true", help="désactiver la perte de Huber")
    calibrate.add_argument("--max-iterations", type=int, default=150)
    calibrate.add_argument("--quiet", action="store_true")

    report = subparsers.add_parser("report", help="afficher le rapport d'un projet enregistré")
    report.add_argument("project")
    return parser


def _run_board(args) -> int:
    spec = _board_from_args(args)
    if args.out.lower().endswith(".pdf"):
        save_board_pdf(spec, args.out, args.page)
    else:
        import cv2

        cv2.imwrite(args.out, render_board(spec))
    print(f"{spec.describe()} -> {args.out}")
    return 0


def _run_frames(args) -> int:
    info = video_info(args.video)
    written = extract_frames(
        args.video,
        args.out,
        VideoExtractOptions(
            stride=args.stride, max_frames=args.max_frames, min_sharpness=args.min_sharpness
        ),
    )
    print(f"{info['frames']} image(s) dans la vidéo, {len(written)} extraite(s) vers {args.out}")
    return 0 if written else 1


def _run_calibrate(args) -> int:
    project = Project(board=_board_from_args(args), camera_model=args.model)
    project.set_camera_folders(args.folders)
    for issue in project.validate():
        print(f"attention : {issue}", file=sys.stderr)
    log = (lambda message: None) if args.quiet else print

    log(f"Mire : {project.board.describe()}")
    summary = run_detection(project, log=log)
    if summary.total == 0:
        print("aucune mire détectée", file=sys.stderr)
        return 2

    options = BundleOptions(
        max_iterations=args.max_iterations,
        robust=not args.no_robust,
        reject_sigma=args.reject_sigma,
        reference_camera=args.reference,
    )
    result = run_calibration(project, options, log=log)
    print()
    print(parameters_report(result, project.board))
    if args.out:
        export_result(args.out, result, project.board, args.fmt)
        print(f"exporté vers {args.out}")
    if args.project:
        project.save(args.project)
        print(f"projet enregistré vers {args.project}")
    return 0 if result.converged else 1


def _run_report(args) -> int:
    project = Project.load(args.project)
    if project.result is None:
        print("ce projet ne contient pas de résultat d'étalonnage", file=sys.stderr)
        return 2
    print(parameters_report(project.result, project.board))
    return 0


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    handlers = {
        "board": _run_board,
        "frames": _run_frames,
        "calibrate": _run_calibrate,
        "report": _run_report,
    }
    try:
        return handlers[args.command](args)
    except (OSError, ValueError, RuntimeError) as error:
        print(f"erreur : {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
