import sys, numpy as np, mujoco
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
o=pol.get_observations()
print("official obs[:12]:", np.round(o[:12],4))
print("official gyro sensor:", np.round(pol.get_base_ang_vel(),4))
print("official proj grav:", np.round(pol.get_projected_gravity(),4))
print("official jointpos:", np.round(o[6:20],4))
print("official cmd:", np.round(o[48:61],4))

# now my replica
m,d,ctrl=F.build("ref"); F.reset(m,d)
gyro,grav,pos,vel,R=F.sensors(m,d)
print("\nreplica gyro (qvel world):", np.round(gyro,4))
print("replica grav:", np.round(grav,4))
print("replica jointpos:", np.round(pos-F.DEFAULT,4))
print("replica qvel:", np.round(vel,4))
print("official jointvel:", np.round(o[20:34],4))
