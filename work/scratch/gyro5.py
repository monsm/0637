import numpy as np, mujoco, math
p="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
m=mujoco.MjModel.from_xml_path(p); d=mujoco.MjData(m)
jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
bid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_BODY,"trunk_base")
def probe(quat, omega, label):
    d.qpos[:]=0; d.qvel[:]=0
    d.qpos[q:q+7]=[0,0,0.5]+list(quat)
    d.qvel[dof+3:dof+6]=omega
    mujoco.mj_forward(m,d)
    R=d.xmat[bid].reshape(3,3)
    res=np.zeros(6)
    mujoco.mj_objectVelocity(m,d,mujoco.mjtObj.mjOBJ_BODY,bid,res,1)   # local
    ang_local=res[0:3]
    res2=np.zeros(6)
    mujoco.mj_objectVelocity(m,d,mujoco.mjtObj.mjOBJ_BODY,bid,res2,0)   # global
    print(f"{label:8s} qvel_ang={omega}  ->  objectVelocity local={np.round(ang_local,4)}  global={np.round(res2[0:3],4)}")
    print(f"          R^T*qvel={np.round(R.T@np.array(omega),4)}   R*qvel={np.round(R@np.array(omega),4)}")
th=math.pi/2
probe([1,0,0,0],[1,0,0],"upright")
probe([math.cos(th/2),0,0,math.sin(th/2)],[1,0,0],"yaw90")
th=math.radians(40)
probe([math.cos(th/2),0,math.sin(th/2),0],[1,0,0],"pitch40")
