# M2 计划 — iOS 优先 · 原生性能 · 全技能

日期：2026-11-08 · 状态：进行中

## 已定方向

| 决策 | 选择 |
|---|---|
| 平台顺序 | **iOS 先通，Android 紧随** |
| 渲染 | **原生 SwiftUI + Metal**（性能优先，不用 Unity/Godot） |
| 控制核心 | **复用官方 Rust `duck-control` + `robotd` 技能调度器** |
| M2 范围 | **全部官方技能**（walk / stand / sitstand / groundpick / kick / roulade / roller） |

## 架构

```
SwiftUI (UI) ── Metal (渲染 15 个刚体) ── 60/120 fps
        │  qpos / 状态
        ▼
Rust duck-mobile-core (静态库, aarch64-apple-ios)
  ├─ duck-control: obs(61) / Policy(ONNX) / Safety / Fall / Pickup
  ├─ robotd::control: 技能优先级与时间窗（官方文件直接复用）
  ├─ 50 Hz 控制循环（20 ms tick）
  └─ RobotIo 实现 ← MuJoCo C API（官方 MJCF + BAM M6）
        │
        ▼
MuJoCo 3.10（iOS 静态库） + ONNX Runtime（iOS 静态/内嵌 framework）
        │
        ▼
官方资源: scene_backlash.xml / STL mesh / velstand.onnx 等
```

### 关键点

1. **`duck-control` 已打通平台无关**：新增 `hardware` feature，移动端 `default-features = false` 即可去掉 `rustypot`/`serialport`。
2. **技能调度器不复制**：`robotd/src/control.rs` 通过 `#[path]` 直接编进移动核心，优先级、时间窗、低通参数与真机同源。
3. **MuJoCo 后端**：实现 `RobotIo::read/write`，读 qpos/qvel/IMU，写关节目标；物理步 0.005 s × 4 = 20 ms。
4. **观测必须含 backlash 输出侧角度**：`qpos[servo] + qpos[backlash]`，与真机 `robotd` 一致（官方 Python 预演脚本没做这一点）。
5. **渲染只消费 qpos**：15 个刚体按 body pose 更新；在 50 Hz tick 之间做插值，渲染帧率与物理解耦。
6. **模型/策略打包**：MJCF + STL + ONNX 作为 app resources；manifest 决定技能/时长/动作缩放。
7. **iOS 构建**：Windows 无法编译/签名；用 GitHub Actions macOS runner 出模拟器构建，真机需要 Apple 签名。

## 里程碑

| 阶段 | 交付 | 验收 | 状态 |
|---|---|---|---|
| M2.0 | Rust 核心编译 + 单元测试；桌面 MuJoCo `RobotIo`；50 Hz 循环 | 与 M1 轨迹逐 tick 对齐 | ✅ 完成（BAM 对拍到 1e-9） |
| M2.1 | iOS 壳 + Metal 渲染 + 虚拟房间 + 走路 | 手机上 velstand 走路，60 fps | 代码完成；待 macOS CI 验证 |
| M2.2 | 全部技能 + 技能 UI + 状态机 | 每个技能可触发、可回退 | ✅ 核心完成（26 s 全技能实测） |
| M2.3 | 触屏/蓝牙手柄 + IMU/相机/麦克风 | 传感器映射与扰动模式 | 部分（IMU shake→push） |
| M2.4 | 策略管理 + intent API | 导入 ONNX、热切换、自定义 AI | ✅ 完成（Reload AI + HTTP intent API） |
| M3 | Android 移植 + AR | 双端一致 + AR 模式 | 未开始 |

## 当前阻塞

- **iOS 编译/签名需要 macOS**。Windows 侧代码、资源与 CI 脚本已完成；
  `.github/workflows/ios.yml` 在 macOS runner 出模拟器构建。
- 真机安装需要 Apple Developer 签名（后续确认）。
- Android 需要 JDK + Android SDK/NDK（未开始）。

## 风险

1. **iOS 原生 + Metal 工作量**：渲染器、交互、资源管线都要自己写。
2. **MuJoCo/ONNX Runtime 的 iOS 集成**：静态库/framework、C ABI、bitcode/架构切片。
3. **全技能范围**：7 类技能 + 状态机 + UI，首版集成风险高（已选，按 M2.2 分步验收）。
4. **保真基准**：M1 用的是官方 CPU 预演；M2.0 应补 `robotd + duck-body` 的官方孪生对照。
