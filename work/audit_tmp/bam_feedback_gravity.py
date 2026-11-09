import json, sys
import numpy as np
import mujoco

scene, params_path, mode = sys.argv[1], sys.argv[2], sys.argv[3]
P = json.load(open(params_path))
m = mujoco.MjModel.from_xml_path(scene)
d = mujoco.MjData(m)
m.opt.timestep = 0.005

kt, R = P["kt"], P["R"]
armature = P["armature"]
vin, vin_drop, vin_min, kp = 7.4, 0.1, 6.0, 200.0
error_gain = (4096.0 / (2.0*np.pi)) / (256.0*885.0)
force_limit = vin*kt/R
f_base, f_strib = P["friction_base"], P["friction_stribeck"]
l_motor, l_ext = P["load_friction_motor"], P["load_friction_external"]
l_motor_s, l_ext_s = P["load_friction_motor_stribeck"], P["load_friction_external_stribeck"]
l_motor_q, l_ext_q = P["load_friction_motor_quad"], P["load_friction_external_quad"]
dtheta, alpha, visc = P["dtheta_stribeck"], P["alpha"], P["friction_viscous"]

act_joint = [int(m.actuator_trnid[a,0]) for a in range(m.nu)]
servo_q = [int(m.jnt_qposadr[j]) for j in act_joint]
servo_dof = [int(m.jnt_dofadr[j]) for j in act_joint]
bl_q = []
for j in act_joint:
    name = mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_JOINT, j)
    b = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_JOINT, f"passive_{name}_backlash")
    bl_q.append(int(m.jnt_qposadr[b]) if b >= 0 else -1)

for a in range(m.nu):
    m.actuator_gaintype[a] = int(mujoco.mjtGain.mjGAIN_FIXED)
    m.actuator_biastype[a] = int(mujoco.mjtBias.mjBIAS_NONE)
    m.actuator_dyntype[a] = int(mujoco.mjtDyn.mjDYN_NONE)
    m.actuator_gainprm[a,:] = 0.0; m.actuator_biasprm[a,:] = 0.0; m.actuator_gear[a,:] = 0.0
    m.actuator_gainprm[a,0] = 1.0; m.actuator_gear[a,0] = 1.0
    m.actuator_ctrllimited[a] = 0
    m.actuator_forcelimited[a] = 1
    m.actuator_forcerange[a] = (-force_limit, force_limit)
    dof = servo_dof[a]
    m.dof_armature[dof] = armature
    m.dof_damping[dof] = 0.0
    m.dof_frictionloss[dof] = 0.0

key = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_KEY, "STAND")
mujoco.mj_resetDataKeyframe(m, d, key)
mujoco.mj_forward(m, d)

q_target = np.array([d.qpos[servo_q[a]] for a in range(m.nu)])
prev_torque = np.zeros(m.nu)

def friction_budget(motor, external, dq):
    gearbox = abs(external*l_ext - motor*l_motor)
    gearbox_s = abs(external*l_ext_s - motor*l_motor_s)
    strib = np.exp(-((abs(dq)/dtheta)**alpha))
    fl = f_base + gearbox + strib*f_strib + gearbox_s*strib
    se, sm = np.sign(external), np.sign(motor)
    enable = se != sm
    dir_m = abs(external) < abs(motor)
    dir_e = abs(external) > abs(motor)
    g2m = l_ext_q*abs(external)**2
    g2e = l_motor_q*abs(motor)**2
    if enable:
        fl += strib*((g2m if dir_m else 0.0) + (g2e if dir_e else 0.0))
    return fl, visc

def friction_dof_force(dof):
    s = 0.0
    for i in range(d.nefc):
        if d.efc_id[i] == dof and d.efc_type[i] == int(mujoco.mjtConstraint.mjCNSTR_FRICTION_DOF):
            s += d.efc_force[i]
    return s

for tick in range(100):  # 2 s
    for sub in range(4):
        vin_eff = max(vin - vin_drop*np.abs(prev_torque).sum(), vin_min)
        torques = np.zeros(m.nu)
        for a in range(m.nu):
            qs = d.qpos[servo_q[a]]
            q_used = qs + (d.qpos[bl_q[a]] if (mode == "encoder" and bl_q[a] >= 0) else 0.0)
            dq = d.qvel[servo_dof[a]]
            duty = np.clip((q_target[a]-q_used)*kp*error_gain, -1.0, 1.0)
            volts = vin_eff*duty
            torque = kt*volts/R - kt*kt*dq/R
            torques[a] = torque
            dof = servo_dof[a]
            external = -d.qfrc_bias[dof] + d.qfrc_constraint[dof] - friction_dof_force(dof)
            motor = d.qfrc_actuator[dof]
            fl, damp = friction_budget(motor, external, dq)
            m.dof_frictionloss[dof] = fl
            m.dof_damping[dof] = damp
            d.ctrl[a] = torque
        mujoco.mj_step(m, d)
        prev_torque = torques

print(f"mode={mode}  trunk_z={d.qpos[2]:.4f}  ncon={d.ncon}")
print("joint                servo_q    backlash    link      target    link-target")
for a in range(m.nu):
    j = act_joint[a]
    name = mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_JOINT, j)
    qs, qb = d.qpos[servo_q[a]], d.qpos[bl_q[a]]
    print(f"{name:20s} {qs: .5f}  {qb: .5f}  {qs+qb: .5f}  {q_target[a]: .5f}  {qs+qb-q_target[a]: .5f}")
