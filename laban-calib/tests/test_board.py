"""Target geometry, detection and printable rendering."""

from __future__ import annotations

import numpy as np
import pytest

from labancalib.board import (
    BoardDetector,
    BoardSpec,
    Detection,
    DetectorOptions,
    draw_detection,
    render_board,
    save_board_pdf,
)

SPECS = [
    BoardSpec("charuco", 8, 6, 0.030, 0.022),
    BoardSpec("acircles", 4, 11, 0.030),
    BoardSpec("circles", 6, 5, 0.030),
    BoardSpec("chessboard", 9, 7, 0.025),
]


@pytest.mark.parametrize("spec", SPECS, ids=lambda s: s.kind)
def test_object_points_are_planar_and_complete(spec):
    points = spec.object_points()
    assert points.shape == (spec.n_points, 3)
    assert np.allclose(points[:, 2], 0.0)
    assert len(np.unique(points[:, :2], axis=0)) == spec.n_points


@pytest.mark.parametrize("spec", SPECS, ids=lambda s: s.kind)
def test_detection_round_trip_on_the_rendered_target(spec):
    """What we render must be what we detect — ids included."""
    image = render_board(spec, pixels_per_metre=2000.0)
    detection = BoardDetector(spec).detect(image)
    assert detection is not None, f"{spec.kind}: mire non détectée sur son propre rendu"
    assert detection.count == spec.n_points
    assert set(np.asarray(detection.ids).tolist()) == set(range(spec.n_points))


def test_acircles_spacing_matches_the_opencv_convention():
    spec = BoardSpec("acircles", 4, 11, 0.030)
    points = spec.object_points()
    # rows are staggered by half a spacing along x and separated by half along y
    assert points[1, 0] - points[0, 0] == pytest.approx(spec.square_size)
    assert points[4, 0] - points[0, 0] == pytest.approx(spec.square_size / 2)
    assert points[4, 1] - points[0, 1] == pytest.approx(spec.square_size / 2)


def test_invalid_geometry_is_rejected():
    with pytest.raises(ValueError):
        BoardSpec("charuco", 8, 6, 0.030, 0.045)  # marker larger than the square
    with pytest.raises(ValueError):
        BoardSpec("charuco", 1, 6, 0.030, 0.020)
    with pytest.raises(ValueError):
        BoardSpec("triangle", 8, 6)


def test_detection_below_the_minimum_is_discarded():
    spec = BoardSpec("charuco", 8, 6, 0.030, 0.022)
    image = render_board(spec, pixels_per_metre=2000.0)
    strict = BoardDetector(spec, DetectorOptions(min_points=200))
    assert strict.detect(image) is None


def test_serialisation_round_trip():
    spec = BoardSpec("charuco", 7, 5, 0.045, 0.033, dictionary="DICT_6X6_250")
    assert BoardSpec.from_dict(spec.to_dict()).to_dict() == spec.to_dict()
    detection = Detection(ids=np.array([1, 4, 9]), points=np.array([[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]))
    restored = Detection.from_dict(detection.to_dict())
    assert np.array_equal(restored.ids, detection.ids)
    assert np.allclose(restored.points, detection.points)


def test_pdf_export_writes_a_file(tmp_path):
    path = tmp_path / "mire.pdf"
    save_board_pdf(BoardSpec("charuco", 8, 6, 0.030, 0.022), str(path))
    assert path.exists() and path.stat().st_size > 1000


def test_overlay_draws_without_touching_the_input():
    spec = BoardSpec("chessboard", 9, 7, 0.025)
    image = render_board(spec, pixels_per_metre=1200.0)
    detection = BoardDetector(spec).detect(image)
    overlay = draw_detection(image, detection, spec, reprojection=detection.points + 0.5)
    assert overlay.shape == (*image.shape, 3)
    assert image.ndim == 2  # untouched
