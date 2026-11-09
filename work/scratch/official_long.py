import sys, numpy as np, mujoco
sys.path.insert(0,"D:/AI/Duck/work/scratch"); sys.path.insert(0,"D:/AI/Duck/upstream/bam")
import infer_stub as IP
xml="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
bam=IP.load_bam_model(IP.BAM_KP_FW, 7.4, None)
model,data,bam_ctrl,names=IP.load_mujoco_with_bam(xml,bam,0.005,0.1,IP.BAM_VIN_MIN)
pol=IP.PolicyInference(model,data,bam_ctrl=bam_ctrl,walking_onnx_path="D:/AI/Duck/policies/velstand.onnx",
  action_scale=1.0,switch_threshold=0.05,use_projected_gravity=True,new_cmd_obs=True)
fj=mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); qa=model.jnt_qposadr[fj]
def reset():
    data.qpos[:]=0; data.qvel[:]=0
    data.qpos[qa:qa+7]=[0,0,0.125,1,0,0,0]
    for i,adr in enumerate(pol.joint_qpos_indices): data.qpos[adr]=pol.default_pose[i]
    for j in range(model.njnt):
        n=mujoco.mj_id2name(model,mujoco.mjtObj.mjOBJ_JOINT,j)
        if n and n.startswith("passive_"): data.qpos[model.jnt_qposadr[j]]=0
    mujoco.mj_forward(model,data); bam_ctrl.reset(data.qpos)
    pol.last_action=np.zeros(pol.n_joints,dtype=np.float32)
for vx in (0.15,0.3,0.4):
    reset(); pol.set_vel_cmd(vx,0,0)
    x0=data.qpos[qa:qa+2].copy()
    traj=[]
    for k in range(int(12.0/0.02)):
        a=pol.infer(); pol.apply_action(a)
        for _ in range(4): bam_ctrl.update(); mujoco.mj_step(model,data)
        if (k+1)%50==0:
            R=data.xmat[mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_BODY,"trunk_base")].reshape(3,3)
            traj.append((round((k+1)*0.02,1), data.qpos[qa]-x0[0], data.qpos[qa+2], np.degrees(np.arccos(np.clip(R[2,2],-1,1)))))
    print(f"== official vx={vx}")
    for t,dx,z,tilt in traj:
        print(f"   t={t:5.1f} dx={dx:+.3f} z={z:.3f} tilt={tilt:4.1f}")
