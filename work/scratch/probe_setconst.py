import mujoco
xml = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/robot_groundcontact_backlash.xml"
spec = mujoco.MjSpec.from_file(xml)
m = spec.compile()
d = mujoco.MjData(m)
jid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "left_hip_yaw")
did = m.jnt_dofadr[jid]
print("before setConst: jnt_solref", m.jnt_solref[jid], "dof_solref", m.dof_solref[did])
m.jnt_solref[jid] = [-5.0e4, -2.0e2]
m.dof_solref[did] = [-5.0e4, -2.0e2]
mujoco.mj_setConst(m, d)
print("after  setConst: jnt_solref", m.jnt_solref[jid], "dof_solref", m.dof_solref[did])
# HOME_FRAME z
