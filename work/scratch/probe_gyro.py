import mujoco, numpy as np
p = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
m = mujoco.MjModel.from_xml_path(p)
d = mujoco.MjData(m)
# find free joint dof
jid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
dof = m.jnt_dofadr[jid]
qadr = m.jnt_qposadr[jid]
# Rotate trunk 90 deg about world z: yaw. Then set angular velocity purely about WORLD x.
th = np.pi/2
d.qpos[qadr:qadr+3] = [0,0,0.125]
d.qpos[qadr+3:qadr+7] = [np.cos(th/2), 0, 0, np.sin(th/2)]  # yaw 90
# world-frame angular velocity omega_w = (1,0,0)
# If qvel angular is global, qvel[3:6] = (1,0,0); if body-local, = R^T*(1,0,0) = (0,-1,0)
d.qvel[dof+0:dof+3] = [0,0,0]
d.qvel[dof+3:dof+6] = [1,0,0]
mujoco.mj_forward(m, d)
# gyro sensor on imu site
sid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_SENSOR, "imu_ang_vel")
adr = m.sensor_adr[sid]
print("sensor gyro (site frame):", d.sensordata[adr:adr+3])
print("freejoint qvel[3:6]     :", d.qvel[dof+3:dof+6])
# body xmat
bid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, "trunk_base")
print("trunk xmat:\n", d.xmat[bid].reshape(3,3))
