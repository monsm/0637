import sys, numpy as np, mujoco
scene = sys.argv[1]
m = mujoco.MjModel.from_xml_path(scene)
d = mujoco.MjData(m)
kid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_KEY, "STAND")
mujoco.mj_resetDataKeyframe(m, d, kid)
mujoco.mj_forward(m, d)
print("qpos trunk", d.qpos[:7])
print("qpos joints", np.round(d.qpos[7:],6))
# foot sites
for name in ["left_foot","right_foot"]:
    sid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_SITE, name)
    print(name, "pos", d.site_xpos[sid], "xmat", d.site_xmat[sid].reshape(3,3).round(6))
# lowest wheel surface point: iterate geoms attached to tire bodies
def body_lowest_vertex(bname):
    bid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_BODY, bname)
    lows=[]
    for g in range(m.ngeom):
        if m.geom_bodyid[g] != bid: continue
        gtype = m.geom_type[g]
        gpos = d.geom_xpos[g]; gmat = d.geom_xmat[g].reshape(3,3)
        if gtype == mujoco.mjtGeom.mjGEOM_MESH:
            mid = m.geom_dataid[g]
            adr = m.mesh_vertadr[mid]; num = m.mesh_vertnum[mid]
            verts = m.mesh_vert[adr:adr+num]
            w = (gmat @ verts.T).T + gpos
            lows.append(w[:,2].min())
            print(bname, "mesh geom", g, "minz", w[:,2].min(), "maxz", w[:,2].max())
        else:
            print(bname, "geom type", gtype)
    return min(lows) if lows else None
mins = {}
for b in ["tire","tire_2","tire_3","tire_4"]:
    mins[b] = body_lowest_vertex(b)
print("wheel minima", mins, "overall", min(mins.values()))
# min z of all mesh collision vertices overall
allmin = 1e9
for g in range(m.ngeom):
    if m.geom_type[g] != mujoco.mjtGeom.mjGEOM_MESH: continue
    mid = m.geom_dataid[g]; adr = m.mesh_vertadr[mid]; num = m.mesh_vertnum[mid]
    verts = m.mesh_vert[adr:adr+num]
    w = (d.geom_xmat[g].reshape(3,3) @ verts.T).T + d.geom_xpos[g]
    if w[:,2].min() < allmin: allmin = w[:,2].min(); gmin = g
print("lowest mesh vertex overall:", allmin, "geom", gmin, mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_GEOM, gmin))
