import sys, numpy as np, mujoco
sys.path.insert(0,"D:/AI/Duck/work/scratch"); sys.path.insert(0,"D:/AI/Duck/upstream/bam")
import infer_stub as IP
import faithful as F
xml="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
bam=IP.load_bam_model(IP.BAM_KP_FW, 7.4, None)
model,data,bam_ctrl,names=IP.load_mujoco_with_bam(xml,bam,0.005,0.1,IP.BAM_VIN_MIN)
m,d,ctrl=F.build("ref")
mujoco.mj_resetData(model,data); mujoco.mj_resetData(m,d)
fj=mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); qa=model.jnt_qposadr[fj]
data.qpos[qa:qa+7]=[0,0,0.125,1,0,0,0]
for i,adr in enumerate([model.jnt_qposadr[model.actuator_trnid[a][0]] for a in range(model.nu)]): data.qpos[adr]=F.DEFAULT[i]
d.qpos[:]=data.qpos
mujoco.mj_forward(model,data); mujoco.mj_forward(m,d)
bam_ctrl.reset(data.qpos); ctrl.reset(d.qpos)
tgt=F.DEFAULT+0.9*np.array([0.1,-0.1,0.2,0.3,0.1,0.05,0.02,0.01,0.0,0.0,-0.1,0.2,0.1,0.05])
bam_ctrl.q_target[:]=tgt; ctrl.q_target[:]=tgt
for step in range(4):
    bam_ctrl.update(); mujoco.mj_step(model,data)
    ctrl.update(); mujoco.mj_step(m,d)
    print(f"substep {step}: dqpos={np.abs(d.qpos-data.qpos).max():.3e} dqvel={np.abs(d.qvel-data.qvel).max():.3e} "
          f"ctrl_diff={np.abs(d.ctrl-data.ctrl).max():.3e} fric_diff={np.abs(m.dof_frictionloss-model.dof_frictionloss).max():.3e}")
