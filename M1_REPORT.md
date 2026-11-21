# M1 桌面验证报告

日期：2026-11-08 · 状态：**通过**

## 目标

用官方资产在 Windows 桌面跑通完整的策略回路，并产出可复现、可逐 tick 对比的参考轨迹：

- 官方 MJCF（`scene_backlash.xml` → `robot_groundcontact_backlash.xml`）
- 官方 ONNX（`velstand.onnx`，默认走路策略）
- 官方执行器模型 BAM M6（Dynamixel XL330）
- 50 Hz 控制回路（20 ms tick，4 × 0.005 s 物理步）
- 61 维观测 → 14 维动作

## 上游版本

| 组件 | 版本 / commit |
|---|---|
| microduck | `da34a22`（2026-11-06，release 0.18.9） |
| microduck_rl | `bae39f5`（2026-10-27） |
| BAM (better-actuator-models) | 1.0.1 @ `62bd8ce`（uv.lock 锁定） |
| 策略集 | Hugging Face `pollen-robotics/microduck-policies` |
| 运行时 | Python 3.12.14（uv venv） |
| MuJoCo | 3.10.0 |
| ONNX Runtime | 1.24.4 |

策略文件（10 个 ONNX + manifest）已下载到 `D:\AI\Duck\policies\`：

`velstand.onnx`（walk，默认）、`alpha_walking.onnx`、`alpha_stand.onnx`、`alpha_sitstand.onnx`、`alpha_ground_pick.onnx`、`ball_kick_left.onnx`、`ball_kick_right.onnx`、`roulade.onnx`、`roller.onnx`、`roller_crouch.onnx`

## 运行方式

`work/run_trace.py` 直接复用官方 `microduck_rl/scripts/infer_policy.py` 的 `PolicyInference` 类与 BAM 装配函数，只去掉 viewer 和终端输入，并按固定命令序列逐 tick 记录 CSV。

```powershell
# 标准走路（0.4 m/s 命令）
.venv\Scripts\python.exe run_trace.py --walk-vx 0.4 --duration 10 `
  --out traces/m1_stand_walk_vx04.csv

# 真机启动姿态（SIT）→ 站起 → 走
.venv\Scripts\python.exe run_trace.py --start-key SIT --walk-vx 0.4 --duration 12 `
  --out traces/m1_sit_boot_walk_vx04.csv
```

## 结果

| 命令 | 真实位移 | 平均速度（含起始 2 s 站立） | 摔倒 |
|---|---|---|---|
| vx = 0.15 | 0.004–0.008 m | ≈ 0 | 0 |
| vx = 0.2 | 0.448 m / 10 s | 0.045 m/s | 0 |
| vx = 0.3 | 1.001 m / 10 s | 0.100 m/s | 0 |
| vx = 0.4 | 1.457 m / 10 s | 0.146 m/s（稳态 ~0.19 m/s） | 0 |
| vx = 0.3 + wz = 0.5 | 0.767 m / 10 s | 0.077 m/s | 0 |
| SIT → vx = 0.4 | 1.881 m / 12 s | 0.157 m/s | 起身阶段 1 次姿态切换 |

官方 `infer_policy.py` 本尊在 vx = 0.4 下实测 `fwd = +0.18…+0.20 m/s`，与我们的运行器一致。

**确定性**：相同输入连续跑两次，CSV SHA256 完全相同
`DD341F91F84E913AEB116B34AB92E007D0AEA0E19A1308243572E194D2861F9E`

## 关键发现

1. **低速死区**：velstand v8 在 0.15 m/s 命令下不迈步（官方脚本同样 0.00）；≥ 0.2 m/s 才进入步态。这不是运行器 bug，是策略特性，移动端交互和验收都要按 ≥ 0.2 设计。
2. **`infer_policy.py` 仅支持 Unix**：模块级 `import termios`，Windows 需 `work/shim/termios.py`、`tty.py`。
3. **backlash 观测差异**：官方预演脚本读取 `qpos[servo]`；真机 `robotd` 的编码器在输出侧，观测应为 `qpos[servo] + qpos[backlash]`（RL 仓库 `BacklashEncoderBamActuator` 的语义）。移动核心必须实现后者，才能与真机 1:1。
4. **BAM 版本必须锁 commit**：分支 head 的 `MujocoController` 参数已改名（`vin_drop_resistance`），与 `infer_policy.py` 不兼容；必须使用 uv.lock 的 `62bd8ce`。
5. **官方 CPU 预演可作为 M1 基准**，但它不是完整 `robotd` 孪生。真正的 1:1 对照应是 `robotd + duck-body`（官方 `scripts/duck-sim`），列入 M1.5。

## 结论

- 官方资产链（MJCF → BAM → ONNX → 50 Hz）在桌面完整跑通。
- 走路、转向、SIT 起身后行走均可复现。
- 参考轨迹已固化，后续移动端核心必须逐 tick 对齐这些轨迹。

## 下一步（M2 决策）

需要确定：

1. 移动平台顺序（Android 先 / iOS 先 / 双端）。
2. UI 与 3D 渲染壳（Unity / Godot / 原生 SwiftUI+Compose / Web 原型）。
3. 核心实现（复用官方 Rust `duck-control` / 新建 C++ 核心 / Python 原型）。
4. M2 范围（走路 + 虚拟房间 / 全技能 / daemon API 优先）。
