"""Multi-camera initialisation and bundle adjustment.

The rig is solved in three steps, mirroring what the UI shows:

1. **Intrinsèques** — each camera is calibrated on its own (see
   :mod:`labancalib.intrinsics`).
2. **Initialisation** — relative camera poses are recovered from the board
   poses shared by camera pairs, then chained along a maximum spanning tree
   rooted on the reference camera.
3. **Optimisation** — every parameter (free intrinsics, camera poses, one
   board pose per capture instant) is refined together by minimising the
   reprojection error, and standard deviations are read off the Jacobian.

The reference camera is fixed at the identity, which fixes the gauge; the
metric scale comes from the physical size of the target.
"""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np
from scipy.optimize import least_squares
from scipy.sparse import lil_matrix

from . import geometry
from .board import BoardSpec, Detection
from .intrinsics import CalibrationError, solve_pose
from .models import (
    CalibrationResult,
    CameraCalibration,
    FISHEYE,
    Intrinsics,
    ParameterMask,
    Pose,
    ViewResidual,
)


@dataclass
class Observation:
    """One board detection, tagged with its camera and capture instant."""

    camera: int
    pose_index: int
    ids: np.ndarray
    points: np.ndarray

    @property
    def count(self) -> int:
        return int(len(self.ids))

    @classmethod
    def from_detection(cls, camera: int, pose_index: int, detection: Detection) -> "Observation":
        return cls(
            camera=camera,
            pose_index=pose_index,
            ids=np.asarray(detection.ids, dtype=int).ravel(),
            points=np.asarray(detection.points, dtype=np.float64).reshape(-1, 2),
        )


@dataclass
class BundleOptions:
    """Optimiser settings exposed in the calibration dialog."""

    max_iterations: int = 150
    tolerance: float = 1e-8
    robust: bool = True
    huber_delta: float = 2.0  # pixels
    reject_sigma: float = 0.0  # 0 disables the outlier rejection pass
    refine_intrinsics: bool = True
    estimate_sigmas: bool = True
    reference_camera: int = 0


# -- initialisation -------------------------------------------------------


def _pose_distance(a: Pose, b: Pose) -> float:
    """Rough distance between two poses, mixing rotation (rad) and translation (m)."""
    dr = geometry.rodrigues(a.rvec).T @ geometry.rodrigues(b.rvec)
    angle = float(np.arccos(np.clip((np.trace(dr) - 1.0) / 2.0, -1.0, 1.0)))
    return angle + float(np.linalg.norm(a.tvec - b.tvec))


def _median_pose(poses: list[Pose]) -> Pose:
    """Geometric-median-like pick: the sample closest to all the others.

    Picking an actual sample (rather than averaging rotation vectors, which is
    wrong on SO(3)) keeps the result valid and immune to a few outliers.
    """
    if len(poses) == 1:
        return poses[0]
    costs = [sum(_pose_distance(p, q) for q in poses) for p in poses]
    return poses[int(np.argmin(costs))]


def view_poses(
    board: BoardSpec, intrinsics: list[Intrinsics], observations: list[Observation]
) -> dict[tuple[int, int], Pose]:
    """Board pose in each camera frame, for every observation (PnP)."""
    out: dict[tuple[int, int], Pose] = {}
    for obs in observations:
        det = Detection(ids=obs.ids, points=obs.points)
        pose = solve_pose(board, det, intrinsics[obs.camera])
        if pose is not None and np.isfinite(pose.tvec).all():
            out[(obs.camera, obs.pose_index)] = pose
    return out


def initial_extrinsics(
    board: BoardSpec,
    intrinsics: list[Intrinsics],
    observations: list[Observation],
    reference: int = 0,
    log=None,
) -> tuple[list[Pose], dict[int, Pose], dict[tuple[int, int], Pose]]:
    """Chain relative poses into a consistent rig.

    Returns ``(camera poses, board poses, per-view PnP poses)``. Camera poses
    map the reference frame (the reference camera) into each camera; board
    poses map board coordinates into the reference frame.
    """
    n_cams = len(intrinsics)
    per_view = view_poses(board, intrinsics, observations)
    if not per_view:
        raise CalibrationError("aucune pose de mire n'a pu être estimée (PnP)")

    # camera pair -> list of relative poses (reference cam a -> cam b)
    seen: dict[int, dict[int, Pose]] = {}
    for (cam, pose_index), pose in per_view.items():
        seen.setdefault(pose_index, {})[cam] = pose
    pair_poses: dict[tuple[int, int], list[Pose]] = {}
    for cams in seen.values():
        for a in cams:
            for b in cams:
                if a == b:
                    continue
                # X_b = T_b (T_a)^-1 X_a
                pair_poses.setdefault((a, b), []).append(cams[b].compose(cams[a].inverse()))

    # maximum spanning tree over the "number of shared captures" weights
    resolved = {reference: Pose()}
    while len(resolved) < n_cams:
        best = None
        for a in list(resolved):
            for b in range(n_cams):
                if b in resolved:
                    continue
                samples = pair_poses.get((a, b))
                if not samples:
                    continue
                if best is None or len(samples) > best[0]:
                    best = (len(samples), a, b, samples)
        if best is None:
            missing = [i for i in range(n_cams) if i not in resolved]
            raise CalibrationError(
                "caméras sans vue commune avec le reste du dispositif : "
                + ", ".join(str(i) for i in missing)
                + " — capturez des poses de mire visibles par deux caméras à la fois"
            )
        count, a, b, samples = best
        rel = _median_pose(samples)
        resolved[b] = rel.compose(resolved[a])
        if log:
            log(f"caméra {b} rattachée à la caméra {a} par {count} pose(s) commune(s)")

    cam_poses = [resolved[i] for i in range(n_cams)]

    board_poses: dict[int, Pose] = {}
    for pose_index, cams in seen.items():
        candidates = []
        for cam, board_in_cam in cams.items():
            # board -> reference = (reference -> cam)^-1 ∘ (board -> cam)
            candidates.append(cam_poses[cam].inverse().compose(board_in_cam))
        board_poses[pose_index] = _median_pose(candidates)
    return cam_poses, board_poses, per_view


# -- parameter packing ----------------------------------------------------


@dataclass
class _Layout:
    n: int = 0
    intr: list[dict[str, int]] = field(default_factory=list)
    cams: dict[int, int] = field(default_factory=dict)
    boards: dict[int, int] = field(default_factory=dict)


def _build_layout(
    intrinsics: list[Intrinsics], mask: ParameterMask, board_indices: list[int], reference: int, refine_intrinsics: bool
) -> _Layout:
    layout = _Layout()
    cursor = 0
    for intr in intrinsics:
        names = mask.free_names(intr.model) if refine_intrinsics else []
        layout.intr.append({name: cursor + i for i, name in enumerate(names)})
        cursor += len(names)
    for cam in range(len(intrinsics)):
        if cam == reference:
            continue
        layout.cams[cam] = cursor
        cursor += 6
    for index in board_indices:
        layout.boards[index] = cursor
        cursor += 6
    layout.n = cursor
    return layout


def _pack(
    layout: _Layout, intrinsics: list[Intrinsics], cam_poses: list[Pose], board_poses: dict[int, Pose]
) -> np.ndarray:
    x = np.zeros(layout.n, dtype=np.float64)
    for intr, slots in zip(intrinsics, layout.intr):
        values = intr.values()
        for name, i in slots.items():
            x[i] = values[name]
    for cam, start in layout.cams.items():
        x[start : start + 3] = cam_poses[cam].rvec
        x[start + 3 : start + 6] = cam_poses[cam].tvec
    for index, start in layout.boards.items():
        x[start : start + 3] = board_poses[index].rvec
        x[start + 3 : start + 6] = board_poses[index].tvec
    return x


def _unpack(
    x: np.ndarray, layout: _Layout, intrinsics: list[Intrinsics], cam_poses: list[Pose], board_poses: dict[int, Pose]
):
    out_intr = []
    for intr, slots in zip(intrinsics, layout.intr):
        clone = Intrinsics(
            model=intr.model,
            image_size=intr.image_size,
            f=intr.f,
            ar=intr.ar,
            cx=intr.cx,
            cy=intr.cy,
            alpha=intr.alpha,
            dist=intr.dist.copy(),
        )
        clone.set_values({name: x[i] for name, i in slots.items()})
        out_intr.append(clone)
    out_cams = []
    for cam, pose in enumerate(cam_poses):
        start = layout.cams.get(cam)
        out_cams.append(pose if start is None else Pose(x[start : start + 3], x[start + 3 : start + 6]))
    out_boards = {
        index: Pose(x[start : start + 3], x[start + 3 : start + 6])
        for index, start in layout.boards.items()
    }
    return out_intr, out_cams, out_boards


# -- optimisation ---------------------------------------------------------


class _Problem:
    """Flattened view of the observations, so a residual pass is pure numpy.

    Every point carries the index of its camera, of its observation and of its
    board pose; projecting the whole problem is then two ``einsum`` calls plus
    one vectorised lens model, instead of one OpenCV call per view.
    """

    def __init__(self, board_points: np.ndarray, observations: list[Observation]):
        self.observations = observations
        self.points = (
            np.vstack([board_points[o.ids] for o in observations])
            if observations
            else np.zeros((0, 3))
        )
        self.measured = (
            np.vstack([o.points for o in observations]) if observations else np.zeros((0, 2))
        )
        counts = [o.count for o in observations]
        self.obs_of_point = np.repeat(np.arange(len(observations)), counts) if counts else np.zeros(0, int)
        self.cam_of_obs = np.array([o.camera for o in observations], dtype=int)
        self.cam_of_point = self.cam_of_obs[self.obs_of_point] if counts else np.zeros(0, int)
        self.pose_keys = sorted({o.pose_index for o in observations})
        row_of_pose = {k: i for i, k in enumerate(self.pose_keys)}
        self.pose_of_obs = np.array([row_of_pose[o.pose_index] for o in observations], dtype=int)
        self.blocks = []
        start = 0
        for count in counts:
            self.blocks.append(slice(start, start + count))
            start += count

    def project(
        self, intrinsics: list[Intrinsics], cam_poses: list[Pose], board_poses: dict[int, Pose]
    ) -> np.ndarray:
        if not self.observations:
            return np.zeros((0, 2))
        Rc = np.stack([geometry.rodrigues(p.rvec) for p in cam_poses])
        tc = np.stack([np.asarray(p.tvec, dtype=np.float64) for p in cam_poses])
        Rb = np.stack([geometry.rodrigues(board_poses[k].rvec) for k in self.pose_keys])
        tb = np.stack([np.asarray(board_poses[k].tvec, dtype=np.float64) for k in self.pose_keys])
        # board -> reference -> camera, composed once per view
        Rc_obs, tc_obs = Rc[self.cam_of_obs], tc[self.cam_of_obs]
        R_obs = np.einsum("nij,njk->nik", Rc_obs, Rb[self.pose_of_obs])
        t_obs = np.einsum("nij,nj->ni", Rc_obs, tb[self.pose_of_obs]) + tc_obs

        core = np.array(
            [[i.f, i.ar, i.cx, i.cy, i.alpha] for i in intrinsics], dtype=np.float64
        )[self.cam_of_point]
        R = R_obs[self.obs_of_point]
        t = t_obs[self.obs_of_point]

        uv = np.empty((len(self.points), 2), dtype=np.float64)
        for model in {i.model for i in intrinsics}:
            cams = [c for c, i in enumerate(intrinsics) if i.model == model]
            selected = np.isin(self.cam_of_point, cams)
            if not np.any(selected):
                continue
            width = 4 if model == FISHEYE else 5
            table = np.zeros((len(intrinsics), width), dtype=np.float64)
            for c in cams:
                d = np.asarray(intrinsics[c].dist, dtype=np.float64).ravel()[:width]
                table[c, : len(d)] = d
            uv[selected] = geometry.project_batch(
                self.points[selected],
                R[selected],
                t[selected],
                core[selected],
                table[self.cam_of_point[selected]],
                model,
            )
        return uv

    def errors(
        self, intrinsics: list[Intrinsics], cam_poses: list[Pose], board_poses: dict[int, Pose]
    ) -> np.ndarray:
        return self.project(intrinsics, cam_poses, board_poses) - self.measured


def _residual_blocks(
    board_points: np.ndarray,
    observations: list[Observation],
    intrinsics: list[Intrinsics],
    cam_poses: list[Pose],
    board_poses: dict[int, Pose],
) -> np.ndarray:
    """Signed pixel errors, stacked as (total_points, 2)."""
    return _Problem(board_points, observations).errors(intrinsics, cam_poses, board_poses)


def _sparsity(layout: _Layout, observations: list[Observation]) -> lil_matrix:
    rows = 2 * sum(o.count for o in observations)
    S = lil_matrix((rows, layout.n), dtype=int)
    row = 0
    for obs in observations:
        block = 2 * obs.count
        columns = list(layout.intr[obs.camera].values())
        start = layout.cams.get(obs.camera)
        if start is not None:
            columns += list(range(start, start + 6))
        start = layout.boards[obs.pose_index]
        columns += list(range(start, start + 6))
        for c in columns:
            S[row : row + block, c] = 1
        row += block
    return S


def bundle_adjust(
    board: BoardSpec,
    intrinsics: list[Intrinsics],
    cam_poses: list[Pose],
    board_poses: dict[int, Pose],
    observations: list[Observation],
    mask: ParameterMask | None = None,
    options: BundleOptions | None = None,
    log=None,
) -> CalibrationResult:
    """Refine the whole rig and report per-view statistics and uncertainties."""
    mask = mask or ParameterMask()
    options = options or BundleOptions()
    observations = [o for o in observations if o.pose_index in board_poses and o.count >= 4]
    if not observations:
        raise CalibrationError("aucune observation exploitable pour l'optimisation")

    board_points = board.object_points()
    trace: list[float] = []

    def run(obs_set: list[Observation], poses: dict[int, Pose]):
        indices = sorted({o.pose_index for o in obs_set})
        lay = _build_layout(
            intrinsics, mask, indices, options.reference_camera, options.refine_intrinsics
        )
        problem = _Problem(board_points, obs_set)

        def fun(x: np.ndarray) -> np.ndarray:
            intr, cams, boards = _unpack(x, lay, intrinsics, cam_poses, poses)
            errors = problem.errors(intr, cams, boards)
            trace.append(float(np.sqrt(np.mean(np.sum(errors**2, axis=1)))))
            return errors.ravel()

        solution = least_squares(
            fun,
            _pack(lay, intrinsics, cam_poses, poses),
            jac_sparsity=_sparsity(lay, obs_set),
            method="trf",
            loss="huber" if options.robust else "linear",
            f_scale=options.huber_delta,
            max_nfev=max(10, options.max_iterations),
            xtol=options.tolerance,
            ftol=options.tolerance,
            gtol=options.tolerance,
            x_scale="jac",
            verbose=0,
        )
        return solution, lay, problem

    result, layout, problem = run(observations, board_poses)

    if options.reject_sigma > 0:
        kept, dropped = _reject_outliers(
            board_points, observations, result.x, layout, intrinsics, cam_poses, board_poses, options.reject_sigma
        )
        if dropped and kept:
            if log:
                log(f"{dropped} point(s) aberrant(s) écarté(s) au-delà de {options.reject_sigma:.1f} σ")
            observations = kept
            indices = sorted({o.pose_index for o in observations})
            board_poses = {k: v for k, v in board_poses.items() if k in indices}
            trace.clear()
            result, layout, problem = run(observations, board_poses)

    out_intr, out_cams, out_boards = _unpack(result.x, layout, intrinsics, cam_poses, board_poses)
    errors = problem.errors(out_intr, out_cams, out_boards)
    norms = np.linalg.norm(errors, axis=1)

    residuals: list[ViewResidual] = []
    error_camera = np.zeros(len(norms), dtype=int)
    for obs, block in zip(observations, problem.blocks):
        residuals.append(
            ViewResidual(
                camera=obs.camera,
                pose_index=obs.pose_index,
                n_points=obs.count,
                rms=geometry.rms(norms[block]),
                max_error=float(np.max(norms[block])) if obs.count else float("nan"),
            )
        )
        error_camera[block] = obs.camera

    sigmas = (
        _standard_deviations(result, layout, len(norms) * 2)
        if options.estimate_sigmas
        else [{} for _ in intrinsics]
    )

    cameras = []
    for i, intr in enumerate(out_intr):
        mask_i = error_camera == i
        cameras.append(
            CameraCalibration(
                name=f"caméra {i}",
                intrinsics=intr,
                pose=out_cams[i],
                sigmas=sigmas[i] if i < len(sigmas) else {},
                rpe=geometry.rms(norms[mask_i]) if np.any(mask_i) else float("nan"),
                n_views=sum(1 for o in observations if o.camera == i),
                n_points=int(np.count_nonzero(mask_i)),
            )
        )

    running_min = np.minimum.accumulate(np.asarray(trace, dtype=np.float64)) if trace else np.zeros(0)
    # scipy reports success only when one of its tolerances fires; a finite
    # difference Jacobian rarely gets the gradient test to trigger, so a flat
    # cost tail counts as converged too — and the message says which happened.
    tail = running_min[-20:]
    plateau = bool(len(tail) >= 5 and (tail.max() - tail.min()) <= 1e-6 * max(tail.max(), 1e-9))
    converged = bool(result.success) or plateau
    message = str(result.message)
    if not result.success and plateau:
        message = "Coût stationnaire : convergence atteinte (plateau du RPE)."
    return CalibrationResult(
        cameras=cameras,
        board_poses=out_boards,
        residuals=residuals,
        errors_xy=errors,
        errors_camera=error_camera,
        rpe=geometry.rms(norms),
        converged=converged,
        iterations=int(result.nfev),
        convergence=[float(v) for v in running_min],
        message=message,
        n_observations=int(len(norms)),
        n_parameters=int(layout.n),
    )


def _reject_outliers(
    board_points, observations, x, layout, intrinsics, cam_poses, board_poses, k_sigma: float
):
    intr, cams, boards = _unpack(x, layout, intrinsics, cam_poses, board_poses)
    errors = _residual_blocks(board_points, observations, intr, cams, boards)
    norms = np.linalg.norm(errors, axis=1)
    if len(norms) == 0:
        return observations, 0
    threshold = float(np.median(norms) + k_sigma * (np.std(norms) or 1e-9))
    kept, dropped, row = [], 0, 0
    for obs in observations:
        block = slice(row, row + obs.count)
        good = norms[block] <= threshold
        row += obs.count
        if np.count_nonzero(good) < 4:
            dropped += obs.count
            continue
        dropped += int(np.count_nonzero(~good))
        kept.append(
            Observation(
                camera=obs.camera,
                pose_index=obs.pose_index,
                ids=obs.ids[good],
                points=obs.points[good],
            )
        )
    return kept, dropped


def _standard_deviations(result, layout: _Layout, n_residuals: int) -> list[dict[str, float]]:
    """Parameter sigmas from the Jacobian at the solution.

    ``cov = s² (JᵀJ)⁻¹`` with ``s²`` the residual variance — the usual
    first-order approximation, valid near a well-conditioned minimum.
    """
    sigmas: list[dict[str, float]] = [{} for _ in layout.intr]
    dof = max(1, n_residuals - layout.n)
    try:
        J = result.jac
        JTJ = (J.T @ J).toarray() if hasattr(J, "toarray") else np.asarray(J).T @ np.asarray(J)
        variance = 2.0 * float(result.cost) / dof
        cov = np.linalg.pinv(JTJ) * variance
        diag = np.clip(np.diag(cov), 0.0, None)
    except (np.linalg.LinAlgError, ValueError, AttributeError):
        return sigmas
    for cam, slots in enumerate(layout.intr):
        for name, i in slots.items():
            sigmas[cam][name] = float(np.sqrt(diag[i]))
    return sigmas
