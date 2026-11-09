import mujoco
for name in ["robot_groundcontact_backlash.xml","robot_allcollisions_backlash.xml"]:
    p = "D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/"+name
    m = mujoco.MjModel.from_xml_path(p)
    print("==", name, "colliding geoms:")
    for g in range(m.ngeom):
        if m.geom_contype[g] or m.geom_conaffinity[g]:
            n = mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_GEOM, g)
            b = mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_BODY, m.geom_bodyid[g])
            print(f"   {g}: geom={n!r} body={b!r} type={m.geom_type[g]}")
