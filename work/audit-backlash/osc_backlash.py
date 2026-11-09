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

data.qpos[:] = 0
data.qpos[tqpos:tqpos+3] = [0,0,0.125]; data.qpos[tqpos+3:tqpos+7] = [1,0,0,0]
data.qpos[qadr] = HOME
mujoco.mj_forward(model, data)
ctrl.reset(data.qpos)
# Oscillate one joint target (left knee) while holding the rest, trunk fixed in the air.
data.qpos[tqpos+2] = 0.25  # lift the duck off the floor so contacts do not dominate
maxplay = np.zeros(model.nu)
crossings = np.zeros(model.nu, dtype=int)
prev = None
for k in range(800):
    t = k * 0.005
    data.qpos[tqpos:tqpos+3] = [0,0,0.25]; data.qpos[tqpos+3:tqpos+7] = [1,0,0,0]
    data.qvel[tdof:tdof+6] = 0.0
    ctrl.q_target[:] = data.qpos[qadr]
    ctrl.q_target[3] += 0.15 * np.sin(2*np.pi*2.0*t)
    ctrl.update(); mujoco.mj_step(model, data)
    play = data.qpos[padr].copy()
    maxplay = np.maximum(maxplay, np.abs(play))
    if prev is not None:
        crossings += (np.sign(play) != np.sign(prev)).astype(int)
    prev = play
print("max |play| deg per joint:", np.round(np.degrees(maxplay), 4).tolist())
print("sign crossings per joint:", crossings.tolist())
print("final play deg:", np.round(np.degrees(data.qpos[padr]), 4).tolist())
print("play velocity peak (rad/s):", np.round(np.max(np.abs(data.qvel[[int(model.jnt_dofadr[j]) for j in passive]])), 4).tolist())
