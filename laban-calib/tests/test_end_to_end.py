"""End-to-end run on rendered images: detection, calibration, export, CLI.

The images are produced by warping a rendered ChArUco target with the exact
homography induced by a known camera, so the whole chain — file listing,
detection, intrinsics, rig initialisation, bundle adjustment, export — is
exercised on real pixels rather than on synthetic point coordinates.
"""

from __future__ import annotations

import os

import cv2
import numpy as np
import pytest

from labancalib import geometry
from labancalib.board import BoardSpec, render_board
from labancalib.cli import main as cli_main
from labancalib.models import PINHOLE
from labancalib.pipeline import run_calibration, run_detection
from labancalib.project import Project
from labancalib.triangulate import Rig

from .synthetic import make_board_poses, make_camera_poses, make_rig

BOARD = BoardSpec("charuco", 8, 6, 0.060, 0.045)


@pytest.fixture(scope="module")
def rendered_rig(tmp_path_factory) -> tuple[str, list]:
    """Write ``cam0/ cam1/ cam2/`` folders of images of a moving target."""
    root = tmp_path_factory.mktemp("rendu")
    pixels_per_metre, margin = 2000.0, 0.02
    sheet = render_board(BOARD, pixels_per_metre, margin=margin)
    height, width = sheet.shape
    intrinsics = make_rig(3, PINHOLE, (1280, 800))
    cameras = make_camera_poses(3, radius=1.2, spacing_deg=20)
    boards = make_board_poses(12, seed=11, distance=1.1)

    source = np.float32([[0, 0], [width, 0], [width, height], [0, height]])
    corners = np.array(
        [[(u / pixels_per_metre) - margin, (v / pixels_per_metre) - margin, 0.0] for u, v in source]
    )
    for camera in range(3):
        os.makedirs(root / f"cam{camera}", exist_ok=True)
    for pose_index, board_pose in enumerate(boards):
        for camera in range(3):
            rvec, tvec = geometry.compose(
                cameras[camera].rvec, cameras[camera].tvec, board_pose.rvec, board_pose.tvec
            )
            destination = intrinsics[camera].project(corners, rvec, tvec).astype(np.float32)
            warped = cv2.warpPerspective(
                sheet,
                cv2.getPerspectiveTransform(source, destination),
                (1280, 800),
                flags=cv2.INTER_AREA,
                borderMode=cv2.BORDER_CONSTANT,
                borderValue=110,
            )
            cv2.imwrite(str(root / f"cam{camera}" / f"pose_{pose_index:03d}.png"), warped)
    return str(root), cameras


def test_full_pipeline_on_rendered_images(rendered_rig):
    root, truth_cameras = rendered_rig
    project = Project(board=BOARD)
    project.set_camera_folders([os.path.join(root, f"cam{i}") for i in range(3)])
    assert project.n_cameras == 3 and project.n_poses == 12
    assert project.validate() == []

    summary = run_detection(project)
    assert summary.total >= 30
    assert all(count >= 8 for count in summary.per_camera.values())

    result = run_calibration(project)
    assert result.converged
    assert result.rpe < 1.5
    # the rig geometry must be recovered even though the renderer ignores
    # lens distortion: baselines are what the downstream triangulation needs
    for estimated, truth in zip(result.cameras, truth_cameras):
        assert np.linalg.norm(estimated.pose.centre - truth.centre) < 0.05

    rig = Rig.from_result(result)
    target = np.array([0.0, 0.0, 1.2])
    observations = {
        camera: rig.intrinsics[camera]
        .project(target.reshape(1, 3), rig.poses[camera].rvec, rig.poses[camera].tvec)
        .ravel()
        for camera in range(3)
    }
    point, error = rig.triangulate_point(observations)
    assert np.linalg.norm(point - target) < 1e-6 and error < 1e-5


def test_cli_calibrates_and_exports(rendered_rig, tmp_path, capsys):
    root, _ = rendered_rig
    output = tmp_path / "rig.json"
    code = cli_main(
        [
            "calibrate",
            *[os.path.join(root, f"cam{i}") for i in range(3)],
            "--kind", "charuco",
            "--cols", "8",
            "--rows", "6",
            "--square", "60",
            "--marker", "45",
            "--out", str(output),
            "--quiet",
        ]
    )
    assert code == 0
    assert output.exists()
    assert "RPE global" in capsys.readouterr().out
    assert Rig.from_json(str(output)).n_cameras == 3


def test_cli_generates_a_printable_target(tmp_path, capsys):
    path = tmp_path / "mire.pdf"
    assert cli_main(["board", "--kind", "charuco", "--square", "40", "--marker", "30", "--out", str(path)]) == 0
    assert path.exists() and path.stat().st_size > 1000
    assert "ChArUco" in capsys.readouterr().out
