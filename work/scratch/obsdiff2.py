import sys, numpy as np, mujoco
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
last=np.zeros(14,dtype=np.float32)
for k in range(30):
    oo=pol.get_observations()
    gyro,grav,pos,vel,R=F.sensors(m,d)
    c13=np.zeros(13,dtype=np.float32); c13[0]=0.15
    orr=np.concatenate([gyro,grav,pos-F.DEFAULT,vel,last,c13]).astype(np.float32)
    d_=oo-orr
    if k%5==0:
        print(f"k={k:2d} max|dobs|={np.abs(d_).max():.5f}  gyro off={np.abs(d_[0:3]).max():.5f} grav={np.abs(d_[3:6]).max():.5f} "
              f"jpos={np.abs(d_[6:20]).max():.5f} jvel={np.abs(d_[20:34]).max():.5f} act={np.abs(d_[34:48]).max():.5f}")
    a=pol.infer()
    # same action -> same target -> both bodies
    tgt=F.DEFAULT+1.0*a
    pol.set_position_targets(tgt)
    ctrl.q_target[:]=tgt
    last=a
    for _ in range(4): bam_ctrl.update(); mujoco.mj_step(model,data)
    for _ in range(4): ctrl.update(); mujoco.mj_step(m,d)
