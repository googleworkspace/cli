"""Image viewer with the detection overlay, zoom and pan."""

from __future__ import annotations

import numpy as np
from PySide6.QtCore import Qt
from PySide6.QtGui import QImage, QPixmap, QWheelEvent
from PySide6.QtWidgets import QGraphicsPixmapItem, QGraphicsScene, QGraphicsView


def to_qimage(image: np.ndarray) -> QImage:
    """Convert an OpenCV BGR (or grayscale) array into a QImage."""
    array = np.ascontiguousarray(image)
    if array.ndim == 2:
        height, width = array.shape
        return QImage(array.data, width, height, width, QImage.Format_Grayscale8).copy()
    height, width, channels = array.shape
    if channels == 4:
        return QImage(array.data, width, height, 4 * width, QImage.Format_ARGB32).copy()
    return QImage(array.data, width, height, 3 * width, QImage.Format_BGR888).copy()


class ImageView(QGraphicsView):
    """Zoomable image panel; the wheel zooms, dragging pans."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self._scene = QGraphicsScene(self)
        self.setScene(self._scene)
        self._item = QGraphicsPixmapItem()
        self._scene.addItem(self._item)
        self.setDragMode(QGraphicsView.ScrollHandDrag)
        self.setRenderHints(self.renderHints())
        self.setBackgroundBrush(Qt.white)
        self.setAlignment(Qt.AlignCenter)
        self._has_image = False
        self._fit_pending = True

    def show_image(self, image: np.ndarray | None, keep_view: bool = False) -> None:
        if image is None:
            self._item.setPixmap(QPixmap())
            self._has_image = False
            return
        pixmap = QPixmap.fromImage(to_qimage(image))
        first = not self._has_image
        self._item.setPixmap(pixmap)
        self._scene.setSceneRect(self._item.boundingRect())
        self._has_image = True
        if first or self._fit_pending or not keep_view:
            self.fit()

    def fit(self) -> None:
        if self._has_image:
            self.fitInView(self._item, Qt.KeepAspectRatio)
            self._fit_pending = False

    def zoom(self, factor: float) -> None:
        if self._has_image:
            self.scale(factor, factor)

    def wheelEvent(self, event: QWheelEvent) -> None:
        if not self._has_image:
            return
        self.setTransformationAnchor(QGraphicsView.AnchorUnderMouse)
        self.zoom(1.15 if event.angleDelta().y() > 0 else 1 / 1.15)

    def resizeEvent(self, event) -> None:
        super().resizeEvent(event)
        if self._fit_pending:
            self.fit()
