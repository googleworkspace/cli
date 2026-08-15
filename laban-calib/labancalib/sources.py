"""Where the calibration images come from: folders, videos, live cameras."""

from __future__ import annotations

import os
from dataclasses import dataclass

import cv2
import numpy as np

IMAGE_EXTENSIONS = (".png", ".jpg", ".jpeg", ".bmp", ".tif", ".tiff", ".pgm", ".ppm")
VIDEO_EXTENSIONS = (".mp4", ".mov", ".avi", ".mkv", ".m4v", ".mpg", ".mpeg", ".webm")


def list_images(folder: str) -> list[str]:
    """Image files of ``folder``, sorted naturally so pose indices line up."""
    if not os.path.isdir(folder):
        raise FileNotFoundError(f"dossier introuvable : {folder}")
    names = [n for n in os.listdir(folder) if n.lower().endswith(IMAGE_EXTENSIONS)]
    return [os.path.join(folder, n) for n in sorted(names, key=_natural_key)]


def _natural_key(name: str):
    """Sort ``img2.png`` before ``img10.png``."""
    parts, digits = [], ""
    for char in name:
        if char.isdigit():
            digits += char
        else:
            if digits:
                parts.append((1, int(digits)))
                digits = ""
            parts.append((0, char.lower()))
    if digits:
        parts.append((1, int(digits)))
    return parts


def read_image(path: str, grayscale: bool = False) -> np.ndarray:
    flag = cv2.IMREAD_GRAYSCALE if grayscale else cv2.IMREAD_COLOR
    image = cv2.imread(path, flag)
    if image is None:
        raise OSError(f"image illisible : {path}")
    return image


def image_size(path: str) -> tuple[int, int]:
    image = read_image(path, grayscale=True)
    return int(image.shape[1]), int(image.shape[0])


def sharpness(image: np.ndarray) -> float:
    """Variance of the Laplacian — the usual cheap blur score."""
    gray = image if image.ndim == 2 else cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    return float(cv2.Laplacian(gray, cv2.CV_64F).var())


@dataclass
class VideoExtractOptions:
    """How to turn a captation video into calibration frames."""

    stride: int = 15
    max_frames: int = 60
    start_frame: int = 0
    end_frame: int = 0  # 0 = until the end
    min_sharpness: float = 0.0  # 0 disables the blur filter
    prefix: str = "frame"


def video_info(path: str) -> dict:
    capture = cv2.VideoCapture(path)
    if not capture.isOpened():
        raise OSError(f"vidéo illisible : {path}")
    try:
        return {
            "frames": int(capture.get(cv2.CAP_PROP_FRAME_COUNT)),
            "fps": float(capture.get(cv2.CAP_PROP_FPS)),
            "width": int(capture.get(cv2.CAP_PROP_FRAME_WIDTH)),
            "height": int(capture.get(cv2.CAP_PROP_FRAME_HEIGHT)),
        }
    finally:
        capture.release()


def extract_frames(
    video_path: str,
    output_dir: str,
    options: VideoExtractOptions | None = None,
    progress=None,
    should_stop=None,
) -> list[str]:
    """Write evenly spaced (and optionally sharp enough) frames as PNG files."""
    options = options or VideoExtractOptions()
    capture = cv2.VideoCapture(video_path)
    if not capture.isOpened():
        raise OSError(f"vidéo illisible : {video_path}")
    os.makedirs(output_dir, exist_ok=True)
    stride = max(1, int(options.stride))
    total = int(capture.get(cv2.CAP_PROP_FRAME_COUNT))
    last = options.end_frame if options.end_frame > 0 else total
    written: list[str] = []
    index = max(0, int(options.start_frame))
    try:
        capture.set(cv2.CAP_PROP_POS_FRAMES, index)
        while index < last and len(written) < options.max_frames:
            ok, frame = capture.read()
            if not ok:
                break
            if should_stop is not None and should_stop():
                break
            if options.min_sharpness <= 0 or sharpness(frame) >= options.min_sharpness:
                path = os.path.join(output_dir, f"{options.prefix}_{index:06d}.png")
                cv2.imwrite(path, frame)
                written.append(path)
            if progress is not None and last > 0:
                progress(min(1.0, (index - options.start_frame) / max(1, last - options.start_frame)))
            index += stride
            capture.set(cv2.CAP_PROP_POS_FRAMES, index)
    finally:
        capture.release()
    return written


class LiveRig:
    """A set of live capture devices grabbed as synchronously as OpenCV allows.

    ``grab()`` issues ``VideoCapture.grab()`` on every device first and only
    then retrieves the frames, which keeps the inter-camera delay down to the
    grab loop instead of a full decode per camera. That is good enough for
    calibration (a hand-held target), not for dynamic capture.
    """

    def __init__(self, devices: list[int | str], width: int = 0, height: int = 0):
        self.devices = list(devices)
        self.captures: list[cv2.VideoCapture] = []
        for device in self.devices:
            capture = cv2.VideoCapture(device)
            if width and height:
                capture.set(cv2.CAP_PROP_FRAME_WIDTH, width)
                capture.set(cv2.CAP_PROP_FRAME_HEIGHT, height)
            self.captures.append(capture)

    @property
    def opened(self) -> list[bool]:
        return [c.isOpened() for c in self.captures]

    def grab(self) -> list[np.ndarray | None]:
        for capture in self.captures:
            capture.grab()
        frames = []
        for capture in self.captures:
            ok, frame = capture.retrieve()
            frames.append(frame if ok else None)
        return frames

    def save_pose(self, output_dirs: list[str], pose_index: int) -> list[str | None]:
        """Grab one synchronised set and write it as pose ``pose_index``."""
        frames = self.grab()
        paths: list[str | None] = []
        for frame, folder in zip(frames, output_dirs):
            if frame is None:
                paths.append(None)
                continue
            os.makedirs(folder, exist_ok=True)
            path = os.path.join(folder, f"pose_{pose_index:04d}.png")
            cv2.imwrite(path, frame)
            paths.append(path)
        return paths

    def release(self) -> None:
        for capture in self.captures:
            capture.release()
        self.captures = []

    def __enter__(self) -> "LiveRig":
        return self

    def __exit__(self, *exc) -> None:
        self.release()


def probe_devices(maximum: int = 8) -> list[int]:
    """Indices of the capture devices that actually open (best effort)."""
    available = []
    for index in range(maximum):
        capture = cv2.VideoCapture(index)
        if capture.isOpened():
            available.append(index)
        capture.release()
    return available
