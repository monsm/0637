# MicroDuck 数字版 — 阶段性成果报告

日期：2026-11-09 · 里程碑：M1 完成、M2 双平台端到端打通

---

## 一、目标与验收口径

把 Pollen Robotics × Hugging Face 的物理 MicroDuck（约 25 cm / 800 g 的 RL 双足机器鸭）
做成 iOS + Android 数字版，**1:1 复刻**外观与机械运动。

「1:1」按 [FIDELITY.md](FIDELITY.md) 的定义执行：

| 层级 | 含义 | 状态 |
|---|---|---|
| L1 模型一致 | 与官方 MJCF / STL / ONNX / BAM 参数逐字节一致 | ✅ |
| L2 运动一致 | 同一策略、同一观测、同一执行器模型，轨迹与官方仿真对齐 | ✅（M1 实测） |
| L3 场景一致 | 与 `duck-sim` 同一场景与控制栈 | ✅ |
| L4 物理机体一致 | 与某一台实体鸭逐参数一致 | ❌ 不可能（个体差异），已声明 |

---

## 二、M1：官方栈验证（桌面/仿真）

**结论：官方模型 + BAM 执行器 + ONNX 策略在本地闭环复现成功。**

| 项 | 结果 |
|---|---|
| 模型 | 官方 `scene_backlash.xml` + 38 个 STL 网格（215,317 顶点 / 431,802 面） |
| 执行器 | BAM M6（XL330，电压控制律 + 反电动势 + Stribeck/方向/二次摩擦），与官方 Python 数值对拍 **误差 < 1e-9** |
| 策略 | `velstand.onnx`（61 维观测 → 14 动作），ONNX Runtime 1.24 |
| 行走实测 | 0.4 m/s 指令 → **1.457 m / 10 s**，trunk 高度 0.114–0.119 m，全程直立 |
| 全技能 | sitstand / groundpick / kick / roulade / roller 全部加载并执行 |

---

## 三、M2：移动端双平台（本次主要成果）

### 3.1 架构

```
┌───────────────────────────────┐
│  iOS (SwiftUI + Metal)        │  Android (Compose + GLES3)  │
└──────────────┬────────────────┘
               │  C ABI (DuckCore)
┌──────────────┴────────────────┐
│  duck-mobile-core (Rust)      │
│  ├ robotd 技能调度器（原样复用）│
│  ├ duck-control：观测/策略/安全 │
│  ├ BAM M6 执行器模型（Rust）    │
│  └ MuJoCo 3.10（端内，静态链接）│
│     + ONNX Runtime（策略推理）  │
└───────────────────────────────┘
```

关键决策：**不重写官方控制逻辑**。`robotd/src/control.rs` 通过 `#[path]` 直接编译进来，
`duck-control` 只加 feature 补丁去掉 Dynamixel 硬件总线，其余（观测布局、策略运行时、
安全、摔倒判定、技能优先级 roulade>kick>groundpick>sit>stand>walk）与官方一致。

### 3.2 CI 实测（GitHub Actions，非本地构建）

两端都在**模拟器/模拟机里真启动、真跑策略、真响应 API**：

| 验证项 | iOS | Android |
|---|---|---|
| 构建产物 | 模拟器 `.app` / 未签名真机 IPA 25.7 MB（含 device slice + 着色器 + 10 个策略） | **APK 49.0 MB** |
| 安装 + 启动 | ✅ | ✅ |
| MuJoCo 场景加载 | ✅ | ✅（nq=35 nv=34 nu=14，38 网格） |
| ONNX 策略推理 | ✅ | ✅ |
| Bring-up（Limp→Homing→Ready） | ✅ `bringup: 2` | ✅ `bringup: 2` |
| 姿态 | `gravity ≈ (0.03, 0.01, −0.999)`，trunk_z = 0.1156 m | `gravity ≈ (0.034, 0.010, −0.999)`，trunk_z = 0.1156 m |
| 当前策略标签 | `walk` | `walk` |
| Intent API `/state` `/skills` | ✅ | ✅ `roulade / kick_left / kick_right` |
| 3D 渲染 | ✅ 视口中心点亮 **100%**，70 个 geom，三帧互不相同 | ✅ 画面检查（点亮率 + 颜色数） |
| 行走 | ✅ `/twist` → trunk 位移 **0.696 m** | ✅ 冒烟内驱动 |
| 坐 / 站 | ✅ `sit → rise` | — |
| 技能逐个运行 | ✅ roulade / kick_left / kick_right | — |
| 地面抓取 | ✅ `label=ground_pick` | — |
| 外力推挤 | ✅ 推后仍 `bringup=2` | — |
| 策略热重载 | ✅ 重载后技能仍可用 | — |
| UI 按钮（XCUITest） | ✅ 逐个点击并断言标签变化 | — |
| 冒烟结论 | **32/32 检查通过** | **smoke test passed** |

模拟器内实测状态（两端一致）：

```json
{"bringup":2,"fallen":false,"gain":200,"label":"walk",
 "gravity":[0.034,0.010,-0.999],"trunk":[0.002,0.0004,0.1156]}
```

### 3.3 App 功能

- 3D 渲染：官方 STL 网格逐 geom 绘制，金属/着色器，跟随 trunk 的第三人称相机
- 控制：摇杆（vx / wz）、Sit/Stand、Ground pick、技能按钮（roulade / kick_left / kick_right）、Reload AI
- 手机传感器：CoreMotion 50 Hz，晃动 → `duck_core_push` 给躯干施加扰动（Twin 语义：
  手机不替代本体感知，只提供外部扰动）
- 本地 Intent API（`0.0.0.0:8765`）：`/state` `/skills` `/twist` `/head` `/body`
  `/skill` `/sit` `/ground_pick` `/push` `/reload`，供用户自己的 AI agent 驱动
- 自定义策略：把 ONNX + manifest 放进 `Documents/policies` 后 Reload AI

---

## 四、期间解决的关键工程问题

调试全部在 CI 内完成（禁止本地移动构建），日志通过 `ci-logs` 分支 + `duck-rust` logcat
标签 + 未 strip 符号库回传。主要根因链：

| # | 问题 | 根因 | 修复 |
|---|---|---|---|
| 0 | **真机/模拟器 3D 全黑**（界面正常） | `stage-resources.sh` 每次构建 `rm -rf DuckResources` 后只拷回 XML/STL/ONNX，**把 `Shaders.metal` 删了**；包里没有着色器 → `Renderer.init?` 返回 nil → MTKView 无 delegate → 一帧不画。整屏点亮率 8.19%，视口中心 **0.0000%**、1 种颜色 | 着色器移到 `ios/DuckApp/Shaders/`（staging 删不到），staging 拷回并缺失即失败；设备包断言；截图检查增加 `--region` 视口区域并检查相邻帧差异 |
| 0b | 地板和朝上的面全黑 | iOS 着色器 `light = normalize(-uniforms.light.xyz)`，传入 `z=+1` 导致光从地板下方照来，`dot((0,0,1), light) < 0`；Android 无此负号 | 改为 `z=-1`，与 Android 一致 |
| 0c | iOS 构建一直红（`7b3b8ec` 起） | `createCore` 内 `self` 非可选，`DispatchQueue.main.async { guard let self }` 编译报错 | 内层闭包加 `[weak self]` |
| 0d | **鸭子散了：零件旋转、错位、悬空** | MuJoCo 编译时给每个 mesh 重算坐标系（`mesh_pos`/`mesh_quat`），`mesh_vert` 是变换后的顶点；渲染器直接读原始 STL，只套了几何体位姿 | 新增 `duck_core_mesh_soup` / `nativeMeshSoup`，直接展开 `mesh_vert`+`mesh_face`（含法线），两端共用；STL 解析器删除 |
| 0e | 整只鸭子是灰的 | `geom_rgba` 对全部 70 个可见 geom 都是默认灰 0.5——颜色在 `mat_rgba`，经 `geom_matid` 引用，只有渲染器解析 | shim 增加 `duck_mj_geom_colour`，按材质解析；16 种颜色到达渲染器 |
| 0f | 面向镜头的一侧几乎全黑 | 固定世界光 `(-0.45,-0.65,-1)`，侧面无漫反射 | 改为跟随相机的头灯（与 MuJoCo 官方视图一致） |
| 0g | 新 shim 函数链接报 `undefined symbol` | shim 的 C 链接来自头文件的 `extern "C"` 块，新函数未声明 → C++ 改名 | 在 `duck_mj_shim.h` 中声明 |
| 1 | Android 闪退 | NDK 的 `libdl.a` 是"静态可执行文件"shim，把 `malloc/newlocale/__cxa_atexit` 静态拉进 .so，scudo 全局未初始化 → 除零 | 用 llvm-ar 只保留 dl 成员重建 `libdl.a`；CI 断言 bionic 符号不出现 |
| 2 | Android 链接失败 | 缺 `__gxx_personality_v0`（C++ ABI）；API 21 stub 缺 `pthread_atfork`；API 28 无 ELF TLS | 补 `c++abi`、`--no-undefined`、`--platform 29`，minSdk 29 |
| 3 | Android 崩溃 | MuJoCo C++ 异常穿越进 Rust → abort | shim 改为 C++17 + try/catch，错误走错误缓冲 |
| 4 | STL 加载失败 | MuJoCo 3.10 的解码器是**插件归档成员**，无引用 → 被归档丢弃 | iOS `-force_load`、Android 在 `--whole-archive` 间直接给归档路径 |
| 5 | Android 加载停滞 | `duck_core_new` 在主线程，UI 冻结 | 移到 `Dispatchers.IO`，UI 显示加载态 |
| 6 | 冒烟测不通 | `adb reverse` 方向错（设备→宿主） | 改 `adb forward`（宿主→设备） |
| 7 | 模拟器慢 50 倍 | runner 用户不在 `kvm` 组，静默退化为软件模拟 | `sudo chmod 666 /dev/kvm` |
| 8 | iOS 装不上 | 根目录 `Resources/` 让 codesign 按 macOS 深包处理 → plist 未绑定；`default.metallib` 触发 Xcode 26 签名限制；`-force_load,path` 逗号形式被 ld64 静默忽略 | 资源目录改名 `DuckResources`、着色器运行时编译、`-force_load` 拆两个参数 |

---

## 四之二、与真机的保真度审计（2026-11-09）

用 6 个维度（关节传感、61 维观测、控制循环、安全/摔倒、执行器与物理、技能/manifest）
逐行对比移动核心与官方 `robotd`/训练栈，共 56 条候选、33 条中等以上，经对抗验证后
**确认 32 条**（1 条被驳回）。已修复的高危项：

| 分歧 | 真机的做法 | 修复 |
|---|---|---|
| 关节观测只读马达侧角度 | 编码器在齿隙**输出侧**：`qpos[servo] + qpos[backlash]`（训练栈 `joint_pos_rel_backlash`、真机 `present_position`） | 观测与 BAM 电压环都改为和值；速度观测同样取和，BAM 的 `dq` 保持马达侧（与 `BacklashEncoderBamActuator` 一致） |
| limp-fall 是固定 500 ms + 冻结姿态 | 陀螺判落地（1.0 rad/s 持续 200 ms，上限 1500 ms），随即 600 ms 把落地姿态插值回站立姿态（gain 160），再交还策略；触发有「正在驱动/未摔倒/不忙/不在坐/IMU 就绪」门控 | 按原状态机移植为 `LimpFall::{Idle,Limp,Posing}`，删除自创的 2 s 恢复窗 |
| 指令无平滑 | 每个 tick `cmd += 0.2 × (target − cmd)`（`cmd_alpha`/`head_alpha`） | 摇杆 twist、头部、身体姿态都按 0.2 平滑；身体姿态加 `active` 标志（松开即回零、期间 twist 置零） |
| Limp 阶段不写不发步 | 力矩关但是仍然写目标并推进物理 | 补 `safety.apply(..., 0)` |
| Android 从 `onDrawFrame` 跑控制 | 真机固定 50 Hz | 独立 `ScheduledExecutorService` 每 20 ms tick，渲染只读状态 |
| 摇杆偏航反向、无死区 | 0.1 死区，偏航取反 | 两端一致 |
| API 租约 1 s 长于 deadman | deadman 500 ms | 租约改为 500 ms |

审计同时**驳回了 1 条**（IMU 中位数滤波），并在复核中否掉了 2 条中危（BAM 摩擦数组实际正确；
1.75 A 电流限在引用的训练文件里不存在）。

尚未处理的中危项：策略热重载未走「回 homing / carry_over」、扁平 manifest 会得到空技能表、
MuJoCo 积分器（Euler vs 训练用的 implicitfast）与求解迭代次数（100 vs 30）。

---

## 五、iOS 真机安装说明

当前 iOS 产物是**模拟器版**（ad-hoc 签名），装不进物理 iPhone —— 平台限制，非配置问题。

已就绪的真机流水线（`14ff01f`）：配置 5 个 secrets 后 CI 自动额外产出 **`Duck-ios-ipa`**：

`IOS_CERT_P12_BASE64` / `IOS_CERT_PASSWORD` / `IOS_PROVISIONING_PROFILE_BASE64` /
`IOS_PROVISIONING_PROFILE_NAME` / `IOS_TEAM_ID`（详见 [docs/IOS_SIGNING.md](docs/IOS_SIGNING.md)）。

无 secrets 时该 job 自动跳过，不影响双平台构建。CI 签名需付费 Apple 开发者账号；
`development`/`ad-hoc` 还要求目标设备 UDID 已登记。

---

## 六、当前产物

| Artifact | 大小 | 说明 |
|---|---|---|
| `Duck-android` | 49.0 MB | 可直接安装的 debug APK |
| `Duck-android-smoke-logs` | 3.0 MB | logcat、intent 应答、阶段日志 |
| `Duck-core-unstripped` | 2.9 MB | 带符号的 .so，用于 tombstone 符号化 |
| `Duck-ios-app` / `Duck-ios-simulator` | 26.7 / 25.8 MB | 模拟器 `.app` 与 zip |
| `Duck-ios-unsigned-ipa` | 25.7 MB | **未签名真机 IPA**（device arm64，含着色器 + 10 个策略），用 Sideloadly/AltStore 自签后安装 |
| `Duck-ios-smoke-logs` | 0.8 MB | 三帧截图、NSLog 轨迹、intent 应答、会话日志 |
| `Duck-ios-ipa` | — | 配置签名 secrets 后产出 |

---

## 七、未完成 / 下一步

| 项 | 状态 |
|---|---|
| iOS 真机 IPA | 流水线就绪，等签名凭据 |
| 相机 / LiDAR / AR 模式 | 规划见 [docs/SENSORS_PLAN.md](docs/SENSORS_PLAN.md)，未实现 |
| 麦克风 / 扬声器 | 未实现 |
| 渲染器 PBR / 阴影 / 帧间插值 | 当前为基础 PBR + 单光源，未做插值 |
| Android 真机实测 | CI 仅覆盖模拟器（API 30 x86_64）；arm64 真机未跑 |
| 性能指标（帧率/功耗） | 未采集 |

---

## 八、仓库与复现

- 仓库：`github.com/monsm/0637`
- Android/iOS 构建全部由 GitHub Actions 完成，产物在对应 run 的 Artifacts
- 上游固定：`pollen-robotics/microduck@da34a22` + `patches/duck-control-mobile.patch`
- CI 迭代记录见 [M2_PROGRESS.md](M2_PROGRESS.md)
