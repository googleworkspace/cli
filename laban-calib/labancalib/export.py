"""Exporting a calibrated rig for the downstream Laban-Notation pipeline."""

from __future__ import annotations

import json
import os
from datetime import datetime, timezone

import cv2
import numpy as np

from .board import BoardSpec
from .models import CalibrationResult, DIST_NAMES, FISHEYE

EXPORT_FORMATS = ("json", "opencv", "text")


def rig_dictionary(result: CalibrationResult, board: BoardSpec, notes: str = "") -> dict:
    """Self-contained description of the rig, ready to triangulate with.

    Poses are given both ways: ``pose`` maps the reference frame into the
    camera (what a projection needs) and ``centre``/``R_world_from_camera``
    give the camera placement in the reference frame (what a viewer needs).
    """
    cameras = []
    for index, camera in enumerate(result.cameras):
        intr = camera.intrinsics
        inverse = camera.pose.inverse()
        cameras.append(
            {
                "index": index,
                "name": camera.name,
                "model": intr.model,
                "image_size": list(intr.image_size),
                "K": intr.K.tolist(),
                "distortion": [float(v) for v in intr.dist],
                "distortion_names": list(DIST_NAMES[intr.model]),
                "parameters": {k: float(v) for k, v in intr.values().items()},
                "sigmas": {k: float(v) for k, v in camera.sigmas.items()},
                "pose": {
                    "rvec": camera.pose.rvec.tolist(),
                    "tvec": camera.pose.tvec.tolist(),
                    "T_camera_from_reference": camera.pose.matrix().tolist(),
                },
                "centre": camera.pose.centre.tolist(),
                "R_reference_from_camera": inverse.R.tolist(),
                "rpe": float(camera.rpe),
                "n_views": int(camera.n_views),
                "n_points": int(camera.n_points),
            }
        )
    return {
        "format": "laban-calib/rig",
        "version": 1,
        "created": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "units": "metres, pixels, radians",
        "reference_camera": 0,
        "board": board.to_dict(),
        "board_description": board.describe(),
        "rpe": float(result.rpe),
        "converged": bool(result.converged),
        "n_observations": int(result.n_observations),
        "cameras": cameras,
        "notes": notes,
    }


def export_json(path: str, result: CalibrationResult, board: BoardSpec, notes: str = "") -> str:
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(rig_dictionary(result, board, notes), handle, ensure_ascii=False, indent=2)
    return path


def export_opencv(path: str, result: CalibrationResult, board: BoardSpec, notes: str = "") -> str:
    """Write an OpenCV ``FileStorage`` (.yml/.xml) readable by cv2.FileStorage."""
    storage = cv2.FileStorage(path, cv2.FILE_STORAGE_WRITE)
    try:
        storage.write("format", "laban-calib/rig")
        storage.write("board", board.describe())
        storage.write("square_size", float(board.square_size))
        storage.write("rpe", float(result.rpe))
        storage.write("n_cameras", int(len(result.cameras)))
        for index, camera in enumerate(result.cameras):
            prefix = f"camera_{index}"
            storage.write(f"{prefix}_name", camera.name)
            storage.write(f"{prefix}_model", camera.intrinsics.model)
            storage.write(f"{prefix}_image_size", np.array(camera.intrinsics.image_size, dtype=np.int32))
            storage.write(f"{prefix}_K", camera.intrinsics.K)
            storage.write(f"{prefix}_dist", np.asarray(camera.intrinsics.dist, dtype=np.float64).reshape(1, -1))
            storage.write(f"{prefix}_rvec", np.asarray(camera.pose.rvec, dtype=np.float64).reshape(3, 1))
            storage.write(f"{prefix}_tvec", np.asarray(camera.pose.tvec, dtype=np.float64).reshape(3, 1))
            storage.write(f"{prefix}_rpe", float(camera.rpe))
        if notes:
            storage.write("notes", notes)
    finally:
        storage.release()
    return path


def parameters_report(result: CalibrationResult, board: BoardSpec, mask=None) -> str:
    """The text shown in the « Paramètres » tab and written by ``--format text``."""
    lines: list[str] = []
    lines.append(f"Mire : {board.describe()}")
    lines.append(
        f"RPE global : {result.rpe:.6f} px    "
        f"observations : {result.n_observations}    paramètres : {result.n_parameters}"
    )
    lines.append(
        "Convergence : " + ("oui" if result.converged else "NON") + f" ({result.iterations} évaluations)"
    )
    lines.append("")
    for index, camera in enumerate(result.cameras):
        intr = camera.intrinsics
        lines.append(f"Caméra {index} — {camera.name}")
        lines.append(f"  Modèle : {intr.model}    image : {intr.image_size[0]}×{intr.image_size[1]} px")
        lines.append(f"  Paramètres intrinsèques :")
        for name, value in intr.values().items():
            if intr.model == FISHEYE and name == "alpha":
                continue
            sigma = camera.sigmas.get(name)
            state = "libre" if sigma else "fixe"
            sigma_text = f"+/- {sigma:12.8f}" if sigma else " " * 17
            lines.append(f"    {name:<6}{value:16.8f}  {sigma_text}   ({state})")
        centre = camera.pose.centre
        lines.append(
            f"  Position (repère de la caméra de référence) : "
            f"({centre[0]:+.4f}, {centre[1]:+.4f}, {centre[2]:+.4f}) m"
        )
        lines.append(f"  Distance à la référence : {np.linalg.norm(centre):.4f} m")
        lines.append(f"  Rotation (Rodrigues) : {np.array2string(camera.pose.rvec, precision=6)}")
        lines.append(f"  RPE : {camera.rpe:.6f} px sur {camera.n_points} point(s), {camera.n_views} vue(s)")
        lines.append("")
    return "\n".join(lines)


def export_text(path: str, result: CalibrationResult, board: BoardSpec, notes: str = "") -> str:
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(parameters_report(result, board))
        if notes:
            handle.write("\nNotes\n-----\n" + notes + "\n")
    return path


def export_result(
    path: str, result: CalibrationResult, board: BoardSpec, fmt: str = "", notes: str = ""
) -> str:
    """Dispatch on ``fmt`` (or on the file extension when ``fmt`` is empty)."""
    if not fmt:
        extension = os.path.splitext(path)[1].lower()
        fmt = {".json": "json", ".yml": "opencv", ".yaml": "opencv", ".xml": "opencv"}.get(extension, "text")
    if fmt == "json":
        return export_json(path, result, board, notes)
    if fmt == "opencv":
        return export_opencv(path, result, board, notes)
    if fmt == "text":
        return export_text(path, result, board, notes)
    raise ValueError(f"format d'export inconnu : {fmt}")
