import mujoco
xml = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/robot_groundcontact_backlash.xml"
m = mujoco.MjModel.from_xml_path(xml); d = mujoco.MjData(m)
jid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "left_hip_yaw"); did = m.jnt_dofadr[jid]
print("initial: jnt_solref", m.jnt_solref[jid], " dof_solref", m.dof_solref[did])
# emulate duck_mj_zero_joint_friction exactly: only jnt_solref/jnt_solimp
m.jnt_solref[jid] = [-5.0e4, -2.0e2]
m.jnt_solimp[jid] = [0.99,0.9999,0.001,0.5,2.0]
print("after setting jnt only: jnt", m.jnt_solref[jid], " dof", m.dof_solref[did])
mujoco.mj_setConst(m, d)
print("after mj_setConst:      jnt", m.jnt_solref[jid], " dof", m.dof_solref[did])
