# Duck — MicroDuck 数字版

iOS / Android 上的 MicroDuck 数字孪生。目标：官方 MJCF 身体 + 官方 ONNX 策略 + 同一套控制语义，行为与官方 `duck-sim` 对齐。

## 目录

```
D:\AI\Duck
├─ FIDELITY.md          保真度定义、能做到/做不到的边界
├─ M1_REPORT.md         桌面验证报告（已完成）
├─ M2_PLAN.md           iOS 优先 / 原生 Metal / Rust 核心 / 全技能
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
├─ .github/workflows/   iOS macOS CI
└─ work/
   ├─ .venv/            Python 3.12 环境
   ├─ run_trace.py      无界面官方策略轨迹运行器
   ├─ body_bam.py       BAM MuJoCo TCP body server
   ├─ shim/             Windows 下 termios/tty 兼容层
   └─ traces/           M1 参考轨迹 CSV
```

## 快速复现

```powershell
D:\AI\Duck\work\.venv\Scripts\python.exe D:\AI\Duck\work\run_trace.py `
  --walk-vx 0.4 --duration 10 --out D:\AI\Duck\work\traces\check.csv
```

## CI（推送 GitHub 构建）

推送仓库后自动运行：

| 工作流 | Runner | 产物 |
|---|---|---|
| `.github/workflows/ios.yml` | macOS 14 | `Duck.app`（模拟器，未签名） |
| `.github/workflows/android.yml` | Ubuntu | `Duck-android`（debug APK） |

两个工作流都会自行拉取上游 MJCF/STL 与官方 ONNX（`upstream/`、`policies/` 不入库），
构建 MuJoCo/ONNX Runtime 并编译 Rust 核心；本地不需要 Android SDK 或 Xcode。
iOS 真机安装才需要 Apple Developer 签名。

## 关键约束

- 走路命令需 ≥ 0.2 m/s（策略低速死区）。
- BAM 必须用 uv.lock 锁定的 `62bd8ce`。
- 真机 `robotd` 的关节观测包含 backlash 输出侧角度，移动核心需实现同样语义。
