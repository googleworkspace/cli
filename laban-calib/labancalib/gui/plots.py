"""Matplotlib panels: convergence, initialisation, RPE and coverage."""

from __future__ import annotations

import numpy as np
from matplotlib.backends.backend_qtagg import FigureCanvasQTAgg
from matplotlib.figure import Figure
from matplotlib.patches import Circle

from ..models import CalibrationResult
from ..project import Project

CAMERA_COLOURS = [
    "#1f77b4",
    "#d62728",
    "#2ca02c",
    "#ff7f0e",
    "#9467bd",
    "#8c564b",
    "#e377c2",
    "#17becf",
]


def camera_colour(index: int) -> str:
    return CAMERA_COLOURS[index % len(CAMERA_COLOURS)]


class PlotCanvas(FigureCanvasQTAgg):
    """A figure that knows how to redraw itself from a project."""

    title = ""

    def __init__(self, parent=None):
        self.figure = Figure(figsize=(5, 3.2), layout="constrained")
        super().__init__(self.figure)
        self.setParent(parent)
        self.clear("Aucun résultat — lancez l'optimisation.")

    def clear(self, message: str = "") -> None:
        self.figure.clear()
        if message:
            axes = self.figure.add_subplot(111)
            axes.text(0.5, 0.5, message, ha="center", va="center", fontsize=9, color="#666666")
            axes.set_axis_off()
        self.draw_idle()

    def update_from(self, project: Project) -> None:
        result = project.result
        if result is None:
            self.clear("Aucun résultat — lancez l'optimisation.")
            return
        self.figure.clear()
        self.plot(self.figure, project, result)
        self.draw_idle()

    def plot(self, figure: Figure, project: Project, result: CalibrationResult) -> None:
        raise NotImplementedError


class ConvergenceCanvas(PlotCanvas):
    title = "Convergence"

    def plot(self, figure, project, result):
        axes = figure.add_subplot(111)
        values = np.asarray(result.convergence, dtype=float)
        if values.size == 0:
            axes.text(0.5, 0.5, "pas de trace d'optimisation", ha="center", va="center")
            axes.set_axis_off()
            return
        axes.plot(values, color="#1f77b4", linewidth=1.4)
        axes.axhline(result.rpe, color="#d62728", linestyle="--", linewidth=1.0, label=f"RPE final {result.rpe:.4f} px")
        axes.set_xlabel("évaluations de la fonction de coût")
        axes.set_ylabel("RPE (px)")
        axes.set_yscale("log" if values.max() > 10 * max(values.min(), 1e-6) else "linear")
        axes.grid(alpha=0.3)
        axes.legend(fontsize=8)
        axes.set_title("Décroissance du RPE", fontsize=10)


class InitialisationCanvas(PlotCanvas):
    title = "Initialisation"

    def plot(self, figure, project, result):
        axes = figure.add_subplot(111)
        coverage = project.coverage()
        if not coverage:
            axes.text(0.5, 0.5, "aucune détection", ha="center", va="center")
            axes.set_axis_off()
            return
        poses = sorted(coverage)
        counts = [coverage[p] for p in poses]
        axes.bar(poses, counts, color="#2ca02c", width=0.8)
        axes.axhline(2, color="#d62728", linestyle="--", linewidth=1.0, label="minimum pour lier deux caméras")
        axes.set_xlabel("pose (instant de capture)")
        axes.set_ylabel("caméras voyant la mire")
        axes.set_yticks(range(0, max(counts) + 1))
        axes.grid(alpha=0.3, axis="y")
        axes.legend(fontsize=8)
        axes.set_title("Recouvrement inter-caméras par pose", fontsize=10)


class RpeScatterCanvas(PlotCanvas):
    title = "Nuage RPE"

    def plot(self, figure, project, result):
        axes = figure.add_subplot(111)
        errors = np.asarray(result.errors_xy, dtype=float)
        if errors.size == 0:
            axes.text(0.5, 0.5, "aucun résidu", ha="center", va="center")
            axes.set_axis_off()
            return
        for index in range(len(result.cameras)):
            mask = result.errors_camera == index
            if not np.any(mask):
                continue
            axes.scatter(
                errors[mask, 0],
                errors[mask, 1],
                s=4,
                alpha=0.45,
                color=camera_colour(index),
                label=f"caméra {index} ({result.camera_rpe(index):.3f} px)",
            )
        limit = float(np.percentile(np.abs(errors), 99.5)) * 1.2 or 1.0
        axes.set_xlim(-limit, limit)
        axes.set_ylim(-limit, limit)
        axes.axhline(0, color="#999999", linewidth=0.6)
        axes.axvline(0, color="#999999", linewidth=0.6)
        for sigma in (1, 2):
            axes.add_patch(
                Circle(
                    (0, 0), sigma * result.rpe, fill=False, color="#666666", linestyle=":", linewidth=0.8
                )
            )
        axes.set_aspect("equal")
        axes.set_xlabel("erreur x (px)")
        axes.set_ylabel("erreur y (px)")
        axes.grid(alpha=0.25)
        axes.legend(fontsize=8, markerscale=2)
        axes.set_title("Résidus de reprojection", fontsize=10)


class RpeBarsCanvas(PlotCanvas):
    title = "Barres RPE"

    def plot(self, figure, project, result):
        axes = figure.add_subplot(111)
        if not result.residuals:
            axes.text(0.5, 0.5, "aucune vue", ha="center", va="center")
            axes.set_axis_off()
            return
        order = sorted(result.residuals, key=lambda r: (r.camera, r.pose_index))
        positions = np.arange(len(order))
        axes.bar(
            positions,
            [r.rms for r in order],
            color=[camera_colour(r.camera) for r in order],
            width=0.9,
        )
        axes.axhline(result.rpe, color="#333333", linestyle="--", linewidth=1.0, label=f"RPE global {result.rpe:.4f} px")
        axes.set_xlabel("vue (caméra, pose)")
        axes.set_ylabel("RPE de la vue (px)")
        step = max(1, len(order) // 12)
        axes.set_xticks(positions[::step])
        axes.set_xticklabels([f"{r.camera}·{r.pose_index}" for r in order[::step]], fontsize=7, rotation=90)
        axes.grid(alpha=0.3, axis="y")
        axes.legend(fontsize=8)
        axes.set_title("Erreur par vue", fontsize=10)


class CoverageCanvas(PlotCanvas):
    title = "Couverture"

    def update_from(self, project: Project) -> None:
        # Coverage is meaningful as soon as detection has run, before optimising.
        if not project.detections:
            self.clear("Aucune détection — lancez « Détecter la mire ».")
            return
        self.figure.clear()
        self.plot(self.figure, project, project.result)
        self.draw_idle()

    def plot(self, figure, project, result):
        cameras = max(1, project.n_cameras)
        columns = min(3, cameras)
        rows = int(np.ceil(cameras / columns))
        for index in range(cameras):
            axes = figure.add_subplot(rows, columns, index + 1)
            points = [
                d.points
                for (camera, _), d in project.detections.items()
                if camera == index and d.count
            ]
            if points:
                stacked = np.vstack(points)
                axes.hexbin(stacked[:, 0], stacked[:, 1], gridsize=22, cmap="viridis", mincnt=1)
            size = None
            if result is not None and index < len(result.cameras):
                size = result.cameras[index].intrinsics.image_size
            if size and size[0] and size[1]:
                axes.set_xlim(0, size[0])
                axes.set_ylim(size[1], 0)
            else:
                axes.invert_yaxis()
            axes.set_title(f"caméra {index}", fontsize=9)
            axes.tick_params(labelsize=7)
        figure.suptitle("Couverture du champ par les points détectés", fontsize=10)


CANVAS_TYPES = (
    ConvergenceCanvas,
    InitialisationCanvas,
    RpeScatterCanvas,
    RpeBarsCanvas,
    CoverageCanvas,
)
