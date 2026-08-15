"""The calibration project: cameras, images, detections, results, on disk.

A *pose* is one capture instant. All cameras of the rig contribute (at most)
one image per pose, and pose ``i`` is the ``i``-th image of every camera — the
same convention as the reference tools, and the reason synchronised capture
matters.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field

from .board import BoardSpec, Detection, DetectorOptions
from .bundle import BundleOptions, Observation
from .models import CalibrationResult, ParameterMask, PINHOLE

FORMAT_VERSION = 1


@dataclass
class CameraSource:
    """One camera of the rig and the images it contributed."""

    name: str
    images: list[str] = field(default_factory=list)
    folder: str = ""
    device: str = ""

    def image_at(self, pose_index: int) -> str | None:
        if 0 <= pose_index < len(self.images):
            return self.images[pose_index]
        return None

    def to_dict(self) -> dict:
        return {"name": self.name, "images": list(self.images), "folder": self.folder, "device": self.device}

    @classmethod
    def from_dict(cls, data: dict) -> "CameraSource":
        return cls(
            name=data.get("name", "caméra"),
            images=list(data.get("images", [])),
            folder=data.get("folder", ""),
            device=data.get("device", ""),
        )


@dataclass
class Project:
    """Everything the application holds in memory, and saves as ``.lcalib``."""

    board: BoardSpec = field(default_factory=BoardSpec)
    camera_model: str = PINHOLE
    cameras: list[CameraSource] = field(default_factory=list)
    detections: dict[tuple[int, int], Detection] = field(default_factory=dict)
    detector_options: DetectorOptions = field(default_factory=DetectorOptions)
    bundle_options: BundleOptions = field(default_factory=BundleOptions)
    parameter_mask: ParameterMask = field(default_factory=ParameterMask)
    result: CalibrationResult | None = None
    path: str = ""
    notes: str = ""

    # -- structure --------------------------------------------------------

    @property
    def n_cameras(self) -> int:
        return len(self.cameras)

    @property
    def n_poses(self) -> int:
        return max((len(c.images) for c in self.cameras), default=0)

    def image_at(self, camera: int, pose_index: int) -> str | None:
        if 0 <= camera < len(self.cameras):
            return self.cameras[camera].image_at(pose_index)
        return None

    def detection_at(self, camera: int, pose_index: int) -> Detection | None:
        return self.detections.get((camera, pose_index))

    def poses_with_detection(self, camera: int) -> list[int]:
        return sorted(p for (c, p) in self.detections if c == camera)

    def coverage(self) -> dict[int, int]:
        """Number of cameras that detected the target, per pose."""
        counts: dict[int, int] = {}
        for (_, pose_index) in self.detections:
            counts[pose_index] = counts.get(pose_index, 0) + 1
        return counts

    def observations(self, min_points: int = 4) -> list[Observation]:
        """Detections in the form the optimiser consumes."""
        out = []
        for (camera, pose_index), detection in sorted(self.detections.items()):
            if detection.count >= min_points:
                out.append(Observation.from_detection(camera, pose_index, detection))
        return out

    def detections_for(self, camera: int) -> dict[int, Detection]:
        return {p: d for (c, p), d in self.detections.items() if c == camera}

    def clear_detections(self) -> None:
        self.detections.clear()
        self.result = None

    def add_camera(self, name: str = "", images: list[str] | None = None, folder: str = "") -> CameraSource:
        camera = CameraSource(name=name or f"caméra {len(self.cameras)}", images=images or [], folder=folder)
        self.cameras.append(camera)
        return camera

    def remove_camera(self, index: int) -> None:
        if not 0 <= index < len(self.cameras):
            return
        del self.cameras[index]
        renumbered = {}
        for (camera, pose_index), detection in self.detections.items():
            if camera == index:
                continue
            renumbered[(camera - 1 if camera > index else camera, pose_index)] = detection
        self.detections = renumbered
        self.result = None

    def validate(self) -> list[str]:
        """Human-readable warnings about the current setup (never raises)."""
        issues: list[str] = []
        if not self.cameras:
            issues.append("aucune caméra définie")
        counts = {len(c.images) for c in self.cameras}
        if len(counts) > 1:
            detail = ", ".join(f"{c.name}: {len(c.images)}" for c in self.cameras)
            issues.append(
                "les caméras n'ont pas le même nombre d'images — la pose i doit être "
                f"la i-ème image de chaque caméra ({detail})"
            )
        if any(len(c.images) == 0 for c in self.cameras):
            issues.append("au moins une caméra n'a aucune image")
        return issues

    # -- persistence ------------------------------------------------------

    def to_dict(self) -> dict:
        return {
            "format": FORMAT_VERSION,
            "board": self.board.to_dict(),
            "camera_model": self.camera_model,
            "cameras": [c.to_dict() for c in self.cameras],
            "detections": [
                {"camera": c, "pose": p, **d.to_dict()} for (c, p), d in sorted(self.detections.items())
            ],
            "detector_options": vars(self.detector_options),
            "bundle_options": vars(self.bundle_options),
            "parameter_mask": self.parameter_mask.to_dict(),
            "result": self.result.to_dict() if self.result else None,
            "notes": self.notes,
        }

    @classmethod
    def from_dict(cls, data: dict) -> "Project":
        version = int(data.get("format", 0))
        if version > FORMAT_VERSION:
            raise ValueError(
                f"projet écrit par une version plus récente (format {version} > {FORMAT_VERSION})"
            )
        project = cls(
            board=BoardSpec.from_dict(data.get("board", {})),
            camera_model=data.get("camera_model", PINHOLE),
            cameras=[CameraSource.from_dict(c) for c in data.get("cameras", [])],
            parameter_mask=ParameterMask.from_dict(data.get("parameter_mask", {})),
            notes=data.get("notes", ""),
        )
        known_detector = set(vars(DetectorOptions()))
        project.detector_options = DetectorOptions(
            **{k: v for k, v in data.get("detector_options", {}).items() if k in known_detector}
        )
        known_bundle = set(vars(BundleOptions()))
        project.bundle_options = BundleOptions(
            **{k: v for k, v in data.get("bundle_options", {}).items() if k in known_bundle}
        )
        for entry in data.get("detections", []):
            project.detections[(int(entry["camera"]), int(entry["pose"]))] = Detection.from_dict(entry)
        if data.get("result"):
            project.result = CalibrationResult.from_dict(data["result"])
        return project

    def save(self, path: str) -> None:
        payload = self.to_dict()
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, ensure_ascii=False, indent=1)
        self.path = path

    @classmethod
    def load(cls, path: str) -> "Project":
        with open(path, encoding="utf-8") as handle:
            project = cls.from_dict(json.load(handle))
        project.path = path
        return project

    # -- convenience ------------------------------------------------------

    def set_camera_folders(self, folders: list[str]) -> None:
        """Replace the rig with one camera per folder (the ``Set Images`` action)."""
        from .sources import list_images

        self.cameras = []
        self.clear_detections()
        for folder in folders:
            images = list_images(folder)
            self.add_camera(name=os.path.basename(os.path.normpath(folder)) or "caméra", images=images, folder=folder)
