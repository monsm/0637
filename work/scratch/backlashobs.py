"""Is the observed position the servo angle or servo+backlash? Does it change the gait?"""
import numpy as np, sys, mujoco, onnxruntime as ort
sys.path.insert(0,"D:/AI/Duck/work/scratch")
import faithful as F

def run(vx, sum_backlash, seconds=8.0, mode="ref"):
    m,d,ctrl=F.build(mode); F.reset(m,d)
    sess=ort.InferenceSession(F.ONNX); iname=sess.get_inputs()[0].name; oname=sess.get_outputs()[0].name
    last=np.zeros(14,dtype=np.float32); ema=np.zeros(3); prev=None
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]
    x0=None
    for k in range(int(seconds/0.02)):
        gyro,grav,pos,vel,R=F.sensors(m,d)
        if not sum_backlash:
            pos=np.array([d.qpos[mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n)] and 0 for n in []]) if False else np.array([d.qpos[m.jnt_qposadr[mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n)]] for n in F.WIRE])
            vel=np.array([d.qvel[m.jnt_dofadr[mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n)]] for n in F.WIRE])
        ema += F.CMD_ALPHA*(np.array([vx,0,0])-ema)
        c13=np.zeros(13,dtype=np.float32); c13[0:3]=ema
        o=np.concatenate([gyro,grav,pos-F.DEFAULT,vel,last,c13]).astype(np.float32)
        a=sess.run([oname],{iname:o.reshape(1,-1)})[0].squeeze(0).astype(np.float32); last=a
        tg=F.DEFAULT+F.ACTION_SCALE*a
        if prev is not None:
            for j in F.HEAD: tg[j]=F.HEAD_ALPHA*tg[j]+(1-F.HEAD_ALPHA)*prev[j]
            for j in range(14):
                if j in F.HEAD: continue
                tg[j]=F.LEGS_ALPHA*tg[j]+(1-F.LEGS_ALPHA)*prev[j]
        prev=tg; ctrl.q_target[:]=tg
        for _ in range(4): ctrl.update(); mujoco.mj_step(m,d)
        if x0 is None: x0=d.qpos[q:q+2].copy()
    return np.linalg.norm(d.qpos[q:q+2]-x0)/seconds

for vx in (0.15,0.3):
    print(f"vx={vx}: obs servo+backlash={run(vx,True):.3f} m/s   obs servo-only={run(vx,False):.3f} m/s")
