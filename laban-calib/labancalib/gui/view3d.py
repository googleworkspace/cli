"""3D view of the rig: camera frustums and the board poses that were captured.

The dark panel in the top right of the window. Cameras are drawn as frustums
with their optical axis, board poses as filled quads, and the reference camera
is highlighted — the same reading as the reference tools, so a wrong extrinsic
(a camera pointing the wrong way) is obvious at a glance.
"""

from __future__ import annotations

import numpy as np
from matplotlib.backends.backend_qtagg import FigureCanvasQTAgg
from matplotlib.figure import Figure
from matplotlib.ticker import MaxNLocator
from mpl_toolkits.mplot3d.art3d import Poly3DCollection

from ..models import CalibrationResult
from ..project import Project
from .plots import camera_colour

BACKGROUND = "#3a3a3a"
FOREGROUND = "#dddddd"


class View3D(FigureCanvasQTAgg):
    """Camera placement and captured board poses, in the reference frame."""

    def __init__(self, parent=None):
        self.figure = Figure(figsize=(4, 3), layout="constrained", facecolor=BACKGROUND)
        super().__init__(self.figure)
        self.setParent(parent)
        self.axes = self.figure.add_subplot(111, projection="3d")
        self._highlight: int | None = None
        self.clear()

    def clear(self, message: str = "Aucun résultat d'étalonnage.") -> None:
        self.figure.clear()
        self.axes = self.figure.add_subplot(111, projection="3d")
        self._style_axes()
        self.axes.text2D(0.5, 0.5, message, transform=self.axes.transAxes, color=FOREGROUND, ha="center", fontsize=9)
        self.draw_idle()

    def _style_axes(self) -> None:
        axes = self.axes
        axes.set_facecolor(BACKGROUND)
        for pane in (axes.xaxis, axes.yaxis, axes.zaxis):
            pane.set_pane_color((0.23, 0.23, 0.23, 1.0))
            pane.line.set_color(FOREGROUND)
            pane.label.set_color(FOREGROUND)
        axes.tick_params(colors=FOREGROUND, labelsize=6, pad=0)
        for axis in (axes.xaxis, axes.yaxis, axes.zaxis):
            axis.set_major_locator(MaxNLocator(4))
        axes.grid(color="#555555")

    def highlight_pose(self, pose_index: int | None) -> None:
        """Emphasise one board pose (called when the tree selection changes)."""
        self._highlight = pose_index

    def update_from(self, project: Project) -> None:
        result = project.result
        if result is None or not result.cameras:
            self.clear()
            return
        self.figure.clear()
        self.axes = self.figure.add_subplot(111, projection="3d")
        self._style_axes()
        self._draw(project, result)
        self.draw_idle()

    def _draw(self, project: Project, result: CalibrationResult) -> None:
        axes = self.axes
        points: list[np.ndarray] = []
        scale = self._scene_scale(result)

        for index, camera in enumerate(result.cameras):
            centre = camera.pose.centre
            rotation = camera.pose.inverse().R  # camera axes expressed in the reference frame
            points.append(centre)
            self._draw_frustum(centre, rotation, camera.intrinsics, scale, camera_colour(index), index == 0)
            axes.text(*centre, f" {index}", color=FOREGROUND, fontsize=8)

        corners = self._board_outline(project)
        for pose_index, pose in sorted(result.board_poses.items()):
            world = (pose.R @ corners.T).T + pose.tvec
            points.append(world.mean(axis=0))
            emphasised = pose_index == self._highlight
            axes.add_collection3d(
                Poly3DCollection(
                    [world],
                    facecolor="#ffffff" if emphasised else "#cccccc",
                    edgecolor="#ff9900" if emphasised else "#888888",
                    linewidths=1.4 if emphasised else 0.5,
                    alpha=0.85 if emphasised else 0.35,
                )
            )

        self._set_equal_aspect(np.asarray(points))
        for setter, label in (
            (axes.set_xlabel, "x (m)"),
            (axes.set_ylabel, "y (m)"),
            (axes.set_zlabel, "z (m)"),
        ):
            setter(label, fontsize=7, color=FOREGROUND, labelpad=-4)
        # y points down in the camera frame: look slightly from above and behind
        axes.view_init(elev=-65, azim=-70)

    def _scene_scale(self, result: CalibrationResult) -> float:
        centres = np.array([c.pose.centre for c in result.cameras])
        if len(centres) < 2:
            return 0.15
        spread = float(np.linalg.norm(centres.max(axis=0) - centres.min(axis=0)))
        return max(0.08, 0.20 * (spread or 1.0))

    def _draw_frustum(self, centre, rotation, intrinsics, scale, colour, is_reference: bool) -> None:
        width, height = intrinsics.image_size
        if not width or not height or not intrinsics.f:
            width, height, focal = 4.0, 3.0, 4.0
        else:
            focal = intrinsics.f
        half_x = 0.5 * width / focal * scale
        half_y = 0.5 * height / focal * scale
        local = np.array(
            [
                [0.0, 0.0, 0.0],
                [-half_x, -half_y, scale],
                [half_x, -half_y, scale],
                [half_x, half_y, scale],
                [-half_x, half_y, scale],
            ]
        )
        world = (rotation @ local.T).T + centre
        faces = [
            [world[0], world[1], world[2]],
            [world[0], world[2], world[3]],
            [world[0], world[3], world[4]],
            [world[0], world[4], world[1]],
        ]
        self.axes.add_collection3d(
            Poly3DCollection(faces, facecolor=colour, edgecolor=colour, alpha=0.30 if not is_reference else 0.45)
        )
        image_plane = [world[1], world[2], world[3], world[4]]
        self.axes.add_collection3d(
            Poly3DCollection([image_plane], facecolor="none", edgecolor=colour, linewidths=1.2)
        )
        axis_end = centre + rotation[:, 2] * scale * 1.6
        self.axes.plot(*zip(centre, axis_end), color=colour, linewidth=1.0, linestyle="--")

    def _board_outline(self, project: Project) -> np.ndarray:
        points = project.board.object_points()
        low, high = points.min(axis=0), points.max(axis=0)
        return np.array(
            [
                [low[0], low[1], 0.0],
                [high[0], low[1], 0.0],
                [high[0], high[1], 0.0],
                [low[0], high[1], 0.0],
            ]
        )

    def _set_equal_aspect(self, points: np.ndarray) -> None:
        if points.size == 0:
            return
        centre = (points.max(axis=0) + points.min(axis=0)) / 2.0
        radius = float(np.max(points.max(axis=0) - points.min(axis=0))) / 2.0 or 0.5
        radius *= 1.25
        self.axes.set_xlim(centre[0] - radius, centre[0] + radius)
        self.axes.set_ylim(centre[1] - radius, centre[1] + radius)
        self.axes.set_zlim(centre[2] - radius, centre[2] + radius)
        self.axes.set_box_aspect((1, 1, 1))
