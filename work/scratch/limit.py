"""Push the knee against its mechanical limit and measure penetration/rebound."""
import numpy as np, mujoco
from bam.model import load_model
from bam.mujoco import MujocoController

XML = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
DEFAULT = np.array([0.0,-0.0873,-0.4579,-0.0049,0.4530,0.3491,0.3491,0.0,0.0,0.0,0.0873,0.4579,0.0049,-0.4530])
WIRE = ["left_hip_yaw","left_hip_roll","left_hip_pitch","left_knee","left_ankle",
        "neck_pitch","head_pitch","head_yaw","head_roll",
        "right_hip_yaw","right_hip_roll","right_hip_pitch","right_knee","right_ankle"]

def build(mode):
    spec = mujoco.MjSpec.from_file(XML)
    bam = load_model(motor_name="xl330", model="m6")
    bam.actuator.kp=200.0; bam.actuator.vin=7.4; bam.actuator.max_current=None
    flim = 7.4*bam.kt.value/bam.R.value
    names=[]
    for act in spec.actuators:
        tgt=act.target; tn=tgt.name if hasattr(tgt,'name') else str(tgt)
        if tn.startswith("passive_"): continue
        act.set_to_motor(); act.forcelimited=True; act.forcerange=(-flim,flim); act.ctrllimited=False
        act.gear=[1.0,0,0,0,0,0]; names.append(act.name)
        for j in spec.joints:
            if j.name==tn:
                j.armature=float(bam.actuator.get_extra_inertia())
                j.damping=np.zeros((3,1)); j.frictionloss=0.0
                if mode=="ref":
                    j.solref_friction=(-5.0e4,-2.0e2); j.solimp_friction=(0.99,0.9999,0.001,0.5,2.0)
                elif mode=="core":
                    j.solref_limit=(-5.0e4,-2.0e2); j.solimp_limit=(0.99,0.9999,0.001,0.5,2.0)
                break
    m=spec.compile(); m.opt.timestep=0.005
    d=mujoco.MjData(m)
    ctrl=MujocoController(bam,names,m,d,vin_drop_gain=0.1,vin_min=6.0)
    return m,d,ctrl

def run(mode, target_knee=2.0, seconds=2.0):
    m,d,ctrl=build(mode)
    mujoco.mj_resetData(m,d)
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint")
    q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
    d.qpos[q:q+7]=[0,0,0.125,1,0,0,0]
    for i,n in enumerate(WIRE):
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n); d.qpos[m.jnt_qposadr[j]]=DEFAULT[i]
        b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        if b>=0: d.qpos[m.jnt_qposadr[b]]=0.0
    mujoco.mj_forward(m,d)
    ctrl.reset(d.qpos)
    kj=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"left_knee")
    kq=m.jnt_qposadr[kj]; kd=m.jnt_dofadr[kj]
    hi = m.jnt_range[kj][1]
    tgt=list(DEFAULT); tgt[3]=target_knee
    ctrl.q_target[:]=np.array(tgt)
    rows=[]
    for k in range(int(seconds/0.02)):
        for _ in range(4):
            d.qvel[dof:dof+6]=0.0; d.qpos[q:q+7]=[0,0,0.125,1,0,0,0]
            ctrl.update(); mujoco.mj_step(m,d)
        # qpos cannot exceed the limit in MuJoCo (constraint), but the *constraint force*
        # and the effective impedance show in qfrc_constraint and the efc rows.
        rows.append((round((k+1)*0.02,2), d.qpos[kq], d.qvel[kd], d.qfrc_constraint[kd]))
    return hi, rows

if __name__=="__main__":
    for mode in ("ref","core"):
        hi,rows=run(mode)
        peak=max(rows, key=lambda r: abs(r[3]))
        print(f"== {mode}: knee limit {hi:.4f} rad, peak |qfrc_constraint|={abs(peak[3]):.4f} Nm at t={peak[0]}")
        for t,p,v,f in rows:
            if t<=0.3:
                print(f"   t={t:.2f} q={p:.5f} (limit-{hi-p:.2e}) v={v:+.3f} f={f:+.4f}")
