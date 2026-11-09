import pathlib, sys
src = pathlib.Path("D:/AI/Duck/ios/DuckApp/DuckResources/robot/microduck/scene_backlash.xml")
out = pathlib.Path("D:/AI/Duck/work/scratch/scene_allcollisions_backlash.xml")
txt = src.read_text().replace('include file="robot_groundcontact_backlash.xml"','include file="robot_allcollisions_backlash.xml"')
out.write_text(txt)
print("wrote", out)
