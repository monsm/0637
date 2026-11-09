import mujoco
for name in ["robot_groundcontact_backlash.xml","robot_allcollisions_backlash.xml"]:
    p = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/"+name
    m = mujoco.MjModel.from_xml_path(p)
    print("==", name)
    for g in range(m.ngeom):
        n = mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_GEOM, g)
        if not n: continue
        if (m.geom_contype[g] or m.geom_conaffinity[g]):
            if 'foot' in n or 'collision' in n:
                print(f"  {n}: contype={m.geom_contype[g]} conaff={m.geom_conaffinity[g]} condim={m.geom_condim[g]} friction={m.geom_friction[g]} priority={m.geom_priority[g]}")
