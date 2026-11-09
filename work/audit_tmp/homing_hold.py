import sys
import numpy as np
sys.path.insert(0, r"D:\AI\Duck\work")
import body_bam as bb

body = bb.Body(bb.DEFAULT_XML, kp=200.0, vin=7.4, vin_drop_gain=0.1)
body.set_targets(list(bb.HOME_POSE))
body.set_torque(True)
for i in range(400):          # 2 s of 5 ms substeps
    body.step()
    if i % 80 == 79:
        s = body.sensors()
        dev = np.array(s["positions"]) - np.array(bb.HOME_POSE)
        print(f"t={s['sim_time']:.2f} trunk_z={s['trunk_z']:.4f} max|djoint|={np.abs(dev).max():.4f} "
              f"gravity_z={s['imu']['gravity'][2]:.3f}", flush=True)
s = body.sensors()
print("final trunk", s["trunk"], "quat", s["imu"]["quat"])
