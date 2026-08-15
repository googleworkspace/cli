"""Main window — the layout of the reference application, in French.

    ┌ arbre poses/caméras ┬ image + calques de détection ┬ vue 3D du dispositif ┐
    │                     ├──────────────────────────────┼──────────────────────┤
    │                     │ journal                      │ onglets d'analyse    │
    └─────────────────────┴──────────────────────────────┴──────────────────────┘
"""

from __future__ import annotations

import os
from datetime import datetime

import numpy as np
from PySide6.QtCore import Qt
from PySide6.QtGui import QAction, QKeySequence
from PySide6.QtWidgets import (
    QFileDialog,
    QHBoxLayout,
    QLabel,
    QMainWindow,
    QMessageBox,
    QPlainTextEdit,
    QProgressBar,
    QPushButton,
    QSplitter,
    QTabWidget,
    QTreeWidget,
    QTreeWidgetItem,
    QVBoxLayout,
    QWidget,
)

from ..board import draw_detection
from ..export import EXPORT_FORMATS, export_result, parameters_report
from ..models import MODEL_LABELS
from ..pipeline import camera_image_sizes, reprojection_for_view
from ..project import Project
from ..sources import list_images, read_image
from . import workers
from .dialogs import (
    BoardDialog,
    LiveCaptureDialog,
    MediaPipeDialog,
    SolverDialog,
    SourcesDialog,
    VideoDialog,
)
from .image_view import ImageView
from .plots import CANVAS_TYPES
from .view3d import View3D

PROJECT_FILTER = "Projet d'étalonnage (*.lcalib);;Tous les fichiers (*)"


class MainWindow(QMainWindow):
    def __init__(self, project: Project | None = None):
        super().__init__()
        self.project = project or Project()
        self.thread = None
        self.worker = None
        self.setWindowTitle("Calib Laban — étalonnage multi-caméras")
        self.resize(1500, 900)

        self._build_widgets()
        self._build_actions()
        self._build_layout()
        self.refresh_all()
        self.log("Prêt. Définissez les images de chaque caméra pour commencer.")

    # -- construction -----------------------------------------------------

    def _build_widgets(self) -> None:
        self.tree = QTreeWidget()
        self.tree.setHeaderLabels(["Pose", "Caméra", "Points", "RPE (px)"])
        self.tree.setColumnWidth(0, 110)
        self.tree.itemSelectionChanged.connect(self._selection_changed)

        self.image_view = ImageView()
        self.image_label = QLabel("Aucune image sélectionnée.")
        self.image_label.setAlignment(Qt.AlignLeft | Qt.AlignVCenter)

        self.log_view = QPlainTextEdit()
        self.log_view.setReadOnly(True)
        self.log_view.setMaximumBlockCount(5000)
        self.log_view.setMinimumHeight(150)
        self.log_view.setStyleSheet("font-family: monospace; font-size: 11px;")

        self.view3d = View3D()

        self.tabs = QTabWidget()
        self.parameters_view = QPlainTextEdit()
        self.parameters_view.setReadOnly(True)
        self.parameters_view.setStyleSheet("font-family: monospace; font-size: 11px;")
        self.canvases = [canvas_type() for canvas_type in CANVAS_TYPES]
        self.tabs.addTab(self.canvases[0], self.canvases[0].title)  # Convergence
        self.tabs.addTab(self.canvases[1], self.canvases[1].title)  # Initialisation
        self.tabs.addTab(self._parameters_tab(), "Paramètres")
        for canvas in self.canvases[2:]:
            self.tabs.addTab(canvas, canvas.title)

        self.progress = QProgressBar()
        self.progress.setMaximumWidth(220)
        self.progress.setVisible(False)
        self.status_label = QLabel("")
        self.statusBar().addPermanentWidget(self.status_label)
        self.statusBar().addPermanentWidget(self.progress)

    def _parameters_tab(self) -> QWidget:
        widget = QWidget()
        layout = QVBoxLayout(widget)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.parameters_view, 1)
        row = QHBoxLayout()
        export_button = QPushButton("Exporter…")
        export_button.clicked.connect(self.export_result)
        row.addWidget(export_button)
        row.addStretch(1)
        layout.addLayout(row)
        return widget

    def _build_actions(self) -> None:
        file_menu = self.menuBar().addMenu("&Fichier")
        self._action(file_menu, "Nouveau projet", self.new_project, QKeySequence.New)
        self._action(file_menu, "Ouvrir un projet…", self.open_project, QKeySequence.Open)
        self._action(file_menu, "Enregistrer", self.save_project, QKeySequence.Save)
        self._action(file_menu, "Enregistrer sous…", self.save_project_as, QKeySequence.SaveAs)
        file_menu.addSeparator()
        self._action(file_menu, "Exporter l'étalonnage…", self.export_result)
        file_menu.addSeparator()
        self._action(file_menu, "Quitter", self.close, QKeySequence.Quit)

        calibration_menu = self.menuBar().addMenu("&Étalonnage")
        self.action_sources = self._action(
            calibration_menu, "Définir les images…", self.choose_sources
        )
        self._action(calibration_menu, "Importer des vidéos…", self.import_videos)
        self._action(calibration_menu, "Capture en direct…", self.live_capture)
        calibration_menu.addSeparator()
        self._action(calibration_menu, "Configurer la mire…", self.configure_board)
        self._action(calibration_menu, "Modèle et optimisation…", self.configure_solver)
        calibration_menu.addSeparator()
        self.action_detect = self._action(calibration_menu, "Détecter la mire", self.detect, "F5")
        self.action_optimise = self._action(
            calibration_menu, "Optimiser les caméras", self.calibrate, "F6"
        )
        self.action_stop = self._action(calibration_menu, "Interrompre", self.stop_worker, "Esc")
        self.action_stop.setEnabled(False)

        analysis_menu = self.menuBar().addMenu("&Analyse")
        self.action_mediapipe = self._action(
            analysis_menu, "Trianguler des points MediaPipe…", self.triangulate_mediapipe, "F7"
        )

        view_menu = self.menuBar().addMenu("&Affichage")
        self._action(view_menu, "Ajuster l'image", self.image_view.fit)
        self._action(view_menu, "Zoom avant", lambda: self.image_view.zoom(1.25), QKeySequence.ZoomIn)
        self._action(view_menu, "Zoom arrière", lambda: self.image_view.zoom(0.8), QKeySequence.ZoomOut)
        view_menu.addSeparator()
        self._action(view_menu, "Effacer le journal", self.log_view.clear)

        help_menu = self.menuBar().addMenu("&Aide")
        self._action(help_menu, "Mode d'emploi", self.show_help)
        self._action(help_menu, "À propos", self.show_about)

        toolbar = self.addToolBar("Étalonnage")
        toolbar.setMovable(False)
        toolbar.addAction(self.action_sources)
        toolbar.addAction(self.action_detect)
        toolbar.addAction(self.action_optimise)
        toolbar.addSeparator()
        self.model_label = QLabel()
        toolbar.addWidget(self.model_label)

    def _action(self, menu, title, slot, shortcut=None) -> QAction:
        action = QAction(title, self)
        action.triggered.connect(slot)
        if shortcut is not None:
            action.setShortcut(shortcut)
        menu.addAction(action)
        return action

    def _build_layout(self) -> None:
        centre = QSplitter(Qt.Vertical)
        image_panel = QWidget()
        image_layout = QVBoxLayout(image_panel)
        image_layout.setContentsMargins(0, 0, 0, 0)
        image_layout.addWidget(self.image_label)
        image_layout.addWidget(self.image_view, 1)
        centre.addWidget(image_panel)
        centre.addWidget(self.log_view)
        centre.setStretchFactor(0, 3)
        centre.setStretchFactor(1, 1)
        centre.setSizes([560, 240])

        right = QSplitter(Qt.Vertical)
        right.addWidget(self.view3d)
        right.addWidget(self.tabs)
        right.setStretchFactor(0, 1)
        right.setStretchFactor(1, 1)
        right.setSizes([400, 420])

        splitter = QSplitter(Qt.Horizontal)
        splitter.addWidget(self.tree)
        splitter.addWidget(centre)
        splitter.addWidget(right)
        splitter.setSizes([300, 660, 540])
        self.setCentralWidget(splitter)

    # -- logging and state ------------------------------------------------

    def log(self, message: str) -> None:
        stamp = datetime.now().strftime("%H:%M:%S")
        for index, line in enumerate(str(message).splitlines() or [""]):
            self.log_view.appendPlainText(f"{stamp} {line}" if index == 0 else f"         {line}")
        self.log_view.verticalScrollBar().setValue(self.log_view.verticalScrollBar().maximum())

    def refresh_all(self) -> None:
        self.refresh_tree()
        self.refresh_results()
        self.refresh_status()

    def refresh_status(self) -> None:
        project = self.project
        self.model_label.setText(f"   Modèle : {MODEL_LABELS[project.camera_model]}    ")
        self.status_label.setText(
            f"{project.n_cameras} caméra(s) · {project.n_poses} pose(s) · "
            f"{len(project.detections)} détection(s) · {project.board.label}"
        )
        busy = self.thread is not None
        self.action_detect.setEnabled(not busy and project.n_cameras > 0)
        self.action_optimise.setEnabled(not busy and bool(project.detections))
        self.action_sources.setEnabled(not busy)
        self.action_mediapipe.setEnabled(not busy and project.result is not None)
        self.action_stop.setEnabled(busy)

    def refresh_tree(self) -> None:
        self.tree.blockSignals(True)
        self.tree.clear()
        rpe_by_view = {}
        if self.project.result:
            rpe_by_view = {(r.camera, r.pose_index): r for r in self.project.result.residuals}
        for pose_index in range(self.project.n_poses):
            parent = QTreeWidgetItem([f"{pose_index}", "", "", ""])
            parent.setData(0, Qt.UserRole, (None, pose_index))
            seen = 0
            for camera in range(self.project.n_cameras):
                if self.project.image_at(camera, pose_index) is None:
                    continue
                detection = self.project.detection_at(camera, pose_index)
                residual = rpe_by_view.get((camera, pose_index))
                child = QTreeWidgetItem(
                    [
                        "",
                        str(camera),
                        str(detection.count) if detection else "—",
                        f"{residual.rms:.3f}" if residual else "",
                    ]
                )
                child.setData(0, Qt.UserRole, (camera, pose_index))
                if detection is None:
                    child.setForeground(1, Qt.gray)
                else:
                    seen += 1
                parent.addChild(child)
            parent.setText(2, f"{seen}/{self.project.n_cameras}")
            if seen == 0:
                parent.setForeground(0, Qt.gray)
            self.tree.addTopLevelItem(parent)
        self.tree.expandAll()
        self.tree.blockSignals(False)

    def refresh_results(self) -> None:
        for canvas in self.canvases:
            canvas.update_from(self.project)
        self.view3d.update_from(self.project)
        if self.project.result:
            self.parameters_view.setPlainText(
                parameters_report(self.project.result, self.project.board)
            )
        else:
            self.parameters_view.setPlainText(
                "Aucun résultat.\n\n"
                "1. « Définir les images… » : un dossier par caméra.\n"
                "2. « Configurer la mire… » : géométrie réelle de la mire imprimée.\n"
                "3. « Détecter la mire » (F5).\n"
                "4. « Optimiser les caméras » (F6)."
            )

    # -- selection --------------------------------------------------------

    def _selection_changed(self) -> None:
        items = self.tree.selectedItems()
        if not items:
            return
        camera, pose_index = items[0].data(0, Qt.UserRole)
        self.view3d.highlight_pose(pose_index)
        self.view3d.update_from(self.project)
        if camera is None:
            self.image_label.setText(f"Pose {pose_index}")
            return
        self.show_view(camera, pose_index)

    def show_view(self, camera: int, pose_index: int) -> None:
        path = self.project.image_at(camera, pose_index)
        if path is None:
            self.image_view.show_image(None)
            return
        try:
            image = read_image(path)
        except OSError as error:
            self.image_label.setText(str(error))
            return
        detection, reprojection = reprojection_for_view(self.project, camera, pose_index)
        overlay = draw_detection(image, detection, self.project.board, reprojection)
        self.image_view.show_image(overlay)
        parts = [f"Caméra {camera} · pose {pose_index}", os.path.basename(path)]
        if detection:
            parts.append(f"{detection.count} point(s)")
        else:
            parts.append("mire non détectée")
        if reprojection is not None:
            parts.append("croix = reprojection")
        self.image_label.setText("   —   ".join(parts))

    # -- actions ----------------------------------------------------------

    def new_project(self) -> None:
        self.project = Project()
        self.refresh_all()
        self.log("Nouveau projet.")

    def open_project(self) -> None:
        path, _ = QFileDialog.getOpenFileName(self, "Ouvrir un projet", "", PROJECT_FILTER)
        if not path:
            return
        try:
            self.project = Project.load(path)
        except (OSError, ValueError) as error:
            QMessageBox.critical(self, "Ouverture impossible", str(error))
            return
        self.refresh_all()
        self.log(f"Projet ouvert : {path}")

    def save_project(self) -> None:
        if not self.project.path:
            self.save_project_as()
            return
        self.project.save(self.project.path)
        self.log(f"Projet enregistré : {self.project.path}")

    def save_project_as(self) -> None:
        path, _ = QFileDialog.getSaveFileName(self, "Enregistrer le projet", "etalonnage.lcalib", PROJECT_FILTER)
        if path:
            self.project.save(path)
            self.log(f"Projet enregistré : {path}")

    def choose_sources(self) -> None:
        dialog = SourcesDialog([c.folder for c in self.project.cameras if c.folder], self)
        if not dialog.exec():
            return
        folders = dialog.folders()
        if not folders:
            return
        try:
            self.project.set_camera_folders(folders)
        except (OSError, FileNotFoundError) as error:
            QMessageBox.critical(self, "Dossier illisible", str(error))
            return
        for index, camera in enumerate(self.project.cameras):
            self.log(f"caméra {index} : {len(camera.images)} image(s) — {camera.folder}")
        for issue in self.project.validate():
            self.log(f"attention : {issue}")
        self.refresh_all()

    def import_videos(self) -> None:
        dialog = VideoDialog(self)
        if not dialog.exec():
            return
        worker = workers.VideoWorker(dialog.videos(), dialog.output_dirs(), dialog.options())
        folders = dialog.output_dirs()

        def finished(_result) -> None:
            self._worker_done()
            try:
                self.project.set_camera_folders(folders)
            except OSError as error:
                QMessageBox.critical(self, "Extraction", str(error))
                return
            self.refresh_all()
            self.log("Extraction terminée : les dossiers sont devenus les caméras du projet.")

        self._start(worker, finished, "Extraction des images…")

    def live_capture(self) -> None:
        folder = QFileDialog.getExistingDirectory(self, "Dossier où enregistrer les captures")
        if not folder:
            return
        dialog = LiveCaptureDialog(folder, self.project.board, self.project.detector_options, self)
        accepted = dialog.exec()
        folders = [f for f in dialog.folders() if os.path.isdir(f) and list_images(f)]
        if accepted and folders:
            self.project.set_camera_folders(folders)
            self.refresh_all()
            self.log(f"{len(folders)} caméra(s) capturée(s) en direct.")

    def configure_board(self) -> None:
        dialog = BoardDialog(self.project.board, self.project.detector_options, self)
        if not dialog.exec():
            return
        board, options = dialog.values()
        changed = board.to_dict() != self.project.board.to_dict()
        self.project.board = board
        self.project.detector_options = options
        if changed and self.project.detections:
            self.project.clear_detections()
            self.log("Mire modifiée : les détections précédentes ont été effacées.")
        self.log(f"Mire : {board.describe()}")
        self.refresh_all()

    def configure_solver(self) -> None:
        dialog = SolverDialog(
            self.project.camera_model,
            self.project.parameter_mask,
            self.project.bundle_options,
            max(1, self.project.n_cameras),
            self,
        )
        if not dialog.exec():
            return
        model, mask, options = dialog.values()
        self.project.camera_model = model
        self.project.parameter_mask = mask
        self.project.bundle_options = options
        self.log(
            f"Modèle : {MODEL_LABELS[model]} — paramètres libres : "
            + ", ".join(mask.free_names(model))
        )
        self.refresh_status()

    def detect(self) -> None:
        if not self.project.cameras:
            QMessageBox.information(self, "Aucune image", "Définissez d'abord les images des caméras.")
            return
        self.log(f"Détection de la mire — {self.project.board.describe()}")
        worker = workers.DetectionWorker(self.project)

        def finished(summary) -> None:
            self._worker_done()
            self.log(summary.text())
            for issue in self.project.validate():
                self.log(f"attention : {issue}")
            self.refresh_all()

        self._start(worker, finished, "Détection…")

    def calibrate(self) -> None:
        if not self.project.detections:
            QMessageBox.information(self, "Aucune détection", "Lancez d'abord la détection (F5).")
            return
        sizes = camera_image_sizes(self.project)
        worker = workers.CalibrationWorker(self.project, self.project.bundle_options, sizes)

        def finished(result) -> None:
            self._worker_done()
            self.project.result = result
            self.refresh_all()
            self.tabs.setCurrentIndex(2)

        self._start(worker, finished, "Optimisation…")

    def triangulate_mediapipe(self) -> None:
        """Reconstruct MediaPipe joint tracks in 3D on the calibrated rig."""
        if not self.project.result:
            QMessageBox.information(
                self,
                "Dispositif non étalonné",
                "Étalonnez d'abord les caméras (F6) : la triangulation a besoin de leurs poses.",
            )
            return
        names = [camera.name for camera in self.project.result.cameras]
        dialog = MediaPipeDialog(names, self)
        if not dialog.exec():
            return
        options = dialog.values()
        self.log(f"Triangulation MediaPipe — {len(options['sources'])} fichier(s) de points")

        def finished(reconstruction) -> None:
            self._worker_done()
            errors = reconstruction.errors[np.isfinite(reconstruction.errors)]
            self.log(
                f"{reconstruction.n_frames} trame(s) — "
                f"{reconstruction.completeness() * 100:.1f} % des points reconstruits — "
                f"erreur de reprojection médiane "
                f"{np.median(errors) if errors.size else float('nan'):.3f} px"
            )
            lengths = reconstruction.segment_lengths()
            for name in ("epaule_gauche–epaule_droite", "hanche_gauche–hanche_droite"):
                if name in lengths:
                    self.log(f"    {name} : {lengths[name] * 100:.1f} cm (médiane)")
            path, _ = QFileDialog.getSaveFileName(
                self,
                "Enregistrer les trajectoires 3D",
                "points3d.npz",
                "Archive NumPy (*.npz);;CSV (*.csv)",
            )
            if path:
                reconstruction.save(path)
                self.log(f"Trajectoires 3D enregistrées : {path}")

        self._start(workers.TriangulationWorker(self.project.result, options), finished, "Triangulation…")

    def export_result(self) -> None:
        if not self.project.result:
            QMessageBox.information(self, "Rien à exporter", "Lancez d'abord l'optimisation (F6).")
            return
        path, selected = QFileDialog.getSaveFileName(
            self,
            "Exporter l'étalonnage",
            "etalonnage.json",
            "JSON du dispositif (*.json);;OpenCV FileStorage (*.yml);;Rapport texte (*.txt)",
        )
        if not path:
            return
        fmt = {"JSON du dispositif (*.json)": "json", "OpenCV FileStorage (*.yml)": "opencv"}.get(
            selected, ""
        )
        if fmt not in EXPORT_FORMATS:
            fmt = ""
        try:
            export_result(path, self.project.result, self.project.board, fmt, self.project.notes)
        except (OSError, ValueError) as error:
            QMessageBox.critical(self, "Export impossible", str(error))
            return
        self.log(f"Étalonnage exporté : {path}")

    # -- worker plumbing --------------------------------------------------

    def _start(self, worker, on_finished, label: str) -> None:
        if self.thread is not None:
            QMessageBox.information(self, "Traitement en cours", "Attendez la fin du traitement en cours.")
            return
        self.worker = worker
        self.progress.setVisible(True)
        self.progress.setValue(0)
        self.statusBar().showMessage(label)

        def failed(message: str) -> None:
            self._worker_done()
            self.log(f"ÉCHEC — {message}")
            QMessageBox.critical(self, "Échec du traitement", message)

        self.thread = workers.start(
            worker,
            on_finished,
            failed,
            on_progress=lambda value: self.progress.setValue(int(value * 100)),
            on_message=self.log,
        )
        self.refresh_status()

    def _worker_done(self) -> None:
        self.thread = None
        self.worker = None
        self.progress.setVisible(False)
        self.statusBar().clearMessage()
        self.refresh_status()

    def stop_worker(self) -> None:
        if self.worker is not None:
            self.worker.request_stop()
            self.log("Interruption demandée…")

    # -- help -------------------------------------------------------------

    def show_help(self) -> None:
        QMessageBox.information(
            self,
            "Mode d'emploi",
            "<b>Étalonnage d'un dispositif multi-caméras</b>"
            "<ol>"
            "<li><b>Configurer la mire</b> : saisissez la géométrie réelle de la mire imprimée "
            "(un côté de case erroné fausse toute l'échelle métrique).</li>"
            "<li><b>Définir les images</b> : un dossier par caméra. La pose <i>i</i> est la "
            "<i>i</i>-ème image de chaque dossier — les prises doivent être synchronisées.</li>"
            "<li><b>Détecter la mire</b> (F5).</li>"
            "<li><b>Optimiser les caméras</b> (F6) : intrinsèques, puis pose relative, puis "
            "ajustement de faisceaux.</li>"
            "<li><b>Exporter</b> le dispositif en JSON pour la triangulation des articulations.</li>"
            "</ol>"
            "<p>Pour lier deux caméras, il faut des poses où la mire est vue <i>simultanément</i> "
            "par les deux. Visez au moins 20 poses par caméra, réparties sur tout le champ et à "
            "plusieurs inclinaisons.</p>",
        )

    def show_about(self) -> None:
        from .. import __version__

        QMessageBox.about(
            self,
            "À propos",
            f"<b>Calib Laban {__version__}</b><br>"
            "Étalonnage multi-caméras pour la captation du mouvement dansé.<br><br>"
            "Projet de recherche Laban-Notation.<br>"
            "Cœur de calcul : OpenCV, SciPy, NumPy — interface : PySide6.",
        )

    def closeEvent(self, event) -> None:
        if self.thread is not None:
            self.stop_worker()
            self.thread.quit()
            self.thread.wait(3000)
        super().closeEvent(event)
