import importlib.util
import os

spec = importlib.util.spec_from_file_location('cf', 'scripts/check-frame.py')
cf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cf)

base = os.path.join(os.environ['USERPROFILE'], 'AppData', 'Local', 'Temp', 'andshot3')
f = cf.load(os.path.join(base, 'android-screen.png'))
region = (0.3, 0.3, 0.7, 0.7)
for v in (100, 120, 140, 160, 170, 180):
    print('region >=', v, '->', cf.count_neutral_bright(f, v, 26, region))
print('whole  >=', 170, '->', cf.count_neutral_bright(f, 170, 26))

# What is the shell's actual colour inside the region?
w, h, ch, px = f['width'], f['height'], f['channels'], f['pixels']
left, right = int(w * 0.3), int(w * 0.7)
top, bottom = int(h * 0.3), int(h * 0.7)
buckets = {}
for y in range(top, bottom):
    for x in range(left, right):
        i = (y * w + x) * ch
        r, g, b = px[i], px[i + 1], px[i + 2]
        high, low = max(r, g, b), min(r, g, b)
        if high - low <= 30:  # neutral
            key = high // 32 * 32
            buckets[key] = buckets.get(key, 0) + 1
print('neutral brightness buckets in region:', sorted(buckets.items()))
