"""Headless entry point — same engine as the GUI, for batch and cluster runs.

    python -m labancalib.cli board --kind charuco --out mire.pdf
    python -m labancalib.cli calibrate cam0/ cam1/ cam2/ --model fisheye --out rig.json
"""

from __future__ import annotations

import argparse
import os
import sys

import numpy as np

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

    pose = subparsers.add_parser("pose", help="extraire les points MediaPipe d'une vidéo")
    pose.add_argument("video")
    pose.add_argument("--out", required=True, help="fichier .json, .npy ou .npz à écrire")
    pose.add_argument("--model", help="modèle .task de MediaPipe (voir --help pour l'URL)")
    pose.add_argument("--min-confidence", type=float, default=0.5)

    triangulate = subparsers.add_parser(
        "triangulate", help="reconstruire des points MediaPipe en 3D sur un dispositif étalonné"
    )
    triangulate.add_argument("rig", help="dispositif exporté (JSON)")
    triangulate.add_argument("landmarks", nargs="+", help="un fichier de points par caméra, dans l'ordre du dispositif")
    triangulate.add_argument("--out", help="fichier .npz ou .csv à écrire")
    triangulate.add_argument("--min-visibility", type=float, default=0.5)
    triangulate.add_argument("--max-error", type=float, default=0.0, help="rejet au-delà de N px de reprojection")
    triangulate.add_argument("--person", type=int, default=0, help="indice de la personne suivie")
    triangulate.add_argument(
        "--offsets", type=int, nargs="+", help="décalage en trames par caméra (démarrage tardif)"
    )
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


def _run_pose(args) -> int:
    from .mediapipe_io import estimate_from_video, save_landmarks

    landmarks = estimate_from_video(
        args.video,
        model_path=args.model,
        min_detection_confidence=args.min_confidence,
        progress=lambda value: None,
    )
    detected = int(np.isfinite(landmarks[:, :, 0]).any(axis=1).sum())
    save_landmarks(args.out, landmarks, source=args.video)
    print(f"{len(landmarks)} trame(s), {detected} avec une personne détectée -> {args.out}")
    return 0 if detected else 2


def _run_triangulate(args) -> int:
    from .mediapipe_io import triangulate_mediapipe
    from .triangulate import Rig

    rig = Rig.from_json(args.rig)
    reconstruction = triangulate_mediapipe(
        rig,
        args.landmarks,
        min_visibility=args.min_visibility,
        offsets=args.offsets,
        person=args.person,
        max_error=args.max_error,
    )
    errors = reconstruction.errors[np.isfinite(reconstruction.errors)]
    print(
        f"{reconstruction.n_frames} trame(s), "
        f"{reconstruction.completeness() * 100:.1f} % des points reconstruits, "
        f"erreur de reprojection médiane {np.median(errors) if errors.size else float('nan'):.3f} px"
    )
    print("\nLongueurs de segments (médianes, m) — un écart-type élevé trahit un problème :")
    for name, length in sorted(reconstruction.segment_lengths().items()):
        print(f"    {name:<40} {length:.4f}")
    if args.out:
        reconstruction.save(args.out)
        print(f"\nécrit vers {args.out}")
    return 0


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    handlers = {
        "board": _run_board,
        "frames": _run_frames,
        "calibrate": _run_calibrate,
        "report": _run_report,
        "pose": _run_pose,
        "triangulate": _run_triangulate,
    }
    try:
        return handlers[args.command](args)
    except (OSError, ValueError, RuntimeError) as error:
        print(f"erreur : {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
