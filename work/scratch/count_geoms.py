import mujoco, sys
for name in ["robot_groundcontact_backlash.xml","robot_allcollisions_backlash.xml","robot_walk_backlash.xml"]:
    p = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/"+name
    try:
        m = mujoco.MjModel.from_xml_path(p)
    except Exception as e:
        print(name, "ERR", e); continue
    # count collision geoms (contype or conaffinity nonzero) excluding world
    n = sum(1 for g in range(m.ngeom) if (m.geom_contype[g] or m.geom_conaffinity[g]))
    names = [mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_GEOM, g) for g in range(m.ngeom)]
    foot = [(n_, m.geom_friction[g][0], m.geom_solref[g][0]) for g,n_ in enumerate(names) if n_ and ('foot' in n_)]
    print(name, "ngeom", m.ngeom, "colliding", n, "nq", m.nq, "nu", m.nu)
    print("   feet:", foot)
