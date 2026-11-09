"""Isolate the friction-constraint stiffness: a single hinge with frictionloss,
loaded below its budget, no BAM involved. Compares default solref vs the
reference's stiff solref, and shows where the app's jnt_solref actually lands."""
import mujoco, numpy as np
xml = """<mujoco>
 <worldbody><body name="b" pos="0 0 0"><joint name="j" type="hinge" axis="0 0 1"
   frictionloss="0.05" damping="0" armature="0.001"/>
   <geom type="capsule" fromto="0 0 0 0.2 0 0" size="0.02" mass="0.5"/></body></worldbody>
 <actuator><motor joint="j" gear="1" ctrllimited="true" ctrlrange="-1 1"/></actuator>
</mujoco>"""
def run(stiff_friction=False, stiff_limit=False, load=0.02, secs=2.0):
    m = mujoco.MjModel.from_xml_string(xml); d = mujoco.MjData(m)
    m.opt.gravity[:] = 0
    if stiff_friction:
        m.dof_solref[0] = [-5e4,-2e2]; m.dof_solimp[0] = [0.99,0.9999,0.001,0.5,2.0]
    if stiff_limit:
        m.jnt_solref[0] = [-5e4,-2e2]; m.jnt_solimp[0] = [0.99,0.9999,0.001,0.5,2.0]
    for _ in range(int(secs/m.opt.timestep)):
        d.ctrl[0] = 0.0
        d.qfrc_applied[0] = load   # constant external torque, below frictionloss
        mujoco.mj_step(m,d)
    return np.degrees(d.qpos[0])
for label,kw in [("default (app: jnt stiffened, friction untouched)",dict(stiff_limit=True)),
                 ("reference (dof_solref stiffened)",dict(stiff_friction=True)),
                 ("neither",dict())]:
    for load in (0.02,0.05,0.08):
        print(f"{label:52s} load={load:.3f} Nm -> moved {run(load=load,**kw):8.4f}°")
