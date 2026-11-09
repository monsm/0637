import numpy as np, mujoco
from bam.model import load_model
from bam.mujoco import MujocoController

XML = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
DEFAULT = np.array([0.0,-0.0873,-0.4579,-0.0049,0.4530,0.3491,0.3491,0.0,0.0,0.0,0.0873,0.4579,0.0049,-0.4530])
WIRE = ["left_hip_yaw","left_hip_roll","left_hip_pitch","left_knee","left_ankle",
        "neck_pitch","head_pitch","head_yaw","head_roll",
        "right_hip_yaw","right_hip_roll","right_hip_pitch","right_knee","right_ankle"]

def build(friction_on_friction, limit_on_friction, force_vin=8.2):
    """friction_on_friction: reference behaviour (stiffen dof_solref).
       limit_on_friction: the core's behaviour (stiffen jnt_solref)."""
    spec = mujoco.MjSpec.from_file(XML)
    bam = load_model(motor_name="xl330", model="m6")
    bam.actuator.kp=200.0; bam.actuator.vin=7.4; bam.actuator.max_current=None
    flim = force_vin*bam.kt.value/bam.R.value
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
                if friction_on_friction:
                    j.solref_friction=(-5.0e4,-2.0e2); j.solimp_friction=(0.99,0.9999,0.001,0.5,2.0)
                if limit_on_friction:
                    j.solref_limit=(-5.0e4,-2.0e2); j.solimp_limit=(0.99,0.9999,0.001,0.5,2.0)
                break
    m=spec.compile(); m.opt.timestep=0.005
    d=mujoco.MjData(m)
    ctrl=MujocoController(bam,names,m,d,vin_drop_gain=0.1,vin_min=6.0)
    return m,d,ctrl

def hold(friction_on_friction, limit_on_friction, seconds=6.0, pose=None, target=None):
    m,d,ctrl=build(friction_on_friction, limit_on_friction)
    mujoco.mj_resetData(m,d)
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint")
    q=m.jnt_qposadr[jid]
    d.qpos[q:q+7]=[0,0,0.125,1,0,0,0]
    pose = DEFAULT if pose is None else pose
    for i,n in enumerate(WIRE):
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n)
        d.qpos[m.jnt_qposadr[j]]=pose[i]
        b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        if b>=0: d.qpos[m.jnt_qposadr[b]]=0.0
    mujoco.mj_forward(m,d)
    ctrl.reset(d.qpos)
    target = pose if target is None else target
    ctrl.q_target[:]=target
    for k in range(int(seconds/0.02)):
        for _ in range(4):
            ctrl.update(); mujoco.mj_step(m,d)
    err=[]
    for i,n in enumerate(WIRE):
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n)
        b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        p=d.qpos[m.jnt_qposadr[j]] + (d.qpos[m.jnt_qposadr[b]] if b>=0 else 0.0)
        err.append(p-target[i])
    R=np.zeros(9); mujoco.mju_quat2Mat(R,d.qpos[q+3:q+7])
    tilt=float(np.degrees(np.arccos(np.clip(R.reshape(3,3)[2,2],-1,1))))
    return np.array(err), d.qpos[q+2], tilt

if __name__=="__main__":
    for label, ff, ll in [("ref: stiffen friction(dof_solref)", True, False),
                          ("core: stiffen limit(jnt_solref)", False, True),
                          ("neither", False, False)]:
        err,z,tilt = hold(ff,ll, seconds=6.0)
        print(f"== {label}: trunk z={z:.4f} tilt={tilt:.1f}deg")
        for i,n in enumerate(WIRE):
            print(f"   {n:16s} err={np.degrees(err[i]):+7.3f} deg")
