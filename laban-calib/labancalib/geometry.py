"""Rigid-body geometry, camera projection and triangulation helpers.

All poses are stored the OpenCV way: a pose ``(rvec, tvec)`` maps a point from
the reference frame into the camera frame, i.e. ``X_cam = R(rvec) @ X_ref + tvec``.
Lengths are metres, angles radians.
"""

from __future__ import annotations

import numpy as np

try:  # pragma: no cover - exercised implicitly everywhere
    import cv2
except ImportError as exc:  # pragma: no cover
    raise ImportError(
        "OpenCV is required: pip install opencv-contrib-python"
    ) from exc


def rodrigues(rvec: np.ndarray) -> np.ndarray:
    """Rotation vector -> 3x3 rotation matrix."""
    R, _ = cv2.Rodrigues(np.asarray(rvec, dtype=np.float64).reshape(3, 1))
    return R


def inv_rodrigues(R: np.ndarray) -> np.ndarray:
    """3x3 rotation matrix -> rotation vector."""
    rvec, _ = cv2.Rodrigues(np.asarray(R, dtype=np.float64))
    return rvec.reshape(3)


def compose(rvec_a, tvec_a, rvec_b, tvec_b):
    """Return the pose ``A ∘ B`` (apply B first, then A)."""
    r, t, *_ = cv2.composeRT(
        np.asarray(rvec_b, dtype=np.float64).reshape(3, 1),
        np.asarray(tvec_b, dtype=np.float64).reshape(3, 1),
        np.asarray(rvec_a, dtype=np.float64).reshape(3, 1),
        np.asarray(tvec_a, dtype=np.float64).reshape(3, 1),
    )
    return r.reshape(3), t.reshape(3)


def invert(rvec, tvec):
    """Inverse of a rigid transform."""
    R = rodrigues(rvec)
    t = np.asarray(tvec, dtype=np.float64).reshape(3)
    return inv_rodrigues(R.T), -(R.T @ t)


def camera_centre(rvec, tvec) -> np.ndarray:
    """Position of the camera centre expressed in the reference frame."""
    R = rodrigues(rvec)
    return -(R.T @ np.asarray(tvec, dtype=np.float64).reshape(3))


def build_K(f: float, ar: float, cx: float, cy: float, alpha: float = 0.0) -> np.ndarray:
    """Assemble an intrinsic matrix from the (f, ar, cx, cy, alpha) parameters.

    ``ar`` is the aspect ratio fy/fx and ``alpha`` the normalised skew, so that
    ``K = [[f, alpha*f, cx], [0, ar*f, cy], [0, 0, 1]]``.
    """
    return np.array(
        [[f, alpha * f, cx], [0.0, ar * f, cy], [0.0, 0.0, 1.0]], dtype=np.float64
    )


def split_K(K: np.ndarray) -> tuple[float, float, float, float, float]:
    """Inverse of :func:`build_K`: returns ``(f, ar, cx, cy, alpha)``."""
    K = np.asarray(K, dtype=np.float64)
    f = float(K[0, 0])
    ar = float(K[1, 1] / f) if f else 1.0
    alpha = float(K[0, 1] / f) if f else 0.0
    return f, ar, float(K[0, 2]), float(K[1, 2]), alpha


def project_points(
    object_points: np.ndarray,
    rvec: np.ndarray,
    tvec: np.ndarray,
    K: np.ndarray,
    dist: np.ndarray,
    model: str = "pinhole",
) -> np.ndarray:
    """Project 3D points (N,3) into the image plane, returning (N,2) pixels.

    Implemented on top of :func:`project_batch` rather than
    ``cv2.projectPoints`` for one reason: OpenCV silently ignores the skew term
    ``K[0,1]``, so routing every projection through the same code keeps the
    optimiser and the overlays consistent with the reported ``alpha``. With
    ``alpha = 0`` the two agree to ~1e-13 px (see ``test_geometry.py``).
    """
    obj = np.asarray(object_points, dtype=np.float64).reshape(-1, 3)
    n = len(obj)
    R = np.repeat(rodrigues(rvec)[None], n, axis=0)
    t = np.repeat(np.asarray(tvec, dtype=np.float64).reshape(1, 3), n, axis=0)
    f, ar, cx, cy, alpha = split_K(K)
    core = np.repeat(np.array([[f, ar, cx, cy, alpha]]), n, axis=0)
    width = 4 if model == "fisheye" else 5
    d = np.zeros(width)
    flat = np.asarray(dist, dtype=np.float64).ravel()[:width]
    d[: len(flat)] = flat
    return project_batch(obj, R, t, core, np.repeat(d[None], n, axis=0), model)


def distort_normalised(
    x: np.ndarray, y: np.ndarray, coeffs: np.ndarray, model: str = "pinhole"
) -> tuple[np.ndarray, np.ndarray]:
    """Apply the lens model to normalised coordinates, elementwise.

    ``coeffs`` is (N, 5) for the pinhole model (k1 k2 p1 p2 k3) and (N, 4) for
    the fisheye one (k1..k4). Vectorised counterpart of the OpenCV routines —
    it is what makes the bundle adjustment fast enough to stay interactive.
    """
    if model == "fisheye":
        r = np.sqrt(x * x + y * y)
        theta = np.arctan(r)
        t2 = theta * theta
        theta_d = theta * (
            1.0
            + coeffs[:, 0] * t2
            + coeffs[:, 1] * t2**2
            + coeffs[:, 2] * t2**3
            + coeffs[:, 3] * t2**4
        )
        # scale -> 1 at the optical axis, where r and theta vanish together
        scale = np.where(r > 1e-12, theta_d / np.where(r > 1e-12, r, 1.0), 1.0)
        return x * scale, y * scale
    k1, k2, p1, p2, k3 = (coeffs[:, i] for i in range(5))
    r2 = x * x + y * y
    radial = 1.0 + k1 * r2 + k2 * r2**2 + k3 * r2**3
    xd = x * radial + 2.0 * p1 * x * y + p2 * (r2 + 2.0 * x * x)
    yd = y * radial + p1 * (r2 + 2.0 * y * y) + 2.0 * p2 * x * y
    return xd, yd


def project_batch(
    object_points: np.ndarray,
    rotations: np.ndarray,
    translations: np.ndarray,
    core: np.ndarray,
    coeffs: np.ndarray,
    model: str = "pinhole",
) -> np.ndarray:
    """Project N points, each with its own pose and intrinsics, in one shot.

    ``rotations`` is (N,3,3), ``translations`` (N,3), ``core`` (N,5) holding
    ``f, ar, cx, cy, alpha`` per point, and ``coeffs`` the distortion
    coefficients per point. Returns (N,2) pixels.
    """
    cam = np.einsum("nij,nj->ni", rotations, object_points) + translations
    z = cam[:, 2]
    safe_z = np.where(np.abs(z) < 1e-9, np.sign(z) * 1e-9 + 1e-12, z)
    x, y = cam[:, 0] / safe_z, cam[:, 1] / safe_z
    xd, yd = distort_normalised(x, y, coeffs, model)
    f, ar, cx, cy, alpha = (core[:, i] for i in range(5))
    u = f * xd + alpha * f * yd + cx
    v = ar * f * yd + cy
    return np.stack([u, v], axis=1)


def undistort_points(
    image_points: np.ndarray, K: np.ndarray, dist: np.ndarray, model: str = "pinhole"
) -> np.ndarray:
    """Undistort (N,2) pixels into normalised camera coordinates (N,2)."""
    pts = np.asarray(image_points, dtype=np.float64).reshape(-1, 1, 2)
    K = np.asarray(K, dtype=np.float64)
    dist = np.asarray(dist, dtype=np.float64).reshape(-1, 1)
    if model == "fisheye":
        out = cv2.fisheye.undistortPoints(pts, K, dist[:4])
    else:
        out = cv2.undistortPoints(pts, K, dist)
    return np.asarray(out, dtype=np.float64).reshape(-1, 2)


def projection_matrix(rvec, tvec, K) -> np.ndarray:
    """3x4 projection matrix ``K [R|t]`` (pinhole only, for triangulation)."""
    R = rodrigues(rvec)
    t = np.asarray(tvec, dtype=np.float64).reshape(3, 1)
    return np.asarray(K, dtype=np.float64) @ np.hstack([R, t])


def triangulate(
    observations: list[tuple[np.ndarray, np.ndarray, np.ndarray]],
) -> np.ndarray:
    """Linear (DLT) triangulation of one 3D point seen by >= 2 cameras.

    ``observations`` is a list of ``(rvec, tvec, xy_normalised)`` where the
    observation is already undistorted and expressed in normalised camera
    coordinates (see :func:`undistort_points`). Returns the 3D point in the
    reference frame.
    """
    if len(observations) < 2:
        raise ValueError("triangulation needs at least two observations")
    rows = []
    for rvec, tvec, xy in observations:
        R = rodrigues(rvec)
        t = np.asarray(tvec, dtype=np.float64).reshape(3, 1)
        P = np.hstack([R, t])  # normalised coordinates -> K is identity
        x, y = float(xy[0]), float(xy[1])
        rows.append(x * P[2] - P[0])
        rows.append(y * P[2] - P[1])
    A = np.asarray(rows, dtype=np.float64)
    _, _, vt = np.linalg.svd(A)
    X = vt[-1]
    if abs(X[3]) < 1e-12:
        raise ValueError("degenerate triangulation (point at infinity)")
    return X[:3] / X[3]


def rms(values: np.ndarray) -> float:
    """Root-mean-square of a residual array (empty -> nan)."""
    values = np.asarray(values, dtype=np.float64).ravel()
    if values.size == 0:
        return float("nan")
    return float(np.sqrt(np.mean(values**2)))
