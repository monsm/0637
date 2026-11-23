# _archive · 桌面版时代的遗物（2026-11-23 归档）

产品只有 iOS 与 Android 两端（README「产品只有 iOS 和 Android 两端」）。
M1 阶段曾用 Windows 桌面跑通完整策略回路（Python + MuJoCo + ONNX 逐 tick
对比官方 duck-sim），桌面 sim harness 的代码已随桌面版移除；这里归档的是
它的报告与本机构建产物，仅作历史留痕，不再维护、不再运行。

| 目录 | 内容 |
|---|---|
| `desktop-m1/` | M1 桌面验证报告、MUJOCO_LOG.TXT |
| `desktop-m1/scripts/` | 桌面 MuJoCo 试验脚本（jaw_*、GLSL 着色器、traces/ 轨迹 CSV）——`scripts/reference-render.py` 取代了 jaw 渲染试验的正式职能 |
| `builds/` | 桌面版时代的构建残留：mujoco-3.10 源码与 cmake 构建、桌面 sim 的 Rust target 目录（target-mj-r2/3/4）、参考渲染、杂项试验 |

`builds/` 与 `desktop-m1/scripts/` 是二进制/草稿，已加入 .gitignore——本机留痕，不入库；
`desktop-m1/M1_REPORT.md` 入库（文档引用它）。

## 仍然在用、没有归档的（两端共用，勿删）

- `work/mingw-binutils/`、`work/ort-win/`、`work/shim/` — **Rust 核心本机测试**所需
  （`cargo test` 在 Windows 上链接 MuJoCo/ORT；见记忆 local-core-test-loop）。
- `work/verify_odom_roller/`、`work/audit_fp/` 等 — 官方语义验证脚本，随批使用。

## 本机软件

桌面版 Python 依赖（pip: mujoco / mujoco-studio / glfw / onnxruntime）与相关
软件是否卸载，见 REPORT.md 当日批次记录中的卸载清单——卸载需老板逐项确认。
