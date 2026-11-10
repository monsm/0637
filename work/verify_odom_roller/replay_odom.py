import sys, csv, numpy as np, mujoco
csv_path, walk_path = sys.argv[1], sys.argv[2]
ALPHA16 = {
 "left": np.array([
  [-0.018300,-0.019510,0.005894],[-0.001500,-0.019659,0.001516],[0.015500,-0.019659,0.002082],[0.031699,-0.019061,0.005531],
  [-0.018500,-0.006960,0.002891],[-0.001500,-0.006960,-0.000607],[0.015500,-0.006960,-0.000573],[0.032500,-0.006960,0.002863],
  [-0.018500,0.005740,0.003709],[-0.001500,0.005740,0.000500],[0.015500,0.005740,0.000474],[0.032500,0.005740,0.003683],
  [-0.018300,0.018290,0.007998],[-0.001500,0.018439,0.003859],[0.015500,0.018439,0.004318],[0.031699,0.017841,0.007574]]),
 "right": np.array([
  [-0.018300,-0.018290,0.007998],[-0.001500,-0.018439,0.003859],[0.015500,-0.018439,0.004320],[0.031699,-0.017841,0.007574],
  [-0.018500,-0.005740,0.003712],[-0.001500,-0.005740,0.000500],[0.015500,-0.005740,0.000474],[0.032500,-0.005740,0.003681],
  [-0.018500,0.006960,0.002893],[-0.001500,0.006960,-0.000607],[0.015500,0.006960,-0.000573],[0.032500,0.006960,0.002863],
  [-0.018300,0.019510,0.005894],[-0.001500,0.019659,0.001516],[0.015500,0.019659,0.002080],[0.031699,0.019061,0.005531]])}
# read CSV
rows = list(csv.DictReader(open(csv_path)))
njoint = 14
joint_names = ["left_hip_yaw","left_hip_roll","left_hip_pitch","left_knee","left_ankle",
               "neck_pitch","head_pitch","head_yaw","head_roll",
               "right_hip_yaw","right_hip_roll","right_hip_pitch","right_knee","right_ankle"]
# verify csv qpos order vs model actuators
wm = mujoco.MjModel.from_xml_path(walk_path); wd = mujoco.MjData(wm)
act_names = [mujoco.mj_id2name(wm, mujoco.mjtObj.mjOBJ_ACTUATOR, i) for i in range(wm.nu)]
print("walk model actuators:", act_names)
print("csv joints assumed  :", joint_names)
# build mapping actuators: the CSV's qpos_i comes from roller model's actuator order; verify it equals joint_names
rm = mujoco.MjModel.from_xml_path(r"D:/AI/Duck/upstream/microduck_rl/src/mjlab_microduck/robot/microduck/scene_rollers.xml")
roller_act = [mujoco.mj_id2name(rm, mujoco.mjtObj.mjOBJ_ACTUATOR, i) for i in range(rm.nu)]
print("roller actuators    :", roller_act)

def quat2mat(q):
    m = np.zeros(9); mujoco.mju_quat2Mat(m, np.asarray(q, dtype=np.float64)); return m.reshape(3,3)

class Odo:
    SWITCH_MARGIN=-0.010; SWITCH_CONFIRM_TICKS=2
    def __init__(self):
        self.anchor_foot=0; self.anchor_local=np.zeros(3); self.anchor_xy=np.zeros(2); self.position=np.zeros(3)
        self.pending=None; self.pending_ticks=0; self.needs_init=True
    def update(self, feet, R):
        if self.needs_init:
            self.anchor_xy=(R@feet[self.anchor_foot][0])[:2].copy(); self.needs_init=False
        self._reproject(R, feet)
        cand=self._lowest(R, feet)
        if cand is None: self.pending=None; self.pending_ticks=0
        else:
            foot, local, xy = cand
            if self.pending is not None and self.pending[0]==foot: self.pending_ticks+=1
            else: self.pending=cand; self.pending_ticks=1
            if self.pending_ticks>=self.SWITCH_CONFIRM_TICKS:
                foot,local,xy=self.pending; self.pending=None; self.anchor_foot=foot; self.anchor_local=local; self.anchor_xy=xy
                self._reproject(R,feet); self.pending_ticks=0
    def _reproject(self,R,feet):
        pos,rot=feet[self.anchor_foot]; contact=R@(rot@self.anchor_local+pos)
        self.position=np.array([self.anchor_xy[0]-contact[0], self.anchor_xy[1]-contact[1], -contact[2]])
    def _lowest(self,R,feet):
        lowest=-self.SWITCH_MARGIN; best=None
        for foot,(pos,rot) in enumerate(feet):
            world=self.position+(ALPHA16["left" if foot==0 else "right"]@rot.T+pos)@R.T
            i=int(np.argmin(world[:,2]))
            if world[i,2]<lowest: lowest=world[i,2]; best=(foot, (ALPHA16["left" if foot==0 else "right"])[i].copy(), world[i,:2].copy())
        return best

odo=Odo(); sim0=None; out=[]
for k,row in enumerate(rows):
    joints=[float(row[f"qpos_{i}"]) for i in range(njoint)]
    trunk=np.array([float(row["trunk_x"]),float(row["trunk_y"]),float(row["trunk_z"])])
    quat=np.array([float(row["quat_w"]),float(row["quat_x"]),float(row["quat_y"]),float(row["quat_z"])])
    R=quat2mat(quat)
    # walk model FK: trunk at origin, identity; joint angles by name
    wd.qpos[:]=0; wd.qpos[3]=1.0
    for name,a in zip(joint_names, joints):
        jid=mujoco.mj_name2id(wm,mujoco.mjtObj.mjOBJ_JOINT,name); wd.qpos[wm.jnt_qposadr[jid]]=a
    mujoco.mj_forward(wm,wd)
    feet=[]
    for side in ("left","right"):
        sid=mujoco.mj_name2id(wm,mujoco.mjtObj.mjOBJ_SITE,side+"_foot")
        p=wd.site_xpos[sid].copy(); r=wd.site_xmat[sid].reshape(3,3).copy()
        feet.append((p,r))
    odo.update(feet,R)
    if sim0 is None: sim0=trunk.copy()
    out.append((k, odo.position.copy(), trunk.copy(), odo.anchor_foot, odo.anchor_local.copy()))
# report
print("ticks", len(out))
for k in [0,1,2,10,25,50,100,150,200,250,299]:
    if k>=len(out): continue
    i,op,tp,af,al=out[k]
    print(f"tick {k:3d} sim_z {tp[2]:.4f} odo_z {op[2]:.4f} err_z {(op[2]-tp[2])*1000:+7.2f}mm anchorfoot {af} anchor_local {np.round(al,4)}")
i,op,tp,af,al=out[-1]
print("final horizontal: sim delta", np.round(tp[:2]-sim0[:2],4), "odo delta", np.round(op[:2],4), "error", np.round((op[:2]-(tp[:2]-sim0[:2]))*1000,1), "mm")
zs=np.array([o[1][2]-o[2][2] for o in out])
print("odo z error over run: min %.1f max %.1f mm; final %.1f mm" % (zs.min()*1000, zs.max()*1000, zs[-1]*1000))
# anchor switches count
switches=sum(1 for a,b in zip(out[:-1],out[1:]) if a[3]!=b[3])
print("anchor foot changes:", switches)
