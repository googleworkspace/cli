"""Camera models, parameter blocks and calibration results.

The intrinsic parameterisation follows the one used by the reference tools in
the field — ``f``, ``ar`` (aspect ratio fy/fx), ``cx``, ``cy``, ``alpha``
(normalised skew) plus the distortion coefficients — so every scalar reported
in the UI has a physical meaning and its own standard deviation.
"""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

from . import geometry

PINHOLE = "pinhole"
FISHEYE = "fisheye"
CAMERA_MODELS = (PINHOLE, FISHEYE)

MODEL_LABELS = {
    PINHOLE: "Sténopé (Brown-Conrady, k1 k2 p1 p2 k3)",
    FISHEYE: "Fisheye équidistant OpenCV (k1 k2 k3 k4)",
}

DIST_NAMES = {
    PINHOLE: ("k1", "k2", "p1", "p2", "k3"),
    FISHEYE: ("k1", "k2", "k3", "k4"),
}

CORE_NAMES = ("f", "ar", "cx", "cy", "alpha")


def parameter_names(model: str) -> tuple[str, ...]:
    return CORE_NAMES + DIST_NAMES[model]


@dataclass
class Intrinsics:
    """Intrinsic parameters of one camera."""

    model: str = PINHOLE
    image_size: tuple[int, int] = (0, 0)  # (width, height)
    f: float = 0.0
    ar: float = 1.0
    cx: float = 0.0
    cy: float = 0.0
    alpha: float = 0.0
    dist: np.ndarray = field(default_factory=lambda: np.zeros(5))

    def __post_init__(self) -> None:
        if self.model not in CAMERA_MODELS:
            raise ValueError(f"unknown camera model: {self.model!r}")
        self.dist = np.asarray(self.dist, dtype=np.float64).ravel()
        n = len(DIST_NAMES[self.model])
        if len(self.dist) < n:
            self.dist = np.pad(self.dist, (0, n - len(self.dist)))
        self.dist = self.dist[:n]
        self.image_size = (int(self.image_size[0]), int(self.image_size[1]))
        if self.model == FISHEYE:
            # OpenCV's fisheye projection takes the skew as a separate argument
            # and ignores K[0,1]; keeping alpha at zero avoids a silent mismatch.
            self.alpha = 0.0

    # -- conversions ------------------------------------------------------

    @property
    def K(self) -> np.ndarray:
        return geometry.build_K(self.f, self.ar, self.cx, self.cy, self.alpha)

    @property
    def fx(self) -> float:
        return self.f

    @property
    def fy(self) -> float:
        return self.f * self.ar

    @classmethod
    def from_K(cls, K, dist, image_size, model: str = PINHOLE) -> "Intrinsics":
        f, ar, cx, cy, alpha = geometry.split_K(K)
        return cls(
            model=model, image_size=tuple(image_size), f=f, ar=ar, cx=cx, cy=cy, alpha=alpha,
            dist=np.asarray(dist, dtype=np.float64).ravel(),
        )

    @classmethod
    def guess(cls, image_size, model: str = PINHOLE, fov_degrees: float = 60.0) -> "Intrinsics":
        """Rough initial guess from the image size and an assumed field of view."""
        w, h = int(image_size[0]), int(image_size[1])
        f = 0.5 * w / np.tan(np.radians(fov_degrees) / 2.0)
        return cls(model=model, image_size=(w, h), f=float(f), ar=1.0, cx=w / 2.0, cy=h / 2.0)

    def values(self) -> dict[str, float]:
        out = {"f": self.f, "ar": self.ar, "cx": self.cx, "cy": self.cy, "alpha": self.alpha}
        out.update({n: float(v) for n, v in zip(DIST_NAMES[self.model], self.dist)})
        return out

    def set_values(self, values: dict[str, float]) -> None:
        for name in CORE_NAMES:
            if name in values:
                setattr(self, name, float(values[name]))
        for i, name in enumerate(DIST_NAMES[self.model]):
            if name in values:
                self.dist[i] = float(values[name])

    def project(self, object_points, rvec, tvec) -> np.ndarray:
        return geometry.project_points(object_points, rvec, tvec, self.K, self.dist, self.model)

    def to_dict(self) -> dict:
        return {
            "model": self.model,
            "image_size": list(self.image_size),
            **{k: float(v) for k, v in self.values().items()},
            "dist": [float(v) for v in self.dist],
        }

    @classmethod
    def from_dict(cls, data: dict) -> "Intrinsics":
        return cls(
            model=data.get("model", PINHOLE),
            image_size=tuple(data.get("image_size", (0, 0))),
            f=float(data.get("f", 0.0)),
            ar=float(data.get("ar", 1.0)),
            cx=float(data.get("cx", 0.0)),
            cy=float(data.get("cy", 0.0)),
            alpha=float(data.get("alpha", 0.0)),
            dist=np.asarray(data.get("dist", []), dtype=np.float64),
        )


@dataclass
class ParameterMask:
    """Which intrinsic parameters the optimiser is allowed to move."""

    free: set[str] = field(default_factory=lambda: {"f", "ar", "cx", "cy", "k1", "k2", "p1", "p2", "k3", "k4"})

    def is_free(self, name: str) -> bool:
        return name in self.free

    def free_names(self, model: str) -> list[str]:
        names = [n for n in parameter_names(model) if n in self.free]
        if model == FISHEYE:
            names = [n for n in names if n != "alpha"]
        return names

    def to_dict(self) -> dict:
        return {"free": sorted(self.free)}

    @classmethod
    def from_dict(cls, data: dict) -> "ParameterMask":
        return cls(free=set(data.get("free", [])))


@dataclass
class Pose:
    """Rigid transform, reference frame -> camera (or board -> reference)."""

    rvec: np.ndarray = field(default_factory=lambda: np.zeros(3))
    tvec: np.ndarray = field(default_factory=lambda: np.zeros(3))

    def __post_init__(self) -> None:
        self.rvec = np.asarray(self.rvec, dtype=np.float64).reshape(3)
        self.tvec = np.asarray(self.tvec, dtype=np.float64).reshape(3)

    @property
    def R(self) -> np.ndarray:
        return geometry.rodrigues(self.rvec)

    @property
    def centre(self) -> np.ndarray:
        return geometry.camera_centre(self.rvec, self.tvec)

    def inverse(self) -> "Pose":
        return Pose(*geometry.invert(self.rvec, self.tvec))

    def compose(self, other: "Pose") -> "Pose":
        """``self ∘ other``: apply ``other`` first."""
        return Pose(*geometry.compose(self.rvec, self.tvec, other.rvec, other.tvec))

    def matrix(self) -> np.ndarray:
        T = np.eye(4)
        T[:3, :3] = self.R
        T[:3, 3] = self.tvec
        return T

    def to_dict(self) -> dict:
        return {"rvec": self.rvec.tolist(), "tvec": self.tvec.tolist()}

    @classmethod
    def from_dict(cls, data: dict) -> "Pose":
        return cls(rvec=data["rvec"], tvec=data["tvec"])


@dataclass
class CameraCalibration:
    """Everything known about one camera after optimisation."""

    name: str
    intrinsics: Intrinsics
    pose: Pose = field(default_factory=Pose)  # reference camera -> this camera
    sigmas: dict[str, float] = field(default_factory=dict)
    rpe: float = float("nan")
    n_views: int = 0
    n_points: int = 0

    @property
    def baseline(self) -> float:
        return float(np.linalg.norm(self.pose.tvec))

    def to_dict(self) -> dict:
        return {
            "name": self.name,
            "intrinsics": self.intrinsics.to_dict(),
            "pose": self.pose.to_dict(),
            "sigmas": {k: float(v) for k, v in self.sigmas.items()},
            "rpe": float(self.rpe),
            "n_views": int(self.n_views),
            "n_points": int(self.n_points),
        }

    @classmethod
    def from_dict(cls, data: dict) -> "CameraCalibration":
        return cls(
            name=data["name"],
            intrinsics=Intrinsics.from_dict(data["intrinsics"]),
            pose=Pose.from_dict(data.get("pose", {"rvec": [0, 0, 0], "tvec": [0, 0, 0]})),
            sigmas=dict(data.get("sigmas", {})),
            rpe=float(data.get("rpe", float("nan"))),
            n_views=int(data.get("n_views", 0)),
            n_points=int(data.get("n_points", 0)),
        )


@dataclass
class ViewResidual:
    """Per-view reprojection statistics, used by the RPE plots and the tree."""

    camera: int
    pose_index: int
    n_points: int
    rms: float
    max_error: float


@dataclass
class CalibrationResult:
    """Output of a full multi-camera optimisation."""

    cameras: list[CameraCalibration] = field(default_factory=list)
    board_poses: dict[int, Pose] = field(default_factory=dict)  # pose index -> board pose in reference frame
    residuals: list[ViewResidual] = field(default_factory=list)
    errors_xy: np.ndarray = field(default_factory=lambda: np.zeros((0, 2)))
    errors_camera: np.ndarray = field(default_factory=lambda: np.zeros(0, dtype=int))
    rpe: float = float("nan")
    converged: bool = False
    iterations: int = 0
    convergence: list[float] = field(default_factory=list)
    message: str = ""
    n_observations: int = 0
    n_parameters: int = 0

    def camera_rpe(self, index: int) -> float:
        mask = self.errors_camera == index
        if not np.any(mask):
            return float("nan")
        return geometry.rms(np.linalg.norm(self.errors_xy[mask], axis=1))

    def to_dict(self) -> dict:
        return {
            "cameras": [c.to_dict() for c in self.cameras],
            "board_poses": {str(k): v.to_dict() for k, v in self.board_poses.items()},
            "residuals": [
                {
                    "camera": r.camera,
                    "pose_index": r.pose_index,
                    "n_points": r.n_points,
                    "rms": r.rms,
                    "max_error": r.max_error,
                }
                for r in self.residuals
            ],
            "rpe": float(self.rpe),
            "converged": bool(self.converged),
            "iterations": int(self.iterations),
            "convergence": [float(v) for v in self.convergence],
            "message": self.message,
            "n_observations": int(self.n_observations),
            "n_parameters": int(self.n_parameters),
        }

    @classmethod
    def from_dict(cls, data: dict) -> "CalibrationResult":
        return cls(
            cameras=[CameraCalibration.from_dict(c) for c in data.get("cameras", [])],
            board_poses={int(k): Pose.from_dict(v) for k, v in data.get("board_poses", {}).items()},
            residuals=[
                ViewResidual(
                    camera=r["camera"],
                    pose_index=r["pose_index"],
                    n_points=r["n_points"],
                    rms=r["rms"],
                    max_error=r["max_error"],
                )
                for r in data.get("residuals", [])
            ],
            rpe=float(data.get("rpe", float("nan"))),
            converged=bool(data.get("converged", False)),
            iterations=int(data.get("iterations", 0)),
            convergence=list(data.get("convergence", [])),
            message=data.get("message", ""),
            n_observations=int(data.get("n_observations", 0)),
            n_parameters=int(data.get("n_parameters", 0)),
        )
