import sys, numpy as np, mujoco
scene = sys.argv[1]
walk = sys.argv[2]
ALPHA16_LEFT = np.array([
[-0.018300, -0.019510, 0.005894],[-0.001500, -0.019659, 0.001516],[0.015500, -0.019659, 0.002082],[0.031699, -0.019061, 0.005531],
[-0.018500, -0.006960, 0.002891],[-0.001500, -0.006960, -0.000607],[0.015500, -0.006960, -0.000573],[0.032500, -0.006960, 0.002863],
[-0.018500, 0.005740, 0.003709],[-0.001500, 0.005740, 0.000500],[0.015500, 0.005740, 0.000474],[0.032500, 0.005740, 0.003683],
[-0.018300, 0.018290, 0.007998],[-0.001500, 0.018439, 0.003859],[0.015500, 0.018439, 0.004318],[0.031699, 0.017841, 0.007574]])
ALPHA16_RIGHT = np.array([
[-0.018300, -0.018290, 0.007998],[-0.001500, -0.018439, 0.003859],[0.015500, -0.018439, 0.004320],[0.031699, -0.017841, 0.007574],
[-0.018500, -0.005740, 0.003712],[-0.001500, -0.005740, 0.000500],[0.015500, -0.005740, 0.000474],[0.032500, -0.005740, 0.003681],
[-0.018500, 0.006960, 0.002893],[-0.001500, 0.006960, -0.000607],[0.015500, 0.006960, -0.000573],[0.032500, 0.006960, 0.002863],
[-0.018300, 0.019510, 0.005894],[-0.001500, 0.019659, 0.001516],[0.015500, 0.019659, 0.002080],[0.031699, 0.019061, 0.005531]])
sets = {"left": ALPHA16_LEFT, "right": ALPHA16_RIGHT}

m = mujoco.MjModel.from_xml_path(scene); d = mujoco.MjData(m)
kid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_KEY, "STAND")
mujoco.mj_resetDataKeyframe(m, d, kid); mujoco.mj_forward(m, d)
# joint angles from roller keyframe
angles = {}
for j in range(m.njnt):
    name = mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_JOINT, j)
    if name not in (None,) and not name.endswith("freejoint"):
        angles[name] = d.qpos[m.jnt_qposadr[j]]
print("angles", {k: round(v,6) for k,v in angles.items()})
# lowest wheel vertex z (rigid shift to put it on floor)
low = 1e9
for g in range(m.ngeom):
    if m.geom_type[g] != mujoco.mjtGeom.mjGEOM_MESH: continue
    mid = m.geom_dataid[g]; adr = m.mesh_vertadr[mid]; num = m.mesh_vertnum[mid]
    verts = m.mesh_vert[adr:adr+num]
    w = (d.geom_xmat[g].reshape(3,3) @ verts.T).T + d.geom_xpos[g]
    low = min(low, w[:,2].min())
print("lowest mesh vertex z at keyframe:", low)
H_roll = -low  # trunk height if lowest mesh point on floor
print("=> trunk height when body rests on its lowest mesh point H_roll =", H_roll)
# roller's own foot sites and ALPHA16 in roller frame with trunk raised by -low
print("\nRoller model itself:")
for foot, sidname in [("left","left_foot"),("right","right_foot")]:
    sid = mujoco.mj_name2id(m, mujoco.mjtObj.mjOBJ_SITE, sidname)
    pos = d.site_xpos[sid].copy(); mat = d.site_xmat[sid].reshape(3,3).copy()
    pts = pos + ALPHA16_LEFT @ mat.T if foot=="left" else pos + ALPHA16_RIGHT @ mat.T
    print(f"  {sidname} origin z {pos[2]:.6f}; ALPHA16 min z {pts[:,2].min():.6f} (after raising trunk by {-low:.6f}: origin z {pos[2]-low:.6f}, min z {pts[:,2].min()-low:.6f})")

# walk model FK at same angles, trunk at origin identity
wm = mujoco.MjModel.from_xml_path(walk); wd = mujoco.MjData(wm)
wd.qpos[:] = 0; wd.qpos[3] = 1.0  # identity quat
for j in range(wm.njnt):
    name = mujoco.mj_id2name(wm, mujoco.mjtObj.mjOBJ_JOINT, j)
    if name in angles:
        wd.qpos[wm.jnt_qposadr[j]] = angles[name]
mujoco.mj_forward(wm, wd)
print("\nWalk kinematics model (odometry FK), same 14 angles, trunk at origin:")
for foot, sidname in [("left","left_foot"),("right","right_foot")]:
    sid = mujoco.mj_name2id(wm, mujoco.mjtObj.mjOBJ_SITE, sidname)
    pos = wd.site_xpos[sid].copy(); mat = wd.site_xmat[sid].reshape(3,3).copy()
    pts = pos + sets[foot] @ mat.T
    print(f"  {sidname} origin z {pos[2]:.6f}; ALPHA16 min z {pts[:,2].min():.6f}; argmin {pts[:,2].argmin()}")
# odometry boot estimate (anchor_local=0,left)
sid = mujoco.mj_name2id(wm, mujoco.mjtObj.mjOBJ_SITE, "left_foot")
print("  boot reported trunk z = -left_foot origin z =", -wd.site_xpos[sid][2])
best = 1e9
for foot, sidname in [("left","left_foot"),("right","right_foot")]:
    sid = mujoco.mj_name2id(wm, mujoco.mjtObj.mjOBJ_SITE, sidname)
    pos = wd.site_xpos[sid]; mat = wd.site_xmat[sid].reshape(3,3)
    pts = pos + sets[foot] @ mat.T
    if pts[:,2].min() < best: best = pts[:,2].min()
print("  after-anchor-switch reported trunk z = -min(ALPHA16 z) =", -best)
print("\nDifferences vs actual trunk height H_roll:")
print("  boot estimate error (estimate-actual) =", -wd.site_xpos[mujoco.mj_name2id(wm, mujoco.mjtObj.mjOBJ_SITE, "left_foot")][2] - H_roll)
print("  anchor17 estimate error (estimate-actual) =", -best - H_roll)
print("  walk sole lowest point height above roller floor =", H_roll + best)
