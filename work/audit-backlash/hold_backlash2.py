import sys
import numpy as np
import mujoco
sys.path.insert(0, r"D:/AI/Duck/upstream/bam")
from bam.model import load_model
from bam.mujoco import MujocoController

xml = sys.argv[1]
HOME = np.array([0.0,-0.0873,-0.4579,-0.0049,0.4530,0.3491,0.3491,0.0,0.0,0.0,0.0873,0.4579,0.0049,-0.4530])
spec = mujoco.MjSpec.from_file(xml)
names = []
for act in spec.actuators:
    tgt = act.target
    tgt_name = tgt.name if hasattr(tgt, "name") else str(tgt)
    if tgt_name.startswith("passive_"):
        continue
    act.set_to_motor(); act.forcelimited = True; act.forcerange = (-0.55, 0.55)
    act.ctrllimited = False; act.gear = [1.0,0,0,0,0,0]
    names.append(act.name)
    for joint in spec.joints:
        if joint.name == tgt_name:
            joint.damping = np.zeros((3,1)); joint.frictionloss = 0.0
            joint.solref_friction = [-5.0e4,-2.0e2]; joint.solimp_friction = [0.99,0.9999,0.001,0.5,2.0]
            break
model = spec.compile(); model.opt.timestep = 0.005
data = mujoco.MjData(model)
bam_model = load_model(motor_name="xl330", model="m6")
bam_model.actuator.kp = 200.0; bam_model.actuator.vin = 7.4; bam_model.actuator.max_current = None
ctrl = MujocoController(bam_model, names, model, data, vin_drop_gain=0.1, vin_min=6.0)
trunk = int(mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_JOINT, "trunk_base_freejoint"))
tqpos, tdof = int(model.jnt_qposadr[trunk]), int(model.jnt_dofadr[trunk])
act_joints = [int(model.actuator_trnid[a,0]) for a in range(model.nu)]
qadr = [int(model.jnt_qposadr[j]) for j in act_joints]
passive = [mujoco.mj_name2id(model, mujoco.mjtObj.mjOBJ_JOINT, f"passive_{n}_backlash") for n in names]
padr = [int(model.jnt_qposadr[j]) for j in passive]
pdof = [int(model.jnt_dofadr[j]) for j in passive]

data.qpos[:] = 0
data.qpos[tqpos:tqpos+3] = [0,0,0.125]; data.qpos[tqpos+3:tqpos+7] = [1,0,0,0]
data.qpos[qadr] = HOME
mujoco.mj_forward(model, data)
print("qfrc_bias on passive DOFs at home:", np.round(data.qfrc_bias[pdof], 6).tolist())
print("qfrc_bias on servo DOFs at home:  ", np.round(data.qfrc_bias[[int(model.jnt_dofadr[j]) for j in act_joints]], 6).tolist())
ctrl.reset(data.qpos); ctrl.q_target[:] = data.qpos[qadr]
for k in range(200):
    data.qpos[tqpos:tqpos+3] = [0,0,0.125]; data.qpos[tqpos+3:tqpos+7] = [1,0,0,0]
    data.qvel[tdof:tdof+6] = 0.0
    ctrl.update(); mujoco.mj_step(model, data)
    if k in (0, 10, 50, 199):
        print(k, "passive deg:", np.round(np.degrees(data.qpos[padr]), 4).tolist())
