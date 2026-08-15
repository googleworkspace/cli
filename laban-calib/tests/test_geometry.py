"""Projection, pose algebra and triangulation."""

from __future__ import annotations

import cv2
import numpy as np
import pytest

from labancalib import geometry
from labancalib.models import FISHEYE, Intrinsics, PINHOLE, Pose


@pytest.fixture
def points() -> np.ndarray:
    rng = np.random.default_rng(0)
    board = rng.uniform(-0.2, 0.2, (60, 3))
    board[:, 2] = 0.0
    return board


@pytest.mark.parametrize(
    "model, dist",
    [
        (PINHOLE, [-0.12, 0.05, 0.001, -0.0008, 0.002]),
        (FISHEYE, [0.01, -0.008, 0.006, -0.003]),
    ],
)
def test_projection_matches_opencv(points, model, dist):
    """Our vectorised lens model must agree with OpenCV's (no skew)."""
    intrinsics = Intrinsics(
        model=model, image_size=(1280, 800), f=900.0, ar=1.002, cx=641.0, cy=399.0, dist=np.array(dist)
    )
    rvec, tvec = np.array([0.1, -0.2, 0.05]), np.array([0.02, -0.03, 1.7])
    ours = intrinsics.project(points, rvec, tvec)

    obj = points.reshape(-1, 1, 3)
    if model == FISHEYE:
        reference, _ = cv2.fisheye.projectPoints(
            obj.reshape(1, -1, 3),
            rvec.reshape(3, 1),
            tvec.reshape(3, 1),
            intrinsics.K,
            np.asarray(dist, dtype=np.float64).reshape(4, 1),
        )
    else:
        reference, _ = cv2.projectPoints(
            obj, rvec, tvec, intrinsics.K, np.asarray(dist, dtype=np.float64)
        )
    assert np.allclose(ours, np.asarray(reference).reshape(-1, 2), atol=1e-9)


def test_fisheye_intrinsics_force_zero_skew():
    intrinsics = Intrinsics(model=FISHEYE, image_size=(640, 480), f=300.0, alpha=0.01)
    assert intrinsics.alpha == 0.0


def test_pose_inverse_and_compose_round_trip():
    pose = Pose(np.array([0.2, -0.1, 0.35]), np.array([0.4, -0.2, 1.1]))
    identity = pose.compose(pose.inverse())
    assert np.allclose(identity.rvec, 0.0, atol=1e-12)
    assert np.allclose(identity.tvec, 0.0, atol=1e-12)


def test_compose_matches_matrix_product():
    a = Pose(np.array([0.1, 0.2, -0.3]), np.array([0.1, 0.0, 0.4]))
    b = Pose(np.array([-0.2, 0.05, 0.1]), np.array([-0.3, 0.2, 0.9]))
    assert np.allclose(a.compose(b).matrix(), a.matrix() @ b.matrix(), atol=1e-12)


def test_camera_centre_is_the_inverse_translation():
    pose = Pose(np.array([0.3, 0.1, -0.2]), np.array([0.5, -0.4, 1.2]))
    assert np.allclose(pose.centre, pose.inverse().tvec)


def test_triangulation_recovers_a_known_point():
    target = np.array([0.12, -0.05, 1.4])
    poses = [Pose(), Pose(np.array([0.0, -0.3, 0.0]), np.array([-0.5, 0.0, 0.05]))]
    observations = []
    for pose in poses:
        camera = geometry.rodrigues(pose.rvec) @ target + pose.tvec
        observations.append((pose.rvec, pose.tvec, camera[:2] / camera[2]))
    assert np.allclose(geometry.triangulate(observations), target, atol=1e-9)


def test_triangulation_needs_two_views():
    with pytest.raises(ValueError):
        geometry.triangulate([(np.zeros(3), np.zeros(3), np.zeros(2))])


def test_build_and_split_K_round_trip():
    K = geometry.build_K(900.0, 1.002, 640.0, 400.0, 0.003)
    assert np.allclose(geometry.split_K(K), (900.0, 1.002, 640.0, 400.0, 0.003))
