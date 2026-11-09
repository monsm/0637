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

def reset(m,d):
    mujoco.mj_resetData(m,d)
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]
    d.qpos[q:q+7]=[0,0,0.125,1,0,0,0]
    for i,n in enumerate(WIRE):
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n); d.qpos[m.jnt_qposadr[j]]=DEFAULT[i]
        b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        if b>=0: d.qpos[m.jnt_qposadr[b]]=0.0
    mujoco.mj_forward(m,d)

def hold(mode, seconds=20.0):
    m,d,ctrl=build(mode); reset(m,d)
    ctrl.q_target[:]=DEFAULT
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]
    rows=[]
    for k in range(int(seconds/0.02)):
        for _ in range(4):
            ctrl.update(); mujoco.mj_step(m,d)
        if (k+1)%250==0:
            pos=[]; bl=[]
            for n in WIRE:
                j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n)
                b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
                pos.append(d.qpos[m.jnt_qposadr[j]]); bl.append(d.qpos[m.jnt_qposadr[b]] if b>=0 else 0.0)
            rows.append((round((k+1)*0.02,1), np.array(pos)-DEFAULT, np.array(bl), d.qpos[q:q+3].copy()))
    return rows

if __name__=="__main__":
    for mode in ("ref","core"):
        rows=hold(mode)
        print("==", mode)
        for t,err,bl,trunk in rows:
            print(f" t={t:5.1f} trunk={np.round(trunk,3)} maxjoint_err={np.degrees(np.abs(err)).max():6.3f}deg "
                  f"hip_pitchL={np.degrees(err[2]):+7.3f} kneeL={np.degrees(err[3]):+7.3f} neck={np.degrees(err[5]):+7.3f} "
                  f"backlash_max={np.degrees(np.abs(bl)).max():.4f}deg")
