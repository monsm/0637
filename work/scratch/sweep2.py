import numpy as np, sys
sys.path.insert(0,"D:/AI/Duck/work/scratch")
import apppath as A
A.XML="D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml"
for vx in (0.1,0.15,0.2,0.3):
    dist,tilt=A.run(vx, seconds=8.0)
    print(f"groundcontact vx={vx:.2f} -> {np.linalg.norm(dist)/8:.3f} m/s ({100*np.linalg.norm(dist)/8/vx:.0f}%), tilt {tilt:.1f}")
