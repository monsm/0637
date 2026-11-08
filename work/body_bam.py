#!/usr/bin/env python3
"""BAM-actuated MuJoCo body server for duck-control (protocol 1).

Mirrors the official `duck-body` wire protocol
(`duck_control::sim::RemoteIo`): newline-delimited JSON over TCP, one request
and one answer per line.

Differences from `microduck_rl`'s `body_server.py`:
  - actuators are the official BAM M6 XL330 model (voltage control, friction),
    the same stack `infer_policy.py` uses and the same one a real servo runs;
  - the scene defaults to `scene_backlash.xml` (velstand's training body);
  - trunk pose is logged every second, so a headless run can be scored.

The physics loop runs at 50 Hz (4 x 0.005 s per tick) in its own thread. A
duck is held at its placement until `torque: true`, exactly like the official
server, so the daemon's bring-up sees a stable robot.
"""
from __future__ import annotations

import argparse
import json
import socket
import socketserver
import sys
import threading
import time
from pathlib import Path

import numpy as np

WORK = Path(__file__).resolve().parent
sys.path.insert(0, str(WORK))
import run_trace as rt  # noqa: E402  (stubs termios/tty on Windows)

import mujoco  # noqa: E402

ip = rt.load_infer_policy()

PROTOCOL = 1
TIMESTEP = 0.005
DECIMATION = 4
PERIOD = TIMESTEP * DECIMATION

JOINT_NAMES = (
    "left_hip_yaw", "left_hip_roll", "left_hip_pitch", "left_knee", "left_ankle",
    "neck_pitch", "head_pitch", "head_yaw", "head_roll", "mouth",
    "right_hip_yaw", "right_hip_roll", "right_hip_pitch", "right_knee", "right_ankle",
)
MOUTH_INDEX = JOINT_NAMES.index("mouth")

# 14 policy joints + the mouth slot, in wire order.
HOME_POSE = tuple(ip.DEFAULT_POSE[:9].tolist()) + (0.0,) + tuple(ip.DEFAULT_POSE[9:].tolist())

DEFAULT_XML = (
    WORK.parent
    / "upstream"
    / "microduck_rl"
    / "src"
    / "mjlab_microduck"
    / "robot"
    / "microduck"
    / "scene_backlash.xml"
)


def gravity_in_trunk(quat: np.ndarray) -> np.ndarray:
    rotation = np.zeros(9)
    mujoco.mju_quat2Mat(rotation, quat)
    return -rotation.reshape(3, 3).T[:, 2]


class Body:
    """One duck: official MJCF + BAM, exposed as protocol-1 sensors and targets."""

    def __init__(self, xml: Path, kp: float = 200.0, vin: float = 7.4,
                 vin_drop_gain: float = 0.1, verbose: bool = False):
        self.verbose = verbose
        self.lock = threading.Lock()

        bam_model = ip.load_bam_model(kp, vin, None)
        self.bam_model = bam_model
        self.model, self.data, self.bam_ctrl, actuator_names = ip.load_mujoco_with_bam(
            str(xml), bam_model, TIMESTEP,
            vin_drop_gain if vin_drop_gain > 0 else None,
            ip.BAM_VIN_MIN,
        )
        print(f"BAM body: {xml.name}, nq={self.model.nq}, nu={self.model.nu}, "
              f"actuators={len(actuator_names)}", flush=True)

        # Actuator by name, never by index: an MJCF edit reorders silently.
        self.actuators: list[int] = []
        self.to_wire: list[int] = []
        for wire_index, name in enumerate(JOINT_NAMES):
            found = mujoco.mj_name2id(self.model, mujoco.mjtObj.mjOBJ_ACTUATOR, name)
            if found >= 0:
                self.actuators.append(found)
                self.to_wire.append(wire_index)
        if not self.actuators:
            raise SystemExit("no actuated joints in the model")

        self.qpos_adr = np.array(
            [self.model.jnt_qposadr[self.model.actuator_trnid[a, 0]] for a in self.actuators]
        )
        self.qvel_adr = np.array(
            [self.model.jnt_dofadr[self.model.actuator_trnid[a, 0]] for a in self.actuators]
        )
        self.actuator_slice = np.array(self.actuators)

        self.trunk = int(self.model.jnt_qposadr[
            mujoco.mj_name2id(self.model, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
        ])
        self.trunk_dof = int(self.model.jnt_dofadr[
            mujoco.mj_name2id(self.model, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
        ])

        self.kp = float(kp)
        self.torque_on = False
        self.released = False
        self.held: tuple[np.ndarray, np.ndarray] | None = None

        self.place()
        self._apply_gain()

    # ── placement / hold ─────────────────────────────────────────────────
    def place(self) -> None:
        data = self.data
        data.qpos[self.trunk + 0] = 0.0
        data.qpos[self.trunk + 1] = 0.0
        data.qpos[self.trunk + 2] = 0.125
        data.qpos[self.trunk + 3:self.trunk + 7] = [1.0, 0.0, 0.0, 0.0]
        data.qvel[self.trunk_dof:self.trunk_dof + 6] = 0.0
        for slot, wire_index in enumerate(self.to_wire):
            data.qpos[self.qpos_adr[slot]] = HOME_POSE[wire_index]
            data.qvel[self.qvel_adr[slot]] = 0.0
        self.bam_ctrl.reset(data.qpos)
        mujoco.mj_forward(self.model, self.data)
        self.remember()
        print(f"placed at HOME: trunk_z={data.qpos[self.trunk + 2]:.3f}", flush=True)

    def remember(self) -> None:
        self.held = (
            self.data.qpos[self.trunk:self.trunk + 7].copy(),
            self.data.qpos[self.qpos_adr].copy(),
        )

    def restore(self) -> None:
        if self.held is None:
            return
        trunk, joints = self.held
        self.data.qpos[self.trunk:self.trunk + 7] = trunk
        self.data.qpos[self.qpos_adr] = joints
        self.data.qvel[self.trunk_dof:self.trunk_dof + 6] = 0.0
        self.data.qvel[self.qvel_adr] = 0.0

    # ── actuation ────────────────────────────────────────────────────────
    def _apply_gain(self) -> None:
        # BAM's firmware P gain; 200 is the value the XL330 model was fitted at.
        self.bam_model.actuator.kp = self.kp if self.torque_on else 0.0

    def set_gain(self, kp: int) -> None:
        with self.lock:
            self.kp = float(kp)
            self._apply_gain()

    def set_torque(self, on: bool) -> None:
        with self.lock:
            self.torque_on = bool(on)
            if on:
                self.released = True
                # Torque arriving must not fling the robot at a stale target.
                self.bam_ctrl.reset(self.data.qpos)
            self._apply_gain()
            if not on:
                self.data.ctrl[:] = 0.0

    def set_targets(self, targets: list[float]) -> None:
        if len(targets) != len(JOINT_NAMES):
            raise ValueError(f"expected {len(JOINT_NAMES)} targets, got {len(targets)}")
        with self.lock:
            for slot, wire_index in enumerate(self.to_wire):
                self.bam_ctrl.q_target[slot] = float(targets[wire_index])

    # ── sensors ──────────────────────────────────────────────────────────
    def sensors(self) -> dict:
        with self.lock:
            sim_time = float(self.data.time)
            positions = self.data.qpos[self.qpos_adr].copy()
            velocities = self.data.qvel[self.qvel_adr].copy()
            force = self.data.actuator_force[self.actuator_slice].copy()
            quat = self.data.qpos[self.trunk + 3:self.trunk + 7].copy()
            gyro = self.data.qvel[self.trunk_dof + 3:self.trunk_dof + 6].copy()
            trunk = [float(v) for v in self.data.qpos[self.trunk:self.trunk + 3]]
            trunk_z = trunk[2]

        wire_pos = [0.0] * len(JOINT_NAMES)
        wire_vel = [0.0] * len(JOINT_NAMES)
        wire_cur = [0.0] * len(JOINT_NAMES)
        for slot, wire_index in enumerate(self.to_wire):
            wire_pos[wire_index] = float(positions[slot])
            wire_vel[wire_index] = float(velocities[slot])
            wire_cur[wire_index] = abs(float(force[slot])) * 100.0

        return {
            "positions": wire_pos,
            "velocities": wire_vel,
            "currents_ma": wire_cur,
            "trunk_z": trunk_z,
            "trunk": trunk,
            "sim_time": sim_time,
            "imu": {
                "gyro": [float(v) for v in gyro],
                "gravity": [float(v) for v in gravity_in_trunk(quat)],
                "quat": [float(v) for v in quat],
            },
        }

    def slow_sensors(self) -> dict:
        return {"volts": 7.4, "temps_c": [32.0] * len(JOINT_NAMES)}

    # ── physics ──────────────────────────────────────────────────────────
    def step(self, times: int = DECIMATION) -> None:
        with self.lock:
            for _ in range(times):
                if self.torque_on:
                    self.bam_ctrl.update()
                else:
                    self.data.ctrl[:] = 0.0
                mujoco.mj_step(self.model, self.data)
                if not self.released:
                    self.restore()


class Handler(socketserver.StreamRequestHandler):
    def handle(self) -> None:
        self.connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        body: Body = self.server.body
        print(f"== daemon connected from {self.client_address}", flush=True)
        for raw in self.rfile:
            try:
                request = json.loads(raw)
                answer = self.dispatch(body, request)
            except Exception as error:  # a bad frame must not take the simulator down
                answer = {"error": str(error)}
            self.wfile.write((json.dumps(answer) + "\n").encode())
            self.wfile.flush()
        print("== daemon disconnected", flush=True)

    def dispatch(self, body: Body, request: dict) -> dict:
        op = request.get("op")
        if op == "hello":
            if request.get("protocol") != PROTOCOL:
                raise ValueError(
                    f"daemon speaks {request.get('protocol')}, simulator speaks {PROTOCOL}"
                )
            return {"protocol": PROTOCOL}
        if op == "read":
            return body.sensors()
        if op == "write":
            body.set_targets(request["targets"])
            return {}
        if op == "gain":
            body.set_gain(int(request["kp"]))
            return {}
        if op == "torque":
            body.set_torque(bool(request["on"]))
            return {}
        if op == "slow":
            return body.slow_sensors()
        if op == "tof":
            return {"rows": 8, "cols": 8, "distance_mm": [0] * 64, "status": [0] * 64}
        raise ValueError(f"unknown op {op!r}")


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--xml", type=Path, default=DEFAULT_XML)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=7801)
    parser.add_argument("--kp", type=float, default=200.0)
    parser.add_argument("--vin", type=float, default=7.4)
    parser.add_argument("--vin-drop-gain", type=float, default=0.1)
    parser.add_argument("--log-every", type=float, default=1.0)
    args = parser.parse_args()

    body = Body(args.xml, args.kp, args.vin, args.vin_drop_gain)
    server = Server((args.host, args.port), Handler)
    server.body = body
    threading.Thread(target=server.serve_forever, daemon=True).start()
    print(f"== listening on {args.host}:{args.port} (protocol {PROTOCOL})", flush=True)

    next_step = time.perf_counter()
    last_log = time.perf_counter()
    try:
        while True:
            body.step()
            next_step += PERIOD
            slack = next_step - time.perf_counter()
            if slack > 0.002:
                time.sleep(slack)
            elif slack < -0.25:
                next_step = time.perf_counter()
            now = time.perf_counter()
            if now - last_log >= args.log_every:
                with body.lock:
                    x, y, z = body.data.qpos[body.trunk:body.trunk + 3]
                    quat = body.data.qpos[body.trunk + 3:body.trunk + 7].copy()
                g = gravity_in_trunk(quat)
                print(f"t={body.data.time:7.2f}s trunk=({x:+.3f},{y:+.3f},{z:.3f}) "
                      f"gravity=({g[0]:+.2f},{g[1]:+.2f},{g[2]:+.2f}) "
                      f"torque={'on' if body.torque_on else 'off'}", flush=True)
                last_log = now
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
