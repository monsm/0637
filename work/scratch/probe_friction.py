import mujoco
xml = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/robot_groundcontact_backlash.xml"
spec = mujoco.MjSpec.from_file(xml)
# find left_hip_yaw joint in spec
for j in spec.joints:
    if j.name == "left_hip_yaw":
        print("before: solref_limit", j.solref_limit, "solref_friction", j.solref_friction)
        j.solref_friction = [-5.0e4, -2.0e2]
        j.solimp_friction = [0.99,0.9999,0.001,0.5,2.0]
        print("after: solref_limit", j.solref_limit, "solref_friction", j.solref_friction)
m = spec.compile()
jid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "left_hip_yaw")
did = m.jnt_dofadr[jid]
print("compiled jnt_solref(limit):", m.jnt_solref[jid])
print("compiled dof_solref(friction):", m.dof_solref[did])
