"""Project persistence, validation and export."""

from __future__ import annotations

import json

import numpy as np
import pytest

from labancalib.board import BoardSpec, Detection
from labancalib.export import export_result, parameters_report, rig_dictionary
from labancalib.models import CalibrationResult, CameraCalibration, Intrinsics, PINHOLE, Pose
from labancalib.project import Project


def _project() -> Project:
    project = Project(board=BoardSpec("charuco", 8, 6, 0.040, 0.030))
    project.add_camera("gauche", ["a0.png", "a1.png", "a2.png"])
    project.add_camera("droite", ["b0.png", "b1.png", "b2.png"])
    for camera in (0, 1):
        for pose in (0, 2):
            project.detections[(camera, pose)] = Detection(
                ids=np.arange(6), points=np.random.default_rng(camera + pose).random((6, 2)) * 100
            )
    return project


def _result() -> CalibrationResult:
    cameras = [
        CameraCalibration(
            name="gauche",
            intrinsics=Intrinsics(PINHOLE, (1280, 800), 900.0, 1.0, 640.0, 400.0, dist=np.zeros(5)),
            pose=Pose(),
            sigmas={"f": 0.8},
            rpe=0.21,
        ),
        CameraCalibration(
            name="droite",
            intrinsics=Intrinsics(PINHOLE, (1280, 800), 910.0, 1.0, 645.0, 402.0, dist=np.zeros(5)),
            pose=Pose(np.array([0.0, -0.3, 0.0]), np.array([-0.5, 0.0, 0.05])),
            sigmas={"f": 0.9},
            rpe=0.23,
        ),
    ]
    return CalibrationResult(cameras=cameras, board_poses={0: Pose(), 2: Pose()}, rpe=0.22, converged=True)


def test_save_load_round_trip(tmp_path):
    project = _project()
    project.result = _result()
    path = tmp_path / "essai.lcalib"
    project.save(str(path))
    reloaded = Project.load(str(path))

    assert reloaded.board.to_dict() == project.board.to_dict()
    assert [c.name for c in reloaded.cameras] == ["gauche", "droite"]
    assert set(reloaded.detections) == set(project.detections)
    assert reloaded.result.rpe == pytest.approx(0.22)
    assert reloaded.result.cameras[1].intrinsics.f == pytest.approx(910.0)


def test_future_format_is_refused(tmp_path):
    path = tmp_path / "futur.lcalib"
    path.write_text(json.dumps({"format": 99}), encoding="utf-8")
    with pytest.raises(ValueError, match="plus récente"):
        Project.load(str(path))


def test_validate_flags_unequal_image_counts():
    project = _project()
    project.cameras[1].images.append("b3.png")
    issues = project.validate()
    assert any("même nombre d'images" in issue for issue in issues)


def test_coverage_and_observations():
    project = _project()
    assert project.coverage() == {0: 2, 2: 2}
    observations = project.observations()
    assert len(observations) == 4
    assert {o.camera for o in observations} == {0, 1}


def test_remove_camera_reindexes_detections():
    project = _project()
    project.remove_camera(0)
    assert project.n_cameras == 1
    assert all(camera == 0 for camera, _ in project.detections)
    assert project.result is None


def test_rig_dictionary_is_self_contained():
    data = rig_dictionary(_result(), BoardSpec("charuco", 8, 6, 0.040, 0.030))
    assert data["format"] == "laban-calib/rig"
    assert len(data["cameras"]) == 2
    first = data["cameras"][0]
    assert set(first) >= {"K", "distortion", "pose", "centre", "model", "image_size"}
    assert np.allclose(np.asarray(first["K"]).shape, (3, 3))


@pytest.mark.parametrize("name, fmt", [("rig.json", "json"), ("rig.yml", "opencv"), ("rig.txt", "text")])
def test_export_formats_write_readable_files(tmp_path, name, fmt):
    path = tmp_path / name
    export_result(str(path), _result(), BoardSpec("charuco", 8, 6, 0.040, 0.030), fmt)
    assert path.exists() and path.stat().st_size > 100


def test_parameters_report_mentions_every_camera():
    report = parameters_report(_result(), BoardSpec("charuco", 8, 6, 0.040, 0.030))
    assert "Caméra 0" in report and "Caméra 1" in report
    assert "+/-" in report
