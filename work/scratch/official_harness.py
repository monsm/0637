"""Use the official infer_policy.PolicyInference itself, headless, on the bundled scene."""
import sys, math, numpy as np, mujoco
sys.path.insert(0, "D:/AI/Duck/work/scratch")
sys.path.insert(0, "D:/AI/Duck/upstream/bam")
from bam.model import load_model
from bam.mujoco import MujocoController
import infer_stub as IP

xml = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
bam = IP.load_bam_model(IP.BAM_KP_FW, 7.4, None)
model, data, bam_ctrl, names = IP.load_mujoco_with_bam(xml, bam, 0.005, 0.1, IP.BAM_VIN_MIN)
policy = IP.PolicyInference(model, data, bam_ctrl=bam_ctrl,
    walking_onnx_path="D:/AI/Duck/policies/velstand.onnx", action_scale=1.0,
    switch_threshold=0.05, use_projected_gravity=True, new_cmd_obs=True)
# place like infer_policy.py
fj = mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint")
qa = model.jnt_qposadr[fj]
data.qpos[qa+0]=0; data.qpos[qa+1]=0; data.qpos[qa+2]=0.125
data.qpos[qa+3:qa+7]=[1,0,0,0]
for i,adr in enumerate(policy.joint_qpos_indices):
    data.qpos[adr]=policy.default_pose[i]
bam_ctrl.reset(data.qpos)
policy.set_position_targets(policy.default_pose)
mujoco.mj_forward(model, data)

for vx in (0.15, 0.3):
    # reset
    data.qpos[qa+0]=0; data.qpos[qa+1]=0; data.qpos[qa+2]=0.125
    data.qpos[qa+3:qa+7]=[1,0,0,0]
    data.qvel[:]=0
    for i,adr in enumerate(policy.joint_qpos_indices): data.qpos[adr]=policy.default_pose[i]
    for j in range(model.njnt):
        n = mujoco.mj_id2name(model, mujoco.mjtObj.mjOBJ_JOINT, j)
        if n and n.startswith("passive_"): data.qpos[model.jnt_qposadr[j]]=0
    mujoco.mj_forward(model,data)
    bam_ctrl.reset(data.qpos)
    policy.last_action = np.zeros(policy.n_joints, dtype=np.float32)
    policy.set_vel_cmd(vx, 0.0, 0.0)
    x0 = data.qpos[qa:qa+2].copy()
    for k in range(int(8.0/0.02)):
        a = policy.infer(); policy.apply_action(a)
        for _ in range(4):
            bam_ctrl.update(); mujoco.mj_step(model, data)
    moved = np.linalg.norm(data.qpos[qa:qa+2]-x0)
    print(f"official infer_policy vx={vx:.2f} (scale 1.0) -> {moved/8:.3f} m/s ({100*moved/8/vx:.0f}%)")
