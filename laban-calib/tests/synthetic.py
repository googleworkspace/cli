"""Synthetic rig generator used by the tests.

Builds a ground-truth multi-camera rig and projects a calibration target into
it, so the estimators can be checked against known values.
"""

from __future__ import annotations

import numpy as np

from labancalib import geometry
from labancalib.board import BoardSpec
from labancalib.bundle import Observation
from labancalib.models import FISHEYE, Intrinsics, PINHOLE, Pose


def make_rig(n_cameras: int = 3, model: str = PINHOLE, image_size=(1280, 800)) -> list[Intrinsics]:
    w, h = image_size
    cameras = []
    for i in range(n_cameras):
        if model == FISHEYE:
            intr = Intrinsics(
                model=FISHEYE,
                image_size=image_size,
                f=300.0 + 8.0 * i,
                ar=1.002,
                cx=w / 2 + 6.0 * i,
                cy=h / 2 - 4.0 * i,
                dist=np.array([0.008, -0.007, 0.009, -0.004]),
            )
        else:
            intr = Intrinsics(
                model=PINHOLE,
                image_size=image_size,
                f=900.0 + 20.0 * i,
                ar=1.001,
                cx=w / 2 + 5.0 * i,
                cy=h / 2 - 3.0 * i,
                dist=np.array([-0.12, 0.05, 0.001, -0.0008, 0.0]),
            )
        cameras.append(intr)
    return cameras


def look_at(centre: np.ndarray, target: np.ndarray) -> Pose:
    """Camera pose looking at ``target``, in a world whose y axis points down."""
    centre = np.asarray(centre, dtype=np.float64)
    forward = np.asarray(target, dtype=np.float64) - centre
    forward /= np.linalg.norm(forward)
    down = np.array([0.0, 1.0, 0.0])
    right = np.cross(down, forward)
    right /= np.linalg.norm(right)
    R = np.vstack([right, np.cross(forward, right), forward])  # world -> camera
    return Pose(geometry.inv_rodrigues(R), -(R @ centre))


def make_camera_poses(n_cameras: int, radius: float = 2.0, spacing_deg: float = 25.0) -> list[Pose]:
    """Cameras on an arc around the capture volume, all looking at its centre."""
    target = np.array([0.0, 0.0, radius])
    poses = []
    for i in range(n_cameras):
        angle = np.radians(spacing_deg * i)
        centre = target + radius * np.array([np.sin(angle), 0.08 * i, -np.cos(angle)])
        poses.append(look_at(centre, target))
    return poses


def make_board_poses(n_poses: int, seed: int = 7, distance: float = 1.8) -> list[Pose]:
    rng = np.random.default_rng(seed)
    poses = []
    for _ in range(n_poses):
        # tilted around the frontal orientation: the printed side stays towards
        # the cameras, as it does when an operator waves the target
        rvec = rng.uniform(-0.45, 0.45, 3)
        tvec = np.array([
            rng.uniform(-0.35, 0.35),
            rng.uniform(-0.30, 0.30),
            distance + rng.uniform(-0.35, 0.55),
        ])
        poses.append(Pose(rvec, tvec))
    return poses


def project_rig(
    board: BoardSpec,
    intrinsics: list[Intrinsics],
    camera_poses: list[Pose],
    board_poses: list[Pose],
    noise: float = 0.0,
    seed: int = 3,
    drop_fraction: float = 0.0,
) -> list[Observation]:
    """Project the target and keep only the points landing inside the image."""
    rng = np.random.default_rng(seed)
    points = board.object_points()
    observations: list[Observation] = []
    for pose_index, board_pose in enumerate(board_poses):
        for cam, intr in enumerate(intrinsics):
            rvec, tvec = geometry.compose(
                camera_poses[cam].rvec, camera_poses[cam].tvec, board_pose.rvec, board_pose.tvec
            )
            in_cam = geometry.rodrigues(rvec) @ points.T + np.asarray(tvec).reshape(3, 1)
            if np.any(in_cam[2] <= 0.05):
                continue
            xy = intr.project(points, rvec, tvec)
            w, h = intr.image_size
            inside = (xy[:, 0] >= 0) & (xy[:, 0] < w) & (xy[:, 1] >= 0) & (xy[:, 1] < h)
            ids = np.flatnonzero(inside)
            if drop_fraction > 0 and len(ids):
                keep = rng.random(len(ids)) >= drop_fraction
                ids = ids[keep]
            if len(ids) < 8:
                continue
            observed = xy[ids]
            if noise > 0:
                observed = observed + rng.normal(0.0, noise, observed.shape)
            observations.append(Observation(camera=cam, pose_index=pose_index, ids=ids, points=observed))
    return observations
