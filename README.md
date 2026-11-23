# Duck — MicroDuck 数字版

iOS / Android 上的 MicroDuck 数字孪生。目标：官方 MJCF 身体 + 官方 ONNX 策略 + 同一套控制语义，行为与官方 `duck-sim` 对齐。

## 目录

```
D:\AI\Duck
├─ FIDELITY.md          保真度定义、能做到/做不到的边界
├─ M2_PLAN.md           iOS 优先 / 原生 Metal / Rust 核心 / 全技能
├─ _archive/            桌面版时代归档（M1 报告 + 构建产物，只读留痕，不再维护）
├─ M2_PROGRESS.md       Rust 控制回路闭环实测
├─ upstream/
│  ├─ microduck/        官方机器人运行时（Rust）
│  ├─ microduck_rl/     官方 RL 训练/仿真（MJCF + mesh + infer_policy.py）
│  └─ bam/              Better Actuator Models（锁定 commit 62bd8ce）
├─ policies/            官方 ONNX 策略 + manifest（来自 Hugging Face）
├─ core/
│  └─ duck-mobile-core/ Rust 移动核心：官方 duck-control + robotd 调度器
│                       + BAM M6 + 端内 MuJoCo + C ABI + manifest
├─ ios/                 SwiftUI + Metal + XcodeGen 工程与构建脚本
├─ android/             Kotlin + GLES3 + Gradle 工程与构建脚本
└─ .github/workflows/   core / iOS / Android 三条 CI
```

## 快速复现

两端都由 CI 自动构建（见下表）。本地验证核心用 CI 同命令：

```bash
cd core && cargo test --locked --workspace -- --include-ignored
```

（需要 Rust 1.99.1 与 ONNX Runtime >= 1.23 的动态库；核心引用官方
fixture，缺文件即失败不是跳过。）

**产品只有 iOS 和 Android 两端**——桌面 sim harness（work/run_trace、
body_bam 与核心的 sim_* bins）已随桌面版移除；`upstream/` 里的官方
duck-sim 仍是验证基准，但不是产品。

## CI（推送 GitHub 构建）

推送仓库后自动运行：

| 工作流 | Runner | 产物 |
|---|---|---|
| `.github/workflows/core.yml` | Ubuntu | 核心 Rust 单元测试（两端共用的那颗心脏） |
| `.github/workflows/ios.yml` | macOS 14 | `Duck.app`（模拟器，未签名） |
| `.github/workflows/android.yml` | Ubuntu | `Duck-android`（debug APK） |

三个工作流都会自行拉取上游 MJCF/STL 与官方 ONNX（`upstream/`、`policies/` 不入库），
构建 MuJoCo/ONNX Runtime 并编译 Rust 核心；本地不需要 Android SDK 或 Xcode。
iOS 真机安装才需要 Apple Developer 签名。

核心的测试在 `core.yml` 里跑：它按工作区解析官方策略与训练场景
（`DUCK_POLICIES_DIR` 可改），CI 设 `DUCK_REQUIRE_FIXTURES=1`，所以**缺一个官方文件就是失败，
不是跳过**——一个什么都没验的绿灯比红灯更糟。

## 关键约束

- 走路命令需 ≥ 0.2 m/s（策略低速死区）。
- BAM 必须用 uv.lock 锁定的 `62bd8ce`。
- 真机 `robotd` 的关节观测包含 backlash 输出侧角度，移动核心需实现同样语义。
