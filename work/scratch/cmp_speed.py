import numpy as np, sys
sys.path.insert(0,"D:/AI/Duck/work/scratch")
import faithful as F
for vx in (0.15,0.2,0.3):
    rows=F.run("ref", lambda t: [vx,0,0], seconds=20.0)
    seg=[r for r in rows if 15<=r[0]<=20]
    d=seg[-1][1][0]-seg[0][1][0]
    print(f"ref CPU path vx={vx:.2f} -> {d/(seg[-1][0]-seg[0][0]):.3f} m/s over 15-20s")
