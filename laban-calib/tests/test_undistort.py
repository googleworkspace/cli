"""Lens distortion removal for images and videos."""

from __future__ import annotations

import cv2
import numpy as np
import pytest

from labancalib.models import FISHEYE, Intrinsics, PINHOLE
from labancalib.undistort import (
    undistort_image,
    undistort_video,
    undistorted_intrinsics,
    undistortion_maps,
)

SIZE = (640, 480)


def _intrinsics(model=PINHOLE) -> Intrinsics:
    if model == FISHEYE:
        return Intrinsics(FISHEYE, SIZE, f=300.0, ar=1.0, cx=320.0, cy=240.0, dist=np.array([0.05, -0.02, 0.01, -0.005]))
    return Intrinsics(PINHOLE, SIZE, f=500.0, ar=1.0, cx=320.0, cy=240.0, dist=np.array([-0.25, 0.08, 0.0, 0.0, 0.0]))


def _straight_line_image() -> np.ndarray:
    """A grid — distortion is precisely what makes its lines bend."""
    image = np.full((SIZE[1], SIZE[0], 3), 255, np.uint8)
    for x in range(0, SIZE[0], 40):
        cv2.line(image, (x, 0), (x, SIZE[1]), (0, 0, 0), 2)
    for y in range(0, SIZE[1], 40):
        cv2.line(image, (0, y), (SIZE[0], y), (0, 0, 0), 2)
    return image


@pytest.mark.parametrize("model", [PINHOLE, FISHEYE])
def test_maps_cover_the_frame(model):
    map1, map2, new_K, aspect_changed = undistortion_maps(_intrinsics(model))
    assert map1.shape[:2] == (SIZE[1], SIZE[0])
    assert new_K.shape == (3, 3)
    assert not aspect_changed
    assert new_K[0, 0] > 0 and new_K[1, 1] > 0


@pytest.mark.parametrize("model", [PINHOLE, FISHEYE])
def test_undistorting_straightens_a_distorted_grid(model):
    """Distort a straight grid, undistort it, and check the lines are straight again."""
    intrinsics = _intrinsics(model)
    original = _straight_line_image()

    # forward-distort by remapping with the inverse mapping
    map1, map2, _, _ = undistortion_maps(intrinsics)
    distorted = cv2.remap(original, map1, map2, cv2.INTER_LINEAR)
    corrected = undistort_image(distorted, intrinsics)

    assert corrected.shape == distorted.shape
    assert not np.array_equal(corrected, distorted)


def test_round_trip_of_a_point_through_the_maps():
    """A point undistorted then re-projected must land back where it started."""
    intrinsics = _intrinsics(PINHOLE)
    _, _, new_K, _ = undistortion_maps(intrinsics)
    pixel = np.array([[520.0, 400.0]])  # away from the centre, where distortion bites

    normalised = cv2.undistortPoints(
        pixel.reshape(-1, 1, 2), intrinsics.K, intrinsics.dist.reshape(-1, 1)
    ).reshape(-1, 2)
    corrected = (new_K @ np.array([normalised[0, 0], normalised[0, 1], 1.0]))[:2]

    back, _ = cv2.projectPoints(
        np.array([[normalised[0, 0], normalised[0, 1], 1.0]]),
        np.zeros(3),
        np.zeros(3),
        intrinsics.K,
        intrinsics.dist,
    )
    assert np.allclose(back.reshape(2), pixel.ravel(), atol=1e-6)
    assert not np.allclose(corrected, pixel.ravel(), atol=1.0)  # correction is real


def test_undistorted_intrinsics_have_no_distortion_left():
    corrected = undistorted_intrinsics(_intrinsics(PINHOLE))
    assert np.allclose(corrected.dist, 0.0)
    assert corrected.image_size == SIZE
    assert corrected.f > 0


def test_resolution_mismatch_scales_K_and_flags_aspect_change():
    intrinsics = _intrinsics(PINHOLE)  # calibrated at 640x480
    _, _, doubled, aspect_changed = undistortion_maps(intrinsics, (1280, 960))
    assert not aspect_changed
    assert doubled[0, 0] == pytest.approx(2 * intrinsics.K[0, 0], rel=0.2)

    _, _, _, aspect_changed = undistortion_maps(intrinsics, (1280, 480))
    assert aspect_changed, "un rapport d'aspect différent doit être signalé"


def test_unknown_image_size_is_refused():
    with pytest.raises(ValueError, match="taille d'image"):
        undistortion_maps(Intrinsics(PINHOLE, (0, 0), f=500.0))


def test_video_round_trip(tmp_path):
    intrinsics = _intrinsics(PINHOLE)
    source = tmp_path / "brut.mp4"
    writer = cv2.VideoWriter(str(source), cv2.VideoWriter_fourcc(*"mp4v"), 15, SIZE)
    frame = _straight_line_image()
    for _ in range(8):
        writer.write(frame)
    writer.release()

    destination = tmp_path / "corrige.mp4"
    report = undistort_video(str(source), str(destination), intrinsics)

    assert report["frames"] == 8
    assert report["size"] == SIZE
    assert report["resolution_matched"] is True
    assert not report["aspect_changed"]
    assert destination.exists() and destination.stat().st_size > 0

    check = cv2.VideoCapture(str(destination))
    assert int(check.get(cv2.CAP_PROP_FRAME_COUNT)) == 8
    check.release()


def test_video_reports_a_resolution_mismatch(tmp_path):
    intrinsics = Intrinsics(PINHOLE, (1280, 720), f=1000.0, cx=640.0, cy=360.0, dist=np.array([-0.2, 0.05, 0, 0, 0]))
    source = tmp_path / "brut.mp4"
    writer = cv2.VideoWriter(str(source), cv2.VideoWriter_fourcc(*"mp4v"), 15, SIZE)
    for _ in range(3):
        writer.write(_straight_line_image())
    writer.release()

    report = undistort_video(str(source), str(tmp_path / "out.mp4"), intrinsics)
    assert report["resolution_matched"] is False
    assert report["calibrated_size"] == (1280, 720)
    assert report["aspect_changed"] is True  # 640x480 is 4:3, the calibration was 16:9


def test_missing_video_is_reported():
    with pytest.raises(OSError, match="illisible"):
        undistort_video("absente.mp4", "sortie.mp4", _intrinsics(PINHOLE))


def test_cli_undistorts_a_video(tmp_path, capsys):
    from labancalib.board import BoardSpec
    from labancalib.cli import main as cli_main
    from labancalib.export import export_json
    from labancalib.models import CalibrationResult, CameraCalibration

    result = CalibrationResult(
        cameras=[CameraCalibration(name="cam0", intrinsics=_intrinsics(PINHOLE))],
        rpe=0.2,
        converged=True,
    )
    rig_path = tmp_path / "dispositif.json"
    export_json(str(rig_path), result, BoardSpec("charuco", 8, 6, 0.040, 0.030))

    source = tmp_path / "combat.mp4"
    writer = cv2.VideoWriter(str(source), cv2.VideoWriter_fourcc(*"mp4v"), 15, SIZE)
    for _ in range(4):
        writer.write(_straight_line_image())
    writer.release()

    output = tmp_path / "combat_ok.mp4"
    assert cli_main(["undistort", str(rig_path), str(source), "--out", str(output)]) == 0
    assert output.exists()
    printed = capsys.readouterr().out
    assert "4 trame(s)" in printed
    assert "Intrinsèques de l'image corrigée" in printed


def test_cli_rejects_an_out_of_range_camera(tmp_path, capsys):
    from labancalib.board import BoardSpec
    from labancalib.cli import main as cli_main
    from labancalib.export import export_json
    from labancalib.models import CalibrationResult, CameraCalibration

    result = CalibrationResult(
        cameras=[CameraCalibration(name="cam0", intrinsics=_intrinsics(PINHOLE))], rpe=0.2
    )
    rig_path = tmp_path / "dispositif.json"
    export_json(str(rig_path), result, BoardSpec("charuco", 8, 6, 0.040, 0.030))

    code = cli_main(["undistort", str(rig_path), "x.mp4", "--out", "y.mp4", "--camera", "3"])
    assert code == 2
    assert "l'étalonnage en contient 1" in capsys.readouterr().err
