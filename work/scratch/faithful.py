"""Faithful replication of the app's tick: read -> command EMA -> low-pass -> BAM write.

Mirrors core/duck-mobile-core/src/runtime.rs + the verbatim robotd control.rs,
against the bundled scene_backlash.xml. Measures ground speed and stopping.
"""
import numpy as np, mujoco, onnxruntime as ort, sys
from bam.model import load_model
from bam.mujoco import MujocoController

XML = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
ONNX = "D:/AI/Duck/policies/velstand.onnx"
DEFAULT = np.array([0.0,-0.0873,-0.4579,-0.0049,0.4530,0.3491,0.3491,0.0,0.0,0.0,0.0873,0.4579,0.0049,-0.4530])
WIRE = ["left_hip_yaw","left_hip_roll","left_hip_pitch","left_knee","left_ankle",
        "neck_pitch","head_pitch","head_yaw","head_roll",
        "right_hip_yaw","right_hip_roll","right_hip_pitch","right_knee","right_ankle"]
HEAD = range(5,9)
ACTION_SCALE = 0.9
HEAD_ALPHA = 0.5
LEGS_ALPHA = 0.7
CMD_ALPHA = 0.2

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

def sensors(m,d):
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
    R=np.zeros(9); mujoco.mju_quat2Mat(R,d.qpos[q+3:q+7]); R=R.reshape(3,3)
    grav=R.T@np.array([0,0,-1.0]); gyro=d.qvel[dof+3:dof+6].copy()
    pos=[]; vel=[]
    for n in WIRE:
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n); b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        pos.append(d.qpos[m.jnt_qposadr[j]] + (d.qpos[m.jnt_qposadr[b]] if b>=0 else 0.0))
        vel.append(d.qvel[m.jnt_dofadr[j]] + (d.qvel[m.jnt_dofadr[b]] if b>=0 else 0.0))
    return gyro, grav, np.array(pos), np.array(vel), R

def run(mode, script, seconds=12.0):
    """script(t) -> twist target [vx,vy,wz]."""
    m,d,ctrl=build(mode); reset(m,d)
    sess=ort.InferenceSession(ONNX); iname=sess.get_inputs()[0].name; oname=sess.get_outputs()[0].name
    last=np.zeros(14,dtype=np.float32)
    ema=np.zeros(3); prev_targets=None
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]
    rows=[]
    for k in range(int(seconds/0.02)):
        t=k*0.02
        gyro,grav,pos,vel,R=sensors(m,d)
        target=np.array(script(t),dtype=float)
        ema += CMD_ALPHA*(target-ema)
        c13=np.zeros(13,dtype=np.float32); c13[0:3]=ema
        o=np.concatenate([gyro,grav,pos-DEFAULT,vel,last,c13]).astype(np.float32)
        a=sess.run([oname],{iname:o.reshape(1,-1)})[0].squeeze(0).astype(np.float32)
        last=a
        targets=DEFAULT+ACTION_SCALE*a
        if prev_targets is not None:
            for j in HEAD:
                targets[j]=HEAD_ALPHA*targets[j]+(1-HEAD_ALPHA)*prev_targets[j]
            for j in range(14):
                if j in HEAD: continue
                targets[j]=LEGS_ALPHA*targets[j]+(1-LEGS_ALPHA)*prev_targets[j]
        prev_targets=targets
        ctrl.q_target[:]=targets
        for _ in range(4):
            ctrl.update(); mujoco.mj_step(m,d)
        if (k+1)%25==0:
            tilt=float(np.degrees(np.arccos(np.clip(R[2,2],-1,1))))
            rows.append((round((k+1)*0.02,2), d.qpos[q:q+3].copy(), tilt, ema.copy()))
    return rows

def report(rows, label):
    print(f"== {label}")
    start=rows[0][1][:2].copy()
    for t,pos,tilt,ema in rows:
        if abs(t-round(t))<1e-6:
            print(f"  t={t:5.1f}s  x={pos[0]-start[0]:+.3f} y={pos[1]-start[1]:+.3f} z={pos[2]:.3f} tilt={tilt:5.1f}° cmd={np.round(ema,2)}")

if __name__=="__main__":
    mode=sys.argv[1] if len(sys.argv)>1 else "core"
    # 0-4 s: walk forward 0.3; 4-6: coast 0.2; 6-8: stop; 8-12: still
    def script(t):
        if t<4.0: return [0.3,0.0,0.0]
        if t<6.0: return [0.15,0.0,0.0]
        return [0.0,0.0,0.0]
    rows=run(mode, script, seconds=12.0)
    report(rows, f"{mode}: 0.3 m/s then stop")
    # speed over the steady window
    seg=[r for r in rows if 3.0<=r[0]<=4.0]
    if len(seg)>=2:
        dist=seg[-1][1][0]-seg[0][1][0]
        print(f"  steady speed 3-4s: {dist/(seg[-1][0]-seg[0][0]):+.3f} m/s (cmd 0.30)")
    seg=[r for r in rows if 10.0<=r[0]<=12.0]
    if len(seg)>=2:
        dist=seg[-1][1][0]-seg[0][1][0]
        print(f"  speed after stop 10-12s: {dist/(seg[-1][0]-seg[0][0]):+.3f} m/s")
