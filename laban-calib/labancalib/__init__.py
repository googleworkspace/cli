"""laban-calib — étalonnage multi-caméras pour la captation de mouvement.

Outil d'étalonnage (intrinsèques + extrinsèques) d'un dispositif de plusieurs
caméras, destiné à la reconstruction 3D du geste dansé dans le projet de
recherche Laban-Notation.

Le cœur de calcul est indépendant de l'interface : ``labancalib.pipeline``
enchaîne détection, initialisation et ajustement de faisceaux, et
``labancalib.triangulate`` réutilise le résultat pour reconstruire les
trajectoires articulaires en 3D.
"""

from .board import BoardDetector, BoardSpec, Detection, DetectorOptions
from .bundle import BundleOptions, Observation, bundle_adjust, initial_extrinsics
from .models import (
    CalibrationResult,
    CameraCalibration,
    FISHEYE,
    Intrinsics,
    ParameterMask,
    PINHOLE,
    Pose,
)
from .pipeline import run_calibration, run_detection
from .project import CameraSource, Project
from .triangulate import Rig

__version__ = "0.1.0"

__all__ = [
    "BoardDetector",
    "BoardSpec",
    "BundleOptions",
    "CalibrationResult",
    "CameraCalibration",
    "CameraSource",
    "Detection",
    "DetectorOptions",
    "FISHEYE",
    "Intrinsics",
    "Observation",
    "PINHOLE",
    "ParameterMask",
    "Pose",
    "Project",
    "Rig",
    "bundle_adjust",
    "initial_extrinsics",
    "run_calibration",
    "run_detection",
    "__version__",
]
