"""Decide the free-joint angular qvel frame by integrating one step."""
import numpy as np, mujoco
p = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
m = mujoco.MjModel.from_xml_path(p); d = mujoco.MjData(m)
jid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
q = m.jnt_qposadr[jid]; dof = m.jnt_dofadr[jid]

def quat_mul(a,b):
    w1,x1,y1,z1=a; w2,x2,y2,z2=b
    return np.array([w1*w2-x1*x2-y1*y2-z1*z2,
                     w1*x2+x1*w2+y1*z2-z1*y2,
                     w1*y2-x1*z2+y1*w2+z1*x2,
                     w1*z2+x1*y2-y1*x2+z1*w2])

# yaw 90 deg, then set angular qvel = (1,0,0) and integrate a tiny step.
th=np.pi/2
d.qpos[q:q+7]=[0,0,0.125,np.cos(th/2),0,0,np.sin(th/2)]
d.qvel[dof:dof+6]=0
d.qvel[dof+3:dof+6]=[1,0,0]
mujoco.mj_forward(m,d)
q0=d.qpos[q+3:q+7].copy()
dt=1e-5
# integrate quaternion manually the way MuJoCo does: qdot = 0.5 * quat(0, omega) * q
def step_quat(qq, omega, dt):
    w=np.array([0.0,*omega])
    return quat_mul(w,qq)*0.5*dt + qq
q_local = step_quat(q0, np.array([1.0,0,0]), dt)          # if qvel is body-local
R = d.xmat[mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_BODY,"trunk_base")].reshape(3,3)
q_world = step_quat(q0, R@np.array([1.0,0,0]), dt)       # if qvel is world-frame
# Actually run MuJoCo with gravity and contacts off for one tiny step
m.opt.gravity[:] = 0
d.ctrl[:] = 0
mujoco.mj_step(m,d)
q1=d.qpos[q+3:q+7].copy()
q1/=np.linalg.norm(q1)
for name,qq in (("body-local prediction",q_local),("world prediction",q_world)):
    qq=qq/np.linalg.norm(qq)
    print(f"{name:24s}: angle to MuJoCo result = {np.degrees(2*np.arccos(min(1,abs(np.dot(qq,q1))))):.4f} deg")
print("qvel after step:", d.qvel[dof+3:dof+6])
print("MuJoCo q after :", q1)
print("q before       :", q0)
