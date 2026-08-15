"""Importing MediaPipe Pose output and reconstructing it in 3D."""

from __future__ import annotations

import json

import numpy as np
import pytest

from labancalib.mediapipe_io import (
    MediaPipeImportError,
    N_LANDMARKS,
    POSE_LANDMARKS,
    build_tracks,
    landmarks_to_array,
    load_landmarks,
    save_landmarks,
    sequence_to_array,
    triangulate_mediapipe,
)
from labancalib.models import PINHOLE
from labancalib.triangulate import Rig

from .synthetic import make_camera_poses, make_rig

mediapipe = pytest.importorskip("mediapipe", reason="mediapipe non installé", minversion=None)


def _rig(n_cameras: int = 3) -> Rig:
    return Rig(
        intrinsics=make_rig(n_cameras, PINHOLE, (1280, 800)),
        poses=make_camera_poses(n_cameras, radius=1.8, spacing_deg=30),
        names=[f"caméra {i}" for i in range(n_cameras)],
    )


def _skeleton(n_frames: int = 6, seed: int = 4) -> np.ndarray:
    """A moving set of 33 points in the capture volume: (n_frames, 33, 3)."""
    rng = np.random.default_rng(seed)
    base = np.column_stack(
        [
            rng.uniform(-0.35, 0.35, N_LANDMARKS),
            rng.uniform(-0.80, 0.80, N_LANDMARKS),
            rng.uniform(1.55, 2.05, N_LANDMARKS),
        ]
    )
    drift = np.linspace(0.0, 0.12, n_frames)[:, None, None] * np.array([1.0, 0.0, -0.5])
    return base[None] + drift


def _landmarks_from(rig: Rig, points: np.ndarray, visibility: float = 0.9) -> list[np.ndarray]:
    """Project ground-truth 3D points into each camera as MediaPipe would report them."""
    sequences = []
    for camera in range(rig.n_cameras):
        width, height = rig.intrinsics[camera].image_size
        pose = rig.poses[camera]
        frames = []
        for frame in points:
            pixels = rig.intrinsics[camera].project(frame, pose.rvec, pose.tvec)
            normalised = pixels / np.array([width, height])
            frames.append(
                np.column_stack(
                    [normalised, np.zeros(len(frame)), np.full(len(frame), visibility)]
                )
            )
        sequences.append(np.stack(frames))
    return sequences


def test_tasks_result_objects_are_read():
    """The real MediaPipe Tasks containers, not a stand-in."""
    from mediapipe.tasks.python.components.containers import NormalizedLandmark
    from mediapipe.tasks.python.vision import PoseLandmarkerResult

    landmarks = [
        NormalizedLandmark(x=0.01 * i, y=0.5, z=0.0, visibility=0.8) for i in range(N_LANDMARKS)
    ]
    result = PoseLandmarkerResult(pose_landmarks=[landmarks], pose_world_landmarks=[])
    array = landmarks_to_array(result)
    assert array.shape == (N_LANDMARKS, 4)
    assert array[3, 0] == pytest.approx(0.03)
    assert array[0, 3] == pytest.approx(0.8)

    empty = PoseLandmarkerResult(pose_landmarks=[], pose_world_landmarks=[])
    assert np.all(np.isnan(landmarks_to_array(empty)))
    assert np.all(np.isnan(landmarks_to_array(None)))


def test_legacy_solutions_shape_is_read():
    """MediaPipe < 1.0 returned a NormalizedLandmarkList under .pose_landmarks."""

    class Landmark:
        def __init__(self, x):
            self.x, self.y, self.z, self.visibility = x, 0.4, 0.1, 0.7

    class List_:
        def __init__(self):
            self.landmark = [Landmark(0.02 * i) for i in range(N_LANDMARKS)]

    class Result:
        def __init__(self):
            self.pose_landmarks = List_()

    array = landmarks_to_array(Result())
    assert array[5, 0] == pytest.approx(0.10)
    assert array[5, 3] == pytest.approx(0.7)


def test_second_person_can_be_selected():
    frame = {"landmarks": [[{"x": 0.1, "y": 0.2}] * N_LANDMARKS, [{"x": 0.8, "y": 0.9}] * N_LANDMARKS]}
    assert landmarks_to_array(frame, person=0)[0, 0] == pytest.approx(0.1)
    assert landmarks_to_array(frame, person=1)[0, 0] == pytest.approx(0.8)
    assert np.all(np.isnan(landmarks_to_array(frame, person=5)))


def test_normalised_coordinates_are_scaled_to_pixels():
    rig = _rig()
    sequences = _landmarks_from(rig, _skeleton(2))
    tracks = build_tracks(sequences, rig=rig)
    width, height = rig.intrinsics[0].image_size
    expected = sequences[0][:, :, :2] * np.array([width, height])
    assert np.allclose(tracks.pixels[0], expected, equal_nan=True)


def test_pixel_coordinates_are_left_alone():
    rig = _rig(1)
    pixels = np.full((2, N_LANDMARKS, 4), 1.0)
    pixels[:, :, 0], pixels[:, :, 1] = 640.0, 400.0
    tracks = build_tracks([pixels], rig=rig, normalised=False)
    assert tracks.pixels[0, 0, 0, 0] == pytest.approx(640.0)


def test_round_trip_recovers_the_3d_skeleton():
    rig = _rig()
    truth = _skeleton()
    reconstruction = triangulate_mediapipe(rig, _landmarks_from(rig, truth))
    assert reconstruction.points.shape == truth.shape
    assert np.nanmax(np.linalg.norm(reconstruction.points - truth, axis=2)) < 1e-6
    assert reconstruction.completeness() == pytest.approx(1.0)
    assert np.all(reconstruction.seen_by == rig.n_cameras)
    assert len(reconstruction.segment_lengths()) > 10


def test_low_visibility_landmarks_are_dropped():
    rig = _rig()
    truth = _skeleton(3)
    sequences = _landmarks_from(rig, truth)
    for camera in (1, 2):
        sequences[camera][:, 7, 3] = 0.1  # one joint barely visible in two cameras
    reconstruction = triangulate_mediapipe(rig, sequences, min_visibility=0.5)
    assert np.all(np.isnan(reconstruction.points[:, 7]))
    assert np.all(reconstruction.seen_by[:, 7] == 1)
    assert np.isfinite(reconstruction.points[:, 8]).all()


def test_offsets_realign_a_late_camera():
    rig = _rig()
    truth = _skeleton(6)
    sequences = _landmarks_from(rig, truth)
    # camera 2 started recording two frames late: its first frame is global frame 2
    sequences[2] = sequences[2][2:]

    misaligned = triangulate_mediapipe(rig, sequences)
    assert np.nanmax(np.linalg.norm(misaligned.points - truth, axis=2)) > 1e-3

    realigned = triangulate_mediapipe(rig, sequences, offsets=[0, 0, 2])
    assert realigned.points.shape == truth.shape
    assert np.nanmax(np.linalg.norm(realigned.points - truth, axis=2)) < 1e-6
    # the two cameras that were rolling from the start still cover frames 0-1
    assert np.all(realigned.seen_by[:2] == 2)
    assert np.all(realigned.seen_by[2:] == 3)


def test_max_error_rejects_an_inconsistent_point():
    rig = _rig()
    truth = _skeleton(3)
    sequences = _landmarks_from(rig, truth)
    sequences[0][:, 4, 0] += 0.20  # camera 0 mislocates one joint by 20 % of the width
    kept = triangulate_mediapipe(rig, sequences)
    assert np.isfinite(kept.points[:, 4]).all()
    filtered = triangulate_mediapipe(rig, sequences, max_error=5.0)
    assert np.all(np.isnan(filtered.points[:, 4]))
    assert np.isfinite(filtered.points[:, 5]).all()


@pytest.mark.parametrize("extension", [".json", ".npy", ".npz"])
def test_landmark_files_round_trip(tmp_path, extension):
    truth = _skeleton(4)
    rig = _rig()
    sequence = _landmarks_from(rig, truth)[0]
    path = tmp_path / f"cam0{extension}"
    save_landmarks(str(path), sequence, source="essai")
    reloaded = load_landmarks(str(path))
    assert reloaded.shape == sequence.shape
    assert np.allclose(reloaded, sequence, equal_nan=True)


def test_bare_list_of_frames_json_is_accepted(tmp_path):
    path = tmp_path / "brut.json"
    frames = [[{"x": 0.5, "y": 0.5, "z": 0.0, "visibility": 0.9}] * N_LANDMARKS for _ in range(3)]
    path.write_text(json.dumps(frames), encoding="utf-8")
    array = load_landmarks(str(path))
    assert array.shape == (3, N_LANDMARKS, 4)
    assert array[0, 0, 0] == pytest.approx(0.5)


def test_json_lines_are_accepted(tmp_path):
    path = tmp_path / "lignes.jsonl"
    line = json.dumps({"landmarks": [[0.25, 0.75, 0.0, 0.95]] * N_LANDMARKS})
    path.write_text("\n".join([line] * 2), encoding="utf-8")
    array = load_landmarks(str(path))
    assert array.shape == (2, N_LANDMARKS, 4)
    assert array[1, 0, 1] == pytest.approx(0.75)


def test_unusable_sources_are_reported_clearly(tmp_path):
    with pytest.raises(MediaPipeImportError, match="extension"):
        load_landmarks(str(tmp_path / "points.csv"))
    empty = tmp_path / "vide.json"
    empty.write_text("", encoding="utf-8")
    with pytest.raises(MediaPipeImportError, match="vide"):
        load_landmarks(str(empty))
    rig = _rig(3)
    with pytest.raises(MediaPipeImportError, match="caméra"):
        build_tracks([np.zeros((2, N_LANDMARKS, 4))], rig=rig)
    with pytest.raises(MediaPipeImportError, match="rig"):
        build_tracks([np.zeros((2, N_LANDMARKS, 4))])


def test_reconstruction_saves_to_npz_and_csv(tmp_path):
    rig = _rig()
    reconstruction = triangulate_mediapipe(rig, _landmarks_from(rig, _skeleton(3)))
    npz = tmp_path / "points3d.npz"
    reconstruction.save(str(npz))
    archive = np.load(npz)
    assert archive["points"].shape == (3, N_LANDMARKS, 3)
    assert list(archive["names"])[:1] == [POSE_LANDMARKS[0]]

    csv = tmp_path / "points3d.csv"
    reconstruction.save(str(csv))
    lines = csv.read_text(encoding="utf-8").splitlines()
    assert lines[0].startswith("trame,point,nom")
    assert len(lines) == 1 + 3 * N_LANDMARKS


def test_cli_triangulates_from_files(tmp_path, capsys):
    """The CLI path: exported rig + one landmark file per camera -> 3D CSV."""
    from labancalib.board import BoardSpec
    from labancalib.cli import main as cli_main
    from labancalib.export import export_json
    from labancalib.models import CalibrationResult, CameraCalibration

    rig = _rig()
    truth = _skeleton(4)
    result = CalibrationResult(
        cameras=[
            CameraCalibration(name=name, intrinsics=intr, pose=pose)
            for name, intr, pose in zip(rig.names, rig.intrinsics, rig.poses)
        ],
        rpe=0.2,
        converged=True,
    )
    rig_path = tmp_path / "dispositif.json"
    export_json(str(rig_path), result, BoardSpec("charuco", 8, 6, 0.040, 0.030))

    files = []
    for camera, sequence in enumerate(_landmarks_from(rig, truth)):
        path = tmp_path / f"cam{camera}.json"
        save_landmarks(str(path), sequence)
        files.append(str(path))

    output = tmp_path / "points3d.csv"
    assert cli_main(["triangulate", str(rig_path), *files, "--out", str(output)]) == 0
    printed = capsys.readouterr().out
    assert "100.0 % des points reconstruits" in printed
    assert "Longueurs de segments" in printed

    rows = output.read_text(encoding="utf-8").splitlines()
    assert len(rows) == 1 + 4 * N_LANDMARKS
    x, y, z = (float(v) for v in rows[1].split(",")[3:6])
    assert np.allclose([x, y, z], truth[0, 0], atol=1e-4)


def test_sequence_to_array_handles_missing_frames():
    array = sequence_to_array([None, {"landmarks": [{"x": 0.2, "y": 0.3}] * N_LANDMARKS}, None])
    assert array.shape == (3, N_LANDMARKS, 4)
    assert np.all(np.isnan(array[0]))
    assert array[1, 0, 0] == pytest.approx(0.2)
