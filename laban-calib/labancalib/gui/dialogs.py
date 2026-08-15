"""Dialogs: target definition, image sources, video import, live capture, solver."""

from __future__ import annotations

import os

from PySide6.QtCore import Qt, QTimer
from PySide6.QtWidgets import (
    QAbstractItemView,
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFileDialog,
    QFormLayout,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QListWidget,
    QMessageBox,
    QPushButton,
    QSpinBox,
    QVBoxLayout,
    QWidget,
)

from ..board import ARUCO_DICTIONARIES, BOARD_KINDS, BoardSpec, DetectorOptions, KIND_LABELS, render_board, save_board_pdf
from ..bundle import BundleOptions
from ..models import CAMERA_MODELS, MODEL_LABELS, ParameterMask, parameter_names
from ..sources import LiveRig, VideoExtractOptions, probe_devices
from .image_view import ImageView


def _buttons(dialog: QDialog) -> QDialogButtonBox:
    box = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
    box.button(QDialogButtonBox.Ok).setText("Valider")
    box.button(QDialogButtonBox.Cancel).setText("Annuler")
    box.accepted.connect(dialog.accept)
    box.rejected.connect(dialog.reject)
    return box


class BoardDialog(QDialog):
    """Define the physical target and export a printable copy."""

    def __init__(self, board: BoardSpec, options: DetectorOptions, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Mire d'étalonnage")
        self.board = board
        self.options = options

        self.kind = QComboBox()
        for kind in BOARD_KINDS:
            self.kind.addItem(KIND_LABELS[kind], kind)
        self.kind.setCurrentIndex(BOARD_KINDS.index(board.kind))
        self.cols = QSpinBox(minimum=2, maximum=60, value=board.cols)
        self.rows = QSpinBox(minimum=2, maximum=60, value=board.rows)
        self.square = QDoubleSpinBox(minimum=1.0, maximum=500.0, decimals=2, value=board.square_size * 1000)
        self.square.setSuffix(" mm")
        self.marker = QDoubleSpinBox(minimum=0.5, maximum=500.0, decimals=2, value=board.marker_size * 1000)
        self.marker.setSuffix(" mm")
        self.dictionary = QComboBox()
        self.dictionary.addItems(ARUCO_DICTIONARIES)
        self.dictionary.setCurrentText(board.dictionary)
        self.legacy = QCheckBox("Mire ChArUco générée par OpenCV < 4.6 (motif hérité)")
        self.legacy.setChecked(board.legacy_pattern)

        self.refine = QCheckBox("Affiner les coins au sous-pixel")
        self.refine.setChecked(options.refine_corners)
        self.clahe = QCheckBox("Égaliser le contraste (CLAHE) — éclairage de studio difficile")
        self.clahe.setChecked(options.clahe)
        self.invert = QCheckBox("Inverser l'image (mire claire sur fond sombre)")
        self.invert.setChecked(options.invert)
        self.min_points = QSpinBox(minimum=4, maximum=200, value=options.min_points)

        self.summary = QLabel()
        self.summary.setWordWrap(True)

        form = QFormLayout()
        form.addRow("Type de mire", self.kind)
        form.addRow("Colonnes", self.cols)
        form.addRow("Rangées", self.rows)
        form.addRow("Côté de case / pas", self.square)
        form.addRow("Côté du marqueur", self.marker)
        form.addRow("Dictionnaire ArUco", self.dictionary)
        form.addRow("", self.legacy)

        detection = QFormLayout()
        detection.addRow("", self.refine)
        detection.addRow("", self.clahe)
        detection.addRow("", self.invert)
        detection.addRow("Points minimum par vue", self.min_points)

        geometry_box = QGroupBox("Géométrie")
        geometry_box.setLayout(form)
        detection_box = QGroupBox("Détection")
        detection_box.setLayout(detection)

        export_row = QHBoxLayout()
        pdf_button = QPushButton("Exporter en PDF à l'échelle…")
        pdf_button.clicked.connect(self._export_pdf)
        png_button = QPushButton("Exporter en PNG…")
        png_button.clicked.connect(self._export_png)
        export_row.addWidget(pdf_button)
        export_row.addWidget(png_button)
        export_row.addStretch(1)

        layout = QVBoxLayout(self)
        layout.addWidget(geometry_box)
        layout.addWidget(detection_box)
        layout.addWidget(self.summary)
        layout.addLayout(export_row)
        layout.addWidget(_buttons(self))

        for widget in (self.kind, self.dictionary):
            widget.currentIndexChanged.connect(self._refresh)
        for widget in (self.cols, self.rows):
            widget.valueChanged.connect(self._refresh)
        for widget in (self.square, self.marker):
            widget.valueChanged.connect(self._refresh)
        self._refresh()

    def _current(self) -> BoardSpec | None:
        try:
            return BoardSpec(
                kind=self.kind.currentData(),
                cols=self.cols.value(),
                rows=self.rows.value(),
                square_size=self.square.value() / 1000.0,
                marker_size=self.marker.value() / 1000.0,
                dictionary=self.dictionary.currentText(),
                legacy_pattern=self.legacy.isChecked(),
            )
        except ValueError:
            return None

    def _refresh(self) -> None:
        charuco = self.kind.currentData() == "charuco"
        self.marker.setEnabled(charuco)
        self.dictionary.setEnabled(charuco)
        self.legacy.setEnabled(charuco)
        spec = self._current()
        if spec is None:
            self.summary.setText(
                "<b style='color:#b00'>Géométrie invalide</b> — le marqueur doit être plus petit que la case."
            )
            return
        self.summary.setText(
            f"<b>{spec.describe()}</b><br>{spec.n_points} points détectables "
            f"({spec.grid[0]}×{spec.grid[1]})."
        )

    def _export_pdf(self) -> None:
        spec = self._current()
        if spec is None:
            return
        path, _ = QFileDialog.getSaveFileName(self, "Exporter la mire", "mire.pdf", "PDF (*.pdf)")
        if path:
            save_board_pdf(spec, path)
            QMessageBox.information(
                self, "Mire exportée", f"{spec.describe()}\n\nImprimer à 100 %, sans mise à l'échelle."
            )

    def _export_png(self) -> None:
        spec = self._current()
        if spec is None:
            return
        path, _ = QFileDialog.getSaveFileName(self, "Exporter la mire", "mire.png", "PNG (*.png)")
        if path:
            import cv2

            cv2.imwrite(path, render_board(spec))

    def values(self) -> tuple[BoardSpec, DetectorOptions]:
        spec = self._current() or self.board
        options = DetectorOptions(
            refine_corners=self.refine.isChecked(),
            refine_window=self.options.refine_window,
            min_points=self.min_points.value(),
            clahe=self.clahe.isChecked(),
            invert=self.invert.isChecked(),
        )
        return spec, options

    def accept(self) -> None:
        if self._current() is None:
            QMessageBox.warning(self, "Géométrie invalide", "Le marqueur doit être plus petit que la case.")
            return
        super().accept()


class SourcesDialog(QDialog):
    """Pick one image folder per camera — the « Définir les images » action."""

    def __init__(self, folders: list[str], parent=None):
        super().__init__(parent)
        self.setWindowTitle("Images par caméra")
        self.resize(560, 320)
        self.list = QListWidget()
        self.list.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.list.addItems(folders)

        add = QPushButton("Ajouter un dossier…")
        add.clicked.connect(self._add)
        remove = QPushButton("Retirer")
        remove.clicked.connect(self._remove)
        up = QPushButton("Monter")
        up.clicked.connect(lambda: self._move(-1))
        down = QPushButton("Descendre")
        down.clicked.connect(lambda: self._move(1))

        buttons = QVBoxLayout()
        for widget in (add, remove, up, down):
            buttons.addWidget(widget)
        buttons.addStretch(1)

        row = QHBoxLayout()
        row.addWidget(self.list, 1)
        row.addLayout(buttons)

        layout = QVBoxLayout(self)
        layout.addWidget(
            QLabel(
                "Un dossier par caméra, dans l'ordre du dispositif. La pose <i>i</i> est la "
                "<i>i</i>-ème image de chaque dossier : les prises doivent être synchronisées."
            )
        )
        layout.addLayout(row)
        layout.addWidget(_buttons(self))

    def _add(self) -> None:
        folder = QFileDialog.getExistingDirectory(self, "Dossier d'images d'une caméra")
        if folder:
            self.list.addItem(folder)

    def _remove(self) -> None:
        for item in self.list.selectedItems():
            self.list.takeItem(self.list.row(item))

    def _move(self, delta: int) -> None:
        row = self.list.currentRow()
        target = row + delta
        if row < 0 or not 0 <= target < self.list.count():
            return
        item = self.list.takeItem(row)
        self.list.insertItem(target, item)
        self.list.setCurrentRow(target)

    def folders(self) -> list[str]:
        return [self.list.item(i).text() for i in range(self.list.count())]


class VideoDialog(QDialog):
    """Import one video per camera and extract calibration frames."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Importer des vidéos")
        self.resize(600, 360)
        self.list = QListWidget()
        add = QPushButton("Ajouter des vidéos…")
        add.clicked.connect(self._add)
        remove = QPushButton("Retirer")
        remove.clicked.connect(self._remove)

        self.stride = QSpinBox(minimum=1, maximum=1000, value=15)
        self.max_frames = QSpinBox(minimum=1, maximum=2000, value=60)
        self.min_sharpness = QDoubleSpinBox(minimum=0.0, maximum=10000.0, decimals=1, value=0.0)
        self.min_sharpness.setToolTip(
            "Variance du laplacien minimale : écarte les images floues. 0 désactive le filtre."
        )
        self.output = QLabel("(dossier de destination non choisi)")
        choose = QPushButton("Dossier de destination…")
        choose.clicked.connect(self._choose_output)
        self.output_dir = ""

        form = QFormLayout()
        form.addRow("Une image toutes les", self.stride)
        form.addRow("Nombre maximum d'images", self.max_frames)
        form.addRow("Netteté minimale", self.min_sharpness)

        buttons = QHBoxLayout()
        buttons.addWidget(add)
        buttons.addWidget(remove)
        buttons.addStretch(1)

        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("Une vidéo par caméra, dans l'ordre du dispositif."))
        layout.addWidget(self.list, 1)
        layout.addLayout(buttons)
        layout.addLayout(form)
        layout.addWidget(choose)
        layout.addWidget(self.output)
        layout.addWidget(_buttons(self))

    def _add(self) -> None:
        paths, _ = QFileDialog.getOpenFileNames(
            self, "Vidéos", "", "Vidéos (*.mp4 *.mov *.avi *.mkv *.m4v *.webm);;Tous les fichiers (*)"
        )
        self.list.addItems(paths)

    def _remove(self) -> None:
        for item in self.list.selectedItems():
            self.list.takeItem(self.list.row(item))

    def _choose_output(self) -> None:
        folder = QFileDialog.getExistingDirectory(self, "Dossier de destination")
        if folder:
            self.output_dir = folder
            self.output.setText(folder)

    def videos(self) -> list[str]:
        return [self.list.item(i).text() for i in range(self.list.count())]

    def output_dirs(self) -> list[str]:
        return [
            os.path.join(self.output_dir, f"cam{index}") for index in range(self.list.count())
        ]

    def options(self) -> VideoExtractOptions:
        return VideoExtractOptions(
            stride=self.stride.value(),
            max_frames=self.max_frames.value(),
            min_sharpness=self.min_sharpness.value(),
        )

    def accept(self) -> None:
        if not self.videos():
            QMessageBox.warning(self, "Aucune vidéo", "Ajoutez au moins une vidéo.")
            return
        if not self.output_dir:
            QMessageBox.warning(self, "Destination manquante", "Choisissez un dossier de destination.")
            return
        super().accept()


class LiveCaptureDialog(QDialog):
    """Preview the connected cameras and capture synchronised poses."""

    def __init__(self, output_dir: str, board: BoardSpec, options: DetectorOptions, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Capture en direct")
        self.resize(900, 560)
        self.output_dir = output_dir
        self.board = board
        self.detector_options = options
        self.rig: LiveRig | None = None
        self.pose_index = 0
        self.captured: list[list[str | None]] = []

        self.devices = QComboBox()
        self.devices.addItems([str(i) for i in range(1, 9)])
        self.devices.setCurrentText("2")
        self.status = QLabel("Aucun flux ouvert.")
        self.previews = QHBoxLayout()
        self.views: list[ImageView] = []

        self.open_button = QPushButton("Ouvrir les flux")
        self.open_button.clicked.connect(self._open)
        self.capture_button = QPushButton("Capturer une pose")
        self.capture_button.clicked.connect(self._capture)
        self.capture_button.setEnabled(False)
        self.overlay = QCheckBox("Superposer la détection")
        self.overlay.setChecked(True)

        controls = QHBoxLayout()
        controls.addWidget(QLabel("Nombre de caméras"))
        controls.addWidget(self.devices)
        controls.addWidget(self.open_button)
        controls.addWidget(self.capture_button)
        controls.addWidget(self.overlay)
        controls.addStretch(1)

        container = QWidget()
        container.setLayout(self.previews)

        layout = QVBoxLayout(self)
        layout.addLayout(controls)
        layout.addWidget(container, 1)
        layout.addWidget(self.status)
        layout.addWidget(_buttons(self))

        self.timer = QTimer(self)
        self.timer.timeout.connect(self._tick)

    def _open(self) -> None:
        self._close_rig()
        count = int(self.devices.currentText())
        available = probe_devices(max(count, 4))
        if len(available) < count:
            QMessageBox.warning(
                self,
                "Flux indisponibles",
                f"{len(available)} périphérique(s) détecté(s) pour {count} demandé(s) : {available}",
            )
            if not available:
                return
            count = len(available)
        self.rig = LiveRig(available[:count])
        for view in self.views:
            view.setParent(None)
        self.views = []
        for _ in range(count):
            view = ImageView()
            self.views.append(view)
            self.previews.addWidget(view)
        self.capture_button.setEnabled(True)
        self.status.setText(f"{count} flux ouvert(s) : {available[:count]}")
        self.timer.start(60)

    def _tick(self) -> None:
        if self.rig is None:
            return
        from ..board import BoardDetector, draw_detection

        detector = BoardDetector(self.board, self.detector_options) if self.overlay.isChecked() else None
        for view, frame in zip(self.views, self.rig.grab()):
            if frame is None:
                continue
            if detector is not None:
                frame = draw_detection(frame, detector.detect(frame), self.board)
            view.show_image(frame, keep_view=True)

    def _capture(self) -> None:
        if self.rig is None:
            return
        folders = [os.path.join(self.output_dir, f"cam{i}") for i in range(len(self.rig.devices))]
        paths = self.rig.save_pose(folders, self.pose_index)
        self.captured.append(paths)
        self.pose_index += 1
        written = sum(1 for p in paths if p)
        self.status.setText(f"Pose {self.pose_index - 1} capturée ({written} image(s)).")

    def folders(self) -> list[str]:
        if self.rig is None:
            return []
        return [os.path.join(self.output_dir, f"cam{i}") for i in range(len(self.rig.devices))]

    def _close_rig(self) -> None:
        self.timer.stop()
        if self.rig is not None:
            self.rig.release()
            self.rig = None

    def done(self, result: int) -> None:
        self._close_rig()
        super().done(result)


class MediaPipeDialog(QDialog):
    """Pick one MediaPipe landmark file per camera and triangulate them."""

    def __init__(self, camera_names: list[str], parent=None):
        super().__init__(parent)
        self.setWindowTitle("Trianguler des points MediaPipe")
        self.resize(640, 380)
        self.camera_names = camera_names
        self.list = QListWidget()
        self.list.addItems([f"{index} · {name} : (aucun fichier)" for index, name in enumerate(camera_names)])
        self.paths: list[str] = ["" for _ in camera_names]

        choose = QPushButton("Choisir le fichier de la caméra sélectionnée…")
        choose.clicked.connect(self._choose_one)
        choose_all = QPushButton("Choisir tous les fichiers (dans l'ordre)…")
        choose_all.clicked.connect(self._choose_all)

        self.min_visibility = QDoubleSpinBox(minimum=0.0, maximum=1.0, decimals=2, value=0.5)
        self.min_visibility.setSingleStep(0.05)
        self.min_visibility.setToolTip("Les points dont la visibilité MediaPipe est inférieure sont ignorés.")
        self.max_error = QDoubleSpinBox(minimum=0.0, maximum=200.0, decimals=1, value=0.0)
        self.max_error.setSuffix(" px")
        self.max_error.setToolTip(
            "Rejette les points dont l'erreur de reprojection dépasse ce seuil. 0 désactive."
        )
        self.person = QSpinBox(minimum=0, maximum=9, value=0)
        self.offsets = QComboBox()
        self.offsets.setEditable(True)
        self.offsets.addItem(", ".join("0" for _ in camera_names))
        self.offsets.setToolTip(
            "Décalage en trames par caméra, séparé par des virgules — pour une caméra démarrée en retard."
        )

        form = QFormLayout()
        form.addRow("Visibilité minimale", self.min_visibility)
        form.addRow("Erreur de reprojection maximale", self.max_error)
        form.addRow("Personne suivie", self.person)
        form.addRow("Décalages (trames)", self.offsets)

        layout = QVBoxLayout(self)
        layout.addWidget(
            QLabel(
                "Un fichier de points par caméra (.json, .jsonl, .npy, .npz), "
                "<b>dans l'ordre du dispositif étalonné</b>."
            )
        )
        layout.addWidget(self.list, 1)
        row = QHBoxLayout()
        row.addWidget(choose)
        row.addWidget(choose_all)
        layout.addLayout(row)
        layout.addLayout(form)
        layout.addWidget(_buttons(self))

    def _filter(self) -> str:
        return "Points MediaPipe (*.json *.jsonl *.npy *.npz);;Tous les fichiers (*)"

    def _choose_one(self) -> None:
        index = self.list.currentRow()
        if index < 0:
            return
        path, _ = QFileDialog.getOpenFileName(self, f"Points de la caméra {index}", "", self._filter())
        if path:
            self._set(index, path)

    def _choose_all(self) -> None:
        paths, _ = QFileDialog.getOpenFileNames(self, "Points de toutes les caméras", "", self._filter())
        for index, path in enumerate(sorted(paths)[: len(self.paths)]):
            self._set(index, path)

    def _set(self, index: int, path: str) -> None:
        self.paths[index] = path
        self.list.item(index).setText(f"{index} · {self.camera_names[index]} : {os.path.basename(path)}")

    def values(self) -> dict:
        text = self.offsets.currentText().replace(";", ",")
        try:
            offsets = [int(part) for part in text.split(",") if part.strip()]
        except ValueError:
            offsets = []
        if len(offsets) != len(self.paths):
            offsets = [0] * len(self.paths)
        return {
            "sources": list(self.paths),
            "min_visibility": self.min_visibility.value(),
            "max_error": self.max_error.value(),
            "person": self.person.value(),
            "offsets": offsets,
        }

    def accept(self) -> None:
        missing = [i for i, path in enumerate(self.paths) if not path]
        if missing:
            QMessageBox.warning(
                self,
                "Fichiers manquants",
                "Aucun fichier pour la/les caméra(s) : " + ", ".join(str(i) for i in missing),
            )
            return
        super().accept()


class SolverDialog(QDialog):
    """Camera model, free parameters and optimiser settings."""

    def __init__(self, model: str, mask: ParameterMask, options: BundleOptions, n_cameras: int, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Modèle et optimisation")
        self.model = QComboBox()
        for name in CAMERA_MODELS:
            self.model.addItem(MODEL_LABELS[name], name)
        self.model.setCurrentIndex(CAMERA_MODELS.index(model))
        self.model.currentIndexChanged.connect(self._refresh_parameters)

        self.checks: dict[str, QCheckBox] = {}
        self.parameter_box = QGroupBox("Paramètres libres")
        self.parameter_layout = QVBoxLayout(self.parameter_box)
        self._mask = mask

        self.reference = QSpinBox(minimum=0, maximum=max(0, n_cameras - 1), value=options.reference_camera)
        self.robust = QCheckBox("Perte robuste de Huber")
        self.robust.setChecked(options.robust)
        self.huber = QDoubleSpinBox(minimum=0.1, maximum=50.0, decimals=2, value=options.huber_delta)
        self.huber.setSuffix(" px")
        self.reject = QDoubleSpinBox(minimum=0.0, maximum=20.0, decimals=1, value=options.reject_sigma)
        self.reject.setToolTip("Écarte les points au-delà de N sigma puis relance. 0 désactive.")
        self.iterations = QSpinBox(minimum=10, maximum=5000, value=options.max_iterations)
        self.sigmas = QCheckBox("Estimer les écarts-types des paramètres")
        self.sigmas.setChecked(options.estimate_sigmas)

        form = QFormLayout()
        form.addRow("Caméra de référence", self.reference)
        form.addRow("", self.robust)
        form.addRow("Seuil de Huber", self.huber)
        form.addRow("Rejet des aberrants (σ)", self.reject)
        form.addRow("Évaluations maximum", self.iterations)
        form.addRow("", self.sigmas)
        solver_box = QGroupBox("Optimisation")
        solver_box.setLayout(form)

        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("Modèle de caméra"))
        layout.addWidget(self.model)
        layout.addWidget(self.parameter_box)
        layout.addWidget(solver_box)
        layout.addWidget(_buttons(self))
        self._refresh_parameters()

    def _refresh_parameters(self) -> None:
        while self.parameter_layout.count():
            item = self.parameter_layout.takeAt(0)
            if item.widget():
                item.widget().setParent(None)
        self.checks = {}
        model = self.model.currentData()
        for name in parameter_names(model):
            if model == "fisheye" and name == "alpha":
                continue
            check = QCheckBox(name)
            check.setChecked(self._mask.is_free(name))
            if name == "f":
                check.setEnabled(False)
                check.setChecked(True)
            self.checks[name] = check
            self.parameter_layout.addWidget(check)

    def values(self) -> tuple[str, ParameterMask, BundleOptions]:
        free = {name for name, check in self.checks.items() if check.isChecked()}
        free.add("f")
        options = BundleOptions(
            max_iterations=self.iterations.value(),
            robust=self.robust.isChecked(),
            huber_delta=self.huber.value(),
            reject_sigma=self.reject.value(),
            estimate_sigmas=self.sigmas.isChecked(),
            reference_camera=self.reference.value(),
        )
        return self.model.currentData(), ParameterMask(free=free), options
