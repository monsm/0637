import sys, mujoco
path = sys.argv[1]
m = mujoco.MjModel.from_xml_path(path)
print("nq", m.nq, "nv", m.nv, "nu", m.nu, "njnt", m.njnt)
jn = lambda j: mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_JOINT, j)
an = lambda a: mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_ACTUATOR, a)
print("--- actuators ---")
for a in range(m.nu):
    j = m.actuator_trnid[a,0]
    print(f"act {a:2d} {an(a):18s} -> joint {j:2d} {jn(j):28s} qposadr {m.jnt_qposadr[j]:2d} dofadr {m.jnt_dofadr[j]:2d} trntype {m.actuator_trntype[a]}")
print("--- joints ---")
for j in range(m.njnt):
    print(f"jnt {j:2d} {str(jn(j)):38s} qposadr {m.jnt_qposadr[j]:2d} dofadr {m.jnt_dofadr[j]:2d} range {m.jnt_range[j]}")
