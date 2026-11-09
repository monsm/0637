import sys, numpy as np, mujoco, onnxruntime as ort
sys.path.insert(0,"D:/AI/Duck/work/scratch"); sys.path.insert(0,"D:/AI/Duck/upstream/bam")
from bam.model import load_model
import infer_stub as IP
import faithful as F

xml="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
bam=IP.load_bam_model(IP.BAM_KP_FW, 7.4, None)
model,data,bam_ctrl,names=IP.load_mujoco_with_bam(xml,bam,0.005,0.1,IP.BAM_VIN_MIN)
pol=IP.PolicyInference(model,data,bam_ctrl=bam_ctrl,walking_onnx_path="D:/AI/Duck/policies/velstand.onnx",
  action_scale=1.0,switch_threshold=0.05,use_projected_gravity=True,new_cmd_obs=True)
fj=mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); qa=model.jnt_qposadr[fj]
data.qpos[qa:qa+7]=[0,0,0.125,1,0,0,0]
for i,adr in enumerate(pol.joint_qpos_indices): data.qpos[adr]=pol.default_pose[i]
mujoco.mj_forward(model,data); bam_ctrl.reset(data.qpos)
pol.set_vel_cmd(0.15,0,0)

m,d,ctrl=F.build("ref"); F.reset(m,d)
sess=ort.InferenceSession(F.ONNX); iname=sess.get_inputs()[0].name; oname=sess.get_outputs()[0].name
last=np.zeros(14,dtype=np.float32); ema=np.zeros(3); prev=None
jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]

for k in range(25):
    # official
    ao = pol.infer(); pol.apply_action(ao)
    for _ in range(4): bam_ctrl.update(); mujoco.mj_step(model,data)
    # replica
    gyro,grav,pos,vel,R=F.sensors(m,d)
    ema += F.CMD_ALPHA*(np.array([0.15,0,0])-ema)
    c13=np.zeros(13,dtype=np.float32); c13[0:3]=ema
    o=np.concatenate([gyro,grav,pos-F.DEFAULT,vel,last,c13]).astype(np.float32)
    ar=sess.run([oname],{iname:o.reshape(1,-1)})[0].squeeze(0).astype(np.float32); last=ar
    tg=F.DEFAULT+F.ACTION_SCALE*ar
    if prev is not None:
        for j in F.HEAD: tg[j]=F.HEAD_ALPHA*tg[j]+(1-F.HEAD_ALPHA)*prev[j]
        for j in range(14):
            if j in F.HEAD: continue
            tg[j]=F.LEGS_ALPHA*tg[j]+(1-F.LEGS_ALPHA)*prev[j]
    prev=tg; ctrl.q_target[:]=tg
    for _ in range(4): ctrl.update(); mujoco.mj_step(m,d)
    if k%4==0:
        print(f"k={k:2d} dact={np.abs(ao-ar).max():.3f} official_x={data.qpos[qa]:+.4f} replica_x={d.qpos[q]:+.4f} "
              f"official_z={data.qpos[qa+2]:.4f} replica_z={d.qpos[q+2]:.4f}")
