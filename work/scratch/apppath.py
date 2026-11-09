"""The app's exact BAM write path, in Python: the shim's external torque is
-dof_bias + dof_constraint - (efc_force where efc_id == the DOF), matching the
GPU training path. Compare with bam.mujoco.MujocoController (CPU rehearsal),
whose efc scan compares efc_id (a DOF) against a JOINT index and so never matches.
"""
import numpy as np, mujoco, onnxruntime as ort
from bam.model import load_model

XML = "D:/AI/Duck/work/scratch/scene_allcollisions_backlash.xml"
ONNX = "D:/AI/Duck/policies/velstand.onnx"
DEFAULT = np.array([0.0,-0.0873,-0.4579,-0.0049,0.4530,0.3491,0.3491,0.0,0.0,0.0,0.0873,0.4579,0.0049,-0.4530])
WIRE = ["left_hip_yaw","left_hip_roll","left_hip_pitch","left_knee","left_ankle",
        "neck_pitch","head_pitch","head_yaw","head_roll",
        "right_hip_yaw","right_hip_roll","right_hip_pitch","right_knee","right_ankle"]
HEAD=range(5,9); ACTION_SCALE=0.9; HEAD_ALPHA=0.5; LEGS_ALPHA=0.7; CMD_ALPHA=0.2

class AppController:
    """Mirror of core/duck-mobile-core/src/mujoco.rs write_bam_inner + bam.rs."""
    def __init__(self, m, d, names, bam):
        self.m, self.d, self.names, self.bam = m, d, names, bam
        self.act_idx = [mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_ACTUATOR, n) for n in names]
        self.joint = [m.actuator_trnid[a][0] for a in self.act_idx]
        self.dof = [m.jnt_dofadr[j] for j in self.joint]
        self.qposadr = [m.jnt_qposadr[j] for j in self.joint]
        self.bl_qpos = []
        for n in names:
            b = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, f"passive_{n}_backlash")
            self.bl_qpos.append(-1 if b < 0 else m.jnt_qposadr[b])
        self.q_target = DEFAULT.copy()
        self.prev_torque = np.zeros(len(names))
        self.torque_on = True
        self.kp = 200.0
        self.vin = 7.4; self.vin_drop = 0.1; self.vin_min = 6.0
        self.error_gain = (4096.0/(2*np.pi))/(256.0*885.0)
        self.force_limit = self.vin*bam.kt.value/bam.R.value

    def step(self, n=4):
        m, d, bam = self.m, self.d, self.bam
        for _ in range(n):
            load = np.abs(self.prev_torque).sum()
            vin_eff = max(self.vin - self.vin_drop*load, self.vin_min)
            torques = np.zeros(len(self.names))
            for i in range(len(self.names)):
                q = d.qpos[self.qposadr[i]] + (d.qpos[self.bl_qpos[i]] if self.bl_qpos[i] >= 0 else 0.0)
                dq = d.qvel[self.dof[i]]
                duty = np.clip((self.q_target[i]-q)*self.kp*self.error_gain, -1, 1)
                volts = vin_eff*duty
                torques[i] = (bam.kt.value*volts/bam.R.value - bam.kt.value**2*dq/bam.R.value) if self.torque_on else 0.0
                dof = self.dof[i]
                # the app's duck_mj_external_torque: efc_id == DOF
                fric = 0.0
                for e in range(d.nefc):
                    if d.efc_id[e] == dof and d.efc_type[e] == mujoco.mjtConstraint.mjCNSTR_FRICTION_DOF:
                        fric += d.efc_force[e]
                external = -d.qfrc_bias[dof] + d.qfrc_constraint[dof] - fric
                motor = d.qfrc_actuator[dof]
                fl = self._frictions(motor, external, dq)
                d.ctrl[self.act_idx[i]] = torques[i]
                m.dof_frictionloss[dof] = fl[0]; m.dof_damping[dof] = fl[1]
            mujoco.mj_step(m, d)
            self.prev_torque = torques.copy()

    def _frictions(self, motor, external, dq):
        b = self.bam
        gear = abs(external*b.load_friction_external.value - motor*b.load_friction_motor.value)
        gear_s = abs(external*b.load_friction_external_stribeck.value - motor*b.load_friction_motor_stribeck.value)
        strib = np.exp(-((abs(dq)/b.dtheta_stribeck.value)**b.alpha.value))
        fl = b.friction_base.value + gear + strib*b.friction_stribeck.value + gear_s*strib
        enable = np.sign(external) != np.sign(motor)
        dir_motor = abs(external) < abs(motor)
        dir_ext = abs(external) > abs(motor)
        g2m = b.load_friction_external_quad.value*abs(external)**2
        g2e = b.load_friction_motor_quad.value*abs(motor)**2
        if enable:
            fl += strib*((g2m if dir_motor else 0.0)+(g2e if dir_ext else 0.0))
        return fl, b.friction_viscous.value

def build():
    spec = mujoco.MjSpec.from_file(XML)
    bam = load_model(motor_name="xl330", model="m6")
    bam.actuator.kp=200.0; bam.actuator.vin=7.4; bam.actuator.max_current=None
    flim = 7.4*bam.kt.value/bam.R.value
    names=[]
    for act in spec.actuators:
        t=act.target; tn=t.name if hasattr(t,'name') else str(t)
        if tn.startswith("passive_"): continue
        act.set_to_motor(); act.forcelimited=True; act.forcerange=(-flim,flim); act.ctrllimited=False
        act.gear=[1.,0,0,0,0,0]; names.append(act.name)
        for j in spec.joints:
            if j.name==tn:
                j.armature=float(bam.actuator.get_extra_inertia()); j.damping=np.zeros((3,1)); j.frictionloss=0.0
                # the app's shim: stiffens jnt_solref (limit), NOT dof_solref (friction)
                j.solref_limit=(-5e4,-2e2); j.solimp_limit=(0.99,0.9999,0.001,0.5,2.0)
                break
    m=spec.compile(); m.opt.timestep=0.005; d=mujoco.MjData(m)
    return m,d,names,bam

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

def run(vx, seconds=8.0):
    m,d,names,bam=build(); reset(m,d); ctrl=AppController(m,d,names,bam)
    sess=ort.InferenceSession(ONNX); iname=sess.get_inputs()[0].name; oname=sess.get_outputs()[0].name
    last=np.zeros(14,dtype=np.float32); ema=np.zeros(3); prev=None
    jid=mujoco.mj_name2id(m,mujoco.mjtObj.mjOBJ_JOINT,"trunk_base_freejoint"); q=m.jnt_qposadr[jid]
    start=None
    for k in range(int(seconds/0.02)):
        gyro,grav,pos,vel,R=sensors(m,d)
        ema += CMD_ALPHA*(np.array([vx,0,0])-ema)
        c13=np.zeros(13,dtype=np.float32); c13[0:3]=ema
        o=np.concatenate([gyro,grav,pos-DEFAULT,vel,last,c13]).astype(np.float32)
        a=sess.run([oname],{iname:o.reshape(1,-1)})[0].squeeze(0).astype(np.float32); last=a
        tg=DEFAULT+ACTION_SCALE*a
        if prev is not None:
            for j in HEAD: tg[j]=HEAD_ALPHA*tg[j]+(1-HEAD_ALPHA)*prev[j]
            for j in range(14):
                if j in HEAD: continue
                tg[j]=LEGS_ALPHA*tg[j]+(1-LEGS_ALPHA)*prev[j]
        prev=tg; ctrl.q_target=tg.copy(); ctrl.step(4)
        if start is None: start=d.qpos[q:q+2].copy()
    return d.qpos[q:q+2]-start, float(np.degrees(np.arccos(np.clip(R[2,2],-1,1))))

if __name__=="__main__":
    for vx in (0.15, 0.3):
        dist,tilt = run(vx)
        print(f"vx={vx:.2f} -> moved {np.linalg.norm(dist):.3f} m in 8 s = {np.linalg.norm(dist)/8:.3f} m/s "
              f"({100*np.linalg.norm(dist)/8/vx:.0f}% of asked), final tilt {tilt:.1f}°")
