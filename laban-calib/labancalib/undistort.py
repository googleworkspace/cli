"""Removing lens distortion from images and videos.

Why this matters for a single camera: with a wide-angle lens, straight lines
bow and the bias grows towards the edges of the frame. Any joint angle or
velocity measured on the raw pixels — the features a Laban analysis is built
on — inherits that bias. Undistorting first removes it, and it is the one
thing a calibration buys you when a second camera is not available.

Undistortion produces a *new* camera: the corrected image has its own
intrinsic matrix (returned by :func:`undistortion_maps`), with no distortion
coefficients. Downstream tools must use that matrix, not the original one.
"""

from __future__ import annotations

import os

import cv2
import numpy as np

from .models import FISHEYE, Intrinsics


def _scaled_intrinsics(intrinsics: Intrinsics, size: tuple[int, int]) -> tuple[np.ndarray, bool]:
    """Rescale K when the footage resolution differs from the calibration one.

    Valid only when the sensor's field of view is unchanged — a different
    resolution at the same framing. A crop or a changed aspect ratio makes the
    calibration inapplicable, which is what the returned flag reports.
    """
    width, height = int(size[0]), int(size[1])
    calibrated = tuple(intrinsics.image_size)
    K = intrinsics.K.copy()
    if not calibrated[0] or not calibrated[1] or (width, height) == calibrated:
        return K, False
    scale_x, scale_y = width / calibrated[0], height / calibrated[1]
    K = np.diag([scale_x, scale_y, 1.0]) @ K
    aspect_changed = abs(scale_x - scale_y) > 1e-3
    return K, aspect_changed


def undistortion_maps(
    intrinsics: Intrinsics, size: tuple[int, int] | None = None, balance: float = 0.0
) -> tuple[np.ndarray, np.ndarray, np.ndarray, bool]:
    """Build the remap tables for one camera.

    Returns ``(map1, map2, new_K, aspect_changed)``. ``balance`` only applies
    to the fisheye model: 0 keeps every pixel valid (tighter field), 1 keeps
    the whole field of view (with invalid borders).
    """
    width, height = (size or intrinsics.image_size)
    width, height = int(width), int(height)
    if not width or not height:
        raise ValueError("taille d'image inconnue : impossible de calculer la dédistorsion")
    K, aspect_changed = _scaled_intrinsics(intrinsics, (width, height))
    dist = np.asarray(intrinsics.dist, dtype=np.float64)

    if intrinsics.model == FISHEYE:
        new_K = cv2.fisheye.estimateNewCameraMatrixForUndistortRectify(
            K, dist[:4], (width, height), np.eye(3), balance=float(balance)
        )
        map1, map2 = cv2.fisheye.initUndistortRectifyMap(
            K, dist[:4], np.eye(3), new_K, (width, height), cv2.CV_16SC2
        )
    else:
        new_K, _ = cv2.getOptimalNewCameraMatrix(K, dist, (width, height), float(balance))
        map1, map2 = cv2.initUndistortRectifyMap(
            K, dist, None, new_K, (width, height), cv2.CV_16SC2
        )
    return map1, map2, np.asarray(new_K, dtype=np.float64), aspect_changed


def undistort_image(image: np.ndarray, intrinsics: Intrinsics, balance: float = 0.0) -> np.ndarray:
    """Undistort a single image (convenience wrapper; builds the maps each call)."""
    height, width = image.shape[:2]
    map1, map2, _, _ = undistortion_maps(intrinsics, (width, height), balance)
    return cv2.remap(image, map1, map2, cv2.INTER_LINEAR)


def undistorted_intrinsics(intrinsics: Intrinsics, size: tuple[int, int] | None = None, balance: float = 0.0) -> Intrinsics:
    """Intrinsics of the corrected image: new K, and no distortion left."""
    width, height = (size or intrinsics.image_size)
    _, _, new_K, _ = undistortion_maps(intrinsics, (int(width), int(height)), balance)
    return Intrinsics.from_K(new_K, np.zeros(len(intrinsics.dist)), (int(width), int(height)), intrinsics.model)


def undistort_video(
    video_path: str,
    output_path: str,
    intrinsics: Intrinsics,
    balance: float = 0.0,
    fourcc: str = "mp4v",
    progress=None,
    should_stop=None,
) -> dict:
    """Undistort every frame of a video, writing a new file.

    Returns a report: frame count, resolutions, the corrected intrinsic matrix
    and whether the footage resolution matched the calibration.
    """
    capture = cv2.VideoCapture(video_path)
    if not capture.isOpened():
        raise OSError(f"vidéo illisible : {video_path}")
    try:
        width = int(capture.get(cv2.CAP_PROP_FRAME_WIDTH))
        height = int(capture.get(cv2.CAP_PROP_FRAME_HEIGHT))
        fps = capture.get(cv2.CAP_PROP_FPS) or 30.0
        total = int(capture.get(cv2.CAP_PROP_FRAME_COUNT))
        map1, map2, new_K, aspect_changed = undistortion_maps(intrinsics, (width, height), balance)

        directory = os.path.dirname(os.path.abspath(output_path))
        os.makedirs(directory, exist_ok=True)
        writer = cv2.VideoWriter(output_path, cv2.VideoWriter_fourcc(*fourcc), fps, (width, height))
        if not writer.isOpened():
            raise OSError(f"impossible d'écrire la vidéo : {output_path} (codec {fourcc})")
        written = 0
        try:
            while True:
                ok, frame = capture.read()
                if not ok:
                    break
                if should_stop is not None and should_stop():
                    break
                writer.write(cv2.remap(frame, map1, map2, cv2.INTER_LINEAR))
                written += 1
                if progress is not None and total > 0:
                    progress(min(1.0, written / total))
        finally:
            writer.release()
    finally:
        capture.release()

    return {
        "frames": written,
        "size": (width, height),
        "calibrated_size": tuple(intrinsics.image_size),
        "resolution_matched": (width, height) == tuple(intrinsics.image_size),
        "aspect_changed": aspect_changed,
        "K": new_K,
        "fps": fps,
        "output": output_path,
    }
