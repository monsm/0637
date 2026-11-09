import sys, numpy as np, mujoco, onnxruntime as ort
sys.path.insert(0,"D:/AI/Duck/work/scratch"); sys.path.insert(0,"D:/AI/Duck/upstream/bam")
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
last=np.zeros(14,dtype=np.float32); ema=np.zeros(3)
# use the SAME command semantics: official feeds raw 0.15 immediately
for k in range(3):
    oo=pol.get_observations()
    gyro,grav,pos,vel,R=F.sensors(m,d)
    c13=np.zeros(13,dtype=np.float32); c13[0]=0.15
    orr=np.concatenate([gyro,grav,pos-F.DEFAULT,vel,last,c13]).astype(np.float32)
    d_=oo-orr
    nz=np.nonzero(np.abs(d_)>1e-6)[0]
    print(f"tick {k}: differing idx {nz.tolist()[:20]}  max|d|={np.abs(d_).max():.5f}")
    if len(nz): 
        for i in nz[:8]: print(f"    idx {i}: official={oo[i]:+.5f} replica={orr[i]:+.5f}")
    ao=pol.infer(); pol.apply_action(ao)
    ar=sess.run([oname],{iname:orr.reshape(1,-1)})[0].squeeze(0).astype(np.float32); last=ar
    # apply same scale 1.0, no LP
    ctrl.q_target[:]=F.DEFAULT+1.0*ar
    for _ in range(4): bam_ctrl.update(); mujoco.mj_step(model,data)
    for _ in range(4): ctrl.update(); mujoco.mj_step(m,d)
