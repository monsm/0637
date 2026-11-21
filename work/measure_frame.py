import importlib.util
import os

spec = importlib.util.spec_from_file_location('cf', 'scripts/check-frame.py')
cf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cf)

base = os.path.join(os.environ['USERPROFILE'], 'AppData', 'Local', 'Temp', 'andshot3')
for name in ('android-screen.png', 'android-screen-2.png'):
    path = os.path.join(base, name)
    if not os.path.exists(path):
        continue
    f = cf.load(path)
    print(
        name,
        f"lit {f['lit']:.2%}",
        '| >=170:', cf.count_neutral_bright(f, 170, 26),
        '| >=140:', cf.count_neutral_bright(f, 140, 26),
        '| >=120:', cf.count_neutral_bright(f, 120, 26),
        '| >=100:', cf.count_neutral_bright(f, 100, 26),
    )
