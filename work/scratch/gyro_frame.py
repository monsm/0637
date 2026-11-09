"""Is the free joint's qvel[3:6] body-frame or world-frame, and what does the
reference obs actually consume? The policy is trained on root_link_ang_vel_b
(body frame)."""
import numpy as np, mujoco
p = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
m = mujoco.MjModel.from_xml_path(p); d = mujoco.MjData(m)
jid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
q = m.jnt_qposadr[jid]; dof = m.jnt_dofadr[jid]
bid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, "trunk_base")

# Yaw the trunk 90 degrees about world z, give it a WORLD-frame angular velocity about x.
th = np.pi/2
d.qpos[q:q+3] = [0,0,0.125]
d.qpos[q+3:q+7] = [np.cos(th/2), 0, 0, np.sin(th/2)]
d.qvel[dof+0:dof+3] = [0,0,0]
d.qvel[dof+3:dof+6] = [1,0,0]     # we will interpret this below
mujoco.mj_forward(m, d)

# mj_objectVelocity: 6D velocity of the body. res[0:3]=angular, res[3:6]=linear.
# flg_local=0 -> global frame; flg_local=1 -> local body frame.
res_g = np.zeros(6); res_l = np.zeros(6)
mujoco.mj_objectVelocity(m, d, mujoco.mjtObj.mjOBJ_BODY, bid, res_g, 0)
mujoco.mj_objectVelocity(m, d, mujoco.mjtObj.mjOBJ_BODY, bid, res_l, 1)
print("qvel[3:6] as set        :", d.qvel[dof+3:dof+6])
print("body angular GLOBAL     :", np.round(res_g[0:3],4))
print("body angular LOCAL      :", np.round(res_l[0:3],4))
R = d.xmat[bid].reshape(3,3)
print("R (body->world)         :\n", np.round(R,3))
print("R^T @ qvel[3:6]         :", np.round(R.T@d.qvel[dof+3:dof+6],4))
print("R @ qvel[3:6]           :", np.round(R@d.qvel[dof+3:dof+6],4))

sid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_SENSOR, "imu_ang_vel")
adr = m.sensor_adr[sid]
print("imu_ang_vel sensor      :", np.round(d.sensordata[adr:adr+3],4))
