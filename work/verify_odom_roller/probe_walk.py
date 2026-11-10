import sys, numpy as np, mujoco
path = sys.argv[1]
m = mujoco.MjModel.from_xml_path(path)
print("nq", m.nq, "njnt", m.njnt)
for i in range(m.njnt):
    print("jnt", i, mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_JOINT, i), "qposadr", m.jnt_qposadr[i], "axis", m.jnt_axis[i])
for i in range(m.nsite):
    print("site", i, mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_SITE, i))
for i in range(m.nbody):
    print("body", i, mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_BODY, i), "parent", m.body_parentid[i])
