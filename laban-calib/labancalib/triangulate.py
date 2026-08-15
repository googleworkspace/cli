"""Using a calibrated rig: multi-view 2D joints -> 3D trajectories.

This is the hand-off to the Laban-Notation analysis. Feed it the 2D joint
tracks produced by a pose estimator on each camera and it returns metric 3D
positions in the reference camera's frame, plus the reprojection error that
tells you how much to trust each point.
"""

from __future__ import annotations

import json
from dataclasses import dataclass

import numpy as np

from . import geometry
from .models import CalibrationResult, Intrinsics, Pose


@dataclass
class Rig:
    """A calibrated multi-camera rig, detached from the GUI project."""

    intrinsics: list[Intrinsics]
    poses: list[Pose]
    names: list[str]

    @property
    def n_cameras(self) -> int:
        return len(self.intrinsics)

    @classmethod
    def from_result(cls, result: CalibrationResult) -> "Rig":
        return cls(
            intrinsics=[c.intrinsics for c in result.cameras],
            poses=[c.pose for c in result.cameras],
            names=[c.name for c in result.cameras],
        )

    @classmethod
    def from_json(cls, path: str) -> "Rig":
        """Read a rig exported by :func:`labancalib.export.export_json`."""
        with open(path, encoding="utf-8") as handle:
            data = json.load(handle)
        intrinsics, poses, names = [], [], []
        for camera in data["cameras"]:
            intrinsics.append(
                Intrinsics.from_K(
                    np.asarray(camera["K"], dtype=np.float64),
                    np.asarray(camera["distortion"], dtype=np.float64),
                    tuple(camera["image_size"]),
                    camera["model"],
                )
            )
            poses.append(Pose(camera["pose"]["rvec"], camera["pose"]["tvec"]))
            names.append(camera.get("name", f"caméra {len(names)}"))
        return cls(intrinsics=intrinsics, poses=poses, names=names)

    def normalise(self, camera: int, points: np.ndarray) -> np.ndarray:
        """Pixels -> undistorted normalised coordinates for one camera."""
        intr = self.intrinsics[camera]
        return geometry.undistort_points(points, intr.K, intr.dist, intr.model)

    def triangulate_point(
        self, observations: dict[int, np.ndarray], refine: bool = True
    ) -> tuple[np.ndarray, float]:
        """Triangulate one 3D point from ``{camera index: (x, y) pixels}``.

        Returns the point in the reference frame and its RMS reprojection
        error in pixels (``nan`` when fewer than two views are available).
        """
        usable = {c: np.asarray(p, dtype=np.float64).ravel() for c, p in observations.items() if p is not None}
        usable = {c: p for c, p in usable.items() if p.size == 2 and np.all(np.isfinite(p))}
        if len(usable) < 2:
            return np.full(3, np.nan), float("nan")
        rays = [
            (self.poses[c].rvec, self.poses[c].tvec, self.normalise(c, p.reshape(1, 2))[0])
            for c, p in usable.items()
        ]
        point = geometry.triangulate(rays)
        if refine:
            point = self._refine_point(point, usable)
        return point, self.reprojection_error(point, usable)

    def _refine_point(self, point: np.ndarray, observations: dict[int, np.ndarray]) -> np.ndarray:
        """Gauss-Newton polish of the DLT solution, minimising pixel error."""
        from scipy.optimize import least_squares

        cameras = sorted(observations)

        def residual(x: np.ndarray) -> np.ndarray:
            out = []
            for camera in cameras:
                pose = self.poses[camera]
                projected = self.intrinsics[camera].project(x.reshape(1, 3), pose.rvec, pose.tvec)
                out.append(projected.ravel() - observations[camera])
            return np.concatenate(out)

        try:
            solution = least_squares(residual, point, method="lm", xtol=1e-12, ftol=1e-12)
            return np.asarray(solution.x, dtype=np.float64)
        except (ValueError, np.linalg.LinAlgError):
            return point

    def reprojection_error(self, point: np.ndarray, observations: dict[int, np.ndarray]) -> float:
        errors = []
        for camera, measured in observations.items():
            pose = self.poses[camera]
            projected = self.intrinsics[camera].project(np.asarray(point).reshape(1, 3), pose.rvec, pose.tvec)
            errors.append(np.linalg.norm(projected.ravel() - np.asarray(measured, dtype=np.float64).ravel()))
        return float(np.sqrt(np.mean(np.square(errors)))) if errors else float("nan")

    def triangulate_tracks(
        self, tracks: np.ndarray, confidence: np.ndarray | None = None, min_confidence: float = 0.0
    ) -> tuple[np.ndarray, np.ndarray]:
        """Triangulate whole 2D tracks.

        ``tracks`` has shape ``(n_cameras, n_frames, n_joints, 2)`` in pixels,
        with ``nan`` where a joint is missing. ``confidence`` (same shape minus
        the last axis) optionally gates the observations. Returns the 3D
        positions ``(n_frames, n_joints, 3)`` and the per-point reprojection
        error ``(n_frames, n_joints)``.
        """
        tracks = np.asarray(tracks, dtype=np.float64)
        if tracks.ndim != 4 or tracks.shape[0] != self.n_cameras or tracks.shape[3] != 2:
            raise ValueError(
                f"tracks doit être de forme ({self.n_cameras}, n_frames, n_joints, 2), reçu {tracks.shape}"
            )
        _, n_frames, n_joints, _ = tracks.shape
        points = np.full((n_frames, n_joints, 3), np.nan)
        errors = np.full((n_frames, n_joints), np.nan)
        for frame in range(n_frames):
            for joint in range(n_joints):
                observations = {}
                for camera in range(self.n_cameras):
                    xy = tracks[camera, frame, joint]
                    if not np.all(np.isfinite(xy)):
                        continue
                    if confidence is not None and confidence[camera, frame, joint] < min_confidence:
                        continue
                    observations[camera] = xy
                if len(observations) >= 2:
                    points[frame, joint], errors[frame, joint] = self.triangulate_point(observations)
        return points, errors
