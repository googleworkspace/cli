"""Single-camera intrinsic calibration (the initialisation step)."""

from __future__ import annotations

import numpy as np
import cv2

from .board import BoardSpec, Detection
from .models import DIST_NAMES, FISHEYE, Intrinsics, ParameterMask, PINHOLE, Pose


class CalibrationError(RuntimeError):
    """Raised when a calibration cannot be computed at all."""


def _flag(name: str) -> int:
    """Look a calibration flag up, tolerating the 4.x/5.x namespace move.

    OpenCV 4 exposes the fisheye flags as ``cv2.fisheye.CALIB_*``; OpenCV 5
    moved them to the top-level ``cv2`` namespace.
    """
    for namespace in (cv2.fisheye, cv2):
        value = getattr(namespace, name, None)
        if value is not None:
            return int(value)
    raise AttributeError(f"OpenCV flag not found: {name}")


def _pinhole_flags(mask: ParameterMask, guess: Intrinsics | None) -> int:
    flags = 0
    if guess is not None:
        flags |= cv2.CALIB_USE_INTRINSIC_GUESS
    if not mask.is_free("ar"):
        flags |= cv2.CALIB_FIX_ASPECT_RATIO
    if not mask.is_free("cx") and not mask.is_free("cy"):
        flags |= cv2.CALIB_FIX_PRINCIPAL_POINT
    if not mask.is_free("k1"):
        flags |= cv2.CALIB_FIX_K1
    if not mask.is_free("k2"):
        flags |= cv2.CALIB_FIX_K2
    if not mask.is_free("k3"):
        flags |= cv2.CALIB_FIX_K3
    if not (mask.is_free("p1") or mask.is_free("p2")):
        flags |= cv2.CALIB_ZERO_TANGENT_DIST
    return flags


def _fisheye_flags(mask: ParameterMask, guess: Intrinsics | None) -> int:
    flags = _flag("CALIB_RECOMPUTE_EXTRINSIC") | _flag("CALIB_FIX_SKEW")
    if guess is not None:
        flags |= _flag("CALIB_USE_INTRINSIC_GUESS")
    for i, name in enumerate(DIST_NAMES[FISHEYE]):
        if not mask.is_free(name):
            flags |= _flag(f"CALIB_FIX_K{i + 1}")
    if not mask.is_free("cx") and not mask.is_free("cy"):
        flags |= _flag("CALIB_FIX_PRINCIPAL_POINT")
    return flags


def calibrate_intrinsics(
    board: BoardSpec,
    detections: dict[int, Detection],
    image_size: tuple[int, int],
    model: str = PINHOLE,
    mask: ParameterMask | None = None,
    guess: Intrinsics | None = None,
) -> tuple[Intrinsics, dict[int, Pose], float, dict[str, float]]:
    """Calibrate one camera from its detections.

    ``detections`` maps a pose index to the board points seen at that pose.
    Returns ``(intrinsics, board pose per view, rms, sigmas)`` where the poses
    map board coordinates into this camera's frame.
    """
    mask = mask or ParameterMask()
    keys = sorted(detections)
    obj_all = board.object_points()
    object_points, image_points = [], []
    for k in keys:
        det = detections[k]
        object_points.append(obj_all[np.asarray(det.ids, dtype=int)].astype(np.float64))
        image_points.append(np.asarray(det.points, dtype=np.float64).reshape(-1, 2))
    if len(object_points) < 3:
        raise CalibrationError(
            f"au moins 3 vues détectées sont nécessaires (reçu {len(object_points)})"
        )

    if model == FISHEYE:
        intr, rvecs, tvecs, rms, sigmas = _calibrate_fisheye(
            object_points, image_points, image_size, mask, guess
        )
    else:
        intr, rvecs, tvecs, rms, sigmas = _calibrate_pinhole(
            object_points, image_points, image_size, mask, guess
        )
    poses = {k: Pose(rvecs[i], tvecs[i]) for i, k in enumerate(keys)}
    return intr, poses, rms, sigmas


def _calibrate_pinhole(object_points, image_points, image_size, mask, guess):
    # cv2.calibrateCamera insists on Point3f/Point2f, i.e. float32 buffers.
    obj = [p.reshape(-1, 1, 3).astype(np.float32) for p in object_points]
    img = [p.reshape(-1, 1, 2).astype(np.float32) for p in image_points]
    if guess is None:
        guess = Intrinsics.guess(image_size, PINHOLE)
    K = guess.K.copy()
    dist = np.zeros(5, dtype=np.float64)
    dist[: len(guess.dist)] = guess.dist[:5]
    flags = _pinhole_flags(mask, guess)
    rms, K, dist, rvecs, tvecs, sd_intr, _, _ = cv2.calibrateCameraExtended(
        obj, img, tuple(image_size), K, dist, flags=flags
    )
    intr = Intrinsics.from_K(K, np.asarray(dist).ravel()[:5], image_size, PINHOLE)
    sd = np.asarray(sd_intr, dtype=np.float64).ravel()
    # OpenCV order: fx, fy, cx, cy, k1, k2, p1, p2, k3, ...
    sigmas = {
        "f": float(sd[0]),
        "ar": float(abs(sd[1] / intr.f)) if intr.f else 0.0,
        "cx": float(sd[2]),
        "cy": float(sd[3]),
        "alpha": 0.0,
    }
    for i, name in enumerate(DIST_NAMES[PINHOLE]):
        sigmas[name] = float(sd[4 + i]) if 4 + i < len(sd) else 0.0
    return intr, [r.ravel() for r in rvecs], [t.ravel() for t in tvecs], float(rms), sigmas


def _calibrate_fisheye(object_points, image_points, image_size, mask, guess):
    obj = [p.reshape(1, -1, 3).astype(np.float64) for p in object_points]
    img = [p.reshape(1, -1, 2).astype(np.float64) for p in image_points]
    if guess is None:
        guess = Intrinsics.guess(image_size, FISHEYE, fov_degrees=120.0)
    K = guess.K.copy()
    dist = np.zeros((4, 1), dtype=np.float64)
    dist[: len(guess.dist), 0] = guess.dist[:4]
    rvecs = [np.zeros((1, 1, 3)) for _ in obj]
    tvecs = [np.zeros((1, 1, 3)) for _ in obj]
    criteria = (cv2.TERM_CRITERIA_EPS + cv2.TERM_CRITERIA_MAX_ITER, 100, 1e-8)
    flags = _fisheye_flags(mask, guess)
    try:
        rms, K, dist, rvecs, tvecs = cv2.fisheye.calibrate(
            obj, img, tuple(image_size), K, dist, rvecs, tvecs, flags | _flag("CALIB_CHECK_COND"), criteria
        )
    except cv2.error:
        # CHECK_COND rejects ill-conditioned views outright; retry tolerantly so
        # the bundle adjustment can sort the outliers out later.
        rms, K, dist, rvecs, tvecs = cv2.fisheye.calibrate(
            obj, img, tuple(image_size), K, dist, rvecs, tvecs, flags, criteria
        )
    intr = Intrinsics.from_K(K, np.asarray(dist).ravel()[:4], image_size, FISHEYE)
    intr.alpha = 0.0
    # cv2.fisheye.calibrate does not report standard deviations; the bundle
    # adjustment computes them from the Jacobian afterwards.
    sigmas = {name: 0.0 for name in ("f", "ar", "cx", "cy", "alpha", *DIST_NAMES[FISHEYE])}
    return intr, [np.asarray(r).ravel() for r in rvecs], [np.asarray(t).ravel() for t in tvecs], float(rms), sigmas


def solve_pose(
    board: BoardSpec, detection: Detection, intrinsics: Intrinsics, guess: Pose | None = None
) -> Pose | None:
    """Board pose in the camera frame for a single view (``None`` if PnP fails)."""
    obj = board.object_points()[np.asarray(detection.ids, dtype=int)].astype(np.float64)
    pts = np.asarray(detection.points, dtype=np.float64).reshape(-1, 2)
    if len(obj) < 4:
        return None
    if intrinsics.model == FISHEYE:
        # Undistort to normalised coordinates, then solve with an identity K.
        from .geometry import undistort_points

        norm = undistort_points(pts, intrinsics.K, intrinsics.dist, FISHEYE)
        K, dist = np.eye(3), np.zeros(5)
        pts_used = norm
    else:
        K, dist = intrinsics.K, intrinsics.dist
        pts_used = pts
    ok, rvec, tvec = cv2.solvePnP(
        obj.reshape(-1, 1, 3),
        np.asarray(pts_used, dtype=np.float64).reshape(-1, 1, 2),
        K,
        np.asarray(dist, dtype=np.float64).reshape(-1, 1),
        flags=cv2.SOLVEPNP_ITERATIVE if guess is not None else cv2.SOLVEPNP_SQPNP,
        rvec=None if guess is None else guess.rvec.reshape(3, 1).copy(),
        tvec=None if guess is None else guess.tvec.reshape(3, 1).copy(),
        useExtrinsicGuess=guess is not None,
    )
    if not ok:
        return None
    return Pose(np.asarray(rvec).ravel(), np.asarray(tvec).ravel())
