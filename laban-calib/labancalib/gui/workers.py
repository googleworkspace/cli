"""Background workers, so a long detection or optimisation never freezes the UI."""

from __future__ import annotations

from PySide6.QtCore import QObject, QThread, Signal

from ..bundle import BundleOptions
from ..pipeline import DetectionSummary, run_calibration, run_detection
from ..project import Project
from ..sources import VideoExtractOptions, extract_frames


class Worker(QObject):
    """Base worker: progress, log lines, and a cooperative stop flag."""

    progress = Signal(float)
    message = Signal(str)
    failed = Signal(str)
    finished = Signal(object)

    def __init__(self):
        super().__init__()
        self._stop = False

    def request_stop(self) -> None:
        self._stop = True

    def should_stop(self) -> bool:
        return self._stop

    def run(self) -> None:  # pragma: no cover - executed in a QThread
        try:
            result = self.work()
        except Exception as error:  # noqa: BLE001 - surfaced verbatim in the log panel
            self.failed.emit(f"{type(error).__name__}: {error}")
            return
        self.finished.emit(result)

    def work(self):
        raise NotImplementedError


class DetectionWorker(Worker):
    """Runs the target detection over every image of the project."""

    def __init__(self, project: Project):
        super().__init__()
        self.project = project

    def work(self) -> DetectionSummary:
        return run_detection(
            self.project,
            progress=self.progress.emit,
            log=self.message.emit,
            should_stop=self.should_stop,
        )


class CalibrationWorker(Worker):
    """Runs intrinsics, rig initialisation and bundle adjustment."""

    def __init__(self, project: Project, options: BundleOptions, image_sizes: dict | None = None):
        super().__init__()
        self.project = project
        self.options = options
        self.image_sizes = image_sizes

    def work(self):
        return run_calibration(
            self.project,
            self.options,
            progress=self.progress.emit,
            log=self.message.emit,
            image_sizes=self.image_sizes,
        )


class VideoWorker(Worker):
    """Extracts frames from one video file per camera."""

    def __init__(self, videos: list[str], output_dirs: list[str], options: VideoExtractOptions):
        super().__init__()
        self.videos = videos
        self.output_dirs = output_dirs
        self.options = options

    def work(self) -> list[list[str]]:
        out = []
        for index, (video, folder) in enumerate(zip(self.videos, self.output_dirs)):
            self.message.emit(f"extraction de {video}")
            frames = extract_frames(
                video,
                folder,
                self.options,
                progress=lambda value, i=index: self.progress.emit(
                    (i + value) / max(1, len(self.videos))
                ),
                should_stop=self.should_stop,
            )
            self.message.emit(f"    {len(frames)} image(s) écrite(s) dans {folder}")
            out.append(frames)
        return out


class TriangulationWorker(Worker):
    """Reconstructs MediaPipe landmarks in 3D on the calibrated rig."""

    def __init__(self, result, options: dict):
        super().__init__()
        self.result = result
        self.options = options

    def work(self):
        from ..mediapipe_io import triangulate_mediapipe
        from ..triangulate import Rig

        rig = Rig.from_result(self.result)
        self.message.emit(f"triangulation sur {rig.n_cameras} caméra(s) étalonnée(s)")
        return triangulate_mediapipe(
            rig,
            self.options["sources"],
            min_visibility=self.options["min_visibility"],
            offsets=self.options["offsets"],
            person=self.options["person"],
            max_error=self.options["max_error"],
        )


def start(worker: Worker, on_finished, on_failed, on_progress=None, on_message=None) -> QThread:
    """Move ``worker`` to a thread, wire the signals, and start it.

    Returns the thread; the caller must keep a reference to both objects for as
    long as the work runs, otherwise Qt collects them mid-flight.
    """
    thread = QThread()
    worker.moveToThread(thread)
    thread.started.connect(worker.run)
    worker.finished.connect(on_finished)
    worker.failed.connect(on_failed)
    if on_progress is not None:
        worker.progress.connect(on_progress)
    if on_message is not None:
        worker.message.connect(on_message)
    worker.finished.connect(thread.quit)
    worker.failed.connect(thread.quit)
    thread.finished.connect(worker.deleteLater)
    thread.start()
    return thread
