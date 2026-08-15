"""MediaPipe Pose -> 3D: reading landmarks and triangulating them on the rig.

MediaPipe gives, per camera and per frame, 33 BlazePose landmarks in **image**
coordinates normalised to [0, 1] (``x`` by the image width, ``y`` by its
height), each with a ``visibility`` score. This module turns those into the
pixel tracks :meth:`labancalib.triangulate.Rig.triangulate_tracks` consumes,
and back into named 3D trajectories.

Note that MediaPipe's ``pose_world_landmarks`` are deliberately *not* used:
they are metric but re-centred on the subject's hips and estimated from a
single view, so they carry no rig geometry. Triangulating the image landmarks
across the calibrated cameras is what yields true metric positions in the
studio frame.

Accepted inputs (see :func:`load_landmarks`):

* live MediaPipe results — legacy ``solutions.pose`` or the Tasks
  ``PoseLandmarker`` — passed as a list of per-frame results;
* JSON files, in any of the shapes those results are usually dumped to;
* ``.npy`` / ``.npz`` arrays of shape ``(n_frames, n_landmarks, 2..4)``.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass

import numpy as np

from .triangulate import Rig

# BlazePose full-body topology, in MediaPipe's own index order.
POSE_LANDMARKS: tuple[str, ...] = (
    "nez",
    "oeil_gauche_interne",
    "oeil_gauche",
    "oeil_gauche_externe",
    "oeil_droit_interne",
    "oeil_droit",
    "oeil_droit_externe",
    "oreille_gauche",
    "oreille_droite",
    "bouche_gauche",
    "bouche_droite",
    "epaule_gauche",
    "epaule_droite",
    "coude_gauche",
    "coude_droit",
    "poignet_gauche",
    "poignet_droit",
    "auriculaire_gauche",
    "auriculaire_droit",
    "index_gauche",
    "index_droit",
    "pouce_gauche",
    "pouce_droit",
    "hanche_gauche",
    "hanche_droite",
    "genou_gauche",
    "genou_droit",
    "cheville_gauche",
    "cheville_droite",
    "talon_gauche",
    "talon_droit",
    "pied_gauche",
    "pied_droit",
)

N_LANDMARKS = len(POSE_LANDMARKS)

# Segments, for plotting and for the segment-length check below.
POSE_CONNECTIONS: tuple[tuple[int, int], ...] = (
    (11, 12), (11, 13), (13, 15), (12, 14), (14, 16),
    (11, 23), (12, 24), (23, 24),
    (23, 25), (25, 27), (27, 29), (29, 31), (27, 31),
    (24, 26), (26, 28), (28, 30), (30, 32), (28, 32),
    (15, 17), (15, 19), (15, 21), (16, 18), (16, 20), (16, 22),
    (0, 11), (0, 12),
)


class MediaPipeImportError(ValueError):
    """Raised when a landmark source cannot be interpreted."""


# -- reading --------------------------------------------------------------


def landmarks_to_array(frame_result, person: int = 0, n_landmarks: int = N_LANDMARKS) -> np.ndarray:
    """One frame of MediaPipe output -> ``(n_landmarks, 4)`` of x, y, z, visibility.

    Accepts a legacy ``solutions.pose`` result, a Tasks ``PoseLandmarkerResult``,
    a bare landmark list, or a per-frame dict/array. Missing frames (no person
    detected) yield an all-``nan`` block, which is exactly what the
    triangulation treats as "not seen".
    """
    empty = np.full((n_landmarks, 4), np.nan)
    if frame_result is None:
        return empty

    landmarks = _extract_landmark_list(frame_result, person)
    if landmarks is None or len(landmarks) == 0:
        return empty

    out = np.full((max(n_landmarks, len(landmarks)), 4), np.nan)
    for index, landmark in enumerate(landmarks):
        out[index] = _landmark_values(landmark)
    return out[:n_landmarks]


def _extract_landmark_list(frame_result, person: int):
    """Dig the landmark list out of the many shapes MediaPipe results take."""
    if isinstance(frame_result, np.ndarray):
        return list(frame_result)
    if isinstance(frame_result, dict):
        for key in ("landmarks", "pose_landmarks", "keypoints", "points"):
            if key in frame_result:
                return _maybe_person(frame_result[key], person)
        return None
    # Tasks API: .pose_landmarks is a list per detected person
    for attribute in ("pose_landmarks", "landmark"):
        value = getattr(frame_result, attribute, None)
        if value is None:
            continue
        # legacy: NormalizedLandmarkList with a .landmark field
        inner = getattr(value, "landmark", None)
        if inner is not None:
            return list(inner)
        return _maybe_person(value, person)
    if isinstance(frame_result, (list, tuple)):
        return _maybe_person(frame_result, person)
    return None


def _maybe_person(value, person: int):
    """Unwrap the per-person nesting of the Tasks API when it is present."""
    items = list(value)
    if not items:
        return []
    first = items[0]
    nested = isinstance(first, (list, tuple)) and first and _looks_like_landmark(first[0])
    if nested:
        if person >= len(items):
            return []
        return list(items[person])
    return items


def _looks_like_landmark(value) -> bool:
    return isinstance(value, dict) or hasattr(value, "x") or (
        isinstance(value, (list, tuple, np.ndarray)) and len(value) >= 2
    )


def _landmark_values(landmark) -> np.ndarray:
    if isinstance(landmark, dict):
        return np.array(
            [
                float(landmark.get("x", np.nan)),
                float(landmark.get("y", np.nan)),
                float(landmark.get("z", np.nan)),
                float(landmark.get("visibility", landmark.get("score", 1.0))),
            ]
        )
    if hasattr(landmark, "x"):
        return np.array(
            [
                float(landmark.x),
                float(landmark.y),
                float(getattr(landmark, "z", np.nan)),
                float(getattr(landmark, "visibility", 1.0)),
            ]
        )
    values = np.asarray(landmark, dtype=np.float64).ravel()
    out = np.full(4, np.nan)
    out[: min(4, len(values))] = values[:4]
    if len(values) < 4:
        out[3] = 1.0
    return out


def sequence_to_array(results, person: int = 0, n_landmarks: int = N_LANDMARKS) -> np.ndarray:
    """A whole camera's sequence -> ``(n_frames, n_landmarks, 4)``."""
    frames = [landmarks_to_array(result, person, n_landmarks) for result in results]
    if not frames:
        return np.zeros((0, n_landmarks, 4))
    return np.stack(frames)


def load_landmarks(path: str, person: int = 0, n_landmarks: int = N_LANDMARKS) -> np.ndarray:
    """Read one camera's landmarks from disk -> ``(n_frames, n_landmarks, 4)``."""
    extension = os.path.splitext(path)[1].lower()
    if extension == ".npy":
        return _from_array(np.load(path), n_landmarks)
    if extension == ".npz":
        archive = np.load(path)
        key = next((k for k in ("landmarks", "tracks", "arr_0") if k in archive), None)
        if key is None:
            raise MediaPipeImportError(
                f"{path} : archive .npz sans tableau reconnu (attendu « landmarks » ou « arr_0 »)"
            )
        return _from_array(archive[key], n_landmarks)
    if extension in (".json", ".jsonl", ".txt"):
        return _from_json(path, person, n_landmarks)
    raise MediaPipeImportError(
        f"{path} : extension non prise en charge (attendu .json, .jsonl, .npy ou .npz)"
    )


def save_landmarks(path: str, landmarks: np.ndarray, source: str = "") -> str:
    """Write ``(n_frames, n_landmarks, 4)`` landmarks in a form we read back.

    JSON keeps MediaPipe's own field names, so the file is also readable by any
    other tool expecting a landmark dump; ``.npy``/``.npz`` keep it compact.
    """
    data = np.asarray(landmarks, dtype=np.float64)
    if path.lower().endswith(".npy"):
        np.save(path, data)
        return path
    if path.lower().endswith(".npz"):
        np.savez_compressed(path, landmarks=data)
        return path
    frames = [
        {
            "landmarks": [
                {"x": float(x), "y": float(y), "z": float(z), "visibility": float(v)}
                for x, y, z, v in frame
            ]
        }
        for frame in data
    ]
    payload = {
        "format": "mediapipe/pose_landmarks",
        "coordinates": "normalised",
        "n_frames": len(frames),
        "landmark_names": list(POSE_LANDMARKS[: data.shape[1]]),
        "source": source,
        "frames": frames,
    }
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, ensure_ascii=False)
    return path


def _from_array(array: np.ndarray, n_landmarks: int) -> np.ndarray:
    data = np.asarray(array, dtype=np.float64)
    if data.ndim != 3 or data.shape[2] < 2:
        raise MediaPipeImportError(
            f"tableau de forme {data.shape} : attendu (n_trames, n_points, 2 à 4)"
        )
    out = np.full((len(data), n_landmarks, 4), np.nan)
    width = min(4, data.shape[2])
    count = min(n_landmarks, data.shape[1])
    out[:, :count, :width] = data[:, :count, :width]
    if data.shape[2] < 4:
        out[:, :count, 3] = 1.0
    return out


def _from_json(path: str, person: int, n_landmarks: int) -> np.ndarray:
    with open(path, encoding="utf-8") as handle:
        text = handle.read().strip()
    if not text:
        raise MediaPipeImportError(f"{path} : fichier vide")
    try:
        data = json.loads(text)
    except json.JSONDecodeError:
        # JSON Lines: one frame per line
        data = [json.loads(line) for line in text.splitlines() if line.strip()]
    if isinstance(data, dict):
        for key in ("frames", "results", "poses", "data"):
            if key in data:
                data = data[key]
                break
        else:
            data = [data]
    if not isinstance(data, list):
        raise MediaPipeImportError(f"{path} : structure JSON non reconnue")
    return sequence_to_array(data, person, n_landmarks)


# -- assembling the rig-wide tracks ---------------------------------------


@dataclass
class TrackSet:
    """Pixel tracks of every camera, aligned on a common frame index."""

    pixels: np.ndarray  # (n_cameras, n_frames, n_landmarks, 2)
    visibility: np.ndarray  # (n_cameras, n_frames, n_landmarks)
    names: tuple[str, ...] = POSE_LANDMARKS

    @property
    def n_frames(self) -> int:
        return int(self.pixels.shape[1])

    def seen_by(self) -> np.ndarray:
        """Number of cameras with a usable observation, per frame and landmark."""
        return np.sum(np.all(np.isfinite(self.pixels), axis=3), axis=0)


def build_tracks(
    sequences: list[np.ndarray],
    rig: Rig | None = None,
    image_sizes: list[tuple[int, int]] | None = None,
    min_visibility: float = 0.5,
    offsets: list[int] | None = None,
    normalised: bool | None = None,
) -> TrackSet:
    """Turn one landmark array per camera into aligned pixel tracks.

    ``sequences[c]`` has shape ``(n_frames, n_landmarks, 4)`` as returned by
    :func:`load_landmarks`. Coordinates are scaled to pixels using each
    camera's image size — taken from ``rig`` unless ``image_sizes`` is given.

    ``offsets`` shifts a camera's sequence in frames (positive means that
    camera started recording later), which is how a small synchronisation error
    between captation files is compensated. Landmarks below ``min_visibility``
    are dropped, and frames are padded with ``nan`` so every camera shares one
    frame index.
    """
    if not sequences:
        raise MediaPipeImportError("aucune séquence de points fournie")
    sizes = _resolve_sizes(len(sequences), rig, image_sizes)
    offsets = list(offsets or [0] * len(sequences))
    if len(offsets) != len(sequences):
        raise MediaPipeImportError(
            f"{len(offsets)} décalage(s) pour {len(sequences)} caméra(s)"
        )

    n_landmarks = max(s.shape[1] for s in sequences)
    lengths = [len(s) + offset for s, offset in zip(sequences, offsets)]
    n_frames = max(lengths) if lengths else 0

    pixels = np.full((len(sequences), n_frames, n_landmarks, 2), np.nan)
    visibility = np.zeros((len(sequences), n_frames, n_landmarks))

    for camera, (sequence, offset) in enumerate(zip(sequences, offsets)):
        if len(sequence) == 0:
            continue
        width, height = sizes[camera]
        scale = _scale_factors(sequence, width, height, normalised)
        start, stop = max(0, offset), max(0, offset) + len(sequence)
        count = sequence.shape[1]
        xy = sequence[:, :, :2] * scale
        scores = sequence[:, :, 3]
        usable = np.isfinite(xy).all(axis=2) & (np.nan_to_num(scores, nan=1.0) >= min_visibility)
        xy = np.where(usable[:, :, None], xy, np.nan)
        pixels[camera, start:stop, :count] = xy
        visibility[camera, start:stop, :count] = np.nan_to_num(scores, nan=0.0)
    return TrackSet(pixels=pixels, visibility=visibility, names=POSE_LANDMARKS[:n_landmarks])


def _resolve_sizes(n_cameras, rig, image_sizes) -> list[tuple[int, int]]:
    if image_sizes is not None:
        if len(image_sizes) != n_cameras:
            raise MediaPipeImportError(
                f"{len(image_sizes)} taille(s) d'image pour {n_cameras} caméra(s)"
            )
        return [tuple(size) for size in image_sizes]
    if rig is None:
        raise MediaPipeImportError("fournissez « rig » ou « image_sizes » pour convertir en pixels")
    if rig.n_cameras != n_cameras:
        raise MediaPipeImportError(
            f"{n_cameras} séquence(s) pour {rig.n_cameras} caméra(s) étalonnée(s) — "
            "l'ordre des fichiers doit suivre celui du dispositif"
        )
    sizes = [tuple(intr.image_size) for intr in rig.intrinsics]
    for index, (width, height) in enumerate(sizes):
        if not width or not height:
            raise MediaPipeImportError(f"taille d'image inconnue pour la caméra {index}")
    return sizes


def _scale_factors(sequence: np.ndarray, width: int, height: int, normalised: bool | None) -> np.ndarray:
    """Scale normalised MediaPipe coordinates to pixels (identity if already pixels)."""
    if normalised is None:
        finite = sequence[:, :, :2][np.isfinite(sequence[:, :, :2])]
        # MediaPipe normalised coordinates live in [0, 1] and only drift a
        # little outside when a joint leaves the frame.
        normalised = bool(finite.size == 0 or np.nanmax(np.abs(finite)) <= 4.0)
    return np.array([width, height], dtype=np.float64) if normalised else np.ones(2)


# -- triangulation --------------------------------------------------------


@dataclass
class PoseReconstruction:
    """3D trajectories reconstructed from the MediaPipe tracks."""

    points: np.ndarray  # (n_frames, n_landmarks, 3) metres, nan when unsolved
    errors: np.ndarray  # (n_frames, n_landmarks) pixels, nan when unsolved
    seen_by: np.ndarray  # (n_frames, n_landmarks) usable cameras
    names: tuple[str, ...]

    @property
    def n_frames(self) -> int:
        return int(self.points.shape[0])

    def completeness(self) -> float:
        """Share of landmark samples that could be reconstructed."""
        total = self.points.shape[0] * self.points.shape[1]
        return float(np.count_nonzero(np.isfinite(self.points[:, :, 0])) / total) if total else 0.0

    def segment_lengths(self) -> dict[str, float]:
        """Median length of each skeleton segment, in metres.

        Constant segment lengths are the honest sanity check on a rig: bones do
        not stretch, so a large spread means the calibration or the association
        across cameras is off.
        """
        out: dict[str, float] = {}
        for a, b in POSE_CONNECTIONS:
            if a >= len(self.names) or b >= len(self.names):
                continue
            lengths = np.linalg.norm(self.points[:, a] - self.points[:, b], axis=1)
            lengths = lengths[np.isfinite(lengths)]
            if lengths.size:
                out[f"{self.names[a]}–{self.names[b]}"] = float(np.median(lengths))
        return out

    def save(self, path: str) -> str:
        """Write the reconstruction as ``.npz`` or ``.csv``."""
        if path.lower().endswith(".csv"):
            with open(path, "w", encoding="utf-8") as handle:
                handle.write("trame,point,nom,x,y,z,erreur_px,cameras\n")
                for frame in range(self.points.shape[0]):
                    for index, name in enumerate(self.names):
                        x, y, z = self.points[frame, index]
                        handle.write(
                            f"{frame},{index},{name},{x:.6f},{y:.6f},{z:.6f},"
                            f"{self.errors[frame, index]:.4f},{int(self.seen_by[frame, index])}\n"
                        )
            return path
        np.savez_compressed(
            path,
            points=self.points,
            errors=self.errors,
            seen_by=self.seen_by,
            names=np.array(self.names),
        )
        return path


def triangulate_mediapipe(
    rig: Rig,
    sources: list,
    min_visibility: float = 0.5,
    offsets: list[int] | None = None,
    person: int = 0,
    max_error: float = 0.0,
    normalised: bool | None = None,
) -> PoseReconstruction:
    """Reconstruct 3D joint trajectories from MediaPipe output, in one call.

    ``sources`` is one entry per camera, **in the order of the calibrated rig**;
    each entry is a path (JSON/JSONL/NPY/NPZ), an array, or a list of live
    MediaPipe results. ``max_error`` (pixels, 0 disables) discards points whose
    reprojection error says the multi-view association cannot be trusted.
    """
    sequences = [_as_sequence(source, person) for source in sources]
    tracks = build_tracks(
        sequences, rig=rig, min_visibility=min_visibility, offsets=offsets, normalised=normalised
    )
    points, errors = rig.triangulate_tracks(tracks.pixels)
    if max_error > 0:
        rejected = np.isfinite(errors) & (errors > max_error)
        points[rejected] = np.nan
        errors[rejected] = np.nan
    return PoseReconstruction(
        points=points, errors=errors, seen_by=tracks.seen_by(), names=tracks.names
    )


def _as_sequence(source, person: int) -> np.ndarray:
    if isinstance(source, str):
        return load_landmarks(source, person)
    if isinstance(source, np.ndarray):
        return _from_array(source, N_LANDMARKS)
    return sequence_to_array(source, person)


# -- optional: run MediaPipe on a video -----------------------------------


MODEL_ENV_VAR = "MEDIAPIPE_POSE_MODEL"
MODEL_URL = (
    "https://storage.googleapis.com/mediapipe-models/pose_landmarker/"
    "pose_landmarker_lite/float16/1/pose_landmarker_lite.task"
)


def _find_model(model_path: str | None) -> str:
    """Locate the ``.task`` bundle the Tasks API needs."""
    candidates = [
        model_path,
        os.environ.get(MODEL_ENV_VAR),
        "pose_landmarker.task",
        "pose_landmarker_lite.task",
        "pose_landmarker_full.task",
    ]
    for candidate in candidates:
        if candidate and os.path.isfile(candidate):
            return candidate
    raise MediaPipeImportError(
        "modèle MediaPipe introuvable. Téléchargez-le une fois :\n"
        f"    curl -L -o pose_landmarker.task {MODEL_URL}\n"
        f"puis passez --model / model_path, ou définissez {MODEL_ENV_VAR}."
    )


def estimate_from_video(
    video_path: str,
    model_path: str | None = None,
    min_detection_confidence: float = 0.5,
    progress=None,
) -> np.ndarray:
    """Run MediaPipe Pose on a video and return ``(n_frames, 33, 4)``.

    Convenience wrapper so a captation video can go straight to landmarks.
    Uses the Tasks ``PoseLandmarker`` (MediaPipe >= 0.10, and the only API left
    in 1.x), which needs a ``.task`` model bundle — see :func:`_find_model`;
    it falls back to the legacy ``solutions.pose`` when running on an older
    MediaPipe. Frames where no person is found come back as ``nan``.
    """
    try:
        import mediapipe as mp
    except ImportError as error:  # pragma: no cover - depends on the environment
        raise MediaPipeImportError("mediapipe n'est pas installé : pip install mediapipe") from error
    import cv2

    capture = cv2.VideoCapture(video_path)
    if not capture.isOpened():
        raise MediaPipeImportError(f"vidéo illisible : {video_path}")
    total = int(capture.get(cv2.CAP_PROP_FRAME_COUNT))
    fps = capture.get(cv2.CAP_PROP_FPS) or 30.0
    frames: list[np.ndarray] = []
    try:
        detector, process = _open_pose_detector(mp, model_path, min_detection_confidence)
        with detector:
            index = 0
            while True:
                ok, frame = capture.read()
                if not ok:
                    break
                rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
                frames.append(landmarks_to_array(process(rgb, int(index * 1000 / fps))))
                index += 1
                if progress is not None and total > 0:
                    progress(min(1.0, index / total))
    finally:
        capture.release()
    return np.stack(frames) if frames else np.zeros((0, N_LANDMARKS, 4))


def _tasks_namespace(tasks, name: str):
    """``mp.tasks.<name>`` (MediaPipe 1.x) or ``mp.tasks.python.<name>`` (0.10.x)."""
    if tasks is None:
        return None
    found = getattr(tasks, name, None)
    if found is not None:
        return found
    return getattr(getattr(tasks, "python", None), name, None)


def _open_pose_detector(mp, model_path: str | None, min_detection_confidence: float):
    """Return ``(context manager, process(rgb, timestamp_ms))`` for either API."""
    tasks = getattr(mp, "tasks", None)
    vision = _tasks_namespace(tasks, "vision")
    if vision is not None:
        base_options = getattr(tasks, "BaseOptions", None) or _tasks_namespace(tasks, "BaseOptions")
        options = vision.PoseLandmarkerOptions(
            base_options=base_options(model_asset_path=_find_model(model_path)),
            running_mode=vision.RunningMode.VIDEO,
            min_pose_detection_confidence=min_detection_confidence,
        )
        landmarker = vision.PoseLandmarker.create_from_options(options)
        return landmarker, lambda rgb, stamp: landmarker.detect_for_video(
            mp.Image(image_format=mp.ImageFormat.SRGB, data=rgb), stamp
        )
    solutions = getattr(mp, "solutions", None)  # MediaPipe < 1.0
    if solutions is None:  # pragma: no cover - depends on the installed version
        raise MediaPipeImportError("version de mediapipe non prise en charge")
    pose = solutions.pose.Pose(
        static_image_mode=False, min_detection_confidence=min_detection_confidence
    )
    return pose, lambda rgb, _stamp: pose.process(rgb)
