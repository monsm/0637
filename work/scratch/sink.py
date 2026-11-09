"""Push the duck over on the app's scene and see where the trunk ends up."""
import numpy as np, mujoco, onnxruntime as ort
from bam.model import load_model
from bam.mujoco import MujocoController

XML="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
DEFAULT=np.array([0.0,-0.0873,-0.4579,-0.0049,0.4530,0.3491,0.3491,0.0,0.0,0.0,0.0873,0.4579,0.0049,-0.4530])
WIRE=["left_hip_yaw","left_hip_roll","left_hip_pitch","left_knee","left_ankle","neck_pitch","head_pitch","head_yaw","head_roll","right_hip_yaw","right_hip_roll","right_hip_pitch","right_knee","right_ankle"]
spec=mujoco.MjSpec.from_file(XML)
bam=load_model(motor_name="xl330",model="m6"); bam.actuator.kp=200.0; bam.actuator.vin=7.4; bam.actuator.max_current=None
flim=7.4*bam.kt.value/bam.R.value; names=[]
for act in spec.actuators:
    t=act.target; tn=t.name if hasattr(t,'name') else str(t)
    if tn.startswith("passive_"): continue
    act.set_to_motor(); act.forcelimited=True; act.forcerange=(-flim,flim); act.ctrllimited=False
    act.gear=[1.,0,0,0,0,0]; names.append(act.name)
    for j in spec.joints:
        if j.name==tn:
            j.armature=float(bam.actuator.get_extra_inertia()); j.damping=np.zeros((3,1)); j.frictionloss=0.0
            j.solref_friction=(-5e4,-2e2); j.solimp_friction=(0.99,0.9999,0.001,0.5,2.0); break
m=spec.compile(); m.opt.timestep=0.005; d=mujoco.MjData(m)
ctrl=MujocoController(bam,names,m,d,vin_drop_gain=0.1,vin_min=6.0)
mujoco.mj_resetData(m,d)
fj=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[fj]; dof=m.jnt_dofadr[fj]
d.qpos[q:q+7]=[0,0,0.125,1,0,0,0]
for i,n in enumerate(WIRE):
    j=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,n); d.qpos[m.jnt_qposadr[j]]=DEFAULT[i]
    b=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,f"passive_{n}_backlash")
    if b>=0: d.qpos[m.jnt_qposadr[b]]=0.0
mujoco.mj_forward(m,d)
ctrl.reset(d.qpos)
# Fall: limp (gain 50) and hold the current pose -- what the app does in limp-fall.
ctrl.model.actuator.kp=50.0
d.qvel[dof:dof+2]=[1.5,0.0]   # hard shove
ctrl.q_target[:]=DEFAULT
print(f"{'t':>5} {'trunk z':>8} {'tilt°':>6} {'min geom z':>10} {'min body z':>10}")
bid=[mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_BODY,n) for n in ["trunk_base","jaw_soft","neck_pitch","yaw_roll_motion"]]
for k in range(int(4.0/0.02)):
    ctrl.update(); mujoco.mj_step(m,d)
    if (k+1)%25==0:
        R=np.zeros(9); mujoco.mju_quat2Mat(R,d.qpos[q+3:q+7]); R=R.reshape(3,3)
        tilt=np.degrees(np.arccos(np.clip(R[2,2],-1,1)))
        gmin=min(d.geom_xpos[g][2] for g in range(m.ngeom) if m.geom_contype[g] or m.geom_conaffinity[g])
        bmin=min(d.xpos[b][2] for b in bid)
        print(f"{(k+1)*0.02:5.2f} {d.qpos[q+2]:8.4f} {tilt:6.1f} {gmin:10.4f} {bmin:10.4f}")
