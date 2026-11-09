"""The app's exact target pipeline (scale 0.9 + head/leg low-pass) with the
official harness, correctly initialised. Compare to the reference (scale 1.0, no LP)."""
import sys, numpy as np, mujoco
sys.path.insert(0,"D:/AI/Duck/work/scratch"); sys.path.insert(0,"D:/AI/Duck/upstream/bam")
import infer_stub as IP
from bam.mujoco import MujocoController
XML="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
HEAD=range(5,9)

def setup():
    bam=IP.load_bam_model(IP.BAM_KP_FW, 7.4, None)
    model,data,ctrl,names=IP.load_mujoco_with_bam(XML,bam,0.005,0.1,IP.BAM_VIN_MIN)
    pol=IP.PolicyInference(model,data,bam_ctrl=ctrl,walking_onnx_path="D:/AI/Duck/policies/velstand.onnx",
        action_scale=1.0,switch_threshold=0.05,use_projected_gravity=True,new_cmd_obs=True)
    fj=mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); qa=model.jnt_qposadr[fj]
    data.qpos[:]=0; data.qvel[:]=0
    data.qpos[qa:qa+7]=[0,0,0.125,1,0,0,0]
    for i,adr in enumerate(pol.joint_qpos_indices): data.qpos[adr]=pol.default_pose[i]
    mujoco.mj_forward(model,data); ctrl.reset(data.qpos)
    pol.last_action=np.zeros(pol.n_joints,dtype=np.float32)
    return model,data,ctrl,pol,qa

def run(vx, scale, lp, seconds=10.0):
    model,data,ctrl,pol,qa=setup()
    pol.set_vel_cmd(vx,0,0)
    prev=None
    x0=data.qpos[qa:qa+2].copy()
    for k in range(int(seconds/0.02)):
        a=pol.infer()
        tg=pol.default_pose+scale*a
        if lp and prev is not None:
            for j in HEAD: tg[j]=0.5*tg[j]+0.5*prev[j]
            for j in range(14):
                if j in HEAD: continue
                tg[j]=0.7*tg[j]+0.3*prev[j]
        prev=tg.copy()
        ctrl.q_target[:]=tg
        for _ in range(4): ctrl.update(); mujoco.mj_step(model,data)
    return np.linalg.norm(data.qpos[qa:qa+2]-x0)/seconds

for vx in (0.15,0.2,0.3,0.4):
    ref=run(vx,1.0,False); app=run(vx,0.9,True)
    print(f"vx={vx:.2f}: ref(1.0,noLP)={ref:.3f} m/s   app(0.9,+LP)={app:.3f} m/s  ({100*app/vx:.0f}% of asked)")
