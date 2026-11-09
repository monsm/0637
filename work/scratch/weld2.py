import numpy as np, mujoco, sys
sys.path.insert(0,"D:/AI/Duck/work/scratch")
from weld import build, DEFAULT, WIRE

def run(mode, seconds=20.0, sag=0.0, seed=0):
    m,d,ctrl=build(mode)
    mujoco.mj_resetData(m,d)
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint")
    q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
    d.qpos[q:q+7]=[0,0,0.125,1,0,0,0]
    for i,n in enumerate(WIRE):
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n); d.qpos[m.jnt_qposadr[j]]=DEFAULT[i]
        b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        if b>=0: d.qpos[m.jnt_qposadr[b]]=0.0
    # start the neck 3 degrees off its target: the motor must push through friction
    nj=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"neck_pitch")
    d.qpos[m.jnt_qposadr[nj]] = DEFAULT[5] + np.radians(sag)
    mujoco.mj_forward(m,d)
    ctrl.reset(d.qpos)
    ctrl.q_target[:]=DEFAULT
    rows=[]
    for k in range(int(seconds/0.02)):
        for _ in range(4):
            d.qvel[dof:dof+6]=0.0; d.qpos[q:q+7]=[0,0,0.125,1,0,0,0]
            ctrl.update(); mujoco.mj_step(m,d)
        if (k+1)%100==0:
            nb=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"passive_neck_pitch_backlash")
            p=d.qpos[m.jnt_qposadr[nj]] + d.qpos[m.jnt_qposadr[nb]]
            rows.append((round((k+1)*0.02,1), np.degrees(p-DEFAULT[5])))
    return rows

if __name__=="__main__":
    for sag in (0.0, 3.0):
        ref=run("ref", sag=sag); core=run("core", sag=sag)
        print(f"== start {sag} deg off target")
        for (t,e1),(t2,e2) in zip(ref,core):
            if t<=6.0 or t%4==0:
                print(f"  t={t:5.1f}s  ref residual={e1:+7.4f}°   core residual={e2:+7.4f}°   diff={e2-e1:+7.4f}°")
