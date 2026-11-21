# Android build

The Android app is Compose + OpenGL ES 3 on top of the same Rust core and C ABI
as iOS. The native library is `libduck_mobile_core.so`, linked against a static
MuJoCo 3.10 build and the ONNX Runtime Android AAR (loaded dynamically).

## CI (the supported path)

Push to GitHub and the `.github/workflows/android.yml` workflow builds an APK on
`ubuntu-latest`:

1. installs JDK 17, Android SDK 35, NDK 27.2, CMake/Ninja;
2. clones `microduck_rl` + `microduck` and downloads the official policy set;
3. builds MuJoCo 3.10 for `arm64-v8a` and `x86_64`;
4. fetches `onnxruntime-android` and copies its `.so` files into `jniLibs`;
5. stages MJCF/STL/ONNX into `app/src/main/assets`;
6. builds the Rust core with `cargo-ndk`;
7. runs `gradle assembleDebug`.

The APK is uploaded as the `Duck-android` artifact.

## Local build (optional)

The same scripts can run on a machine with JDK 17, Android SDK 35, NDK 27.2 and
Rust:

```bash
bash android/scripts/build-mujoco-android.sh
bash android/scripts/fetch-ort-android.sh
bash android/scripts/stage-resources.sh
bash android/scripts/build-rust-android.sh
echo "sdk.dir=$ANDROID_HOME" > android/local.properties
cd android && gradle assembleDebug
```

On Windows, use `android/scripts/stage-resources.ps1` for the assets.

## What runs

- Rust: official `duck-control` (61-D observation, ONNX policies, safety, fall),
  `robotd` skill scheduler, BAM M6 actuator model, in-process MuJoCo.
- Kotlin: `DuckRenderer` draws the official STL meshes with OpenGL ES 3 and
  reads body poses from the core; the joystick streams twist at 50 Hz; buttons
  trigger skills/sit/ground pick.
- Intent API: the app starts the local HTTP server on **8765** at launch
  (`/state`, `/twist`, `/skill`, `/sit`, `/ground_pick`, `/push`, `/reload`).

## Custom AI

Put a `manifest.json` + `.onnx` set in the app's files directory and press
**Reload AI**, or use `POST /reload {"dir":"..."}`. The official manifest types
are reused, so custom policies must respect the 61-D observation / 14-action
contract.
