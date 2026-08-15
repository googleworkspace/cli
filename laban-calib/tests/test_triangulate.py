"""Using a calibrated rig to reconstruct 3D points — the Laban hand-off."""

from __future__ import annotations

import numpy as np
import pytest

from labancalib.board import BoardSpec
from labancalib.export import export_json
from labancalib.models import CalibrationResult, CameraCalibration, FISHEYE, PINHOLE
from labancalib.triangulate import Rig

from .synthetic import make_camera_poses, make_rig


def _rig(model=PINHOLE, n_cameras=3) -> Rig:
    intrinsics = make_rig(n_cameras, model, (640, 480) if model == FISHEYE else (1280, 800))
    poses = make_camera_poses(n_cameras, radius=1.8, spacing_deg=30)
    return Rig(intrinsics=intrinsics, poses=poses, names=[f"caméra {i}" for i in range(n_cameras)])


def _observe(rig: Rig, point: np.ndarray, noise: float = 0.0, seed: int = 0) -> dict[int, np.ndarray]:
    rng = np.random.default_rng(seed)
    observations = {}
    for camera in range(rig.n_cameras):
        pose = rig.poses[camera]
        xy = rig.intrinsics[camera].project(point.reshape(1, 3), pose.rvec, pose.tvec).ravel()
        observations[camera] = xy + rng.normal(0.0, noise, 2) if noise else xy
    return observations


@pytest.mark.parametrize("model", [PINHOLE, FISHEYE])
def test_exact_observations_triangulate_exactly(model):
    rig = _rig(model)
    target = np.array([0.15, -0.10, 1.60])
    point, error = rig.triangulate_point(_observe(rig, target))
    assert np.linalg.norm(point - target) < 1e-6
    assert error < 1e-5


def test_pixel_noise_stays_sub_millimetre():
    rig = _rig()
    target = np.array([0.05, 0.20, 1.70])
    point, error = rig.triangulate_point(_observe(rig, target, noise=0.3, seed=5))
    assert np.linalg.norm(point - target) < 0.003
    assert error < 1.0


def test_a_single_view_cannot_be_triangulated():
    rig = _rig()
    point, error = rig.triangulate_point({0: np.array([320.0, 240.0])})
    assert np.all(np.isnan(point)) and np.isnan(error)


def test_tracks_shape_and_missing_joints():
    rig = _rig()
    targets = np.array([[0.0, 0.0, 1.5], [0.2, -0.1, 1.8]])
    tracks = np.full((rig.n_cameras, 2, len(targets), 2), np.nan)
    for frame in range(2):
        for joint, target in enumerate(targets):
            for camera, xy in _observe(rig, target).items():
                tracks[camera, frame, joint] = xy
    tracks[1:, 0, 1] = np.nan  # joint seen by a single camera in frame 0

    points, errors = rig.triangulate_tracks(tracks)
    assert points.shape == (2, len(targets), 3)
    assert np.all(np.isnan(points[0, 1]))
    assert np.linalg.norm(points[1, 1] - targets[1]) < 1e-6
    assert errors[1, 0] < 1e-5


def test_tracks_reject_a_wrong_shape():
    rig = _rig()
    with pytest.raises(ValueError, match="tracks"):
        rig.triangulate_tracks(np.zeros((rig.n_cameras, 4, 3)))


def test_rig_round_trips_through_the_exported_json(tmp_path):
    rig = _rig()
    result = CalibrationResult(
        cameras=[
            CameraCalibration(name=name, intrinsics=intr, pose=pose)
            for name, intr, pose in zip(rig.names, rig.intrinsics, rig.poses)
        ],
        rpe=0.2,
        converged=True,
    )
    path = tmp_path / "rig.json"
    export_json(str(path), result, BoardSpec("charuco", 8, 6, 0.040, 0.030))

    reloaded = Rig.from_json(str(path))
    target = np.array([-0.08, 0.12, 1.55])
    point, _ = reloaded.triangulate_point(_observe(rig, target))
    assert np.linalg.norm(point - target) < 1e-6
