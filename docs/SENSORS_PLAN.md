# 手机传感器 / AR 集成设计（M2.3 剩余 + M3 AR）

> 原则（见 FIDELITY.md §4）：手机传感器**永远不替代鸭子的本体感知**。
> 鸭子的 61 维观测里只有它自己的陀螺/重力/关节/上一动作/命令；
> 手机传感器只能驱动「世界」——光照、地形、外部扰动、AR 锚定。

## 1. 传感器 → 语义映射

| 手机传感器 | 语义 | 注入点 | 状态 |
|---|---|---|---|
| IMU（加速度/陀螺） | 外部扰动（摇晃=推搡） | `duck_core_push`（躯干速度脉冲） | ✅ iOS 已接 |
| 触屏 | 推/摸（接触点力） | `duck_core_push` 或未来点接触 API | 部分（摇杆已接） |
| 相机（ARKit/ARCore） | AR 模式：鸭子渲染进真实房间；AR 平面 → 虚拟地面 | 渲染层 + `MujocoIo` 地面高度 | 未开始 |
| LiDAR / 深度 | 环境障碍网格 → MuJoCo geom（可选） | `MujocoIo::load` 附加 geom | 未开始 |
| ToF（鸭子头部 8×8） | 仅自治模式需要；官方策略对球"失明" | 暂不需要 | 未开始 |
| 麦克风 | 环境声音事件（app 层，不进 61 维观测） | UI/技能触发 | 未开始 |
| GPS/气压计 | 场景选择标签（非鸭子感官） | UI | 未开始 |
| 蓝牙手柄 | 与官方 `padd` 相同的 intent 通道 | C ABI（已具备） | ✅ C ABI 已具备 |

## 2. AR 模式（iOS 先行）

```text
ARKit Session                    Metal Renderer
  ├─ ARWorldTrackingConfiguration   ├─ 相机内参/位姿 ← ARFrame
  ├─ planeDetection(.horizontal)    ├─ 官方 mesh 按 body 位姿绘制
  └─ sceneDepth (Pro 机型)          └─ 地面 geom 高度 = 检测平面 y
        │
        ▼
  DuckCore（不变）
  └─ 50 Hz 循环照旧；AR 只改「世界」，不改「鸭子」
```

- AR 平面 y 值 → MuJoCo 地面平面高度（`MujocoIo` 暴露 `set_floor_height`），
  使鸭子"站在你的桌子上"成为物理事实而不是视觉贴图。
- 障碍物（可选，V2）：LiDAR 场景网格简化为若干静态 box geom。

## 3. 工程拆分

| 步骤 | 内容 | 依赖 |
|---|---|---|
| S1 | iOS：ARKit 相机透视 + 鸭子锚定到检测平面（只读平面高度） | CI 绿 |
| S2 | `MujocoIo::set_floor_height` + shim `duck_mj_set_floor_z` | S1 |
| S3 | Android：ARCore 对等实现（SceneViewer/ARCore session） | S2 |
| S4 | 麦克风声音事件 → 技能触发（拍手=坐下 之类的用户可配置映射） | S1 |
| S5 | LiDAR 障碍 box 注入（V2，可选） | S2 |

## 4. 验收

- AR 模式下鸭子在检测平面上行走/摔倒/起身，物理与虚拟房间模式一致
  （同一条 50 Hz 回路，仅地面高度不同）。
- 手机坐标系变化（换桌子、斜面）不改变鸭子本体感知分布——
  观测仍来自 MuJoCo，避免 out-of-distribution 输入破坏 1:1。
