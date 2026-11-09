import mujoco, numpy as np
p = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
m = mujoco.MjModel.from_xml_path(p); d = mujoco.MjData(m)
jid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
q = m.jnt_qposadr[jid]; dof = m.jnt_dofadr[jid]
def core_gravity(quat):
    R = np.zeros(9); mujoco.mju_quat2Mat(R, np.asarray(quat,dtype=np.float64)); R=R.reshape(3,3)
    return [-R[2,0], -R[2,1], -R[2,2]]
for name, quat in [("upright",[1,0,0,0]), ("pitch30",[np.cos(np.pi/12),0,np.sin(np.pi/12),0]),
                   ("roll30",[np.cos(np.pi/12),np.sin(np.pi/12),0,0])]:
    d.qpos[q:q+7] = [0,0,0.125]+list(quat)
    mujoco.mj_forward(m,d)
    # mjlab projected_gravity_b = quat_apply_inverse(root_quat, (0,0,-1)) = R^T * (0,0,-1)
    R = d.xmat[mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_BODY,"trunk_base")].reshape(3,3)
    truth = R.T @ np.array([0,0,-1.0])
    print(name, "core=", np.round(core_gravity(quat),4), " truth(R^T*g)=", np.round(truth,4))
