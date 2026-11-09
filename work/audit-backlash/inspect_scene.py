import sys
import mujoco

m = mujoco.MjModel.from_xml_path(sys.argv[1])
print("nq,nv,nu,nbody,njnt:", m.nq, m.nv, m.nu, m.nbody, m.njnt)
for a in range(m.nu):
    j = int(m.actuator_trnid[a, 0])
    name = mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_JOINT, j)
    pj = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, f"passive_{name}_backlash")
    print(
        a, name,
        "servo_qpos", int(m.jnt_qposadr[j]), "servo_dof", int(m.jnt_dofadr[j]),
        "| passive", pj,
        "qpos", int(m.jnt_qposadr[pj]) if pj >= 0 else None,
        "dof", int(m.jnt_dofadr[pj]) if pj >= 0 else None,
        "range", m.jnt_range[pj].tolist() if pj >= 0 else None,
    )
# Kinematics check: both hinges rotate the same body in series.
import numpy as np
d = mujoco.MjData(m)
d.qpos[:] = 0
d.qpos[m.jnt_qposadr[int(m.actuator_trnid[0, 0])]] = 0.10
d.qpos[m.jnt_qposadr[mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "passive_left_hip_yaw_backlash")]] = 0.01
mujoco.mj_forward(m, d)
bid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, "yaw2roll")
quat = d.xquat[bid]
print("yaw2roll quat at servo=0.10, play=0.01:", quat.tolist())
