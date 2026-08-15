"""Calibration target (mire) description, detection and printable rendering.

Three target families are supported:

``charuco``     ChArUco board — a chessboard whose white cells carry ArUco
                markers. Robust to occlusion and to a target running out of
                frame, which is the common case in a dance studio where the
                operator waves the board in front of wide-angle cameras.
``acircles``    Asymmetric circle grid — sub-pixel accurate under diffuse
                studio lighting, but must be fully visible.
``circles``     Symmetric circle grid.
``chessboard``  Classic chessboard (kept because it costs nothing and remains
                the reference target in the literature).

Every detector returns the same pair ``(ids, pts)``: the indices of the board
points that were seen, and their (N,2) sub-pixel image coordinates. Point ids
index :meth:`BoardSpec.object_points`, so partial detections are first-class.
"""

from __future__ import annotations

from dataclasses import dataclass, field

import cv2
import numpy as np

ARUCO_DICTIONARIES = [
    "DICT_4X4_50",
    "DICT_4X4_100",
    "DICT_4X4_250",
    "DICT_5X5_50",
    "DICT_5X5_100",
    "DICT_5X5_250",
    "DICT_6X6_50",
    "DICT_6X6_100",
    "DICT_6X6_250",
    "DICT_7X7_50",
    "DICT_APRILTAG_36h11",
]

BOARD_KINDS = ("charuco", "acircles", "circles", "chessboard")

KIND_LABELS = {
    "charuco": "ChArUco",
    "acircles": "Cercles asymétriques",
    "circles": "Cercles symétriques",
    "chessboard": "Échiquier",
}


@dataclass
class BoardSpec:
    """Geometry of the calibration target.

    ``cols``/``rows`` count squares for ChArUco and chessboard-style targets
    (inner corners are derived), and circles per row/column for circle grids.
    All lengths are metres.
    """

    kind: str = "charuco"
    cols: int = 8
    rows: int = 6
    square_size: float = 0.030
    marker_size: float = 0.022
    dictionary: str = "DICT_5X5_100"
    legacy_pattern: bool = False

    def __post_init__(self) -> None:
        if self.kind not in BOARD_KINDS:
            raise ValueError(f"unknown board kind: {self.kind!r}")
        if self.cols < 2 or self.rows < 2:
            raise ValueError("a board needs at least 2 columns and 2 rows")
        if self.square_size <= 0:
            raise ValueError("square_size must be positive")
        if self.kind == "charuco" and not 0 < self.marker_size < self.square_size:
            raise ValueError("marker_size must be positive and smaller than square_size")

    # -- geometry ---------------------------------------------------------

    @property
    def label(self) -> str:
        return KIND_LABELS[self.kind]

    @property
    def grid(self) -> tuple[int, int]:
        """Number of detectable points along x and y."""
        if self.kind in ("charuco", "chessboard"):
            return self.cols - 1, self.rows - 1
        return self.cols, self.rows

    @property
    def n_points(self) -> int:
        nx, ny = self.grid
        return nx * ny

    @property
    def size_metres(self) -> tuple[float, float]:
        """Physical extent (width, height) of the printed target."""
        if self.kind in ("charuco", "chessboard"):
            return self.cols * self.square_size, self.rows * self.square_size
        pts = self.object_points()
        span = pts.max(axis=0) - pts.min(axis=0)
        # one square of quiet zone so the outer circles are not clipped
        return float(span[0]) + self.square_size, float(span[1]) + self.square_size

    def object_points(self) -> np.ndarray:
        """(N,3) coordinates of every board point in the board frame (z = 0)."""
        nx, ny = self.grid
        s = self.square_size
        if self.kind == "charuco":
            return np.asarray(self.cv_board().getChessboardCorners(), dtype=np.float64)
        if self.kind == "acircles":
            pts = [
                ((2 * j + i % 2) * s * 0.5, i * s * 0.5, 0.0)
                for i in range(ny)
                for j in range(nx)
            ]
            return np.asarray(pts, dtype=np.float64)
        pts = [(j * s, i * s, 0.0) for i in range(ny) for j in range(nx)]
        return np.asarray(pts, dtype=np.float64)

    # -- OpenCV objects ---------------------------------------------------

    def aruco_dictionary(self):
        if not hasattr(cv2.aruco, self.dictionary):
            raise ValueError(f"unknown ArUco dictionary: {self.dictionary}")
        return cv2.aruco.getPredefinedDictionary(getattr(cv2.aruco, self.dictionary))

    def cv_board(self):
        """The ``cv2.aruco.CharucoBoard`` backing a ChArUco target."""
        if self.kind != "charuco":
            raise ValueError("cv_board() is only defined for ChArUco targets")
        board = cv2.aruco.CharucoBoard(
            (self.cols, self.rows),
            float(self.square_size),
            float(self.marker_size),
            self.aruco_dictionary(),
        )
        board.setLegacyPattern(bool(self.legacy_pattern))
        return board

    # -- serialisation ----------------------------------------------------

    def to_dict(self) -> dict:
        return {
            "kind": self.kind,
            "cols": self.cols,
            "rows": self.rows,
            "square_size": self.square_size,
            "marker_size": self.marker_size,
            "dictionary": self.dictionary,
            "legacy_pattern": self.legacy_pattern,
        }

    @classmethod
    def from_dict(cls, data: dict) -> "BoardSpec":
        known = {f for f in cls.__dataclass_fields__}
        return cls(**{k: v for k, v in data.items() if k in known})

    def describe(self) -> str:
        w, h = self.size_metres
        base = f"{self.label} {self.cols}×{self.rows}, case {self.square_size * 1000:.1f} mm"
        if self.kind == "charuco":
            base += f", marqueur {self.marker_size * 1000:.1f} mm, {self.dictionary}"
        return f"{base} ({w * 1000:.0f}×{h * 1000:.0f} mm)"


@dataclass
class Detection:
    """Board points found in a single image."""

    ids: np.ndarray
    points: np.ndarray  # (N,2) float32/float64 pixels
    n_markers: int = 0

    @property
    def count(self) -> int:
        return int(len(self.ids))

    def object_points(self, board: BoardSpec) -> np.ndarray:
        return board.object_points()[self.ids]

    def to_dict(self) -> dict:
        return {
            "ids": np.asarray(self.ids).astype(int).tolist(),
            "points": np.asarray(self.points, dtype=float).reshape(-1, 2).tolist(),
            "n_markers": int(self.n_markers),
        }

    @classmethod
    def from_dict(cls, data: dict) -> "Detection":
        return cls(
            ids=np.asarray(data["ids"], dtype=int),
            points=np.asarray(data["points"], dtype=np.float64).reshape(-1, 2),
            n_markers=int(data.get("n_markers", 0)),
        )


@dataclass
class DetectorOptions:
    """Knobs exposed in the detection dialog."""

    refine_corners: bool = True
    refine_window: int = 5
    min_points: int = 6
    adaptive_threshold: bool = True
    clahe: bool = False
    invert: bool = False
    extra: dict = field(default_factory=dict)


class BoardDetector:
    """Stateful detector — building the OpenCV objects once is worth it."""

    def __init__(self, board: BoardSpec, options: DetectorOptions | None = None):
        self.board = board
        self.options = options or DetectorOptions()
        self._charuco = None
        if board.kind == "charuco":
            params = cv2.aruco.DetectorParameters()
            if self.options.adaptive_threshold:
                params.adaptiveThreshWinSizeMin = 3
                params.adaptiveThreshWinSizeMax = 53
                params.adaptiveThreshWinSizeStep = 10
            params.cornerRefinementMethod = (
                cv2.aruco.CORNER_REFINE_SUBPIX
                if self.options.refine_corners
                else cv2.aruco.CORNER_REFINE_NONE
            )
            self._charuco = cv2.aruco.CharucoDetector(
                board.cv_board(), cv2.aruco.CharucoParameters(), params
            )

    # -- helpers ----------------------------------------------------------

    def _prepare(self, image: np.ndarray) -> np.ndarray:
        gray = image
        if gray.ndim == 3:
            gray = cv2.cvtColor(gray, cv2.COLOR_BGR2GRAY)
        if gray.dtype != np.uint8:
            gray = cv2.normalize(gray, None, 0, 255, cv2.NORM_MINMAX).astype(np.uint8)
        if self.options.clahe:
            gray = cv2.createCLAHE(clipLimit=2.0, tileGridSize=(8, 8)).apply(gray)
        if self.options.invert:
            gray = cv2.bitwise_not(gray)
        return gray

    def _refine(self, gray: np.ndarray, pts: np.ndarray) -> np.ndarray:
        if not self.options.refine_corners or len(pts) == 0:
            return pts
        w = max(2, int(self.options.refine_window))
        criteria = (cv2.TERM_CRITERIA_EPS + cv2.TERM_CRITERIA_MAX_ITER, 40, 1e-4)
        refined = cv2.cornerSubPix(
            gray, np.asarray(pts, dtype=np.float32).reshape(-1, 1, 2), (w, w), (-1, -1), criteria
        )
        return np.asarray(refined, dtype=np.float64).reshape(-1, 2)

    # -- detection --------------------------------------------------------

    def detect(self, image: np.ndarray) -> Detection | None:
        """Find the target in ``image``; returns ``None`` when it is not seen."""
        gray = self._prepare(image)
        kind = self.board.kind
        if kind == "charuco":
            det = self._detect_charuco(gray)
        elif kind in ("circles", "acircles"):
            det = self._detect_circles(gray)
        else:
            det = self._detect_chessboard(gray)
        if det is None or det.count < max(4, self.options.min_points):
            return None
        return det

    def _detect_charuco(self, gray: np.ndarray) -> Detection | None:
        corners, ids, marker_corners, _ = self._charuco.detectBoard(gray)
        if ids is None or len(ids) == 0:
            return None
        pts = np.asarray(corners, dtype=np.float64).reshape(-1, 2)
        return Detection(
            ids=np.asarray(ids, dtype=int).ravel(),
            points=pts,
            n_markers=0 if marker_corners is None else len(marker_corners),
        )

    def _detect_circles(self, gray: np.ndarray) -> Detection | None:
        nx, ny = self.board.grid
        flags = cv2.CALIB_CB_ASYMMETRIC_GRID if self.board.kind == "acircles" else cv2.CALIB_CB_SYMMETRIC_GRID
        flags |= cv2.CALIB_CB_CLUSTERING
        detector = _blob_detector(self.options)
        # Circle grids are found on a dark-on-light target; try the inverse too.
        for image in (gray, cv2.bitwise_not(gray)):
            found, centres = cv2.findCirclesGrid(image, (nx, ny), flags=flags, blobDetector=detector)
            if found:
                pts = np.asarray(centres, dtype=np.float64).reshape(-1, 2)
                return Detection(ids=np.arange(nx * ny, dtype=int), points=pts)
        return None

    def _detect_chessboard(self, gray: np.ndarray) -> Detection | None:
        nx, ny = self.board.grid
        flags = cv2.CALIB_CB_NORMALIZE_IMAGE | cv2.CALIB_CB_EXHAUSTIVE | cv2.CALIB_CB_ACCURACY
        found, corners = cv2.findChessboardCornersSB(gray, (nx, ny), flags=flags)
        if not found:
            found, corners = cv2.findChessboardCorners(
                gray, (nx, ny), flags=cv2.CALIB_CB_ADAPTIVE_THRESH | cv2.CALIB_CB_NORMALIZE_IMAGE
            )
            if not found:
                return None
            corners = self._refine(gray, np.asarray(corners).reshape(-1, 2))
        pts = np.asarray(corners, dtype=np.float64).reshape(-1, 2)
        return Detection(ids=np.arange(nx * ny, dtype=int), points=pts)


def _blob_detector(options: DetectorOptions):
    params = cv2.SimpleBlobDetector.Params()
    params.filterByArea = True
    params.minArea = float(options.extra.get("blob_min_area", 25.0))
    params.maxArea = float(options.extra.get("blob_max_area", 50000.0))
    params.filterByCircularity = True
    params.minCircularity = 0.7
    params.filterByInertia = True
    params.minInertiaRatio = 0.1
    params.filterByConvexity = True
    params.minConvexity = 0.85
    return cv2.SimpleBlobDetector.create(params)


# -- printable rendering --------------------------------------------------


def render_board(board: BoardSpec, pixels_per_metre: float = 4000.0, margin: float = 0.01) -> np.ndarray:
    """Render the target as a grayscale image at a known physical scale."""
    w_m, h_m = board.size_metres
    m = int(round(margin * pixels_per_metre))
    w = int(round(w_m * pixels_per_metre))
    h = int(round(h_m * pixels_per_metre))
    canvas = np.full((h + 2 * m, w + 2 * m), 255, dtype=np.uint8)

    if board.kind == "charuco":
        img = board.cv_board().generateImage((w, h))
    elif board.kind == "chessboard":
        img = np.full((h, w), 255, dtype=np.uint8)
        cell_x, cell_y = w / board.cols, h / board.rows
        for i in range(board.rows):
            for j in range(board.cols):
                if (i + j) % 2 == 0:
                    y0, y1 = int(round(i * cell_y)), int(round((i + 1) * cell_y))
                    x0, x1 = int(round(j * cell_x)), int(round((j + 1) * cell_x))
                    img[y0:y1, x0:x1] = 0
    else:
        img = np.full((h, w), 255, dtype=np.uint8)
        radius = int(round(0.3 * board.square_size * pixels_per_metre))
        offset = 0.5 * board.square_size  # matches the quiet zone in size_metres
        for (x, y, _) in board.object_points():
            cv2.circle(
                img,
                (
                    int(round((x + offset) * pixels_per_metre)),
                    int(round((y + offset) * pixels_per_metre)),
                ),
                radius,
                0,
                -1,
                lineType=cv2.LINE_AA,
            )
    canvas[m : m + img.shape[0], m : m + img.shape[1]] = img
    return canvas


def save_board_pdf(board: BoardSpec, path: str, page: str = "A4") -> None:
    """Write a true-to-scale PDF of the target (so printed squares measure right)."""
    import matplotlib

    matplotlib.use("Agg", force=False)
    import matplotlib.pyplot as plt
    from matplotlib.backends.backend_pdf import PdfPages

    pages = {"A4": (0.210, 0.297), "A3": (0.297, 0.420), "LETTER": (0.216, 0.279)}
    pw, ph = pages.get(page.upper(), pages["A4"])
    w_m, h_m = board.size_metres
    if w_m > h_m and pw < ph:  # landscape target on a portrait page
        pw, ph = ph, pw
    img = render_board(board, pixels_per_metre=4000.0, margin=0.0)

    fig = plt.figure(figsize=(pw / 0.0254, ph / 0.0254))
    ax = fig.add_axes([
        (pw - w_m) / (2 * pw),
        (ph - h_m) / (2 * ph),
        w_m / pw,
        h_m / ph,
    ])
    ax.imshow(img, cmap="gray", vmin=0, vmax=255, interpolation="nearest", aspect="auto")
    ax.set_axis_off()
    fig.text(
        0.5,
        0.02,
        f"{board.describe()} — imprimer à 100 % (sans mise à l'échelle)",
        ha="center",
        fontsize=8,
    )
    with PdfPages(path) as pdf:
        pdf.savefig(fig)
    plt.close(fig)


def draw_detection(
    image: np.ndarray, detection: Detection | None, board: BoardSpec, reprojection: np.ndarray | None = None
) -> np.ndarray:
    """Overlay a detection (green grid + red circles) the way the reference app does."""
    canvas = image if image.ndim == 3 else cv2.cvtColor(image, cv2.COLOR_GRAY2BGR)
    canvas = canvas.copy()
    if detection is None or detection.count == 0:
        return canvas
    nx, _ = board.grid
    index = {int(i): k for k, i in enumerate(np.asarray(detection.ids).ravel())}
    pts = np.asarray(detection.points, dtype=np.float64)
    # green segments between horizontally/vertically adjacent board points
    for point_id, k in index.items():
        for neighbour, same_row in ((point_id + 1, True), (point_id + nx, False)):
            if same_row and (point_id + 1) % nx == 0:
                continue
            if neighbour in index:
                p0 = tuple(np.round(pts[k]).astype(int))
                p1 = tuple(np.round(pts[index[neighbour]]).astype(int))
                cv2.line(canvas, p0, p1, (0, 255, 0), 1, cv2.LINE_AA)
    for p in pts:
        cv2.circle(canvas, tuple(np.round(p).astype(int)), 5, (0, 0, 255), 1, cv2.LINE_AA)
    if reprojection is not None:
        for p in np.asarray(reprojection, dtype=np.float64).reshape(-1, 2):
            cv2.drawMarker(
                canvas, tuple(np.round(p).astype(int)), (255, 128, 0), cv2.MARKER_CROSS, 7, 1, cv2.LINE_AA
            )
    return canvas
