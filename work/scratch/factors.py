import numpy as np, sys
sys.path.insert(0,"D:/AI/Duck/work/scratch")
import faithful as F

def run(scale, head_a, legs_a, vx, seconds=8.0):
    F.ACTION_SCALE=scale; F.HEAD_ALPHA=head_a; F.LEGS_ALPHA=legs_a
    m,d,ctrl=F.build("ref"); F.reset(m,d)
    import mujoco, onnxruntime as ort
    sess=ort.InferenceSession(F.ONNX); iname=sess.get_inputs()[0].name; oname=sess.get_outputs()[0].name
    last=np.zeros(14,dtype=np.float32); ema=np.zeros(3); prev=None
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]
    x0=None
    for k in range(int(seconds/0.02)):
        gyro,grav,pos,vel,R=F.sensors(m,d)
        ema += F.CMD_ALPHA*(np.array([vx,0,0])-ema)
        c13=np.zeros(13,dtype=np.float32); c13[0:3]=ema
        o=np.concatenate([gyro,grav,pos-F.DEFAULT,vel,last,c13]).astype(np.float32)
        a=sess.run([oname],{iname:o.reshape(1,-1)})[0].squeeze(0).astype(np.float32); last=a
        tg=F.DEFAULT+scale*a
        if prev is not None and (head_a<1.0 or legs_a<1.0):
            for j in F.HEAD: tg[j]=head_a*tg[j]+(1-head_a)*prev[j]
            for j in range(14):
                if j in F.HEAD: continue
                tg[j]=legs_a*tg[j]+(1-legs_a)*prev[j]
        prev=tg; ctrl.q_target[:]=tg
        for _ in range(4): ctrl.update(); mujoco.mj_step(m,d)
        if x0 is None: x0=d.qpos[q:q+2].copy()
    return np.linalg.norm(d.qpos[q:q+2]-x0)/seconds

for scale,ha,la,label in [(1.0,1.0,1.0,"scale1.0 no-LP (official)"),
                          (0.9,1.0,1.0,"scale0.9 no-LP"),
                          (1.0,0.5,0.7,"scale1.0 +LP"),
                          (0.9,0.5,0.7,"scale0.9 +LP (app)")]:
    print(f"{label:26s} vx=0.15 -> {run(scale,ha,la,0.15):.3f} m/s   vx=0.30 -> {run(scale,ha,la,0.30):.3f} m/s")
