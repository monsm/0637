# iOS build

The app is SwiftUI + Metal on top of the Rust core. The core is built as an
iOS dylib so MuJoCo and ONNX Runtime stay inside it; Xcode only embeds
`libduck_mobile_core.dylib`.

## Requirements

- macOS with Xcode 15 or newer
- Rust (`rustup`)
- `xcodegen`, `cmake`, `ninja` (`brew install xcodegen cmake ninja`)
- The upstream sources and the official policies in the repo layout:
  `upstream/microduck_rl`, `upstream/microduck`, `policies/`

## Build

```bash
# 1. ONNX Runtime iOS XCFramework (static; CocoaPods archive)
bash ios/scripts/fetch-ort-ios.sh

# 2. MuJoCo 3.10 static libraries (simulator + device)
bash ios/scripts/build-mujoco-ios.sh

# 3. Copy MJCF, STL meshes and ONNX into the app bundle
bash ios/scripts/stage-resources.sh

# 4. Rust core -> ios/Frameworks/libduck_mobile_core.dylib
bash ios/scripts/build-rust-ios.sh aarch64-apple-ios-sim
# device: bash ios/scripts/build-rust-ios.sh aarch64-apple-ios

# 5. Generate and build the Xcode project
cd ios
xcodegen generate
xcodebuild -project Duck.xcodeproj -scheme Duck \
  -sdk iphonesimulator -configuration Debug \
  -derivedDataPath build CODE_SIGNING_ALLOWED=NO build
```

The GitHub Actions workflow `.github/workflows/ios.yml` runs the same steps on
a macOS runner. A simulator build needs no Apple account; installing on a
physical iPhone needs a signing team.

## What runs on device

- Rust: official `duck-control` (61-D observation, ONNX policy, safety, fall),
  official `robotd` skill scheduler, the mobile bring-up/limp-fall loop, and an
  in-process MuJoCo body with the **BAM M6 XL330 torque model**.
- Swift/Metal: renders the official STL meshes, one draw per visual geom,
  reading body transforms from MuJoCo each frame. The joystick streams twist at
  50 Hz (the core's deadman is 500 ms).

## Custom AI / policies

Put a set with `manifest.json` + `.onnx` files in the app's
`Documents/policies/`, then press **Reload AI**. The set is read with the same
`robotd_params::SetManifest` types, so custom policies must respect the 61-D
observation / 14-action contract. Without a user set, the bundled official set
is used.

### Intent API

The app starts a local HTTP server on port **8765**. It only enqueues intents;
the 50 Hz loop applies them. No authentication — it is a local-network
development API.

```bash
curl http://<phone-ip>:8765/state
curl -X POST http://<phone-ip>:8765/twist -d '{"vx":0.4,"vy":0,"wz":0}'
curl -X POST http://<phone-ip>:8765/skill -d '{"name":"roulade"}'
curl -X POST http://<phone-ip>:8765/sit
curl -X POST http://<phone-ip>:8765/ground_pick
curl -X POST http://<phone-ip>:8765/push -d '{"vx":0.6,"vy":0,"vz":0}'
curl -X POST http://<phone-ip>:8765/reload -d '{"dir":"/path/to/policies"}'
curl http://<phone-ip>:8765/skills
```

`twist` must be re-sent at least every 500 ms: the core has robotd's deadman,
and a command stream that stops means "stand still". The same HTTP surface is
the natural place for an LLM/agent bridge.

### Phone sensors

- CoreMotion: a shake above ~1.2 g becomes an external trunk velocity impulse —
  a disturbance, not a replacement for the duck's proprioception (Twin mode).
- Touch: the joystick streams twist at 50 Hz.
- Camera/LiDAR/AR and microphone are not wired yet (M2.3).

The high-level intent surface is also the C ABI (`duck_core_set_twist`,
`duck_core_trigger_skill`, `duck_core_sit_toggle`, `duck_core_ground_pick`,
`duck_core_push`).

## Current limits

- The renderer is a first pass: flat-per-face normals, one directional light,
  a follow camera. No PBR materials/shadows yet.
- Phone sensors are not wired into the loop yet (M2.3). The core runs in Twin
  mode; phone IMU/camera/LiDAR should drive the world, not the duck's
  proprioception, to preserve 1:1.
- Android is not started.
