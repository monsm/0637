import sys
import mujoco
import numpy as np

m = mujoco.MjModel.from_xml_path(sys.argv[1])
d = mujoco.MjData(m)
js = int(m.actuator_trnid[0, 0])
jp = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "passive_left_hip_yaw_backlash")
bid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, "yaw2roll")
def quat(a, b):
    d.qpos[:] = 0
    d.qpos[int(m.jnt_qposadr[js])] = a
    d.qpos[int(m.jnt_qposadr[jp])] = b
    mujoco.mj_forward(m, d)
    return d.xquat[bid].copy()
qa = quat(0.10, 0.01)
qb = quat(0.11, 0.00)
print("q(servo=0.10, play=0.01):", qa.tolist())
print("q(servo=0.11, play=0.00):", qb.tolist())
print("max abs diff:", float(np.max(np.abs(qa - qb))))
# rotation angle difference between servo-only and link poses
q0 = quat(0.10, 0.0)
dot = abs(float(np.dot(qa, q0)))
print("angle between link and servo-only body orientation (rad):", 2*np.arccos(min(1.0, dot)))
