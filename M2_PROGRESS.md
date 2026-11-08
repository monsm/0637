# M2 进度 — Rust 控制回路已闭环

日期：2026-11-08 · 状态：M2.0 进行中

## 已完成

1. **Rust 工具链**：rustc / cargo 1.99.1；Visual Studio C++ Build Tools（MSVC 链接器）。
2. **官方控制核心复用**：
   - `duck-control` 新增 `hardware` feature（`default-features = false` 时去掉 `rustypot` / `serialport`），观测、策略、安全、摔倒逻辑不变。
   - `robotd/src/control.rs`（技能调度器）通过 `#[path]` 直接编译进 `duck-mobile-core`，未复制。
   - `cargo test`：5 passed / 0 failed；8 个需要 ONNX Runtime 的官方测试被 ignore。
3. **Rust 闭环**：
   - `sim_walk` binary：官方 `Policy`（velstand.onnx）+ `Controller`（官方技能调度器）+ `Safety` + `RemoteIo`。
   - 对端：官方 `duck-body` MuJoCo TCP 服务器（protocol 1，`--keyframe HOME`）。
   - 50 Hz 墙钟循环，`ORT_DYLIB_PATH` 指向官方 ONNX Runtime 1.24.4。

## 实测

| 指标 | 结果 |
|---|---|
| 控制频率 | 599 ticks / 12.00 s ≈ **49.9 Hz** |
| 行走状态 | `label=walk` 全程 |
| 实时 trunk 位移 | x 0.776 → 1.928 m（8 s，≈0.16 m/s） |
| 实时 trunk 高度 | 0.114–0.119 m（站立高度） |
| 实时重力 | (0, 0, −1)，直立 |
| 约 9 s 后 | 摔倒（z→0.047，重力倾斜） |

对照 M1（官方 BAM + `infer_policy.py`）：0.4 m/s 命令 → 1.457 m / 10 s。

## 已知差异与待办

1. **body server 用的是 XML 位置执行器，不是 BAM**：M1 用的是 BAM M6。需要写一个 BAM 版 body server（复用 `work/run_trace.py` 的装配逻辑），再对比轨迹。
2. **没有 bring-up / limp-fall / 恢复回路**：`robotd` 有 Limp → Homing（2 s 力矩爬升）→ Ready、limp-fall 预测、sitstand 起身；当前 `sim_walk` 只做了最简 read→step→apply，9 s 摔倒后不会自动恢复。
3. **只加载了 walk 策略**：全技能（sitstand / groundpick / kick / roulade / roller）和 manifest 解析还没接。
4. **iOS 壳与 CI**：原生 SwiftUI + Metal + GitHub Actions macOS 还未开始。

## 下一步

1. 写 `work/body_bam.py`：官方 MJCF + BAM + 同一 TCP 协议，替换位置执行器。
2. 移植 `robotd` 的 tick 顺序与状态机（Limp/Homing/Ready、limp-fall、摔倒恢复）。
3. manifest → 全部技能 + 技能 UI。
4. iOS 壳 + Metal 渲染 + macOS CI 模拟器构建。

## 更新（同日晚）：BAM body + 全技能 runtime 已通

### BAM body server

`work/body_bam.py`：官方 `scene_backlash.xml` + BAM M6 + protocol 1，物理线程 50 Hz，trunk 每秒记录。Rust 回路实测：

- 走路 9 s：x 0.02 → 1.372 m，trunk_z 0.116–0.119，重力 (0,0,−1)
- 与 M1 的 1.457 m / 10 s 对齐

### 全技能 runtime

新增 `src/runtime.rs`（bring-up / deadman / limp-fall / 恢复）与 `src/bin/sim_runtime.rs`，加载官方全套策略：

| 时间 | 行为 | 实测 |
|---|---|---|
| 0–2 s | homing | 2 s 线性 ramp |
| 2–10 s | walk 0.4 m/s | x 0.175→1.237 m（7 s，≈0.15 m/s），全程直立 |
| 10–12 s | stand | 停在 x≈1.234 |
| 12–16 s | sit | trunk_z 0.059 |
| 16–18 s | rise → walk | trunk_z 0.113 |
| 18.5 s | roulade | `label=roulade`，过程中 fallen=true |
| 21 s | kick_left | `label=kick_left` |
| 23 s | ground_pick | `label=ground_pick`，trunk_z 0.058 → 0.115 |
| 全程 | 50 Hz | 1299 ticks / 26.00 s ≈ 49.96 Hz |

**发现**：deadman 是正确行为——`set_twist` 只发一次时，500 ms 后自动归零；控制流必须持续发命令。app 的摇杆/命令层要按此实现。

### 仍待完成

- iOS CI 实际编译验证（需 macOS runner / 签名设备）。
- 手机传感器完整接入（M2.3；IMU 扰动先做）。
- Android（M3）。
- AR 模式。
- 渲染器 PBR/阴影/帧间插值。

## 更新（同日晚）：端内 MuJoCo + BAM + manifest + iOS 工程

### 端内 MuJoCo

- 新增 C shim `native/duck_mj_shim.c`，Rust `MujocoIo` 实现官方 `RobotIo`。
- Windows 用官方 wheel 的 `mujoco.dll`（生成导入库）；iOS 用 MuJoCo 3.10 静态库。
- `sim_inproc` 不再需要 TCP body server。

### BAM M6 移植

- `src/bam.rs`：XL330 M6 电压控制律、DC 电机反电动势、Stribeck/方向/二次摩擦。
- 与官方 Python BAM 数值对拍：电压/扭矩/摩擦误差 < 1e-9（测试锁定）。
- `MujocoIo` 默认走 BAM 扭矩路径；`load_position()` 保留位置执行器对照。
- 实测：走路 8 s，x 0.003→1.254 m（≈0.157 m/s），全程直立；sit 正常。

### manifest

- 直接复用 `robotd_params::SetManifest` / `SetPolicy`：
  walk/sitstand/groundpick 槽位、episodic 技能、时长、chain、action_scale、
  地面拾取周期/结束相位、sitstand rise/ramp 全部从 manifest 读取。
- `PolicyBundle::tuning()` 使用 walk 条目的 action_scale。
- 自定义策略集放入 `Documents/policies` 后可在 app 内 **Reload AI**。

### iOS 工程

- `ios/project.yml`（XcodeGen）+ SwiftUI + Metal 渲染 + C ABI 桥接。
- `ios/scripts/`：ORT xcframework、MuJoCo 静态库、资源 staging、Rust dylib。
- `.github/workflows/ios.yml`：macOS runner 出模拟器构建。
- 说明：`ios/README.md`。
- 注意：当前 Windows 环境无法编译/签名 iOS；CI 是唯一验证路径。

## 更新（同日深夜）：intent API + 手机 IMU

### 本地 intent API

- `src/intent.rs`：无依赖 HTTP 服务（std::net），命令进入队列，由 50 Hz 循环消费。
- 端点：`/state` `/skills` `/twist` `/head` `/body` `/skill` `/sit`
  `/ground_pick` `/push` `/reload`。
- `duck_core_start_intent_server(core, 8765)`；iOS app 启动时自动开启。
- 测试：HTTP → FFI → runtime 全链路（`intent_server_drives_the_core`）。

### 手机 IMU

- iOS `CMMotionManager` 50 Hz；shake > 1.2 g → `duck_core_push` 施加躯干速度扰动。
- 这是 Twin 语义：手机不替代鸭子的本体感知，只提供外部扰动。
- 实测：走路中 push 0.6 m/s，0.1 s 内速度跳变、随后恢复直立行走。

### 完整 BAM 技能表（26 s）

| 时间 | 行为 | 结果 |
|---|---|---|
| 2–10 s | walk 0.4 m/s | x 0.003→1.436 m，直立 |
| 6 s | push 0.6 m/s | 短暂扰动，恢复 |
| 12–16 s | sit | trunk_z 0.059 |
| 16–18 s | rise → walk | 直立 |
| 18.5 s | roulade | 翻滚，fallen→恢复 |
| 21 s | kick_left | 正常 |
| 23 s | ground_pick | trunk_z 0.093–0.115 |
| 全程 | 50 Hz | 1299 ticks / 26.00 s |

### 仍待完成（更新）

- CI 迭代修复中（见下）。
- 相机/LiDAR/AR、麦克风（M2.3 剩余）。
- Android（M3）。
- 渲染器 PBR/阴影/帧间插值。

## CI 迭代（同日）

仓库：`github.com/monsm/0637`（已推送）。所有 Android/iOS 构建都走 GitHub CI，
本地不做移动端构建。

| 3 | 2a38ef4 | iOS：MuJoCo **cmake configure** 失败（summary 匿名不可见）；Android 结果未显示 | 失败日志改为 **commit 回仓库**（`ci-logs/`，`[skip ci]` 防循环） |
| 4 | 5868bcc | 被 946b8a3 的并发取消覆盖 | — |
| 6 | 66ac28e | iOS `CMAKE_SKIP_INSTALL_RULES=ON` 没压住 qhull bundle 错误；Android 30s 失败且无注解（失败步在 install-sdk 之前，无日志可拷） | iOS 改 `-DCMAKE_MACOSX_BUNDLE=OFF`（iOS 下可执行文件默认 bundle，这才是根因）；两个 workflow 每个步骤加 `id`，失败时把**全部步骤 outcome** 写进 `ci-logs/steps.txt` |
| 7 | 847ba60 | 配置通过！编译失败：MuJoCo 给 APPLE 加 `-mmacosx-version-min=11`，iOS 下与 `-mios-simulator-version-min` 冲突；Android `setupandroid=failure`（setup-android@v3 在 ubuntu-24.04 挂了） | 删掉 setup-android action（用 runner 预装 SDK + 自举 cmdline-tools）；iOS 开始补丁 MujocoMacOS.cmake |
| 8 | 5d57314 | 补丁只改了函数，但 CMakeLists 还直接把 `${MUJOCO_MACOS_*}` 加到 mujoco target → 仍残留 `-mmacosx-version-min` | 改为补文件顶层 `if(APPLE)` → iOS 走 else()，两组 flag 与 `-Werror=availability` 全空 |
| 9 | 43a6dc4 | iOS：MuJoCo 配置/编译到 109/111 后链接的是 **shared dylib**，脚本找不到 `libmujoco.a`；Android：`aligned_alloc` 已被 API 28 修好，但 `localtime_r` 检测仍报错（缓存里的 CMakeCache 是旧 flags，env CFLAGS 未生效） | MuJoCo CMakeLists 把 `add_library(mujoco SHARED)` 写死 → 打补丁改 **STATIC**；Android 需要重建时先删构建目录，保证 `_POSIX_C_SOURCE` 进入 cache |
| 10 | 6ce1a62 + 75606eb | iOS：MuJoCo 静态库成功、stage 成功，`rustcore` 失败 = **CI 克隆的上游没带我们的 duck-control 补丁**（feature 不存在）；Android：STATIC 化后 `install(EXPORT)` 依赖 miniz/lodepng 报错 | 导出 `patches/duck-control-mobile.patch`，CI 固定 checkout `da34a22` 后 apply；Android 加 `CMAKE_SKIP_INSTALL_RULES=ON` |
| 11 | 324c099 / e30b08b | Android：`localtime_r` 仍失败——env `CFLAGS` 被 NDK toolchain 覆盖 | 改为**源码补丁**：MuJoCo 的 localtime 检测加 `__ANDROID__`（bionic 有 localtime_r） |
| 12 | c1b4802 / cb7e3cc | Android：Rust 编译错误 `[0i8; 1024]`——Android 上 `c_char` 是 `u8`（Windows 是 i8，本地过了）；iOS 日志通道被 main 上的日志提交互相冲突弄丢 | `c_char` 修正 + `#![allow(unsafe_op_in_unsafe_fn)]`；日志改推独立分支 `ci-logs`（force push，永不冲突） |
| 13 | 55a6d35 / af69f10 | Android：jni.rs `c_char` 未导入（E0425）→ 修；链接时找不到 `libc++_static.a` → build.rs 加 NDK per-triple lib 搜索路径；iOS：runner 排队慢 + 连续推送把运行取消了几轮，日志停在补丁前 | 批量修复，一次推送 |
| 14 | 9af3a72 | 两个平台都在 **Rust core** 失败：`build.rs` 里 `env::consts::OS.as_str()` 是不稳定 API（`str_as_str`，E0658）——runner 的 rustc 比本地 1.99.1 旧；之前的 `c++_static` 报错是**旧日志**（Android 的 force-push 用 main 上残留的旧 `ci-logs/` 覆盖了 iOS 刚发的日志） | `core/rust-toolchain.toml` 钉死 1.99.1；删掉 main 上的旧 `ci-logs/`；发布日志步骤先 fetch `ci-logs` 分支再叠加，平台间不再互相覆盖 |
| 15 | 本地预检后推送 | 本地 `cargo check` 复现并修掉下一个 iOS 失败：`onnx-static = []` 让 ort 连 `std` 都没有（Session API 全变 `!`），且上游 `pickup.rs` 无条件用 `ensure_runtime` | `onnx-static = ["ort/std"]`（链接由 ort-sys 的 `ORT_IOS_XCFWK_LOCATION` 处理）；`ensure_runtime` 在静态路径下为 no-op；Android 的 c++ 改 `-static-libstdc++`（NDK clang 自解析，不再猜 NDK 路径）；SDK 不再下第二个 NDK（用 runner 预装的 27.3） |
| 16 | 2ed5b82 | 两个平台 E0463：`rustup target add` 在工作区根目录跑，而钉版本在 `core/rust-toolchain.toml` → 目标 std 装进了 runner 默认工具链；且旧日志覆盖新日志（overlay 顺序错了） | `--toolchain 1.99.1` 显式安装；发布步骤先 overlay 后拷贝；失败日志同时 tail 到 job log（step summary API 取不到） |
| 17 | ccc40e4 | iOS：rustup 按当前目录选工具链，cargo 在工作区根目录跑 → 用了 runner 默认工具链（无 ios-sim std）；Android：`-nodefaultlibs` 下 `-static-libstdc++` 无效 + MuJoCo 静态库不带 tinyxml2/miniz/lodepng/qhull；桌面 sim 二进制被一起链接 | iOS 脚本 `cd core/` + `RUSTUP_TOOLCHAIN`；Android 显式 `-l static=c++_static` + NDK sysroot 搜索路径 + `--lib`；MuJoCo 脚本输出依赖归档 + `libs.txt` 链接顺序，build.rs 按清单链接 |
| 18 | bd39ff0 | **Android Rust 两个 ABI 全部链接通过**，Gradle 首次编译失败：没有 `gradle.properties`（AndroidX 未启用）；iOS：`-nodefaultlibs` 连 SDK 都不设置 → `libc++` 找不到 | `android/gradle.properties`（useAndroidX）；iOS 显式 `-isysroot`（按 target 选 iphonesimulator/iphoneos）；`ORT_CXX_STDLIB=c++`（ort-sys 自己加 `-l`） |
| 19 | 448743c | **Android 全绿，APK 产物 47.9 MB**；iOS 链接差 qhull：归档真名是 `libqhullstatic_r.a` | 允许清单改为 `qhullstatic_r` |
| 20 | 9af3a72→a2d0aa8 | 安卓 APK 在设备/模拟器上**打开闪退**（用户报告）。logcat：`UnsatisfiedLinkError: dlopen failed: cannot locate symbol __gxx_personality_v0`——共享库默认允许未定义符号，链接不报错、运行时才炸；修后又暴露 `pthread_atfork`（cargo-ndk 默认 API 21） | 补 `c++abi`；链接加 `--no-undefined`；`--platform 28`；冒烟脚本独立成文件（action 内联脚本被 shell 弄坏） |
| 21 | d805192 | app 能启动但 dlopen 时在**静态构造函数**里 SIGSEGV（空指针）。tombstone 调用链 `dlopen → call_constructors → libduck_mobile_core.so` | 改用 `libc++_shared`（并把 NDK 的 .so 拷进 APK）；仍崩 → 上传 **未 strip 的 .so** 做符号化（AGP 打包会 strip，地址不变） |
| 22 | 22e9db8→ | iOS：`simctl install` 报 "Missing bundle ID"。诊断打印出 plist 完整、`codesign -dv` 显示 `Identifier=Duck, Info.plist=not bound`（linker-signed）→ 手动签名无效；改让 Xcode CodeSign 阶段签名后又卡在 `default.metallib`（Xcode 26 视其为未签名嵌套代码）| 着色器改为**运行时编译**（Shaders.metal 作为资源随包，不再产生根目录 metallib）；`CODE_SIGN_IDENTITY=-` 让 Xcode 自己签 |

注：push 用的凭据走 Windows 凭据管理器（`wincred` helper，repo 级配置）；
`gh.exe` helper 路径已失效，与本项目无关。
