# Included into every project() call via -DCMAKE_PROJECT_INCLUDE.
# MuJoCo FetchContent-pulls qhull, whose install(TARGETS) has no BUNDLE
# DESTINATION — on iOS, executables default to MACOSX_BUNDLE and the install
# command errors out at configure time. We only ever extract libmujoco.a and
# never install anything, so bundles are off everywhere in the build tree.
set(CMAKE_MACOSX_BUNDLE OFF)
