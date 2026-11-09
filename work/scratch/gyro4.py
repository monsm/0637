import numpy as np, mujoco, math
p="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
m=mujoco.MjModel.from_xml_path(p); d=mujoco.MjData(m)
m.opt.gravity[:]=0
jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
# yaw 90deg body, angular qvel = (1,0,0), no gravity, step 0.2 s
th=math.pi/2
d.qpos[q:q+7]=[0,0,0.5,math.cos(th/2),0,0,math.sin(th/2)]
d.qvel[:]=0; d.qvel[dof+3:dof+6]=[1,0,0]
mujoco.mj_forward(m,d)
q0=d.qpos[q+3:q+7].copy()
for _ in range(40): mujoco.mj_step(m,d)   # 40 * 0.005 = 0.2 s
q1=d.qpos[q+3:q+7].copy()
def to_mat(qq):
    R=np.zeros(9); mujoco.mju_quat2Mat(R,qq); return R.reshape(3,3)
R0=to_mat(q0); R1=to_mat(q1)
# relative rotation in body frame: R0^T R1
rel=R0.T@R1
ang=np.degrees(np.arccos(np.clip((np.trace(rel)-1)/2,-1,1)))
axis=np.array([rel[2,1]-rel[1,2], rel[0,2]-rel[2,0], rel[1,0]-rel[0,1]])
axis/=np.linalg.norm(axis)
print(f"body-frame rotation over 0.2s: {ang:.2f}° about body axis {np.round(axis,3)}")
relw=R1@R0.T
angw=np.degrees(np.arccos(np.clip((np.trace(relw)-1)/2,-1,1)))
axisw=np.array([relw[2,1]-relw[1,2], relw[0,2]-relw[2,0], relw[1,0]-relw[0,1]]); axisw/=np.linalg.norm(axisw)
print(f"world-frame rotation over 0.2s: {angw:.2f}° about world axis {np.round(axisw,3)}")
print("if qvel is BODY-local: expect ~11.46° about body x=(1,0,0)")
print("if qvel is WORLD:      expect ~11.46° about world x=(1,0,0), i.e. body axis R0^T@x=", np.round(R0.T@np.array([1,0,0]),3))
