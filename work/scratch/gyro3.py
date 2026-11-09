import numpy as np, mujoco
p="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
m=mujoco.MjModel.from_xml_path(p); d=mujoco.MjData(m)
jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
bid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_BODY,"trunk_base")
sid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_SENSOR,"imu_ang_vel"); adr=m.sensor_adr[sid]
def test(quat, omega, label):
    d.qpos[q:q+7]=[0,0,0.125]+list(quat); d.qvel[:]=0
    d.qvel[dof+3:dof+6]=omega
    mujoco.mj_forward(m,d)
    R=d.xmat[bid].reshape(3,3)
    res=np.zeros(6); mujoco.mj_objectVelocity(m,d,mujoco.mjtObj.mjOBJ_BODY,bid,res,1)  # local
    print(f"{label}: qvel_ang={omega} -> sensor={np.round(d.sensordata[adr:adr+3],4)} "
          f"R^T*qvel={np.round(R.T@np.array(omega),4)} R*qvel={np.round(R@np.array(omega),4)} objVel_local={np.round(res[3:6],4)}")
import math
test([1,0,0,0],[1,0,0],"upright")
th=math.pi/2
test([math.cos(th/2),0,0,math.sin(th/2)],[1,0,0],"yaw90")
# pitch 40 deg about y
th=math.radians(40)
test([math.cos(th/2),0,math.sin(th/2),0],[1,0,0],"pitch40")
