"""Measure whether the XL330 firmware current limit (1.75 A) that the training
BAM actuator applies would bind in the app-equivalent CPU rehearsal, and whether
turning it on changes the closed-loop trajectory.

Run with work/.venv/Scripts/python.exe.
"""
import os
import sys

import numpy as np

sys.path.insert(0, r"D:\AI\Duck\work")
import run_trace as rt  # noqa: E402

import mujoco  # noqa: E402

ip = rt.load_infer_policy()

ROBOT_DIR = r"D:\AI\Duck\upstream\microduck_rl\src\mjlab_microduck\robot\microduck"
XML = os.path.join(ROBOT_DIR, "scene_backlash.xml")
ONNX = r"D:\AI\Duck\policies\velstand.onnx"

KT = 0.36601349688984386
R = 2.8113923539223227
IMAX = 1.75
KP = 200.0
EG = ip.BAM_KP_FW  # kp is 200; error_gain comes from the loaded actuator


def init_stand(model, data, ctrl, policy, trunk_z=0.125, joint_offset=0.0):
    free = mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
    qa = int(model.jnt_qposadr[free])
    va = int(model.jnt_dofadr[free])
    data.qpos[qa + 0] = 0.0
    data.qpos[qa + 1] = 0.0
    data.qpos[qa + 2] = trunk_z
    data.qpos[qa + 3 : qa + 7] = [1.0, 0.0, 0.0, 0.0]
    data.qvel[va : va + 6] = 0.0
    for i, qi in enumerate(policy.joint_qpos_indices):
        data.qpos[qi] = policy.default_pose[i] + joint_offset
        data.qvel[policy.joint_qvel_indices[i]] = 0.0
    ctrl.reset(data.qpos)
    policy.set_position_targets(data.qpos[policy.joint_qpos_indices].copy())
    mujoco.mj_forward(model, data)


def rollout(max_current, seconds, vx, trunk_z=0.125, joint_offset=0.0, probe=False):
    bam_model = ip.load_bam_model(KP, 7.4, max_current)
    model, data, ctrl, names = ip.load_mujoco_with_bam(
        XML, bam_model, 0.005, 0.1, 6.0
    )
    policy = ip.PolicyInference(
        model,
        data,
        bam_ctrl=ctrl,
        walking_onnx_path=ONNX,
        action_scale=1.0,
        new_cmd_obs=True,
        use_projected_gravity=True,
    )
    init_stand(model, data, ctrl, policy, trunk_z, joint_offset)
    eg = bam_model.actuator.error_gain
    decimation = 4
    dt = 0.02
    n_ticks = int(round(seconds / dt))
    traj = []
    stats = {
        "substeps": 0,
        "bind_substeps": 0,
        "max_abs_current": 0.0,
        "max_extra_torque": 0.0,
        "max_duty": 0.0,
        "per_joint_bind": np.zeros(model.nu, dtype=int),
        "per_joint_max_cur": np.zeros(model.nu),
        "per_joint_max_extra_tau": np.zeros(model.nu),
    }
    for tick in range(n_ticks):
        t = tick * dt
        policy.set_vel_cmd(vx if t >= 2.0 else 0.0, 0.0, 0.0)
        action = policy.infer()
        policy.apply_action(action)
        for _ in range(decimation):
            if probe:
                q = data.qpos[ctrl.qpos_indexes]
                dq = data.qvel[ctrl.dof_indexes]
                vin_eff = max(
                    7.4 - 0.1 * np.abs(ctrl._prev_motor_torque).sum(), 6.0
                )
                duty = (ctrl.q_target - q) * KP * eg
                center = KT * dq / vin_eff
                span = R * IMAX / vin_eff
                limited = np.clip(duty, center - span, center + span)
                delta = limited - duty
                cur = (duty * vin_eff - KT * dq) / R
                bind = np.abs(delta) > 1e-12
                stats["substeps"] += 1
                stats["bind_substeps"] += int(bind.any())
                stats["max_abs_current"] = max(stats["max_abs_current"], float(np.abs(cur).max()))
                stats["max_duty"] = max(stats["max_duty"], float(np.abs(duty).max()))
                extra_tau = KT * vin_eff / R * np.abs(delta)
                stats["max_extra_torque"] = max(stats["max_extra_torque"], float(extra_tau.max()))
                stats["per_joint_bind"] += bind.astype(int)
                stats["per_joint_max_cur"] = np.maximum(stats["per_joint_max_cur"], np.abs(cur))
                stats["per_joint_max_extra_tau"] = np.maximum(
                    stats["per_joint_max_extra_tau"], extra_tau
                )
            ctrl.update()
            mujoco.mj_step(model, data)
        traj.append(
            np.concatenate(
                [
                    data.qpos[policy.joint_qpos_indices].copy(),
                    data.qpos[policy.trunk_qpos_index : policy.trunk_qpos_index + 3].copy()
                    if hasattr(policy, "trunk_qpos_index")
                    else np.array([]),
                ]
            )
        )
    # Always record the final free-joint pose for the comparison.
    free = mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
    qa = int(model.jnt_qposadr[free])
    final_trunk = data.qpos[qa : qa + 3].copy()
    return np.array(traj), stats, final_trunk, model, data, ctrl


def run_case(label, seconds=8.0, vx=0.3, trunk_z=0.125, joint_offset=0.0):
    print(f"\n=== {label} (vx={vx}, trunk_z={trunk_z}, joint_offset={joint_offset}) ===")
    traj_app, stats, trunk_app, *_ = rollout(
        None, seconds, vx, trunk_z, joint_offset, probe=True
    )
    traj_train, _, trunk_train, *_ = rollout(
        IMAX, seconds, vx, trunk_z, joint_offset, probe=False
    )
    frac = stats["bind_substeps"] / max(stats["substeps"], 1)
    print(
        f"probe: substeps={stats['substeps']} bind_any={stats['bind_substeps']} "
        f"({100*frac:.2f}% of substeps have >=1 joint clamped)"
    )
    print(
        f"max |duty|={stats['max_duty']:.3f}  max |current|={stats['max_abs_current']:.3f} A "
        f"(limit {IMAX})  max extra torque={stats['max_extra_torque']:.4f} Nm"
    )
    n = min(len(traj_app), len(traj_train))
    diff = traj_app[:n] - traj_train[:n]
    joints_only = diff[:, :14]
    print(
        f"closed-loop qpos diff over {n} ticks (end of 8s): "
        f"max|dq|={np.abs(joints_only).max():.5f} rad  "
        f"rmse={np.sqrt((joints_only**2).mean()):.5f} rad"
    )
    print(f"final trunk app={trunk_app} train={trunk_train} "
          f"dz={trunk_app[2]-trunk_train[2]:+.5f} m")
    return traj_app, traj_train, stats


if __name__ == "__main__":
    import sys as _sys

    which = _sys.argv[1] if len(_sys.argv) > 1 else "all"
    if which in ("all", "nominal"):
        run_case("stand then walk 0.3 m/s")
    if which in ("all", "recover"):
        run_case("recovery from 0.4 rad joint offset", vx=0.3, trunk_z=0.10, joint_offset=0.4)
    if which in ("all", "large"):
        run_case(
            "large transient: 1.5 rad joint offset (stand-up scale)",
            seconds=4.0,
            vx=0.0,
            trunk_z=0.09,
            joint_offset=1.5,
        )
