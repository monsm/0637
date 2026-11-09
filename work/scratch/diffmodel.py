import sys, numpy as np, mujoco
sys.path.insert(0,"D:/AI/Duck/work/scratch"); sys.path.insert(0,"D:/AI/Duck/upstream/bam")
import infer_stub as IP
import faithful as F
xml="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
bam=IP.load_bam_model(IP.BAM_KP_FW, 7.4, None)
model,data,bam_ctrl,names=IP.load_mujoco_with_bam(xml,bam,0.005,0.1,IP.BAM_VIN_MIN)
m,d,ctrl=F.build("ref")
print("nu", model.nu, m.nu, "nq", model.nq, m.nq, "nv", model.nv, m.nv, "ngeom", model.ngeom, m.ngeom)
for field in ["dof_armature","dof_damping","dof_frictionloss","dof_solref","jnt_solref","actuator_forcerange",
              "actuator_gainprm","actuator_biasprm","actuator_gear","actuator_forcelimited","actuator_ctrllimited",
              "dof_solimp","jnt_solimp"]:
    a=getattr(model,field); b=getattr(m,field)
    if a.shape!=b.shape:
        print(field,"SHAPE DIFF",a.shape,b.shape); continue
    diff=np.abs(a-b)
    print(f"{field:22s} maxdiff={diff.max():.3e}")
print("timestep", model.opt.timestep, m.opt.timestep)
print("iterations", model.opt.iterations, m.opt.iterations, "ls", model.opt.ls_iterations, m.opt.ls_iterations)
print("solver", model.opt.solver, m.opt.solver, "cone", model.opt.cone, m.opt.cone)
print("integrator", model.opt.integrator, m.opt.integrator)
print("noslip", model.opt.noslip_iterations, m.opt.noslip_iterations)
print("geom_friction maxdiff", np.abs(model.geom_friction-m.geom_friction).max())
print("geom_solref maxdiff", np.abs(model.geom_solref-m.geom_solref).max())
print("body_mass maxdiff", np.abs(model.body_mass-m.body_mass).max())
