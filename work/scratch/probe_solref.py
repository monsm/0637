import sys, numpy as np, mujoco

xml = sys.argv[1]
spec = mujoco.MjSpec.from_file(xml)
m = spec.compile()
d = mujoco.MjData(m)
# print defaults for a servo joint (left_hip_yaw)
jid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "left_hip_yaw")
did = m.jnt_dofadr[jid]
print("joint id", jid, "dof", did)
print("jnt_solref (limits):", m.jnt_solref[jid])
print("dof_solref (friction):", m.dof_solref[did])
print("dof_frictionloss:", m.dof_frictionloss[did], "dof_damping:", m.dof_damping[did], "dof_armature:", m.dof_armature[did])
# Now set a frictionloss so a FRICTION_DOF constraint exists, step, inspect efc_id semantics
for j in range(m.njnt):
    if not m.jnt_limited[j] and m.jnt_type[j] != mujoco.mjtJoint.mjJNT_FREE:
        pass
m.dof_frictionloss[did] = 0.5
m.dof_frictionloss[m.jnt_dofadr[mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT,"left_hip_roll")]] = 0.5
mujoco.mj_forward(m, d)
mujoco.mj_step(m, d)
ids = []
for i in range(d.nefc):
    if d.efc_type[i] == mujoco.mjtConstraint.mjCNSTR_FRICTION_DOF:
        ids.append((i, int(d.efc_id[i]), float(d.efc_force[i])))
print("FRICTION_DOF constraints (index, efc_id, force):", ids)
print("actuated dofs:", [m.jnt_dofadr[mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT,n)] for n in ["left_hip_yaw","left_hip_roll"]])
print("joint ids:", [mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT,n) for n in ["left_hip_yaw","left_hip_roll"]])
