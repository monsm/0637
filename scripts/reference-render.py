import struct, zlib, sys
import numpy as np
import mujoco

def write_png(path, rgb):
    h, w, _ = rgb.shape
    raw = b''.join(b'\x00' + rgb[y].tobytes() for y in range(h))
    def chunk(t, d):
        c = t + d
        return struct.pack('>I', len(d)) + c + struct.pack('>I', zlib.crc32(c))
    out = b'\x89PNG\r\n\x1a\n'
    out += chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
    out += chunk(b'IDAT', zlib.compress(raw))
    out += chunk(b'IEND', b'')
    open(path, 'wb').write(out)

from pathlib import Path
root = Path(__file__).resolve().parent.parent
xml = str(root / "upstream/microduck_rl/src/mjlab_microduck/robot/microduck/scene_backlash.xml")
out = root / "work" / "refrender"
out.mkdir(parents=True, exist_ok=True)
m = mujoco.MjModel.from_xml_path(xml)
d = mujoco.MjData(m)
names = [m.key(i).name for i in range(m.nkey)]
print("keyframes:", names, "nkey", m.nkey, "nbody", m.nbody, "ngeom", m.ngeom, "nmesh", m.nmesh)
kid = names.index("STAND") if "STAND" in names else 0
mujoco.mj_resetDataKeyframe(m, d, kid)
mujoco.mj_forward(m, d)

trunk = 1
for i in range(m.nbody):
    nm = mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_BODY, i)
    if nm and "trunk" in nm.lower():
        trunk = i
        break
print("trunk body:", trunk, mujoco.mj_id2name(m, mujoco.mjtObj.mjOBJ_BODY, trunk), "xpos", d.xpos[trunk])

r = mujoco.Renderer(m, height=480, width=640)
for az in (90, 180, 270):
    cam = mujoco.MjvCamera()
    cam.lookat = d.xpos[trunk].copy()
    cam.distance = 0.85
    cam.azimuth = az
    cam.elevation = -18
    r.update_scene(d, camera=cam)
    img = r.render()
    write_png(rf"D:\AI\Duck\work\refrender\ref-{az}.png", np.ascontiguousarray(img))
    print("wrote", az)
