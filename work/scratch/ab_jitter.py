import sys, numpy as np, mujoco
sys.path.insert(0,"D:/AI/Duck/work/scratch"); sys.path.insert(0,"D:/AI/Duck/upstream/bam")
import infer_stub as IP
from bam.mujoco import MujocoController
XML="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"

def load(which):
    bam=IP.load_bam_model(IP.BAM_KP_FW, 7.4, None)
    flim=8.2*bam.kt.value/bam.R.value
    spec=mujoco.MjSpec.from_file(XML); names=[]
    for act in spec.actuators:
        t=act.target; tn=t.name if hasattr(t,'name') else str(t)
        if tn.startswith("passive_"): continue
        act.set_to_motor(); act.forcelimited=True; act.forcerange=(-flim,flim); act.ctrllimited=False
        act.gear=[1.,0,0,0,0,0]; names.append(act.name)
        for j in spec.joints:
            if j.name==tn:
                j.armature=float(bam.actuator.get_extra_inertia()); j.damping=np.zeros((3,1)); j.frictionloss=0.0
                if which=="ref": j.solref_friction=(-5e4,-2e2); j.solimp_friction=(0.99,0.9999,0.001,0.5,2.0)
                else: j.solref_limit=(-5e4,-2e2); j.solimp_limit=(0.99,0.9999,0.001,0.5,2.0)
                break
    model=spec.compile(); model.opt.timestep=0.005
    return model, mujoco.MjData(model), names, bam

def run(which, vx, jitter, seconds=10.0):
    model,data,names,bam=load(which)
    ctrl=MujocoController(bam,names,model,data,vin_drop_gain=0.1,vin_min=6.0)
    pol=IP.PolicyInference(model,data,bam_ctrl=ctrl,walking_onnx_path="D:/AI/Duck/policies/velstand.onnx",
        action_scale=1.0,switch_threshold=0.05,use_projected_gravity=True,new_cmd_obs=True)
    fj=mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); qa=model.jnt_qposadr[fj]
    rng=np.random.default_rng(jitter)
    data.qpos[:]=0; data.qvel[:]=0
    data.qpos[qa:qa+7]=[0,0,0.125,1,0,0,0]
    for i,adr in enumerate(pol.joint_qpos_indices):
        data.qpos[adr]=pol.default_pose[i]+(rng.normal(0,0.01) if jitter else 0.0)
    mujoco.mj_forward(model,data); ctrl.reset(data.qpos)
    pol.set_vel_cmd(vx,0,0)
    x0=data.qpos[qa:qa+2].copy()
    for k in range(int(seconds/0.02)):
        a=pol.infer(); pol.apply_action(a)
        for _ in range(4): ctrl.update(); mujoco.mj_step(model,data)
    return np.linalg.norm(data.qpos[qa:qa+2]-x0)/seconds

for vx in (0.15,0.2):
    refs=[run("ref",vx,s) for s in (0,1,2,3,4)]
    apps=[run("app",vx,s) for s in (0,1,2,3,4)]
    print(f"vx={vx}: ref {np.round(refs,3)} mean={np.mean(refs):.3f} | app {np.round(apps,3)} mean={np.mean(apps):.3f}")
