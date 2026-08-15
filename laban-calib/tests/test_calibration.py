"""Calibration accuracy against a synthetic rig with known ground truth."""

from __future__ import annotations

import numpy as np
import pytest

from labancalib.board import BoardSpec, Detection
from labancalib.bundle import BundleOptions, bundle_adjust, initial_extrinsics
from labancalib.intrinsics import CalibrationError, calibrate_intrinsics, solve_pose
from labancalib.models import FISHEYE, ParameterMask, PINHOLE

from .synthetic import make_board_poses, make_camera_poses, make_rig, project_rig

BOARD = BoardSpec("charuco", 9, 7, 0.040, 0.030)


def _detections(observations, camera):
    return {o.pose_index: Detection(o.ids, o.points) for o in observations if o.camera == camera}


def _solve(model, n_cameras, noise, **kwargs):
    truth_intrinsics = make_rig(n_cameras, model, (640, 480) if model == FISHEYE else (1280, 800))
    truth_cameras = make_camera_poses(n_cameras, radius=1.6, spacing_deg=25)
    truth_boards = make_board_poses(kwargs.get("n_poses", 24), distance=1.4)
    observations = project_rig(
        BOARD, truth_intrinsics, truth_cameras, truth_boards, noise=noise, drop_fraction=kwargs.get("drop", 0.0)
    )
    estimated = [
        calibrate_intrinsics(
            BOARD, _detections(observations, c), truth_intrinsics[c].image_size, model
        )[0]
        for c in range(n_cameras)
    ]
    camera_poses, board_poses, _ = initial_extrinsics(BOARD, estimated, observations)
    result = bundle_adjust(
        BOARD,
        estimated,
        camera_poses,
        board_poses,
        observations,
        ParameterMask(),
        BundleOptions(robust=False),
    )
    return result, truth_intrinsics, truth_cameras, observations


@pytest.mark.parametrize("model", [PINHOLE, FISHEYE])
def test_noise_free_rig_is_recovered_almost_exactly(model):
    result, truth_intrinsics, truth_cameras, _ = _solve(model, 3, noise=0.0)
    assert result.rpe < 0.01
    for estimated, truth in zip(result.cameras, truth_intrinsics):
        assert estimated.intrinsics.f == pytest.approx(truth.f, rel=2e-3)
        assert estimated.intrinsics.cx == pytest.approx(truth.cx, abs=1.5)
        assert estimated.intrinsics.cy == pytest.approx(truth.cy, abs=1.5)
    for estimated, truth in zip(result.cameras, truth_cameras):
        # sub-centimetre placement over a ~1.5 m baseline
        assert np.linalg.norm(estimated.pose.centre - truth.centre) < 0.01


def test_reprojection_error_matches_the_injected_noise():
    """With sigma px of noise per axis, the RPE must land near sigma*sqrt(2)."""
    sigma = 0.15
    result, _, truth_cameras, _ = _solve(PINHOLE, 3, noise=sigma, drop=0.1)
    assert result.rpe == pytest.approx(sigma * np.sqrt(2), rel=0.15)
    assert result.converged
    for estimated, truth in zip(result.cameras, truth_cameras):
        assert np.linalg.norm(estimated.pose.centre - truth.centre) < 0.02


def test_sigmas_are_reported_for_every_free_parameter():
    result, _, _, _ = _solve(PINHOLE, 2, noise=0.2)
    for camera in result.cameras:
        assert set(camera.sigmas) == set(ParameterMask().free_names(PINHOLE))
        assert all(value > 0 for value in camera.sigmas.values())
        assert camera.sigmas["f"] < 5.0  # a well-conditioned problem


def test_view_residuals_cover_every_observation():
    result, _, _, observations = _solve(PINHOLE, 3, noise=0.1)
    assert len(result.residuals) == len(observations)
    assert result.n_observations == sum(o.count for o in observations)
    assert result.errors_xy.shape == (result.n_observations, 2)


def test_outlier_rejection_removes_a_corrupted_point():
    truth_intrinsics = make_rig(2, PINHOLE)
    truth_cameras = make_camera_poses(2, radius=1.6, spacing_deg=25)
    truth_boards = make_board_poses(20, distance=1.4)
    observations = project_rig(BOARD, truth_intrinsics, truth_cameras, truth_boards, noise=0.05)
    observations[0].points[0] += 40.0  # a mis-detected corner

    estimated = [
        calibrate_intrinsics(BOARD, _detections(observations, c), truth_intrinsics[c].image_size, PINHOLE)[0]
        for c in range(2)
    ]
    camera_poses, board_poses, _ = initial_extrinsics(BOARD, estimated, observations)
    total = sum(o.count for o in observations)
    cleaned = bundle_adjust(
        BOARD,
        estimated,
        camera_poses,
        board_poses,
        observations,
        ParameterMask(),
        BundleOptions(robust=False, reject_sigma=4.0),
    )
    assert cleaned.n_observations < total
    assert cleaned.rpe < 0.2


def test_disconnected_cameras_are_reported_clearly():
    truth_intrinsics = make_rig(2, PINHOLE)
    truth_cameras = make_camera_poses(2, radius=1.6, spacing_deg=25)
    boards = make_board_poses(20, distance=1.4)
    observations = project_rig(BOARD, truth_intrinsics, truth_cameras, boards, noise=0.05)
    # keep camera 0 on even poses and camera 1 on odd ones: never seen together
    split = [o for o in observations if (o.camera == 0) == (o.pose_index % 2 == 0)]
    estimated = [
        calibrate_intrinsics(BOARD, _detections(split, c), truth_intrinsics[c].image_size, PINHOLE)[0]
        for c in range(2)
    ]
    with pytest.raises(CalibrationError, match="vue commune"):
        initial_extrinsics(BOARD, estimated, split)


def test_too_few_views_is_rejected():
    truth = make_rig(1, PINHOLE)
    cameras = make_camera_poses(1)
    observations = project_rig(BOARD, truth, cameras, make_board_poses(2, distance=1.4))
    with pytest.raises(CalibrationError, match="3 vues"):
        calibrate_intrinsics(BOARD, _detections(observations, 0), truth[0].image_size, PINHOLE)


@pytest.mark.parametrize("model", [PINHOLE, FISHEYE])
def test_pnp_recovers_the_board_pose(model):
    truth = make_rig(1, model, (640, 480) if model == FISHEYE else (1280, 800))
    cameras = make_camera_poses(1)
    boards = make_board_poses(3, distance=1.4)
    observations = project_rig(BOARD, truth, cameras, boards, noise=0.0)
    observation = observations[0]
    pose = solve_pose(BOARD, Detection(observation.ids, observation.points), truth[0])
    expected = boards[observation.pose_index]
    assert np.allclose(pose.tvec, expected.tvec, atol=2e-3)
    assert np.allclose(pose.rvec, expected.rvec, atol=2e-3)
