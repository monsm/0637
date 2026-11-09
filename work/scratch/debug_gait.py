import numpy as np, mujoco, onnxruntime as ort
import gait as G
m,d,ctrl = G.build(True, False)
G.reset(m,d)
sess=ort.InferenceSession(G.ONNX); iname=sess.get_inputs()[0].name; oname=sess.get_outputs()[0].name
last=np.zeros(14,dtype=np.float32)
c13=np.zeros(13,dtype=np.float32); c13[0]=0.3
jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]
for k in range(50):
    o=G.obs(m,d,last,c13)
    a=sess.run([oname],{iname:o.reshape(1,-1)})[0].squeeze(0).astype(np.float32)
    last=a
    ctrl.q_target[:]=G.DEFAULT+0.9*a
    for _ in range(4):
        ctrl.update(); mujoco.mj_step(m,d)
    if k%5==0:
        R=np.zeros(9); mujoco.mju_quat2Mat(R,d.qpos[q+3:q+7]); R=R.reshape(3,3)
        print(f"k={k:3d} t={d.time:.2f} trunk={np.round(d.qpos[q:q+3],4)} quat={np.round(d.qpos[q+3:q+7],3)} zaxis={np.round(R[:,2],2)} act={np.round(a[:4],2)} qtarget0={np.round(ctrl.q_target[0],3)}")
