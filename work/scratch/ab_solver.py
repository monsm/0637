"""Official harness, one variable: which solver array gets the BAM stiff values.
ref  = solref_friction (what bam.mujoco / bam.mjlab do)
app  = jnt_solref (what core/duck-mobile-core/native/duck_mj_shim.cpp does)
"""
import sys, numpy as np, mujoco
sys.path.insert(0,"D:/AI/Duck/work/scratch"); sys.path.insert(0,"D:/AI/Duck/upstream/bam")
import infer_stub as IP
from bam.model import load_model
from bam.mujoco import MujocoController

XML="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"

def load(which):
    bam=IP.load_bam_model(IP.BAM_KP_FW, 7.4, None)
    kt,R=bam.kt.value,bam.R.value; flim=8.2*kt/R
    spec=mujoco.MjSpec.from_file(XML); names=[]
    for act in spec.actuators:
        t=act.target; tn=t.name if hasattr(t,'name') else str(t)
        if tn.startswith("passive_"): continue
        act.set_to_motor(); act.forcelimited=True; act.forcerange=(-flim,flim); act.ctrllimited=False
        act.gear=[1.,0,0,0,0,0]; names.append(act.name)
        for j in spec.joints:
            if j.name==tn:
                j.armature=float(bam.actuator.get_extra_inertia()); j.damping=np.zeros((3,1)); j.frictionloss=0.0
                if which=="ref":
                    j.solref_friction=(-5e4,-2e2); j.solimp_friction=(0.99,0.9999,0.001,0.5,2.0)
                else:
                    j.solref_limit=(-5e4,-2e2); j.solimp_limit=(0.99,0.9999,0.001,0.5,2.0)
                break
    model=spec.compile(); model.opt.timestep=0.005; data=mujoco.MjData(model)
    ctrl=MujocoController(bam,names,model,data,vin_drop_gain=0.1,vin_min=6.0)
    return model,data,ctrl,bam

def run(which, vx, seconds=10.0):
    model,data,bam_ctrl,bam=load(which)
    pol=IP.PolicyInference(model,data,bam_ctrl=bam_ctrl,walking_onnx_path="D:/AI/Duck/policies/velstand.onnx",
        action_scale=1.0,switch_threshold=0.05,use_projected_gravity=True,new_cmd_obs=True)
    fj=mujoco.mj_name2id(model,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); qa=model.jnt_qposadr[fj]
    data.qpos[:]=0; data.qvel[:]=0
    data.qpos[qa:qa+7]=[0,0,0.125,1,0,0,0]
    for i,adr in enumerate(pol.joint_qpos_indices): data.qpos[adr]=pol.default_pose[i]
    mujoco.mj_forward(model,data); bam_ctrl.reset(data.qpos)
    pol.set_vel_cmd(vx,0,0)
    x0=data.qpos[qa:qa+2].copy()
    for k in range(int(seconds/0.02)):
        a=pol.infer(); pol.apply_action(a)
        for _ in range(4): bam_ctrl.update(); mujoco.mj_step(model,data)
    return np.linalg.norm(data.qpos[qa:qa+2]-x0)/seconds

for vx in (0.15,0.3,0.4):
    r=run("ref",vx); a=run("app",vx)
    print(f"vx={vx:.2f}: ref(dof_solref)={r:.3f} m/s   app(jnt_solref)={a:.3f} m/s   delta={a-r:+.3f}")
