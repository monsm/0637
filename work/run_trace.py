#!/usr/bin/env python3
"""Headless MicroDuck policy trace runner.

Runs the *official* inference stack from `microduck_rl/scripts/infer_policy.py`
on Windows (no viewer), using:

  - official MJCF scene (default: scene_backlash.xml, the velstand training body)
  - official ONNX policy (default: velstand.onnx, the shipped walk policy)
  - official BAM M6 XL330 actuator model

It writes a per-tick CSV trace and prints phase metrics. This trace is the M1
reference the digital-duck runtime must reproduce.

Usage:
  python run_trace.py --out traces/velstand_baseline.csv
"""

import argparse
import csv
import importlib.util
import math
import os
import sys
import types

import numpy as np

WORK_DIR = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(WORK_DIR, "..", "upstream", "microduck_rl"))
ROBOT_DIR = os.path.join(REPO, "src", "mjlab_microduck", "robot", "microduck")

# Fixed command schedule: (seconds, (vx, vy, wz)). Matches the ranges the
# official docs use for these policies.
DEFAULT_SCHEDULE = [
    (2.0, (0.0, 0.0, 0.0)),    # stand
    (5.0, (0.15, 0.0, 0.0)),   # walk forward
    (2.0, (0.0, 0.0, 0.0)),    # stand
    (3.0, (0.0, 0.10, 0.0)),   # strafe left
    (2.0, (0.0, 0.0, 0.0)),    # stand
    (3.0, (0.0, 0.0, 0.5)),    # turn left
    (2.0, (0.0, 0.0, 0.0)),    # stand
    (5.0, (0.30, 0.0, 0.0)),   # walk fast
    (2.0, (0.0, 0.0, 0.0)),    # stand
]


def stub_unix_modules():
    """infer_policy.py imports termios/tty at module level (Linux/macOS only)."""
    if sys.platform != "win32":
        return
    termios = types.ModuleType("termios")
    termios.tcgetattr = lambda fd: None
    termios.tcsetattr = lambda *a, **k: None
    termios.TCSADRAIN = 0
    sys.modules.setdefault("termios", termios)
    tty = types.ModuleType("tty")
    tty.setcbreak = lambda fd: None
    sys.modules.setdefault("tty", tty)


def load_infer_policy():
    stub_unix_modules()
    path = os.path.join(REPO, "scripts", "infer_policy.py")
    spec = importlib.util.spec_from_file_location("microduck_infer_policy", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--onnx", default=os.path.abspath(os.path.join(WORK_DIR, "..", "policies", "velstand.onnx")))
    ap.add_argument("--xml", default=os.path.join(ROBOT_DIR, "scene_backlash.xml"))
    ap.add_argument("--out", default=os.path.abspath(os.path.join(WORK_DIR, "traces", "velstand_baseline.csv")))
    ap.add_argument("--vin", type=float, default=7.4)
    ap.add_argument("--vin-drop-gain", type=float, default=0.1)
    ap.add_argument("--kp-fw", type=float, default=200.0)
    ap.add_argument("--duration", type=float, default=None,
                    help="Override total duration (default: sum of schedule)")
    ap.add_argument("--no-bam", action="store_true",
                    help="Use XML position actuators instead of the BAM M6 model")
    ap.add_argument("--start-key", default=None,
                    help="Start from a keyframe (SIT/FOLD/STAND) instead of the hand-set stand pose")
    ap.add_argument("--walk-vx", type=float, default=None,
                    help="Hold this vx after a 2 s stand for the whole run")
    ap.add_argument("--walk-vy", type=float, default=0.0)
    ap.add_argument("--walk-wz", type=float, default=0.0)
    ap.add_argument("--print-every", type=int, default=100)
    args = ap.parse_args()

    import mujoco

    ip = load_infer_policy()

    if args.walk_vx is not None:
        total_s = args.duration if args.duration is not None else 12.0
        schedule = [
            (2.0, (0.0, 0.0, 0.0)),
            (max(total_s - 2.0, 0.1), (args.walk_vx, args.walk_vy, args.walk_wz)),
        ]
    else:
        schedule = DEFAULT_SCHEDULE
        total_s = sum(s for s, _ in schedule)
        if args.duration is not None:
            total_s = args.duration

    os.makedirs(os.path.dirname(args.out), exist_ok=True)

    # --- body + actuator model, exactly as the official rehearsal ---
    if args.no_bam:
        model = mujoco.MjModel.from_xml_path(args.xml)
        model.opt.timestep = 0.005
        data = mujoco.MjData(model)
        bam_ctrl = None
        actuator_names = [mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_ACTUATOR, i)
                          for i in range(model.nu)]
        print("legacy XML position actuators (--no-bam)")
    else:
        bam_model = ip.load_bam_model(args.kp_fw, args.vin, None)
        vin_drop_gain = args.vin_drop_gain if args.vin_drop_gain > 0 else None
        model, data, bam_ctrl, actuator_names = ip.load_mujoco_with_bam(
            args.xml, bam_model, 0.005, vin_drop_gain, ip.BAM_VIN_MIN)
    print(f"nq={model.nq} nv={model.nv} nu={model.nu} actuators={len(actuator_names)}")

    # --- policy (official class, velstand only: it holds the stand at zero cmd) ---
    policy = ip.PolicyInference(
        model, data,
        bam_ctrl=bam_ctrl,
        walking_onnx_path=args.onnx,
        action_scale=1.0,
        new_cmd_obs=True,
        use_projected_gravity=True,
    )

    # --- initial pose: official main() stand pose, or a scene keyframe ---
    freejoint_id = mujoco.mj_name2id(
        model, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
    qpos_adr = int(model.jnt_qposadr[freejoint_id])
    qvel_adr = int(model.jnt_dofadr[freejoint_id])
    if args.start_key:
        kid = mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_KEY, args.start_key)
        if kid < 0:
            raise SystemExit(f"no keyframe named {args.start_key!r}")
        mujoco.mj_resetDataKeyframe(model, data, kid)
        mujoco.mj_forward(model, data)
        if bam_ctrl is not None:
            bam_ctrl.reset(data.qpos)
        policy.set_position_targets(data.qpos[policy.joint_qpos_indices].copy())
        print(f"start keyframe: {args.start_key} (trunk z={data.qpos[qpos_adr + 2]:.3f})")
    else:
        data.qpos[qpos_adr + 0] = 0.0
        data.qpos[qpos_adr + 1] = 0.0
        data.qpos[qpos_adr + 2] = 0.125
        data.qpos[qpos_adr + 3:qpos_adr + 7] = [1, 0, 0, 0]
        for i, qi in enumerate(policy.joint_qpos_indices):
            data.qpos[qi] = policy.default_pose[i]
        if bam_ctrl is not None:
            bam_ctrl.reset(data.qpos)
        policy.set_position_targets(policy.default_pose)
        mujoco.mj_forward(model, data)

    obs = policy.get_observations()
    print(f"obs={obs.size} (expect 61), action_dim={policy.n_joints}")
    print(f"xml={args.xml}")
    print(f"onnx={args.onnx}")
    print(f"schedule={schedule}")

    decimation = 4
    dt = decimation * model.opt.timestep  # 0.02 s = 50 Hz
    n_ticks = int(round(total_s / dt))

    # Flatten schedule to per-tick commands.
    phase_of_tick = np.zeros(n_ticks, dtype=np.int32)
    cmds = np.zeros((n_ticks, 3), dtype=np.float64)
    phase = 0
    phase_end = schedule[0][0]
    tick = 0
    el = 0.0
    for tick in range(n_ticks):
        el = tick * dt
        while phase + 1 < len(schedule) and el >= phase_end:
            phase += 1
            phase_end += schedule[phase][0]
        phase_of_tick[tick] = phase
        cmds[tick] = schedule[phase][1]

    columns = [
        "tick", "t", "phase",
        "cmd_vx", "cmd_vy", "cmd_wz",
        "trunk_x", "trunk_y", "trunk_z",
        "quat_w", "quat_x", "quat_y", "quat_z",
        "vx_body", "vy_body", "yaw_rate",
        "up_z",
    ]
    columns += [f"action_{i}" for i in range(policy.n_joints)]
    columns += [f"qpos_{i}" for i in range(policy.n_joints)]
    columns += [f"qvel_{i}" for i in range(policy.n_joints)]

    rows = []
    last_cmd = None
    falls = 0
    min_up_z = 1.0
    start_xy = None

    for tick in range(n_ticks):
        t = tick * dt
        cmd = tuple(float(x) for x in cmds[tick])
        if cmd != last_cmd:
            policy.set_vel_cmd(*cmd)
            last_cmd = cmd

        action = policy.infer()
        policy.apply_action(action)
        for _ in range(decimation):
            if bam_ctrl is not None:
                bam_ctrl.update()
            mujoco.mj_step(model, data)

        quat = data.qpos[qpos_adr + 3:qpos_adr + 7].astype(np.float64)
        pos = data.qpos[qpos_adr:qpos_adr + 3].astype(np.float64)
        v_world = np.array([
            data.qvel[qvel_adr + 0],
            data.qvel[qvel_adr + 1],
            data.qvel[qvel_adr + 2],
        ], dtype=np.float64)
        v_body = policy.quat_rotate_inverse(quat.astype(np.float32), v_world.astype(np.float32))
        yaw_rate = float(data.qvel[qvel_adr + 5])
        # projected gravity in body frame; upright => z = -1
        up_z = float(policy.get_projected_gravity()[2])
        min_up_z = min(min_up_z, up_z)
        if up_z > -0.5:
            # count transitions only
            if tick > 0 and float(rows[-1]["up_z"]) <= -0.5:
                falls += 1
        if start_xy is None:
            start_xy = pos[:2].copy()

        row = {
            "tick": tick, "t": t, "phase": int(phase_of_tick[tick]),
            "cmd_vx": cmd[0], "cmd_vy": cmd[1], "cmd_wz": cmd[2],
            "trunk_x": pos[0], "trunk_y": pos[1], "trunk_z": pos[2],
            "quat_w": quat[0], "quat_x": quat[1], "quat_y": quat[2], "quat_z": quat[3],
            "vx_body": float(v_body[0]), "vy_body": float(v_body[1]),
            "yaw_rate": yaw_rate, "up_z": up_z,
        }
        for i in range(policy.n_joints):
            row[f"action_{i}"] = float(action[i])
            row[f"qpos_{i}"] = float(data.qpos[policy.joint_qpos_indices[i]])
            row[f"qvel_{i}"] = float(data.qvel[policy.joint_qvel_indices[i]])
        rows.append(row)

        if args.print_every and tick % args.print_every == 0:
            print(f"[{t:5.2f}s] z={pos[2]:.3f} up_z={up_z:+.2f} "
                  f"v_body=({v_body[0]:+.3f},{v_body[1]:+.3f}) cmd=({cmd[0]:+.2f},{cmd[1]:+.2f},{cmd[2]:+.2f})")

    with open(args.out, "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=columns)
        w.writeheader()
        w.writerows(rows)

    # --- summary ---
    xs = np.array([r["trunk_x"] for r in rows])
    ys = np.array([r["trunk_y"] for r in rows])
    zs = np.array([r["trunk_z"] for r in rows])
    dist = float(np.hypot(xs[-1] - xs[0], ys[-1] - ys[0]))
    print("\n=== summary ===")
    print(f"ticks={n_ticks} duration={total_s:.1f}s")
    print(f"trunk travel: {dist:.3f} m  ({dist/total_s:.3f} m/s avg over whole run)")
    print(f"trunk z: min={zs.min():.3f} max={zs.max():.3f} final={zs[-1]:.3f}")
    print(f"min projected-gravity z: {min_up_z:+.3f} (upright = -1.0)")
    print(f"fall transitions: {falls}")

    # per-phase achieved body-frame velocity over the last 60% of the phase
    print(f"{'phase':>5} {'cmd(vx,vy,wz)':>22} {'achieved(vx,vy,yaw)':>28}")
    for p, (dur, c) in enumerate(schedule):
        idx = np.where(phase_of_tick == p)[0]
        if len(idx) == 0:
            continue
        tail = idx[int(len(idx) * 0.4):]
        avx = float(np.mean([rows[i]["vx_body"] for i in tail]))
        avy = float(np.mean([rows[i]["vy_body"] for i in tail]))
        ayaw = float(np.mean([rows[i]["yaw_rate"] for i in tail]))
        print(f"{p:>5} {str(tuple(round(x,2) for x in c)):>22} "
              f"{f'({avx:+.3f},{avy:+.3f},{ayaw:+.3f})':>28}")

    print(f"\ntrace written: {args.out}")


if __name__ == "__main__":
    main()
