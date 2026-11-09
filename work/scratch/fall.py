"""Push the duck over and watch it lie / recover, on the two candidate models.
The app loads scene_backlash (groundcontact). VelStand trained on allcollisions."""
import numpy as np, mujoco, onnxruntime as ort
from bam.model import load_model
from bam.mujoco import MujocoController

SCENES = {
  "groundcontact (app)": "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml",
  "allcollisions (train)": "D:/AI/Duck/work/scratch/scene_allcollisions_backlash.xml",
}
ONNX = "D:/AI/Duck/policies/velstand.onnx"
DEFAULT = np.array([0.0,-0.0873,-0.4579,-0.0049,0.4530,0.3491,0.3491,0.0,0.0,0.0,0.0873,0.4579,0.0049,-0.4530])
WIRE = ["left_hip_yaw","left_hip_roll","left_hip_pitch","left_knee","left_ankle",
        "neck_pitch","head_pitch","head_yaw","head_roll",
        "right_hip_yaw","right_hip_roll","right_hip_pitch","right_knee","right_ankle"]
HEAD=range(5,9); ACTION_SCALE=0.9; HEAD_ALPHA=0.5; LEGS_ALPHA=0.7; CMD_ALPHA=0.2

def build(xml, mode="ref"):
    spec = mujoco.MjSpec.from_file(xml)
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
                j.solref_friction=(-5.0e4,-2.0e2); j.solimp_friction=(0.99,0.9999,0.001,0.5,2.0)
                break
    m=spec.compile(); m.opt.timestep=0.005
    d=mujoco.MjData(m)
    return m,d,MujocoController(bam,names,m,d,vin_drop_gain=0.1,vin_min=6.0)

def reset(m,d):
    mujoco.mj_resetData(m,d)
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]
    d.qpos[q:q+7]=[0,0,0.125,1,0,0,0]
    for i,n in enumerate(WIRE):
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n); d.qpos[m.jnt_qposadr[j]]=DEFAULT[i]
        b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        if b>=0: d.qpos[m.jnt_qposadr[b]]=0.0
    mujoco.mj_forward(m,d)

def sensors(m,d):
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
    R=np.zeros(9); mujoco.mju_quat2Mat(R,d.qpos[q+3:q+7]); R=R.reshape(3,3)
    grav=R.T@np.array([0,0,-1.0]); gyro=d.qvel[dof+3:dof+6].copy()
    pos=[]; vel=[]
    for n in WIRE:
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n); b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        pos.append(d.qpos[m.jnt_qposadr[j]] + (d.qpos[m.jnt_qposadr[b]] if b>=0 else 0.0))
        vel.append(d.qvel[m.jnt_dofadr[j]] + (d.qvel[m.jnt_dofadr[b]] if b>=0 else 0.0))
    return gyro,grav,np.array(pos),np.array(vel),R

def run(xml, seconds=14.0):
    m,d,ctrl=build(xml); reset(m,d)
    sess=ort.InferenceSession(ONNX); iname=sess.get_inputs()[0].name; oname=sess.get_outputs()[0].name
    last=np.zeros(14,dtype=np.float32); prev=None
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
    log=[]
    for k in range(int(seconds/0.02)):
        gyro,grav,pos,vel,R=sensors(m,d)
        c13=np.zeros(13,dtype=np.float32)
        o=np.concatenate([gyro,grav,pos-DEFAULT,vel,last,c13]).astype(np.float32)
        a=sess.run([oname],{iname:o.reshape(1,-1)})[0].squeeze(0).astype(np.float32); last=a
        tg=DEFAULT+ACTION_SCALE*a
        if prev is not None:
            for j in HEAD: tg[j]=HEAD_ALPHA*tg[j]+(1-HEAD_ALPHA)*prev[j]
            for j in range(14):
                if j in HEAD: continue
                tg[j]=LEGS_ALPHA*tg[j]+(1-LEGS_ALPHA)*prev[j]
        prev=tg; ctrl.q_target[:]=tg
        # shove at t=3s: a strong lateral trunk velocity (robotd push / a hand)
        if k==150:
            d.qvel[dof+0]+=1.2
        for _ in range(4): ctrl.update(); mujoco.mj_step(m,d)
        if (k+1)%25==0:
            # lowest point of any trunk/head/leg geom (penetration check)
            gmin=min(d.geom_xpos[g][2] for g in range(m.ngeom)
                     if (m.geom_contype[g] or m.geom_conaffinity[g]))
            log.append((round((k+1)*0.02,1), d.qpos[q+2], float(np.degrees(np.arccos(np.clip(R[2,2],-1,1)))), gmin,
                        int(m.geom_bodyid[0]), m.ngeom))
    return log, m

if __name__=="__main__":
    for label,xml in SCENES.items():
        log,m=run(xml)
        print(f"== {label}  (colliding geoms={sum(1 for g in range(m.ngeom) if m.geom_contype[g] or m.geom_conaffinity[g])})")
        for t,z,tilt,gmin,_,_ in log:
            if t>=2.5:
                print(f"   t={t:5.1f} trunk_z={z:6.3f} tilt={tilt:5.1f}° lowest_geom_z={gmin:+.4f}")
