import numpy as np, mujoco, onnxruntime as ort, sys
from bam.model import load_model
from bam.mujoco import MujocoController

XML = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
ONNX = "D:/AI/Duck/policies/velstand.onnx"
DEFAULT = np.array([0.0,-0.0873,-0.4579,-0.0049,0.4530,0.3491,0.3491,0.0,0.0,0.0,0.0873,0.4579,0.0049,-0.4530])
WIRE = ["left_hip_yaw","left_hip_roll","left_hip_pitch","left_knee","left_ankle",
        "neck_pitch","head_pitch","head_yaw","head_roll",
        "right_hip_yaw","right_hip_roll","right_hip_pitch","right_knee","right_ankle"]

def build(mode, force_vin=7.4):
    """mode: 'core' -> stiffen jnt_solref (limit), 'ref' -> stiffen solref_friction, 'none'."""
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

def obs(m,d,last,c13):
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]; dof=m.jnt_dofadr[jid]
    R=np.zeros(9); mujoco.mju_quat2Mat(R,d.qpos[q+3:q+7]); R=R.reshape(3,3)
    grav=R.T@np.array([0,0,-1.0]); gyro=d.qvel[dof+3:dof+6].copy()
    pos=[]; vel=[]
    for n in WIRE:
        j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n); b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
        pos.append(d.qpos[m.jnt_qposadr[j]] + (d.qpos[m.jnt_qposadr[b]] if b>=0 else 0.0))
        vel.append(d.qvel[m.jnt_dofadr[j]] + (d.qvel[m.jnt_dofadr[b]] if b>=0 else 0.0))
    return np.concatenate([gyro,grav,np.array(pos)-DEFAULT,np.array(vel),last,c13]).astype(np.float32)

def run(mode, cmd, seconds=20.0, act_scale=0.9):
    m,d,ctrl=build(mode); reset(m,d)
    sess=ort.InferenceSession(ONNX); iname=sess.get_inputs()[0].name; oname=sess.get_outputs()[0].name
    last=np.zeros(14,dtype=np.float32); c13=np.zeros(13,dtype=np.float32); c13[0:3]=cmd
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]
    out=[]
    for k in range(int(seconds/0.02)):
        o=obs(m,d,last,c13)
        a=sess.run([oname],{iname:o.reshape(1,-1)})[0].squeeze(0).astype(np.float32); last=a
        ctrl.q_target[:]=DEFAULT+act_scale*a
        for _ in range(4):
            ctrl.update(); mujoco.mj_step(m,d)
        if (k+1)%250==0:
            out.append((round((k+1)*0.02,1), d.qpos[q:q+3].copy(), d.qpos[q+3:q+7].copy(), last.copy()))
    return out

if __name__=="__main__":
    cmd=[float(x) for x in sys.argv[1:4]]
    a=run("core",cmd); b=run("ref",cmd); c=run("none",cmd)
    print("cmd", cmd)
    print(f"{'t':>5} | {'core x,y,z':>26} | {'ref x,y,z':>26} | {'max|dact| core-ref':>18}")
    for (t1,p1,q1,a1),(t2,p2,q2,a2) in zip(a,b):
        print(f"{t1:5.1f} | {p1[0]:+7.3f} {p1[1]:+7.3f} {p1[2]:.3f} | {p2[0]:+7.3f} {p2[1]:+7.3f} {p2[2]:.3f} | {np.max(np.abs(a1-a2)):8.3f}")
    # divergence between core and none
    print("\ncore vs none (should be identical: neither stiffens friction):")
    for (t1,p1,_,a1),(t3,p3,_,a3) in zip(a,c):
        print(f"{t1:5.1f} dx={p1[0]-p3[0]:+.5f} dz={p1[2]-p3[2]:+.5f} dact={np.max(np.abs(a1-a3)):.4f}")
