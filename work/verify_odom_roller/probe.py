import sys, numpy as np, mujoco
scene = sys.argv[1]
m = mujoco.MjModel.from_xml_path(scene)
d = mujoco.MjData(m)
print("nq", m.nq, "njnt", m.njnt, "nbody", m.nbody, "nsite", m.nsite)
print("keyframes", [mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_KEY, i) for i in range(m.nkey)])
# list joint names
for i in range(m.njnt):
    print("jnt", i, mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_JOINT, i), m.jnt_type[i], m.jnt_qposadr[i])
