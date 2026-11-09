import math, sys, numpy as np, mujoco, onnxruntime as ort
from bam.model import load_model
from bam.mujoco import MujocoController

XML = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
ONNX = "D:/AI/Duck/policies/velstand.onnx"
DEFAULT = np.array([0.0,-0.0873,-0.4579,-0.0049,0.4530,0.3491,0.3491,0.0,0.0,0.0,0.0873,0.4579,0.0049,-0.4530])
WIRE = ["left_hip_yaw","left_hip_roll","left_hip_pitch","left_knee","left_ankle",
        "neck_pitch","head_pitch","head_yaw","head_roll",
        "right_hip_yaw","right_hip_roll","right_hip_pitch","right_knee","right_ankle"]

def build(stiff_friction, stiff_limit, force_vin=8.2):
    spec = mujoco.MjSpec.from_file(XML)
    bam = load_model(motor_name="xl330", model="m6")
    bam.actuator.kp = 200.0; bam.actuator.vin = 7.4; bam.actuator.max_current = None
    kt, R = bam.kt.value, bam.R.value
    flim = force_vin*kt/R
    names=[]
    for act in spec.actuators:
        tgt = act.target
        tn = tgt.name if hasattr(tgt,'name') else str(tgt)
        if tn.startswith("passive_"): continue
        act.set_to_motor(); act.forcelimited=True; act.forcerange=(-flim,flim); act.ctrllimited=False
        act.gear=[1.0,0,0,0,0,0]; names.append(act.name)
        for j in spec.joints:
            if j.name==tn:
                j.armature=float(bam.actuator.get_extra_inertia())
                j.damping=np.zeros((3,1)); j.frictionloss=0.0
                if stiff_friction:
                    j.solref_friction=(-5.0e4,-2.0e2); j.solimp_friction=(0.99,0.9999,0.001,0.5,2.0)
                if stiff_limit:
                    j.solref_limit=(-5.0e4,-2.0e2); j.solimp_limit=(0.99,0.9999,0.001,0.5,2.0)
                break
    m=spec.compile(); m.opt.timestep=0.005
    d=mujoco.MjData(m)
    ctrl=MujocoController(bam,names,m,d,vin_drop_gain=0.1,vin_min=6.0)
    return m,d,ctrl

def reset(m,d):
    mujoco.mj_resetData(m,d)
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint")
    q=m.jnt_qposadr[jid]
    d.qpos[q:q+7]=[0,0,0.125,1,0,0,0]
    for i,n in enumerate(WIRE):
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n)
        d.qpos[m.jnt_qposadr[j]]=DEFAULT[i]
        b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        if b>=0: d.qpos[m.jnt_qposadr[b]]=0.0
    mujoco.mj_forward(m,d)

def obs(m,d,last,c13):
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint")
    q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
    quat=d.qpos[q+3:q+7].copy()
    R=np.zeros(9); mujoco.mju_quat2Mat(R,quat); R=R.reshape(3,3)
    grav=R.T@np.array([0,0,-1.0])
    gyro=d.qvel[dof+3:dof+6].copy()
    pos=[]; vel=[]
    for n in WIRE:
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n)
        b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        p=d.qpos[m.jnt_qposadr[j]] + (d.qpos[m.jnt_qposadr[b]] if b>=0 else 0.0)
        v=d.qvel[m.jnt_dofadr[j]] + (d.qvel[m.jnt_dofadr[b]] if b>=0 else 0.0)
        pos.append(p); vel.append(v)
    return np.concatenate([gyro,grav,np.array(pos)-DEFAULT,np.array(vel),last,c13]).astype(np.float32)

def run(stiff_friction, stiff_limit, cmd, seconds=6.0, act_scale=0.9, report_every=1.0):
    m,d,ctrl=build(stiff_friction,stiff_limit); reset(m,d)
    sess=ort.InferenceSession(ONNX); iname=sess.get_inputs()[0].name; oname=sess.get_outputs()[0].name
    last=np.zeros(14,dtype=np.float32)
    c13=np.zeros(13,dtype=np.float32); c13[0:3]=cmd
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint")
    q=m.jnt_qposadr[jid]
    steps=int(seconds/0.02)
    out=[]
    for k in range(steps):
        o=obs(m,d,last,c13)
        a=sess.run([oname],{iname:o.reshape(1,-1)})[0].squeeze(0).astype(np.float32)
        last=a
        ctrl.q_target[:]=DEFAULT+act_scale*a
        for _ in range(4):
            ctrl.update(); mujoco.mj_step(m,d)
        if (k+1) % int(report_every/0.02) == 0:
            R=np.zeros(9); mujoco.mju_quat2Mat(R,d.qpos[q+3:q+7]); R=R.reshape(3,3)
            fwd=R[:,0]
            xy=d.qpos[q:q+2]
            out.append(( (k+1)*0.02, xy.copy(), d.qpos[q+2], float(np.degrees(np.arccos(np.clip(fwd[2],-1,1)))) ))
    return m,d,out

if __name__ == "__main__":
    cmd = [float(x) for x in sys.argv[1:4]] if len(sys.argv)>3 else [0.3,0.0,0.0]
    for label, sf, sl in [("ref(stiff friction+limit)", True, True), ("core(stiff friction only)", True, False)]:
        m,d,rows=run(sf, sl, cmd, seconds=6.0)
        print("==", label, "cmd", cmd)
        start=None
        for t,xy,z,tilt in rows:
            if start is None: start=xy.copy()
            print(f"  t={t:4.1f}s  x={xy[0]-start[0]:+.3f} y={xy[1]-start[1]:+.3f} z={z:.3f} tilt={tilt:5.1f}deg")
