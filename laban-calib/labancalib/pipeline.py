"""Orchestration: detection and calibration runs, with progress and logging.

The GUI and the CLI both drive these two functions, so the two front-ends can
never drift apart.
"""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

from .board import BoardDetector, Detection
from .bundle import BundleOptions, bundle_adjust, initial_extrinsics
from .intrinsics import CalibrationError, calibrate_intrinsics
from .models import CalibrationResult, Intrinsics
from .project import Project
from .sources import read_image


def _noop(*_args, **_kwargs) -> None:
    return None


@dataclass
class DetectionSummary:
    """What the detection pass found, per camera."""

    per_camera: dict[int, int] = field(default_factory=dict)
    per_camera_points: dict[int, int] = field(default_factory=dict)
    missed: list[tuple[int, int]] = field(default_factory=list)
    unreadable: list[str] = field(default_factory=list)
    image_sizes: dict[int, tuple[int, int]] = field(default_factory=dict)

    @property
    def total(self) -> int:
        return sum(self.per_camera.values())

    def text(self) -> str:
        lines = [f"{self.total} vue(s) détectée(s)"]
        for camera in sorted(self.per_camera):
            lines.append(
                f"    caméra {camera} : {self.per_camera[camera]} vue(s), "
                f"{self.per_camera_points.get(camera, 0)} point(s)"
            )
        if self.missed:
            lines.append(f"    {len(self.missed)} image(s) sans mire détectée")
        if self.unreadable:
            lines.append(f"    {len(self.unreadable)} image(s) illisible(s)")
        return "\n".join(lines)


def run_detection(
    project: Project, progress=None, log=None, should_stop=None
) -> DetectionSummary:
    """Detect the target in every image of the project.

    Existing detections are replaced. Returns a per-camera summary; images
    where the target is not seen are simply left out (that is normal — a
    hand-held target is rarely visible from every camera at once).
    """
    progress = progress or _noop
    log = log or _noop
    detector = BoardDetector(project.board, project.detector_options)
    summary = DetectionSummary()
    project.clear_detections()

    total = sum(len(camera.images) for camera in project.cameras)
    done = 0
    for camera_index, camera in enumerate(project.cameras):
        found = points = 0
        for pose_index, path in enumerate(camera.images):
            if should_stop is not None and should_stop():
                log("détection interrompue")
                return summary
            try:
                image = read_image(path, grayscale=True)
            except OSError:
                summary.unreadable.append(path)
                done += 1
                continue
            summary.image_sizes.setdefault(camera_index, (image.shape[1], image.shape[0]))
            detection = detector.detect(image)
            if detection is None:
                summary.missed.append((camera_index, pose_index))
            else:
                project.detections[(camera_index, pose_index)] = detection
                found += 1
                points += detection.count
            done += 1
            if total:
                progress(done / total)
        summary.per_camera[camera_index] = found
        summary.per_camera_points[camera_index] = points
        log(f"caméra {camera_index} ({camera.name}) : {found}/{len(camera.images)} vue(s), {points} point(s)")
    return summary


def camera_image_sizes(project: Project) -> dict[int, tuple[int, int]]:
    """Image size per camera, read from the first readable image."""
    sizes: dict[int, tuple[int, int]] = {}
    for index, camera in enumerate(project.cameras):
        for path in camera.images:
            try:
                image = read_image(path, grayscale=True)
            except OSError:
                continue
            sizes[index] = (int(image.shape[1]), int(image.shape[0]))
            break
    return sizes


def run_calibration(
    project: Project,
    options: BundleOptions | None = None,
    progress=None,
    log=None,
    image_sizes: dict[int, tuple[int, int]] | None = None,
) -> CalibrationResult:
    """Full solve: per-camera intrinsics, rig initialisation, bundle adjustment."""
    progress = progress or _noop
    log = log or _noop
    options = options or project.bundle_options
    observations = project.observations()
    if not observations:
        raise CalibrationError("aucune détection : lancez d'abord « Détecter la mire »")
    sizes = image_sizes or camera_image_sizes(project)

    log("Étape 1/3 — intrinsèques par caméra")
    intrinsics: list[Intrinsics] = []
    for camera in range(project.n_cameras):
        detections = project.detections_for(camera)
        size = sizes.get(camera)
        if size is None:
            raise CalibrationError(f"taille d'image inconnue pour la caméra {camera}")
        if len(detections) < 3:
            raise CalibrationError(
                f"caméra {camera} : {len(detections)} vue(s) détectée(s), 3 minimum"
            )
        intr, _, rms, sigmas = calibrate_intrinsics(
            project.board, detections, size, project.camera_model, project.parameter_mask
        )
        intrinsics.append(intr)
        log(
            f"    caméra {camera} : rpe {rms:.4f} px, f {intr.f:.2f}, "
            f"c ({intr.cx:.1f}, {intr.cy:.1f}), {len(detections)} vue(s)"
        )
        progress(0.35 * (camera + 1) / max(1, project.n_cameras))

    log("Étape 2/3 — initialisation du dispositif")
    camera_poses, board_poses, _ = initial_extrinsics(
        project.board, intrinsics, observations, options.reference_camera, log=lambda m: log("    " + m)
    )
    for index, pose in enumerate(camera_poses):
        centre = pose.centre
        log(
            f"    caméra {index} : centre ({centre[0]:+.3f}, {centre[1]:+.3f}, {centre[2]:+.3f}) m, "
            f"base {np.linalg.norm(centre):.3f} m"
        )
    progress(0.45)

    log("Étape 3/3 — ajustement de faisceaux")
    result = bundle_adjust(
        project.board,
        intrinsics,
        camera_poses,
        board_poses,
        observations,
        project.parameter_mask,
        options,
        log=lambda m: log("    " + m),
    )
    for index, camera in enumerate(result.cameras):
        if index < len(project.cameras):
            camera.name = project.cameras[index].name
    progress(1.0)
    log(
        f"RPE global {result.rpe:.4f} px sur {result.n_observations} point(s), "
        f"{result.n_parameters} paramètre(s), {result.iterations} évaluation(s) — "
        + ("convergé" if result.converged else "NON convergé")
    )
    project.result = result
    return result


def reprojection_for_view(
    project: Project, camera: int, pose_index: int
) -> tuple[Detection | None, np.ndarray | None]:
    """Detection and reprojected points for one view, for the image overlay."""
    detection = project.detection_at(camera, pose_index)
    result = project.result
    if detection is None or result is None or camera >= len(result.cameras):
        return detection, None
    board_pose = result.board_poses.get(pose_index)
    if board_pose is None:
        return detection, None
    calibration = result.cameras[camera]
    pose = calibration.pose.compose(board_pose)
    points = project.board.object_points()[np.asarray(detection.ids, dtype=int)]
    return detection, calibration.intrinsics.project(points, pose.rvec, pose.tvec)
