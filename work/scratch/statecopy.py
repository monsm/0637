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
m,d,ctrl=F.build("ref")
# copy state exactly
mujoco.mj_resetData(m,d)
d.qpos[:]=data.qpos; d.qvel[:]=data.qvel
mujoco.mj_forward(m,d); ctrl.reset(d.qpos)
print("qpos diff", np.abs(d.qpos-data.qpos).max(), "qvel", np.abs(d.qvel-data.qvel).max())
gyro,grav,pos,vel,R=F.sensors(m,d)
oo=pol.get_observations()
c13=np.zeros(13,dtype=np.float32); c13[0]=0.15
orr=np.concatenate([gyro,grav,pos-F.DEFAULT,vel,np.zeros(14,np.float32),c13]).astype(np.float32)
print("obs diff at same state:", np.abs(oo-orr).max())
print("official jpos:", np.round(oo[6:20],4))
print("replica  jpos:", np.round(orr[6:20],4))
print("official jvel:", np.round(oo[20:34],4))
print("replica  jvel:", np.round(vel,4))
print("official gyro:", np.round(oo[0:3],4), " replica:", np.round(gyro,4))
print("official grav:", np.round(oo[3:6],4), " replica:", np.round(grav,4))
print("sensor gyro:", np.round(pol.get_base_ang_vel(),4))
print("qvel trunk[3:6]:", data.qvel[model.jnt_dofadr[fj]+3:model.jnt_dofadr[fj]+6])
