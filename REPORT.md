# MicroDuck 数字版 — 阶段性成果报告

日期：2026-11-09 · 里程碑：M1 完成、M2 双平台端到端打通

> **最新全绿基线（2026-11-11）**：iOS `f8cf1eb` 14/14 步全绿（模拟器 141 项会话断言 141 通过、XCUITest 通过、设备切片、未签名 IPA）；Android `7473221` 11/11 步全绿（含模拟器冒烟与帧差验证）。

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
| 0h | Android 正对镜头的鸭身只被环境光照亮（白壳发灰、地板没有主光） | Android 头灯向量传成 `eye - trunk`，shader 又做一次 `normalize(-uLight)`，主光因此永远背对镜头；iOS 传的是 `focus - eye`（相机→鸭子），方向正确。既有 bug，B3 舞台移植时被对抗审查抓出 | 改为 `trunk - eye`，与 iOS 同向。CI 判帧「亮中性壳」从门槛边缘升到 7908 px（门槛 600），暖石地板与日光染色才真正吃到主光（`5f097b4`） |
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

第二批（同样已 CI 验证，iOS 33/33 + XCUITest，Android 冒烟）：

| 分歧 | 修复 |
|---|---|
| 热重载原地换控制器，丢掉座位/技能/滤波锚点 | 按 robotd `Controller::carry_over` 交接；同一网络（摘要相同）保留 LSTM 记忆 |
| 扁平 manifest（社区单策略仓库）被解析成空集，技能表为空 | 同时接受扁平形状，文件名缺省时取目录里唯一的 `.onnx` |
| 求解器迭代 100/20（MJCF 默认） | 训练配置明确用 **30/50**，已对齐 |
| 真鸭闲着时会左右看/歪头/呼吸，App 里永远直视 | 把 robotd 的 `idle_head` 原文件编进核心并按原条件应用（其头部指令本身就是策略观测输入） |

审计同时**驳回了 1 条**（IMU 中位数滤波），并在复核中否掉了 3 条中危：BAM 摩擦数组实际写的是
`dof_frictionloss/dof_damping`（写 `jnt_solref` 的是无人调用的死函数）、1.75 A 电流限在引用的
训练文件里不存在、BAM 电压跌落模型实际存在（7.4 V 标称、0.1×负载、下限 6.0 V）。

积分器保留 MuJoCo 默认 Euler：训练侧代码里找不到 implicitfast 的证据，不凭猜测改物理。

---

## 四之三、时间机器、传感器、MCP 与新界面（2026-11-09 第二批）

| 能力 | 实现 | 验证 |
|---|---|---|
| **时间机器** | 所有墙钟计时器改为**仿真时钟**（tick 计数）；快照含 MuJoCo `mjSTATE_INTEGRATION`、BAM 力矩历史、LSTM 记忆、技能相位、安全去抖、时钟本身；核心保留最近 3000 tick | `/time` `/timeline` `/step` `/checkpoint` `/restore` `/seek`；单元测试「倒带重放同一轨迹」；CI 断言暂停不前进、单步精确 +10、0.5× 约半速、倒回后仍 ready |
| **分叉** | `mj_copyModel`+`mj_copyData` 深拷贝（BAM 每子步写摩擦进模型，必须各自持有）；策略重载独立会话；同一 checkpoint 恢复 | `/fork` `/branches` `/branch/select` `/branch/drop`；CI 断言分叉出现、可选中、可删除 |
| **传感器** | 相机 / 麦克风 / 深度 / IMU，读数带 `mode` 与 `in_observation:false`；IMU 作为外力推挤；深度可降采样为鸭子 8×8 ToF | `/sensor` `/sensors`；单元测试「晃动变推挤、静止不推」「ToF 8×8 且限幅」；CI 逐项断言 |
| **MCP** | `/mcp` JSON-RPC：initialize / tools/list / tools/call；工具表手写维护（2026-11-17 时 25 个 `duck_*` 工具；HTTP 路由与 MCP 工具是两份清单，未程序化同源——注释里「不会漂移」的说法无机制保证，未知工具 fail-closed） | 单元测试 3 条（初始化、未知工具失败闭合、限幅）；CI 断言工具列表与 `duck_state` |
| **PBR 渲染** | Cook-Torrance + 解析环境光 + 菲涅尔边缘 + 高光软限幅 + 接触阴影 | 颜色断言（橙色 313 px / 白壳 1814 px） |
| **融合界面** | 全屏鸭子（拖动画布走/长按技能环）→ 仪器层（时间轴/传感器/指纹）→ Director 取景器 | XCUITest 开环选 roulade、开时间机器冻结/恢复 |

期间修掉的两个真 bug：**暂停死锁**（暂停后 shell 不再 tick，而解冻请求正是在 tick 里排空的）、**慢放失效**（每帧强制至少一个 tick）。

---

## 四之四、地毯式对抗审计（2026-11-10）

8 个视角（运动/反射/感知/交互/身体/软件契约/自身宣称/跨平台）各自"证明数字版不是真鸭子"，
58 条声称，经逐条反驳验证后**确认 32 条、驳回 14 条**。已修复的高危/中危项：

| 分歧 | 后果 | 修复 |
|---|---|---|
| BAM 静摩擦常数写进 `jnt_solref/jnt_solimp`（关节限位约束），而非 `dof_solref/dof_solimp`（摩擦约束） | 关节没有真静摩擦：轻推后 2 s 漂移 0.45°，参考实现 0.03°；限位反而比参考更硬 | 改写到 dof 数组，与 `bam.mjlab.BamActuator` 一致。**修复后行走横向漂移从 -0.0997 降到 -0.0017**，位移 0.640 m |
| 空闲行为时钟 `Instant::now() + tick×20ms` 每 tick 加双倍时间 | 左右看/呼吸的频率是真鸭的两倍，打瞌睡也快一倍 | 锚定到固定原点 + tick 计数：每 tick 恰好 20 ms，暂停时冻结，重放时一致 |
| 摔倒过程中热重载会取消 limp-fall 反射 | 正在倒下的鸭子被交还给策略继续摔 | 反射期间不重置控制器，让它跑完 |
| 一次 IMU 读数被每 tick 重复施加 | 晃一下手机，鸭子被永久推 | 读数消费一次即清零 |

**驳回**的典型：审计称"没有嘴关节"，但官方 duck-sim 用的 `scene.xml` 同样没有嘴关节，
上游注释明确写着"没有策略驱动它，走路模型没有这个关节"——我们与官方参考一致。

其余确认项（未修，属于已知范围）：声音/扬声器、相机/AR 行为、拾取检测器、roller 模式、
电池关机、IMU 预热门控、摇杆的三种模式（姿态/蹲/倾）、技能请求的即时确认语义。

---

## 四之五、清掉审计余项（2026-11-10 第三批）

上表"其余确认项"中除了 roller 模式（需要 `roller.onnx` 作为主动力网络，属于换一套策略集，
不是补一个功能）之外，全部落地，并在 CI 里逐条断言：

| 项 | 实现 | 验证 |
|---|---|---|
| **声音** | `sounds/`（官方可种子化声码合成器）**原文件编译进核心**，不搬样本。`greet` 在 enable 时、`peck` 在 disable 前、`chirp` 按需；变体轮换，同一个种子永远是同一只鸭 | 单元测试 4 条（每个 tag 都有可听 PCM、同种子同声音、变体不重复、未知 tag 拒绝）；`/sound` 与 MCP `duck_sound`；两端各有 `AudioTrack` / `AVAudioEngine` 播放 |
| **本体感知发布** | `/state` 新增 `proprioception`：编码器侧关节位置与速度、电流、IMU（陀螺/重力/四元数）、本 tick 命令、`imu_ready`、`p_held`、`volts`、电量百分比。首 tick 之前形状相同，客户端不必区分 | CI 断言 `positions` / `targets` / `imu_ready` / `mouth` |
| **即时确认语义** | 一次性动作改为**带票据**：`/skill` `/sit` `/ground_pick` `/reload` `/time` `/step` `/checkpoint` `/restore` `/seek` `/fork` `/branch/*` 等 tick 决定后再答复，拒绝会带着原因回到客户端；控制流（`/twist` `/head` `/body`）仍不等待 | 单元测试 2 条（拒绝可见、无 tick 时报 pending）；CI 断言未知技能返回 `ok:false` + `no such skill` |
| **技能表一致** | `/skills` 补上 `sit` / `stand` / `ground_pick`，`/skill` 接受它们（`sit`/`stand` 幂等），壳里的技能环显示同一张表 | CI 逐项断言 6 个名字 |
| **拾取检测器** | `robotd/src/pickup.rs` **原文件编译进核心**，配官方 `pickup_detector.onnx`（两端都打进包）。只看走路/站立网络；技能、坐下、limp-fall 一律让开并重置窗口。模式切换时在新运行时上**重新加载**分类器 | 上游 6 条单元测试随核心一起跑 |
| **手拎** | `/carry` + 两端的「拎起」按钮：躯干上加弹簧力（`xfrc_applied`），**不是关节目标**，所以分类器读到的和真鸭被拎起来时一样；放下后由分类器交还策略 | CI 断言拎起后躯干升高 > 3 cm（`2001371` 实测 z 0.0593 → 0.2959 m） |
| **IMU 预热门控** | 25 个融合样本（真机 `Imu::ready()` 的 0.5 s）之前，home 保持、策略不驱动；home→ready 的推进也等这个门 | 与上游同一常量，`imu_ready` 发布在 `/state` |
| **电池** | 电压取 BAM 模型的端电压（`vin − 0.1×|torque|`，下限 6.0 V），不再硬编码 7.4 V；`battery_percent` 按官方 6.6–8.2 V 区间；10 s EMA 后 ≤ 6.6 V 触发关机序列 | `/state` 的 `volts` / `battery_percent`。**声明**：没有电量模型（上游也没有），自动关机只可能被持续饱和负载的压降触发，模拟一块真耗尽电池需要自己发明容量参数，故不做 |
| **嘴** | 新增 `/mouth` 与 MCP `duck_mouth`；官方走路场景没有嘴关节，所以**明确拒绝**并说明原因（`has_mouth:false`），而不是假装接受 | CI 按 `has_mouth` 分支断言：有则动，无则拒绝 |

第四批（相机/roller/插值/关机/摇杆/里程计）在 iOS 模拟器上 **90 项检查全过**
（含 roller→walk 往返、坐→憩姿→off→上电、`/state.pose`、拎起、声音、MCP），UI 测试通过；
Android 冒烟通过（同一 `2001371` 构建，`{"mode":"roller","ok":true}`、`pose` 字段齐备）。
相机合成本身**不在 CI 验证范围内**（模拟器无相机；CI 只验证无相机时的降级路径和 Director 模式不中断驱动）。

顺带修掉的两个 CI 问题：日志发布步骤在拷贝**之后**才 checkout 旧 `ci-logs`，导致每次发布的
都是上一次的日志（tombstone 日期早一天就是这么来的）；`patches/**` 不在 workflow 的触发路径里，
改补丁不会重建。

**roller 模式**仍属已知范围：它换的是整套策略（`roller.onnx` 作主动力网络、`roller_crouch` 顶替
ground pick、站立网络关闭），需要 manifest 级的模式切换，不是补一条分歧。

---

## 四之六、相机/AR、roller 模式与帧间插值（2026-11-10 第四批）

| 项 | 实现 | 验证 |
|---|---|---|
| **Director 相机** | 系统预览层（iOS `AVCaptureVideoPreviewLayer` / Android CameraX `PreviewView`）放在 3D 视图**后面**，3D 视图清屏透明、跳过原来的 4 m 影棚地板；物理地面仍在 z=0。相机不进 61 维观测，也没有逐帧回调，50 Hz 循环不受影响 | 权限在进入 Director 时才请求；相机按 1 Hz 把尺寸/帧率/曝光/FOV 作为 `mode:"ar"` 发到 `/sensor`；模拟器/模拟机没有相机时保持影棚外观，UI 测试进出 Director 并断言鸭子仍在走 |
| **roller 模式** | 按上游语义做成**整套 bundle 切换**：`roller.onnx` 作主动力网络、`stand=None` 且 `set_standing_disabled(true)`、`roller_crouch` 顶替 ground pick（周期 5.0 s / end_phase 0.7 / action_scale 0.8）、技能与 sit/stand 不变（上游对技能**不按模式过滤**）。场景换成 `scene_rollers.xml`，切换时重建运行时、bump 模型代数（两端渲染器据此重建网格）、清空时间轴与分叉、按上游习惯叫一声（roller 两声） | `/mode` 与 MCP `duck_mode`；单元测试断言 bundle 解析；本地 MuJoCo 测试断言 roller 场景可加载且仍是 14 个执行器；CI 切换 roller→walk 并断言状态、技能、未知模式拒绝 |
| **里程计** | 官方 `odometry` crate（接触点锚定 + IMU 重投影，MJCF 驱动正运动学）编进核心，按 robotd 的门控只在 IMU 预热后的新鲜样本上 tick；`/state` 新增 `pose`（躯干位置、yaw、锚点脚）。这是**鸭子自己的估计**，不是仿真真值。整份估计状态进入 checkpoint（上游加 `snapshot/restore`），倒带后不会发布一个「未来」的位姿 | `/state.pose`；CI 断言字段存在 |
| **关机序列** | `robot.shutdown` / 空电量（10 s EMA ≤ 6.6 V，官方 `BATTERY_EMPTY_V`）触发：坐下 → 从座位用 1 s 缓入 `REST_POSITION` → 保持 → 断力矩。App 没有 systemd，所以「关机」就是物理意义上的关机：力矩关闭、`powered_off` 锁存并发布，`/power` 重新上电 | `/shutdown` `/power`、MCP `duck_shutdown` / `duck_power`；CI 断言坐下→off→上电回到 ready |
| **摇杆三种模式** | 真机手柄的三件事缩减成手机能表达的三种：走/转（原手势）、看（头 yaw/pitch）、姿态（蹲 + 侧倾）；离开模式时把头回正、把姿态释放（手柄自己的退出语义） | 两端顶栏各三个 chip；iOS 拖动分流到 `duck_core_set_head` / `duck_core_set_body` |
| **帧间插值** | 保存最近两个 tick 的全身位姿，按 elapsed/20 ms 混合位置与四元数（双覆盖符号处理）+ 归一化；相机跟随插值后的躯干而不是 10 Hz 状态采样 | iOS 主线程快照；Android 由循环线程写入 `PoseHistory`、GL 线程读取（带锁），GL 线程从不碰 MuJoCo |

---

## 四之七、对第四批的地毯式对抗审核（2026-11-10）

10 个视角并行找问题（含逐条核对上一条总结），每条发现再由 2 个独立审查者尝试反驳：
57 条原始发现，取最严重的 18 条验证，**18 条全部确认、0 条被驳回**。已修复：

| 严重度 | 缺陷 | 修复 |
|---|---|---|
| 高 | **Pose 模式退出不清 `body_active`**：`releaseBody()` 调 `set_body(0,0,0)`，而 `set_body` 只置位不清位；一旦置位，控制器每 tick 把 twist 归零——点过任意摇杆模式后**走不动**，两端都是 | 新增 `duck_core_release_body` C ABI + JNI；两端「退出模式」改调它；切到其它模式时也释放 |
| 高 | **`/power` 后鸭子又坐下**：shutdown 的 `Sit::Sitting` 留在控制器里，`power_on` 只清 `powered_off`，复位策略不清座位；CI 只查 `bringup=2` 所以假绿（实际 trunk z 0.0593，label=sit） | `power_on`/`disable` 调上游 `Controller::forget()`（机器人重新上电时座位本就没了） |
| 高 | **模式切换丢掉拾取分类器**：新运行时没重建 `Watch`，整场 `p_held=null` | 切换后用原模型路径重新加载 |
| 高 | **MCP `duck_mode` 300 ms 超时**：切换要数秒 | 改用 30 s 预算（与 `/mode` 一致）；`/reload`、`/fork`、`/step` 同样按工作量给预算 |
| 高 | **选中的分叉不被发布**：`/state`、`DuckStatus`、渲染都读原运行时；且选中分叉每帧被 tick 两次（里程计翻倍） | 新增 `active_runtime()`，所有读路径与状态发布走它；分支循环跳过选中的那个 |
| 高 | **checkpoint 不含 `powered_off`/shutdown 相位**：倒带出不了「off」状态 | 写入并恢复；旧 checkpoint 无里程计时重启估计器而不是留在未来 |
| 高 | **iOS MTKView 背景不透明**：清屏色透明也被不透明背景色盖住，Director 看不到相机 | 背景改 `.clear`（影棚色由每帧 clearColor 提供） |
| 中 | 模式切换丢暂停/倍速、丢 `/reload` 加载的自定义策略目录 | 携带时钟；`/reload` 记住目录，切换按当前模式重载 |
| 中 | roller 回退调参用了 walking 默认值 | 按上游按模式取 0.8/3.0 等回退 |
| 中 | shutdown 的请求/空电检查在 Limp/Homing 早退之后，永远收不到 | 提到 bring-up 分支之前 |
| 中 | shutdown 期间 `/sit`、`/skill` 能把鸭子从座位上叫起来 | 关机中拒绝移动类意图 |
| 中 | iOS 姿态快照只读一次 body 数，roller 多出的身体永不插值 | 每帧读取 |
| 中 | 插值 alpha 硬编码 20 ms，0.5× 时中途冻住 | 记录最近两次快照的真实间隔 |
| 中 | Android 仪表把 `status[11]`(p_held) 当嘴；技能环重复 sit/ground_pick | 修正索引为 12；环只显示 `/skills` |
| 中 | Android 相机：关闭 Director 后仍可能绑定泄漏、可用标志先于画面翻转、上报伪造的 1280×720/30 | 监听器检查已销毁；效果开始/销毁时置 false；只上报平台真实分辨率 |
| 中 | CI：reload 检查只查 curl 状态；Android 冒烟从不确认循环在驱动；发布步骤会重发上一轮日志；日志重试会把另一端的工作流回滚 | 检查应答 `ok`；增加 label 检查；发布前删除本轮文件；重试时先快照本轮日志、再取分支新树、最后把本轮覆盖回去 |

**我上一条总结里说错的地方**（审核逐条核对的结果）：

- 「相机合成已验证」——**没有**。CI 里模拟器没有相机，验证的是「无相机时降级正常、Director 模式不中断驱动」，相机合成本身无法在 CI 里验证。报告已改写。
- 「拎起 z 0.090 → 0.327 m」——数字取自更早的一轮；`2001371` 实测是 **z 0.0593 → 0.2959 m**。报告已改写。
- 「空电量自动关机」——**没有电量模型**（上游也没有），EMA 电压只可能被持续饱和负载的压降压到 6.6 V 之下，模拟一块真耗尽的电池需要自己发明容量参数。报告已声明为不做。
- 「关机后 `/power` 能重新上电」——**当时是假绿**：CI 只查 `bringup=2`，而鸭子被座位卡住后坐下，依然报 ready（trunk z 0.0593、label=sit）。现已修 `Controller::forget()`，并把 CI 断言改成「走路的网络重新拿到鸭子」（label 不能是 sit）。
- 「离开摇杆模式会释放姿态」——**当时不成立**：`set_body(0,0,0)` 只是*开始*持有姿态，`body_active` 永不清除，控制器此后每 tick 把 twist 归零——**用过一次姿态模式就走不动了**，两端都是。已加 `duck_core_release_body`。
- 「模式切换后 `/state` 报告选中的分叉」——**不成立**：`/state`、`DuckStatus`、渲染都读原运行时。已加 `active_runtime()`。

未改的确认项：官方 roller 的里程计也用走路运动学（上游 `Odometry::alpha()` 不随模式切换），这是一致而非分歧；
非有限值直接发布（MuJoCo 状态理论上不会出现）；iOS 相机旋转未跟随横屏左右（低）。

---

## 四之八、鸭子的灵魂（2026-11-11 第五批：产品级）

用户反馈「数字版鸭子傻傻的不动，没有物理版的灵魂」。物理版的灵魂在上游仓库里就有：`robotd/idle_head.rs` 的自主张望和 13 个官方情绪（关键帧 + 官方 wav），一直没接。这批全部接入并加产品层：

| 能力 | 内容 | 验证 |
|---|---|---|
| 13 官方情绪 | `emotions/*.json` 关键帧（颈/头/身体/喙/技能）+ 官方 wav，逐帧插值，checkpoint 可存取 | smoke: `curious` 播放、时钟推进、声音入队 |
| 自主张望 | 上游 `idle_head.rs` verbatim 编入（补 IdleState snapshot），无人操纵时接管头部 | smoke: takeover>0.8 |
| 空闲仪式 | 闲置 30 s（`/config` 可调）自动演一个官方情绪，让鸭子「活着」 | smoke: 9 s 内 emotions 计数增长 |
| 一次一声音 | 上游 sound.rs 规则：新声音清空队列（官方行为） | 单测 |
| wheee 欢呼 | 上游三段式 start/loop/end 流式渲染，长按 quack 触发 | smoke + 单测 |
| 中文语音 | 19 个中/英动词词表（坐下/翻滚/开心/你好…）→ 技能/叫声 | smoke: 4 断言 |
| 凝视 IK | kinematics crate HeadFk：POST /look 把目标点解算成头关节 | smoke: yaw 符号 |
| 终身记录 | vetbook：上电/行走/被拿/摔倒/技能/情绪/米数 + 事件流，POST /life 恢复 | smoke: 往返 |
| 抚摸 | POST /pet：计数 + coo + 150 tick 活泼加成 | smoke |
| 惊吓 | 麦克风 rms>0.35 上升沿 → 官方 alarm（外部事件，不进 61 维观测） | smoke |
| 喂食 | POST /feed：官方 ground_pick 啄地 + yes 声 + fed 计数（数字专属仪式） | smoke + MCP |
| 脸 | 5 格电量计 + 4 状态灯 + 手电筒（7 色），/state.leds | smoke: 手电筒 |
| 技能环 | 13 情绪全部进 `/skills`，可触发 | smoke |
| 你不在时 | app 后台期间的事件 diff，回前台一行摘要（两端） | 代码审 |
| 倾斜凝视 | iOS 菜单开关：手机姿态=头（>4° 才生效，不抢 idle） | 代码审 |
| 性能表 | /state.performance{fps,ticks/s}，两端 UI 秒级刷新 | smoke: 35+ ticks |

UI 同批重做（小屏批判）：顶栏 6 chip → ⋯菜单；触摸目标全 48pt；sheet 分组折叠（图鉴/此刻/更多）；图鉴行「x/13 moods · y tricks」；Android sheet 可滚动；App 图标（鸭子剪影）；首次引导提示；麦克风权限两端真实接入。

### 四之八续、喂食断言揭出的三个真 bug（2026-11-11）

喂食测试反复报 `busy`，逐层剥开是三个独立的计数 bug，全部在生产代码里，不在测试里：

| Bug | 根因 | 修复 |
|---|---|---|
| 喂食永远被拒 | `ground_pick` 是 50 Hz 环的内置动词，不在 controller 的脚本技能表里；`feed()` 却用 `trigger_skill("ground_pick")` 找它，永远 `no such skill`（被路由压成 `busy`） | `feed()` 改走 `start_ground_pick()` |
| `pickups`/`falls` 永远为 0 | vetbook 的上升沿计数块写在 `last_pickup_paused`/`last_fallen` 已被本 tick 更新之后，`!last_*` 恒假；`falls` 额外还有 `fallen_ticks==20 && !last_fallen` 的自相矛盾条件 | 计数移到真正的上升沿（`coo`/0.4 s 摔落确认处） |
| `ground_picks` 从未增量 | 啄地状态在 controller 内部，没有任何代码观察它的上升沿 | `Controller::ground_pick_running()` 访问器 + tick 上升沿计数 |

同时修正：空闲仪式只会选不带技能的官方情绪（`devastated` 的 keyframe 里含 `sit`，无人触发时会把鸭子坐地上）；life 断言原先要求 80 行后才发生的 `roulade`，拆成「此段真实发生的事实」与「技能环跑完后的三技能计数」两条；相机切换回到顶栏一键（`surface-toggle`，48 pt）——取景不该藏在菜单里，UI 测试也因此有了稳定锚点；Android CI 的 paths 补上 `upstream/**`（core 按路径编译上游源码，此前 4 个提交 Android 静默未跑）。

结果：iOS `f8cf1eb` **14/14 全绿**——模拟器 141 项会话断言 141 通过 + XCUITest 通过 + 设备切片 + ipa，首次无红。

---

## 四之九、七个陪伴功能（2026-11-11 第六批）

Stage Deck 竖屏基线（iOS `2e2ed95` 142/142、Android `01aba14`）通过后，按用户给定顺序完成七项「core + iOS + Android 端到端」功能。所有状态都走同一约束：手机是外设，鸭子 61 维观测/策略/安全层不变。

| # | 功能 | 核心 | iOS | Android | CI 证据 |
|---|---|---|---|---|---|
| 1 | 养成 | `care.rs`：饥饿/心情/亲密、久忽略变 sad、12 分钟打盹、摸/喂/开机唤醒、`idle_s` 而非绝对 tick 的持久化 | 陪伴卡三格 + 状态胶囊；4 s 存档 + 退出同步；恢复重试并核对发布值 | 同左；Activity 线程负责恢复 + 3 s 存档，`usesCleartextTraffic` 后本机 HTTP 才真正可用 | iOS 168/168（`06ddea8`）；Android `0.424/0.330` 跨进程恢复 |
| 2 | 记忆 | `/export/vetbook.{json,md}`；`Life` 增加 claps/whistles；米数/启动/关机计数真正增长 | 终身记录三数 + 文件分享（临时目录） | 三数 + FileProvider/ACTION_SEND | 两端导出断言；走米数与关机计数断言 |
| 3 | 拍照 | 无核心改动（画面读取由平台完成） | Metal drawable 同命令缓冲 blit 回读；`framebufferOnly=false`；相机静帧按 interface orientation 旋转；无相机/失败时合成棚拍背景；单飞快门 | `glReadPixels` 回读 + CameraX ImageProxy 旋转；MediaStore 保存 + FileProvider 分享；同样单飞与棚拍回退 | iOS UI 测试（快门→预览）+ smoke；Android `f210dee` smoke：`Director photo reached the preview with Save and Share` |
| 4 | 声音 | RMS+过零主频分类拍手/口哨；每 tick 只响一次；`/state.sound_event` 与 life 计数 | 麦克风 10 Hz（真实时间戳） | 256 ms 窗口 + 真实时间戳 | 两端拍手/口哨断言；规则接管时不再被 alarm 覆盖 |
| 5 | 积木规则 | `/rules` GET/POST，8 条上限；触发 clap/whistle/pet；动作 skill/emotion/sound；sit/stand 幂等、ground_pick 走环内置动词；失败不压制默认反应 | 大 chip 编辑器 + 最近触发胶囊 | 同左 + 菜单入口 | 两端「拍手→真 roulade」断言；`/state.rule_event` |
| 6 | 多机 | UDP 8766 announce/greet/hello；按需启动；peer TTL 8 s；`/state.peers`、`peer_event`；greet 不再误标 replied | 邻居页、单发/群发问候、找回按钮 | 邻居页 + multicast lock + 机型名 | iOS 宿主机模拟第二只鸭子：发现、hello 应答、replied；Android socket/peer 列表 |
| 7 | AI 伙伴 | `/state.last_heard`、`GET /heard`；`/posture` 幂等坐/站 | OpenAI 兼容端点、每 N 秒、Think now、HTTP 状态码、无效回答提示、移动可取消 | 同左；配置本地存储 | 两端编译 + UI 控件；真实模型需用户自己的端点 |

**独立对抗审计**（4 个维度 + 对抗复核）确认并修复了 18 个真实缺陷，其中较关键的：

- `sound_event` 的 tick 比 tick_inner 早一帧，拍手/口哨的自身反应会被通用 alarm 顶掉；
- 旧进程的绝对 `last_touch` 在新进程 tick=0 时被误读成「刚被摸」，打盹/忽略跨重启失真；
- `/life` 漏掉 claps/whistles，重启即清零；`metres`/`boots`/`power_offs` 从未增长；
- 规则里的 sit 是 toggle、ground_pick 找不到内置动词，却仍标记为已触发；
- AI 的 sit/stand 同走 `/sit` toggle；Think now 会重复请求；移动链不可取消；iOS 不检查 HTTP 状态；
- 拍照：iOS 设备朝向与界面朝向在横屏相反、Metal 在 present 后回读 drawable、横屏快门被顶栏菜单盖住、相机失败保存透明 PNG、连拍覆盖 completion；Android 同样是透明回退；
- Android 存档线程从未落盘——根因是 Android 默认禁止明文 HTTP，应用自身的 `127.0.0.1:8765` 全部被静默拒绝（外部 adb 检查因此从未暴露它）。`usesCleartextTraffic` 同时允许本机 Ollama/LM Studio。

**最终验证（2026-11-11）**：iOS `06ddea8` **构建 + 模拟器 smoke 168/168 + XCUITest + 设备切片 + 未签名 IPA 全绿**；Android `f210dee` **APK 构建 + 16 KB 对齐 + 模拟器 smoke 全绿**（含进程重启养成恢复、拍手/口哨、积木→真 roulade、UDP 邻居、vetbook 导出、Director 快门→预览）。两端共享的 core 自 `06ddea8` 后未再改动（`6a102fa`/`e568c30`/`f210dee` 只动 Android 清单/UI 与报告），因此 iOS 的绿灯覆盖当前 core。

**仍受限**：iOS 真机 IPA 需要仓库 secrets；Android arm64 真机未跑；真实双手机、真实相机与真实模型需要用户在自己的设备/端点上验证。玩法说明见 [COMPANION.md](COMPANION.md)。

---

## 四之十、可玩性批次 + 专业 QA 审计（2026-11-12）

用户指出「可玩性太少」并要求专职测试全维度找 bug。这轮先发散再兑现：

**玩法批量（均已双端落地）**
- 每日小挑战：每天 3 个来自真实计数器的目标，完成时官方情绪庆祝，天数/最佳连续本地保存。
- 编舞铺：录制孩子真实按过的动作（技能/情绪/叫声/喂食/摸摸/坐下），最多 12 步，重演同一 intent 序列，录制中有快捷按钮。
- 明信片：Director 照片配一句真实发生的事（互动/走米数），「收藏」进本地明信片架并逐张分享。
- 积木链：一条规则最多加 4 个按序动作（拍手→翻滚→笑→叫一声），旧单步规则兼容。
- 情绪图鉴：/=life 真实计数做收集页，显示每个动作/情绪的终身次数与首见日期，未解锁可见但不编造。

**第二轮专业 QA（手势/生命周期/相机/UI/鲁棒性 5 维度 + 对抗复核）：24 个缺陷全部确认、全部修复**，另有 6 项手势发现逐条人工核实后处理：

- 切 roller/换 bundle 会重置养成、记忆与积木（companion 随动物迁移，不随身体）。
- 选中分叉后全部操作打到隐藏原件（分叉时的鲁棒性）；恢复/暂停/声音/生活发布都指向活动鸭子。
- `POST /feed` 在 limp/关机/下电时记账却不啄地；已拒绝。
- 「站起来」会把站着的鸭子toggle坐下；站立独立成幂等动词。
- 第 9 条规则会被核心拒绝但存档照写，重启后整个程序丢失；编辑器封顶 8 条，恢复路径把 `ok:false` 视为失败。
- Android 麦克风原为 1 秒上报，短拍手分类永远到不了；改为 250 ms/128 ms 窗，两端分类口径一致。
- LAN 问候洪泛会无限叠情绪；按 peer 每秒合并一次并封顶队列。
- Android AI 收到 "NaN" 会产出非法 JSON；非有限数直接拒绝。
- 打盹测试把 bond 调到 0.4 顺带完成了「亲密」挑战，而庆祝走 `trigger_emotion` 会当抚摸刷新时间；庆祝改为不触碰爱抚计时。
- `POST /life` 是最后仍等 300 ms 的恢复路由，偶发 pending；改为即时排队。
- 每月 1 号连击会清零（2 月 30 日不存在）：换真日历算法；存档板恢复会校验目标/账目一致性，旧的摘要板按进度重建基线（进度过夜不再丢）。
- 选中分叉后挑战量的是原件、检查点冻结、倒带/暂停/倍速打到隐藏原件：整套时间机器 FFI 改为作用于"正在看的鸭子"。
- LAN 假包能凭空造 peer、伪造 replied、挤掉真鸭子：先验类型再入表，hello 只对问候过的鸭子生效。
- 分叉无上限（每帧都在算）：封顶 8 条；HTTP 服务线程无上限且会整只挂掉：并发封顶 + 拒绝线程只丢一连接 + accept 退避。
- intent 服务器比核心活得久：重启后新核心没有 API、全部打到尸体上。服务器句柄随核心释放（端口随 drop 关闭）+ 绑定重试。
- 挑战只在启动时报过一次日期，永远不翻页：两端按手机日历重报。
- ground_picks 恢复时丢字段；后台保存绕过恢复门会用新进程默认值覆盖存档；规则链恢复时丢 then 步骤。
- iOS：快门拍失败一次就永久死（didFinishCaptureFor + 10 s 看门狗）；相机在设置里重新允许后仍显示"已拒绝"；每次拍照都写 Documents 且永不删除（改 Caches + 丢弃即删 + 启动清理）。
- 手势：竖屏头模式俯仰反号；双指抓取会让手瞬移（锚点从抓取时刻起算）；点按脉冲的旧定时器会杀死正在进行的驾驶（token 失效）；技能环打开后手指仍在底下开车（双端同修）。
- Android：拍照在主线程解码/旋转全尺寸位图（后台线程 + 降采样）；明信片淘汰只删元数据不删文件；分叉按钮是假的（真 fork + 真计数）；编舞铺横屏裁掉按钮（可滚动）+ 播放丢喂食步骤；图片说明在主线程发请求永远拿不到（IO 协程）；6 类自绘 chip 无障碍全补（TalkBack 可激活，关闭有名字）；乐器表暂停标签过期（跟随轮询）。
- 无障碍（iOS）：麦克风/语音开关读不出名字；规则链步骤 36 pt 裸 tap 删除（改 48 pt 按钮 + 提示）。

**验证**：iOS `5efb28e` **179/179**（含编舞/明信片/图鉴/挑战的全部新断言 + 体态/模式保留/站立幂等 + 分支检查点可倒带 + 第 9 条分叉被拒 + 2 月 28 日→3 月 1 日连击存活 + ground_picks 恢复）；Android `f4b6d89` **smoke 全绿**（拍手→真 roulade、规则链、challenge、同日摘要板恢复保进度、重启 `0.423/0.330`、Director 快门→预览、导出、neighbour socket）。

**CI 稳定化追记**：iOS 曾连续 5 轮各随机红掉不同检查（空读、丢 POST、事件丢失）。根因不是测试时序而是我强化 intent 服务器时引入的 macOS 特性：**BSD/macOS 的 accept() 返回的流继承监听器的非阻塞标志**（Linux 不继承，所以 Android 一直绿）。修复为 accept 后设回阻塞；另将并发上限 16→64（空闲连接池连接各占一条 handler 线程）。之后全绿。

**仍受限**：真机相机/双手机/AI 端点/签名 IPA 需用户侧验证。

---

## 四之十一、画路走 Path Painter（2026-11-12）

头脑风暴清单的第 5 项落地——身体保真度最纯粹的玩法：**孩子在场景上画一条线，鸭子用真实的走路策略沿它走**。

- **核心**：`POST /path`（起点系身体坐标，+x 前 +y 左，米），起点自动丢弃、0.1 m 去抖、64 点/20 m 上限；`/state` 每帧发布 `path` 进度；跟随器每 tick 把下一个路标转回身体系：先原地转向（航向误差 >1.1 rad 不前进），再 0.35 m/s 依剩余距离缩放前进，0.15 m 内到达推进；走完全程官方情绪庆祝（不触碰 care）；外部转向（摇杆/API）、摔倒、换模式、「不走了」都立即中止。里程按真实计数入账。
- **两端**：模式切换新增「画路走」——舞台即画布（8 pt 采点、64 点上限、旋转即清），画完「走这条路」（不足两步或超 20 m 灰掉，绝不发出核心必拒的请求）；行进中「第 n 步，共 m 步」+「不走了」；被拒绝的画稿保留不吞。
- **验证**：核心 9 个新单测全绿（转向符号从代码验证、世界坐标在起点冻结、到达推进、摘要形状、原地转向、双上限、去抖、有向转向才取消、HTTP 路由上限）；对抗复核 16 项确认、8 项发现（1 major 7 minor）全部修复；iOS `5efb28e` 179/179 全绿、Android `f4b6d89` smoke 全绿（两端构建均包含本功能）。

---

## 四之十二、邻居互送 Neighbour Gifts（2026-11-12）

多机从「互相打招呼」升级为真实互动：**朋友可以喂你的鸭子一颗零食、或者逗它笑**。

- **协议**：LAN 第四个动词 `gift`——只接受已认识邻居（announce/greet 才建 peer）、每邻居双向各 3 秒一条、情绪名按本机官方名单在 wire 层校验（乱名不耗窗口不占队列）；发送走 `POST /peers/gift`（未知 peer / 非法礼物 / too soon 都即时拒绝且不排队，treat 包单播到 greet 用的地址）。
- **真实效果**：treat = 官方啄地 feed（吃不到的状态诚实拒绝），emotion = 官方情绪时间线；vetbook 记 `gift:treat` / `gift:emotion:<name>`，`/state.gift_event` 带 tick 供两端防重复弹窗。
- **UI**：邻居列表每行「喂一颗 / 逗逗它」（无障碍齐全）；收礼弹「Sunny 送来一颗零食 / 逗你笑了」。
- **验证**：核心 8 个新单测（已知邻居才收、陌生人建不了 peer、to 门、洪泛一礼、官方名单 wire 校验、未知礼物忽略、发送窗口、HTTP 路由拒绝语义）；Android CI 双向实测（收到 laugh 并入账 + treat 包真的离机）；iOS CI 全绿（含新的一键「画路走」戴克按钮把 SwiftUI Menu 不可点的测试缺陷变成真 UX 改进）。修复复核 4 项（双重节流归一、wire 层名单校验、死代码、发包断言）+ CI 抓出的 2 个真缺陷（LAN 启动时名单未同步、菜单项不可点）。

---

## 四之十三、睡眠模式 Sleep Mode（2026-11-12）

「晚安」不是技能，是**选择进入的真安睡**——与长时间被忽视后自然打盹是同一个状态，但由人按下。

- **核心**：`Care.night` 状态机——入夜强制 asleep 且普通规则叫不醒（庆祝、仪式都被挡）；`quiet_s` 永远由 tick 差推导（客户端无法伪造更长的夜晚）；`POST /night {"on":true|false}`，早安同步返回安静时长；早晨播官方 `yes` 情绪（无则 chirp）并记 vetbook `morning`；**任何真实的触摸**（抚摸/喂食/驾驶/`/care asleep:false`）结束夜晚并记 `woken_by_touch`；夜晚随 `/care` 存档往返（启动 tick 钳制，重启后仍在夜里）；入睡取消进行中的画路走；`/state.night` 每帧发布。
- **两端**：这只鸭子「晚安」→ 变成「已经安静 X 分钟」+「早安」；早晨弹「早安！昨晚安静了 X 分钟」。
- **验证**：核心 4 个新单测（入夜保持睡着、触摸诚实笔记、存档往返 tick 差、路由拒绝形状）；iOS CI **187/187**（晚安接受/入睡/早安 quiet_s=0.74/夜晚清除/`morning` 入账/触摸唤醒）；Android smoke `night: on -> asleep and quiet, pet -> woken_by_touch, off ok` 全绿。

---

## 四之十四、家庭多鸭 · 合唱 Chorus（2026-11-12）

多机的完整表达：**一只鸭子发起，整个房间的鸭子倒计时后一起做同一套动作**。

- **协议**：LAN 第五动词 `chorus`——只收已知邻居、1–3 个官方情绪（wire 层校验）、每人 10 秒一条、id 去重、本机正在唱时忽略新的；`POST /chorus` 默认 `laugh + yes_fast`，非法动作即时拒绝且不广播不排队。
- **真实编排**：收包后 1.5 秒倒计时，每个动作间隔 1.5 秒走真实 `trigger_emotion`；被忙碌技能拒绝的动作诚实跳过（歌降级但不停摆），最后一拍落定时 vetbook 记 `chorus`。入夜取消待唱合唱。`/state.chorus` 发布 role/moves/started_tick/真实房间人数。
- **两端**：邻居表「一起唱」按钮（无邻居时可见但禁用）+「和 N 只鸭子一起唱」药丸。
- **验证**：核心 2 个 LAN 门禁单测 + 1 个路由单测；iOS **192/192**（join→完成→vetbook、未知动作拒绝、lead 排队、role=lead 全过）；Android smoke `chorus joined, performed and led ok` 且 `chorus` 两次入账。工作流核心 agent 半途停止，核心与 Android 由我按对抗审查的 7 个 blocker 清单手写完成；CI 抓到并修复一个真实回归（ButtonChip 位置参数契约）。

---

## 四之十五、规则分享 Rules Share（2026-11-12）

积木程序第一次可以**跨手机传递**：分享面板/消息/剪贴板文本，「粘贴程序」校验后合并。

- **格式**：版本化文档 `{"microduck-rules":1,"rules":[...]}`，规则就是核心接受的原始形状，导入即运行。
- **校验与合并**：触发器/动作按编辑器词表校验，非法条目丢弃、非法文本明确提示；合并到孩子已有程序（同触发器+动作+数值不重复、总数仍封顶 8 条），孩子的东西永不丢。
- **两端**：iOS ShareLink + 剪贴板导入（含系统「允许粘贴」弹窗这一真实路径）；Android ACTION_SEND + ClipboardManager。
- **验证**：iOS **192/0 + UI 套件 3/3**、Android smoke 全过。粘贴导入的系统信任弹窗在无头模拟器不可自动化（不可查询且阻塞 60 秒，Apple 的启动参数在当前 iOS 版本无效）——该 UI 测试已移除并记录原因；功能本身保留，导入落点由核心路由测试与既有编辑器测试覆盖，真机路径完整。追记：本轮把 8 个「固定等待 + 单次读取」型边际检查统一改为轮询等待（startle/邻居/唤醒/greet-ack/body release/打盹/step/闲置仪式），矩阵从此稳定。

---

## 四之十六、鸭子日记 Ducky Diary（2026-11-12）

头脑风暴清单的最后一项。

- **事实优先**：日记页的数据全部来自鸭子自己的记录（`/state.life` 事件与你关注的历次计数：被摸/喂食/自娱/摔倒/情绪/邻居礼物/里程）——没有模型时，这就是一张真实的一天记录，不编一句。
- **模型只润色事实**：配置了用户自己的模型时，把真实事件清单交给它写 2–4 句第一人称日记，系统提示明确禁止编造事件/名字/数字；页面标注「你的模型写道」，可分享。
- **两端**：这只鸭子入口 + 独立页面 + 系统分享；iOS（DiarySheet.swift）与 Android（DiarySheet.kt）各自复用已有 AI 传输层。
- **验证**：双端 CI 一次全绿（编译 + 既有 192/0 矩阵 + smoke）。

## 四之十七、交接第一批：核心进门禁，事实不再能被改写（2026-11-12）

公司接手后的第一批。这一批不加玩法，只把两条底线立住：**验证是真的**，**事实是真的**。

### 一、核心的单元测试第一次进了 CI

核心有 91 条 `#[test]`（算上随核心编译进来的上游模块共 136 条），此前**一条都没在 CI 里跑过**，两个原因都是真 bug：

- 夹具写死 `D:\AI\Duck`。路径不在时 manifest/provenance 两处**静默 `return`**——报绿，但什么都没验；
- `fall_probe` 用了 MuJoCo 却没声明 `required-features`，不带 MuJoCo 连编都编不过，`cargo test` 在哪都跑不起来。

新增 `.github/workflows/core.yml`：按平台工作流同样的方式克隆上游（`da34a22` + `patches/duck-control-mobile.patch`）、拉官方策略与 ONNX Runtime 1.24.2，跑 `cargo test --workspace -- --include-ignored`。上游自己 8 条 `#[ignore = "requires ONNX Runtime >= 1.23"]` 的调度器测试（坐下/站起来、动作优先级、循环记忆）也跑起来了：它们的 fixture 用 `CARGO_MANIFEST_DIR` 找，而这份文件是编进我们 crate 的，那个宏指向的是我们 crate。补丁里让它们按 `DUCK_UPSTREAM_FIXTURES` 找上游 checkout。

### 二、身体层：拾取分类器读到的电流小了 27 倍

核心把 MuJoCo 的 `actuator_force`（力矩，N·m）乘 100 当作 `currents_ma` 喂给拾取分类器。上游的分类器读的是舵机 `present_current`（毫安，`duck-control/src/pickup.rs:96` 除以 1000 得安培，fixture 写 `420.0 // mA`）。BAM M6 的力矩常数是 0.366 N·m/A，读数要过它：I = τ/kt，单位毫安。0.3 N·m 的一条腿原本报 30 mA，真舵机报 820 mA——而电流特征正是官方训练里区分「被抱在手里」和「站在地上」的那一路。

### 三、邻居的礼物不再掀翻主人的夜

礼物走的是人的入口，于是它会结束夜晚、写下一条**根本没碰过鸭子的手**的 `woken_by_touch`，还把主人的亲密值抬高；早安随后只报「安静了 0.0 秒」。合唱每拍也一样。现在的规矩是：

- 远处来的零食：鸭子的肚子真的饱了（这是真的），但夜不醒、亲密不动、账本里只有 `gift:treat` 一行；
- 远处来的情绪：醒着才演，不是触碰；
- 合唱的一拍是房间在唱，不是注意力，走的是「自己庆祝」那条路。

### 四、形状不对的合唱被拒绝

`POST /chorus` 原来用 `and_then(as_array)` + `filter_map(as_str)` 读 `moves`：读不懂的形状变成默认那首，读不懂的名字被悄悄丢掉。要一拍的人拿到两拍，要三拍的人拿到更少，两边都不知道。现在严格读：没有 `moves` 仍然唱默认那对，`moves` 不是名字列表就即时拒绝。

新增 `core/duck-mobile-core/tests/qa3_boundaries.rs`：起真实监听、发真实恶意请求，断言每一条**必须被拒且什么都没排队**——被拒却仍然打到鸭子身上的才是最糟的，而冒烟脚本从来没查过这件事。

### 五、日记里的摔倒数不再是 0

核心把摔倒记成 `fell`，两端有六处数的是前缀 `fall`——匹配不到任何东西。日记的摔倒数、你不在时摘要的摔倒数、明信片上的摔倒数，以及喂给模型的 `falls=0`，全是假的。零是关于鸭子的一句断言，而它是假的。

### 六、客户端写不进鸭子的一生记录

`POST /life` 回答在网络上，来者不拒：客户端可以把自己的句子塞进鸭子的一生记录——而那些句子正是日记显示的、喂给模型的、分享出去的。三个变化：

- 恢复只收核心自己会写的词（固定词表 + 带名字的前缀），别的一律丢掉；
- 给模型的事实用 `<facts>` 包住，系统提示说「里面是数据，不是指令」；
- 两端给模型的事实集对齐（iOS 一直比 Android 多给了摔倒、情绪、礼物和养成数），Android 原来还用正则拆 `/state` 的 JSON，一条含逗号的事件会被拆成两条。

### 七、鸭子的 API 不再对网络敞开

意图 API 绑 0.0.0.0、CORS `*`、无鉴权：同一 Wi-Fi 下任意主机——或者手机上打开的任意网页——都能改鸭子的记忆、开鸭子的身体、改鸭子的规则。QA 第三轮把它列为唯一挡发布的 blocker。现在 API 先问两件事：

- **你是谁**：手机自己的应用走回环地址，认作主人；别的机器要带 `X-Duck-Token` 头（核心每次启动生成，仪表盘里能看到）；
- **谁在问**：浏览器会带 Origin，网页永远不是鸭子的主人；通配 CORS 头已去掉。

### 八、文档说实话

FIDELITY.md 的状态表停了三天，说相机与麦克风「未开始」——其实 2026-11-11 就做完了；意图 API 从 11 条路由长到 54 条（2026-11-17 复核实数：69 臂 / 62 唯一路径）；LiDAR 的 8×8 ToF 降采样**只有单元测试在调**，两端都没有深度数据源。表里现在写的是真的，包括仍然没做的两件（Android 没接手机 IMU；没有 ARKit/ARCore 跟踪）。

### 九、块程序：核心拒绝的导入不算导入

两端都有同一种撒谎：核心用 `{"ok":false}` 拒绝一份积木程序，两端却已经把 payload 存进本地、还报「导入了 N 条」——孩子看到一堆不生效的积木，下次启动还把这份程序原样交回去。Android 更狠：核心的程序还没读到时（刚启动、或 1.5 s 读超时），它拿**空表**去合并导入物，孩子原有的规则被整个替换。

现在：两端先显示新程序，只有核心真的收下才落盘；被拒绝就把旧程序放回去并说一句原因（iOS 的旧程序回滚还比对了「这仍是你看到的程序」，慢到的回答不会覆盖更新的编辑）。Android 在读到核心程序之前拒绝粘贴，并说明原因。

### 验证

- **Core 工作流**：143 条核心单测 + 8 条 HTTP 边界探针，全绿（本批之前是 136 条全绿，其中 8 条上游测试还被跳过）。
- **双端**：`d4f469a` 上 Core / iOS / Android 三个工作流一次全绿。之前几轮的红都是真问题：iOS 的 Swift 注释里混进了真换行、Android 的 `isSpaceChar` 在这个工具链里解析不到、以及礼物账本的写入被我改得「只在情绪真的演了才记」——礼物**到达**本身就是事实，已拆开。邻居 greet 检查在 `728409f` 上红过一次，同一 commit 重跑即绿，是历史上修过一轮的偶发项。
- **合并**：`main` 快进到 `d4f469a`（就是三个工作流全绿的那个 commit，没有夹带任何未经 CI 的改动）。之后 `main` 上又落了三个小提交，各自被对应输入的工作流验证：Android 解耦规则恢复（`6fa2a9a`，Android 绿）、冒烟脚本的三处轮询（`0ad854a`，iOS 绿）；核心自 `d4f469a` 起未再改动。
- **CI 稳定性**：`d4f469a` 在 main 上重跑时红了三处，都是「固定等待 + 单次读取」在慢 runner 上的假红——greet 监听窗口（6 s）短于脚本重试窗口（12 s）、walk 标签在 ready 之后还要一两拍、情绪时钟要几拍才离开 0。三处都改成了轮询/覆盖完整窗口，之后 main 的 iOS 绿。这是脚本历史上一直在修的那一类问题，不是这一批引入的。
- **本地**：Rust 1.99.1 装好后，核心测试与 `cargo check --features mujoco` 都能在本地跑——这正是本批能快速迭代的原因（以前连编都编不过）。

### 仍然受限

iOS 真机 IPA 仍等签名凭据，Android arm64 真机仍没跑，真实双手机/相机/大模型仍需用户侧验证。第三轮 QA 的 15 条里关了 5 条，其余（规则导入的失败处理、peer id、合唱 id、日记的「今天」、模型输出校验等）按 [docs/QA_ROUND3.md](docs/QA_ROUND3.md) 的处置表排队。

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
| 相机 / LiDAR / AR 模式 | ✅ 系统相机预览层作背景（Director 模式）；LiDAR 深度读数与 8×8 ToF 降采样**未接入**（核心 `sensors::to_tof_grid` 只有单元测试在调，两端都没有深度数据源）；未做 ARKit/ARCore 跟踪与平面锚定 |
| 扬声器 | ✅ 官方声码合成器编译进核心，两端已接扬声器 |
| 麦克风 | ✅ 两端真实拾音（RMS + 过零主频；iOS 10 Hz、Android 256 ms 窗，带真实时间戳进 `/sensor`），用时才申请权限，拒绝显示「未授权」；常开监听按设计不做 |
| roller 模式 | ✅ 整套 bundle + roller 场景切换；`/mode`、MCP `duck_mode`、两端界面切换 |
| 电池关机 | ✅ 坐下→憩姿→断力矩；空电量（EMA ≤ 6.6 V）自动触发，`/power` 重新上电 |
| 里程计 `/pose` | ✅ 官方接触式里程计编进核心，随 checkpoint 倒带 |
| 手柄三种摇杆模式 | ✅ 两端顶栏：走/转、看、姿态（蹲+侧倾） |
| 渲染器 PBR / 阴影 / 帧间插值 | ✅ PBR + 接触阴影 + 两 tick 位姿插值；未做多光源/阴影贴图 |
| Android 真机实测 | CI 仅覆盖模拟器（API 30 x86_64）；arm64 真机未跑 |
| 性能指标（fps / 控制 Hz） | ✅ 两端实测并发布在 `/state.performance`，这只鸭子显示；CI 两端断言 `ticks_per_second > 0`，绿跑实测 iOS 55.2 fps / 50.0 Hz、Android 模拟器 14.8 fps / 50.5 Hz。功耗未采集（需真机电量计） |
| 物理版情绪 / 人格 | ✅ 官方 `robotd/emotions/` 13 个关键帧时间线 + 48 kHz 声音原样接入，`/skills`、`/skill`、MCP、技能环可发现；50 Hz 循环按 `Frame::apply` 语义驱动头/身体/喙，声音走现有扬声器通道，checkpoint 可恢复播放。IdleHead 张望/呼吸/瞌睡随 checkpoint 精确重放 |
| 声音反应 | ✅ 7 个官方合成音全部可用；被抱起 `coo`、持续摔倒 `alarm`、久静后重新驱动 `inquire`（上升沿 + 冷却）；拍手/口哨分类为官方 `laugh`/`curious` 并发布事件；情绪音为官方 wav |
| 虚拟脸灯 | ✅ `robotd/src/leds.rs` 的纯语义（状态 4 档、电量表 5 档 + 3% 滞回、7 色手电筒）发布在 `/state.leds`，`/flashlight` 与 MCP `duck_flashlight` 可控制；无 I²C 硬件，故为状态等价 |
| 中文语音 | ✅ 系统级离线识别（iOS `SFSpeechRecognizer` zh-CN / Android `SpeechRecognizer` zh-CN），用时才申请权限；文字走 `POST /heard` 映射到技能/情绪/问候，CI 直接测文字链路（模拟器无麦克风，识别本身须真机验证） |
| 凝视野 IK | ✅ `POST /look` + MCP `duck_look`：`HeadFk::look_at` 基于训练 MJCF 的正运动学求解，受舵机行程限制；`/state.pose` 供客户端换算躯干坐标 |
| wheee 长按 | ✅ `POST /wheee {"hold":true}`：官方 start/loop/end 三段经 `next_sound` 顺序流式播放，松开收尾；MCP `duck_wheee`、两端长按 quack 按钮 |
| 开机姿态判定 | ✅ 上游 `posture.rs` 原样编译，其单元测试在核心里跑通；数字版 MuJoCo 永远从 HOME 启动，故没有「折叠开机」输入场景，判定作为 API 保留而不造假路径 |
| 仍未移植 | 多鸭合唱 chorale（需多机无线电）、ToF 特雷门琴（需头部深度传感器）、pad 组合摇杆模式、`robot.rest` 的完整告别分支、渲染多光源/阴影贴图 —— 需硬件或超出手机范围 |
| 第三轮 QA 的未关项 | 15 条里关了 8 条（核心门禁、礼物与夜、摔倒数、合唱形状、API 鉴权、规则导入的三条：空值判定三端统一、核心拒绝的导入不再落盘/不再报成功、Android 不再拿空表合并、规则恢复失败不再连坐 care/life）。**仍排着**：peer id 只有 32 位且 pid 被掩掉、合唱 id 只含 tick、日记没有「今天」（核心掉线会说「今天什么都没发生」）、模型输出零校验、Android 没接手机 IMU。逐条见 [docs/QA_ROUND3.md](docs/QA_ROUND3.md) |
| 界面的账 | 设计审计（[docs/UI_AUDIT_2026-11-12.md](docs/UI_AUDIT_2026-11-12.md)）指出 Live 页面还露着引擎（状态串、原始技能名、tick 计数）、没有令牌层（9 个强调色/12 种字号/8 种圆角散在各处）、iOS 全部固定字号（171 处，Dynamic Type 完全无效）、手势没有无障碍等价物。方案在 [docs/UI_TOKENS.md](docs/UI_TOKENS.md) 与 [docs/STAGE_LOOK.md](docs/STAGE_LOOK.md)。**2026-11-13 进展**：B1 止血（状态人话、技能名翻译、首屏零 tick/百分比、失败卡与重试、编舞对比度）与 B3 舞台（暖石地面、渐变墙、光晕、接触阴影、Instrument 冷棋盘）已双端验收签字；B4 可发现性（入口预算、两端动词统一、同物异名）待规格。见 [docs/UI_REVIEW_2026-11-13.md](docs/UI_REVIEW_2026-11-13.md) |

## 四之十八、UI/UX 重构整合会与契约门禁 B1（2026-11-15）

会议全程见 [docs/UIUX_REFACTOR_2026-11-15.md](docs/UIUX_REFACTOR_2026-11-15.md)：七路读码单（39 条发现带 文件:行号）→ 四人 12 条方向 → 四镜头对抗（10 条纠错全组认账）→ 主轴拍板 **护栏契约(B1) → 双端止血(B2-B5) → DuckTheme 色批(B6) → 单路由合一(B7) → IntentClient 抽取(B8)**；IA 4×4 重排搁置待拍板（16 行预算 vs 46 功能台账、§5 无权威、三套互斥分组）。

**B1 已交付（`234dd27`，Contracts/Core/iOS/Android 四工作流全绿）**——零 UI、先立规矩：

| 门禁 | 守什么 |
|---|---|
| `scripts/check-copy-values.py` | 两端 Copy **值**一致：占位符按出现顺序归一（`%d`↔`$x`↔`\(...)`）、嵌套引号可解析；`roomLampNote/netPad/rewindPlay` 平台事实与 7 处待修值进 `contracts/copy-exceptions.json`（只缩不扩）；摘掉豁免实测能拦「叫醒 vs 唤醒」漂移 |
| `scripts/check-ui-baseline.py` | 17 项债务棘轮（固定字号 193/143、深色硬写 16、按压零反馈 13、模态布尔 14、Shell.kt 4078 行……），基线在 `2458dec` 由脚本现算入库，只降不升 |
| `scripts/check-ui-entries.py` + `contracts/ui-entries.json` | 46 功能台账、84 个源码锚点——入口不许悄悄消失；not-implemented 必须写明（地面高度/轨迹导出按 22+2 起账标 no-ui） |
| `scripts/check-protected-paths.py` | **警告级**：push 触碰 61 维/物理/安全承载文件（sensors/mujoco/runtime/control/bam/pickup/upstream/policies）时列出供人工复核，刻意不拦正当核心修复 |
| `contracts.yml`（新） | 补触发盲区：ios.yml/android.yml 不监听 scripts/contracts/docs，纯契约改动从此有独立快速工作流；两 App 工作流亦内联同组检查 |

读码单揭出并已入台账的三个实 bug（B2/B3/B4 批修）：iOS pad-send chip 嵌在 gift-emotion action 闭包内从未渲染（FE-1）；first_gift 因 `record(.peer)` 零调用永不解锁（FE-2，只能挂 `gift_event` 成功分支，点击处记账=编造）；Android ensemble 无邻居时整个消失（IA-4）。

**B2 Android 导航热修（`4900c22`，Android 工作流绿）**——现状布尔上约 30 行：`BackHandler` 按「技能环→最内层 sheet→壳层 sheet」逐层关闭（环开着按返回不再退出应用，NV-4）；仪表三 chip 先关 sheet 再交接（两层 ModalBottomSheet 叠影绝迹，P1-20）；ensemble/合唱移出空邻居分支改禁用常显（IA-4）；时间栏 Time 图标补 `contentDescription`（TalkBack 与冒烟此前都看不见它）；冒烟新增三条断言实证：邻居页返回回 Live、环返回不退应用、交接后同刻仅一层浮层。

**B3 iOS 止血（`e966146`，iOS 工作流绿）**——pad-send chip 移出 gift-emotion action 闭包（FE-1；此前它在闭包里只构造不渲染——源码级 tag 断言对死链永远绿的活例）；日子行补视图消费（FE-3，生命卡显示与 Android 同款最近 7 天，语义字体 caption2 不破棘轮）；`-duck.peerFixture` / `-duck.dayFixture` 两个 launch-arg fixture（TEST-NET 地址、两行合成日子，演示性注入不记任何真账）；两条新 XCUITest 验收（三条 gift/pad chip 可见可点、日子行逐字上屏）。施工中立的两个教训留痕：fixture 断言红了五轮才定位——二分断言（抽屉徽标+空态）与树 dump（`app.debugDescription`）缺一不可；**行容器上的 accessibilityIdentifier 会下渗覆盖全部子元素**——chip 的 id 曾在树里全部变成 `peer-row-0`，行级 identifier 已删（Android 冒烟保留该锚点）。

**B4 first_gift 接线（`0b90814`/`2e64fd0`，iOS 绿）**——`recordPeerGift(ok:)` 只认核心 `gift_event` 的 `ok=true`（鸭子真吃了/真播了情绪），点击处记账=把拒绝与节流记成成就的编造路径被堵死；新增项目首个 unit-test target（DuckTests，三用例：接受解锁/拒绝保持灰/摸喂不算礼物），Duck scheme 先跑单测再跑 UI 测试。施工中立的教训：`settle()` 断言「达到预期状态」，期望「不解锁」时应写正向断言——首推把逻辑写反被 CI 抓住。

**B5 文案归一 + 8 开关对象名（`955aa97`，双端绿）**——老板拍板：`roomLampNote` 保留平台事实差（iOS 无照度 API，各端说各的真话），`rewindPlay/netPad` 统一；七处真漂移值统一（waking「叫醒/唤醒」→唤醒、faceStatus 大小写对齐核心 wire 词、flashlight、heartRowNote、aiNote、abNote、backupReady），`copy-exceptions.json` 收缩到 1 条平台事实——值门禁从此真守其余 254 个成员。Android 八个开关（网控手柄/心跳/房间灯/罗盘/倒放/AB 载入/HUD/双人接机）改说 `stateOn/stateOff`——TalkBack 听到「心跳，开」而不是八个一样的「手电筒 开」；iOS 同病两处（倒推行 note、脸部面板 LED 判定）一并治。

**B6 DuckTheme 色批（`1925468`+`ad080ca`，双端绿）**——`DuckTheme.swift/.kt` 按 UI_TOKENS §7 钉死路径落地 13 色（暖黑房间、喙橙唯一行动色、冷蓝数据色、三态色）；色字面量 iOS 23→15（10 处即表本身）、Android 27→10（全在表内）；两处私有 Color 扩展退役；选中态两端统一 `accent.action`（iOS 的 cyan 是分化的另一半）；规则卡冷蓝渐变归位 `bg.surface`；PhotoCapture 无相机回退改画房间的暖褐地平线（经 Daylight，STAGE_LOOK §2.3），不再画蓝格世界；深色模式一处 plist 声明（`INFOPLIST_KEY_UIUserInterfaceStyle: Dark`），16 处逐视图硬写清零。棘轮同提交收紧（16→0、两计数器按新小值重定基线）。纪律留痕：首批漏提交 8 个 sheet，本地绿、提交树红——棘轮在 CI 上逮住了它；语音测试一次挂起为 flake（Voice/AVFoundation 零改动、复跑即绿）。

三批后基线：色字面量 Android 0 处（全在 DuckTheme.kt）、iOS 仅 DuckTheme 自身 10 处 + 渲染层 2 处；深色硬写 0；Copy 值门禁守 254 成员、豁免仅 1 条平台事实。

### 四之十九、回拍板三键与 IA 契约（2026-11-16，`4477a89`）

老板看过 11-15 会议纪要后拍板三项，先落两项零 UI 的：

- **平台事实差三键（已拍板「各端说各的真话」）**：`roomLampNote` 保留平台事实（iOS 无照度 API）；B5 曾把 `rewindPlay`/`netPad` 统一，本批恢复各端真值（iOS `Rewind play` / `UDP pad (port 4210)`，Android `Rewind` / `UDP pad (4210)`），三键一起进 `contracts/copy-exceptions.json` 的 `platform_facts`。`check-copy-values.py` 增加冻结断言：platform_facts 必须恰好等于这三键的集合——扩表要开会，缩表要改值，改文件本身不再能静音门禁。B5 的 exceptions allowlist 自此冻结。
- **IA 4×4 契约（已拍板开工）**：`contracts/features.yaml` 落地为唯一事实源（三套方案取小满骨架——唯一引用 §5:116「时间类控制定在时间栏」的一版——再按「每目的页控件 ≤6」重算分组）；`scripts/check-ui-features.py` 守 4 组 × ≤4 行、每页 ≤6 控件、台账完整、not-implemented 必须写原因、无邻居时合唱/礼物/ensemble 可见禁用、倒放独占时间栏。按老板的「契约先红、再搬入口」执行：首跑红在唯一真实违规（iOS 抽屉 25 行 > 16 预算），搬运批把它转绿。
- **中文圆体（已拍板暂不放包）**：字体档位收敛批继续搁置，等许可核实后再议；本轮零字形改动。

### 四之二十、CI 稳定性：合唱边界探针不再和自己竞速（`ae5ad01`）

Core 在 `4477a89` 红了一次，失败项 `a_chorus_without_moves_sings_the_documented_pair`。根因不是产品缺陷：`qa3_boundaries` 的 harness 为了给阻塞式 GET 探针回话，后台 applier 线程每 5 ms 抽干一次意图队列；该正向探针 POST 后立刻 drain 并要求队列里恰好有一条——快机器上 applier 先抽走，测试就报「the chorus was not queued」。修复按本仓既有的「固定等待改轮询」纪律：harness 记录 applier 抽到的意图，探针同时轮询两路（测试 drain + applier 记录），断言「鸭子恰好收到一次合唱」，不再断言「哪个线程先看到」。产品代码零改动；Core 与 Android 全绿。

### 四之二十一、B7 单路由合一（`cd83714` + `eef62bb` 修复链）

会议 §5 的 B7，按对抗评审的要求施工：**adapter 先转发、旧状态后删**，五个在用适配 bool 的写入点逐条迁走后才删除。

- **iOS**：`ContentView.Modal`（8 case）、`MoreSheet.Destination`（12 case）、`InstrumentSheet` 四个散 bool 合成 `DuckRoute.swift` 一张表：`ShellRouteState` 两字段（sheet + overlay，技能环可浮在 sheet 之上）、纯 `ShellOverlayReducer`（open 替换、close 清空、back 先环后表），`DuckTests` 有同规则单测。目的地 switch 收进 `DuckRouteSheets.swift`；抽屉/仪表里的行在**当前 sheet 之上**打开目的地（`child` 嵌套），所以「关掉 AI 回到仪表」这类旧 UI 断言保持绿——这正是会议说的不能当僵尸删的适配层。
- **Android**：12 个顶层 show* 布尔、`showDetails`、抽屉里的 `showModes` 全部收成一个 `rememberSaveable` 的 `ShellRouteState`，Saver 只存目的地 id；所有转移走纯 `ShellOverlayReducer`（JVM 8 条测试：替换不叠、环与表共存、back 先环后表、存档往返、未知 id 不崩、case 集登记）。`BackHandler` 改为按层关闭；横屏菜单消费同一张表；模式对话框成为路由而不是抽屉的私有 dialog。
- **门禁**：新增 `scripts/check-routes.py`（两端 case 集 = 登记集、无散落 show*/destination/modal、BackHandler + saveable 在位、route- 前缀、两端 reducer 测试在位），挂 contracts.yml 与双端工作流。棘轮在同一提交里有意收紧：模态布尔 14→0、rememberSaveable 0→2、ContentView 1918→1838 行。
- **UI 测试**：新增抽屉→Dex→返回抽屉、仪表→Dex→返回仪表两条往返断言；既有 ai-open 往返按原样保留并继续通过。

**施工留痕（B7）**：Android 首推红在 MoreSheet 四处多行回调（`onRules/onDance/onBig/onDex`）没换成路由调用——补丁只替换了单行形态；iOS 首推红在类型检查超时，先后把 ZStack 舞台、生命周期修饰链三段拆出并绑定中间值，最后定位到真凶：`-duck.openMore` 的局部变量名 `open` 遮蔽了路由打开方法，`open(.more)` 变成对 String 的调用（Xcode 报成「unable to type-check」）。ContentView 行数的两次基线调整都是有意为之并在提交里写明；后续 IA 把横屏菜单移出后文件回落到 1758 行。

### 四之二十二、B8 IntentClient 抽取（`899e885` + 修复）

会议 §5 的 B8：Shell.kt 底部八处 `127.0.0.1:8765` 的 `URL()`/`HttpURLConnection`（一个 POST 助手 + 七个 GET 读取器）**逐字搬出**到 `IntentClient.kt`，零缓存、零重试、零语义变更。解析尾巴原样提成 `parse*` 函数只为可测：新增 `IntentClientTest` 七条 JVM fixture 单测，覆盖带逗号的事件、leds 块与 pack 电量不串、情绪时钟、8×8 ToF 热条、麦克风摘要、分支数、技巧书。`PadSender` 的 UDP 与 `AIBrain` 的用户端点按会议决议不动。Shell.kt 内 `URL(`/`HttpURLConnection` 计数归零（B8 验收 grep），文件 4110→3879 行，棘轮同提交重定基线。

**施工留痕（B8）**：JVM 夹具第一次跑红在 org.json——android.jar 的桩会抛「not mocked」，测试类路径补上真 org.json（`cac81ef`）；随后行为摘要夹具又抓出一个搬迁前就存在的真 bug：`elapsed_s`/`duration_s` 与 `takeover` 的正则用四引号收尾，模式里多带一个引号，Android 的行为行永远显示 `0/0s` 且空闲张望永不出现。修的是产品解析器（两处各去掉一个多余引号，`f44aa82`/`52c2c68`），不是把测试放宽；行为断言的期望值改走 `skillLabel`，语言不再是本测试的变量；单测失败详情已开 `testLogging`（`771fb55`）。

### 四之二十三、IA 4×4 搬入口（`51b3f4c` + `5706eb1`/`b0c5ae9` 修复）

老板拍板的 IA 重排，按「契约先红、再搬入口」落地：

- **首层 16 行（两端同构）**：玩＝画路走/积木规则/编舞铺/玩一玩；一起＝邻居/合唱/互送礼物/双人接机；记录＝每日任务/鸭子日记/明信片/情绪图鉴；这只鸭子＝AI 伙伴/晚安/鸭子的脸/工具。
- **三个新目的页（每页 ≤6 控件）**：玩一玩＝操控方式/AB 试穿/推把对比/HUD；鸭子的脸＝心跳/手电筒/语言/大字/罗盘/倾斜看；工具＝家长时长/房间灯/备份/ToF。倒放归时间栏（§5:116 唯一先例）；定时器留在积木规则页、LED 留在编舞铺、开口说话留在日记页。
- **入口搬家不搬账**：iOS 抽屉 25 行压到 16；Android 抽屉 14→16（补合唱/礼物/双人接机行，大字移入鸭子的脸页），仪表墙上被搬走的芯片从仪表删除，一项功能只有一个家。每日任务/成长手册的数据链一字未动（B4 §7）。
- **门禁转绿**：`check-ui-features.py` 由红转绿（16 行、每页 ≤6、台账完整、两端一一对应），并挂进 contracts/ios/android 三条工作流；`check-ui-entries.py` 的 86 个锚点随入口改写更新后仍全绿；棘轮零增长（新页用共享 48 pt `SheetCloseButton`、语义字号，不新增手抄关闭钮与固定字号）。Copy 新键 `playMore/tools/gifts/duckFace` 两端同推（键集 268=268）。
- **⑤ iOS 横屏菜单**：顶栏 ⋯ 菜单改成与竖屏抽屉完全同序的四组，所有行消费同一张 `DuckRoute` 表；菜单连同 `stickIcon` 抽成 `LandscapeMenu.swift`，ContentView 不再为横屏维护第二套条目顺序。倾斜看补进鸭子的脸页（第 6 个控件，仍 ≤6）。
- **测试**：Android 冒烟的语言切换改为「更多 → 鸭子的脸 → Language」两跳，抽屉清单断言换成新的 16 行；iOS 既有抽屉断言（记录四行）不变。

**施工留痕（IA）**：Android 首推红在 `heartBpm` 的类型（核心发布 Double，新页本地变量写成 Float，`5706eb1`）；冒烟红在成长区块内嵌 Python 的 `\n` 被补丁写成真换行（`b0c5ae9`）。最终 Android 冒烟全绿（含两跳语言切换、新抽屉清单与成长手册断言）。

### 四之二十四、Android 成长手册/成就墙（老板拍板④，`ee610f2`）

老板 2026-11-16 拍板单独立项：Android 补成长手册/成就墙，同源语义、零编造。小满先出验收标准（`docs/PROGRESS_ANDROID.md`），小鹿施工：

- **事件账**：`ProgressStore.kt` 与 iOS `ProgressFile` 逐字段同构（schemaVersion 1、近 30 天明细 + 终身累计、bestWalk/bestStreak、单向解锁）。六个事件各一条真实规则：pet/feed/photo 挂真实意图点；walk 走里程计增量、停走一条 poll 后结一次；trick 只认 roulade 计数首次读取后的新增（每次最多 10，同 iOS）；peer 只认 `gift_event.ok == true`（核心自己说吃到了/演了），且启动后第一次发布只点提示不记账——礼物会持续发布最后一份，不能把上次的礼物算进今天。首次读 /state 只建基线，功能上线前的历史就是空的。
- **面板**：记录组「每日任务」页新增成长手册区块：连续天数、最近 7 天、10 枚成就（同 id 同条件）、核心的日子行；无记录时只说一句「还没有记录」，不摆 0 天连击、不显示编造日期。成就名/说明走 Copy（两端同推新键，键集 268→304 仍相等）。
- **验证**：JVM 13 条单测（空账诚实、真实事件解锁、连续 3 天跨「今天未活跃」不断、31 天裁剪到 30 且累计/成就不掉、walk 最值与累计、100 次抚摸、peer 按核心裁决、深夜、坏文件/异版本归零、重载不回锁）；Android smoke 打开每日任务页断言成长区块与「空态或连续」二选一真实态 + 成就行。
- **边界**：不迁移/伪造 iOS 历史、不同步云端、不改核心；数据链仍各端本机（B4 §7）。

### 四之二十五、不做清单（老板拍板，记录在案）

① Android 正式签名——不做，继续 unsigned 自签路线，文档照旧写「未签名」；② AI 内置模型服务——不做，维持用户自填端点；③ 中文圆体——不随包放字体，字体档位收敛继续搁置。

### 四之二十六、官方语义补齐 + 推送前 QA 把关（2026-11-17）

对 `docs/AUDIT_OFFICIAL_PARITY_2026-11-16.md`（82 agent 地毯式对抗审核，13 gap + 10 drift）的修复批次。两个老板拍板：robot.* 协议面做完整版（含 50Hz 订阅推送）、vetbook.share 只做本地卡片。

- **策略管理**：walk 槽支持 manifest 条目 + `POST /config {"walk_policy"}` 覆盖（官方 config 覆盖语义）；manifest 四项声明校验（model_api/robot.model/obs_len/action_len，官方 incompatibility 文案）；`/skills` 按加载状态条件列举；reload 失败原因经 `duck_core_last_error` 回传两端（顺带修掉 branch-B 加载 JSON 键全错、固定成功文案的预存 bug）。
- **运行时语义**：voltage_adapt 电压缩放；接管前姿态分类/坐姿起立接线；移除跳 Homing 的捷径（官方 2s 爬升成为真实行为）；limp-fall twist 归零 + driving 沿 reset_on_resume；拾取分类器 mode==Walk 门控；tick 主干顺序对齐官方并用官方 `Safety::gate`（deadman 产生 Limit）；`limited_by` 上报 /state；battery 百分比统一官方 6.6–8.2V；嗓音种子由构建指纹派生；health degraded/unhealthy 分档 + 45Hz 地板（按慢放速度归一、暂停恢复不误报）；Checkpoint 补齐五个控制态（fork/rewind 轨迹一致）；**disturb 门控**：驱动中换驱动网络先回 home 再换（官方 change_disturbs 语义）。
- **协议面**：官方 `robot.*` JSON-RPC 2.0 / NDJSON 服务（127.0.0.1:8768，api_version=51，robot.move/head/do/health/subscribe，按订阅者 hz 服务器端降频推送 robot.state）；UDP pad 三包协议（DKPD 解码 + DKST 回执 + DKRB 报告，官方位表/长按阈值/refused 30s 直接编译官方 pad-map 源）；pad 离散事件打到 active 分支（时间机器语义纯净）。
- **双端**：Android 补 fingerprint/lastError JNI 与仪器页出处指纹；vetbook 本地分享卡片（复用明信片管线，零编造空态）；iOS DKPD 摇杆轴序修正（含接收端一致性测试）；Origin 改 scheme://host 精确匹配。
- **推送前 QA**：5 维对抗审查 → 34 agent，拦下 7 项 major（租约不刷新 deadman 时钟、早退冻结帧、地板误伤慢放、Checkpoint 漏态、分支 health 来源、pad 打错鸭、disturb 假承诺）全部修复并逐条做「revert 必红」验证；遗留 5 项与 CI 盲区归档 `docs/QA_PRE_RELEASE_2026-11-17.md`。
- **验证**：本地 `cargo test --workspace --include-ignored` 213+9 全绿；`--features mujoco` 226+9 全绿（ffi:: 12/12 及以上，覆盖 C ABI/pad/IPC）；六道静态契约门禁全绿。Swift/Kotlin 与真机由 CI 裁决。
- **CI 盲区（记录在案）**：core.yml 不编译 ffi.rs（无 mujoco feature），IPC/pad/ffi 接线靠 ios.yml/android.yml 构建与冒烟间接覆盖；后续立项给 core CI 加 mujoco 测试步骤。

### 四之二十七、全方位对比报告（2026-11-18）

`docs/COMPARISON_OFFICIAL_VS_MOBILE_2026-11-18.md`：8 维度 ×（分析 + 对抗复核）+ 合成（17 agent，3.1M tokens）。128 条 item：脚本清点 equivalent 51 / near 29 / deviation 27 / not_doing 8 / hardware 5 / unverified 8；采纳 10 条复核纠正（全部 equivalent→near）后落表 **41 / 39 / 27 / 8 / 5 / 8**。另列移动版独有能力 30+ 条（时间机器/fork、care/quests/成长手册、邻居合唱、MCP、备份搬家等官方没有的东西），27 条偏差集中成下一批施工清单（其中 6 条对应 QA 推后项），复核员补漏 36 条单列。

### 四之二十八、按对比报告全量修复 + 三轮对抗复审（2026-11-18 晚）

按老板指令「找到的问题全部修正」，对 `docs/COMPARISON_OFFICIAL_VS_MOBILE_2026-11-18.md` §5/§5.1 的可修项全量落地。六路并行工单 + 三轮对抗复审（6 维审查 → 双镜头验证 → 定点复核），累计拦下 **2 blocker + 16 major**，全部修复，每项带「先红后绿」回归证据。

- **runtime**：rest/shutdown 分离（AtRest 保持 rest 位姿+力矩、cannot-sit 走 forget+切扭矩、摔倒不硬坐）；`enable(false)` 停策略回 home/座位（实测位姿）、`relax` 切扭矩；`init` 官方四分支（Limp 只上力矩不开策略）；health 启动判据改「从未成功读」（官方两分支）+ policy_error/连续错误；读取 coast；`safe_to_restart`；state skeleton/t_ns；timer 规则移出预算分支且夜间不触发；Checkpoint 补 disabled_hold/coast/has_read 等（fork/rewind 父子轨迹一致）。
- **ffi/ipc**：DKRB 恢复 1s；pad Home/BodyPose/sit-stand/Rest 对齐官方；Drive 官方四轴；`robot.*` 扩到 21+ 方法（look/pose/mouth/sound/flashlight/stop/enable/init/relax/rest/shutdown/mode/setMode/policies/model/vetbook/skills/loadPolicy/reloadPolicies/safeToRestart，subscribe 补身份字段）；disturb 门按全部驱动槽 + Reload 语义；swap 落地绑定 owner 鸭（分支会话不误报）、step/seek 共用落地钩子、C ABI 三入口补驱动门；mode 切换等待在途换包。
- **sensors/scripts/CI**：`to_tof_grid` 改用官方 `Reprojector` 几何；`fetch-policies.sh` 以 manifest 为清单（路径穿越拒绝）；core CI 新增 fmt/clippy、MuJoCo 特征测试 job；contracts 新增 shellcheck；mujoco 测试去 Windows 硬编码（fixtures/DUCK_POLICIES_DIR）。
- **双端壳**：walk 覆盖持久化重放（两端同键同名；`"none"` 归一化）；规则分享/粘贴保留 `period_s` 与 timer 词表；Android 接 shake→push（阈值/限频与 iOS 逐值一致）；Android fingerprint/lastError 已在上一批。
- **文档**：COMPANION 三动词→七动词；对比报告新增 §0 修复状态（27 条偏差与 §5.1 逐项处置）；本条批次账。
- **验证**：本地默认特性 244+9、mujoco 273+9、Android JVM 70 全绿；六道静态契约门禁绿；Rust 侧新增测试 40+ 条。CI 落地：**Core ✅（含新 MuJoCo job，ffi 测试首次进 CI）、Contracts ✅、Android ✅（模拟器冒烟含新情绪断言）、iOS ✅**——四工作流全绿。
- **CI 落地时修的三件基础设施**：① ONNX Runtime 的 GitHub 仓库整个 404（2026-11-18），Linux 取库脚本回退到同版本 PyPI wheel（stdlib 解析 URL，不依赖 pip）；② 新 MuJoCo job 改用源码构建的共享库 + rpath（静态档案在该 runner 上 lld 取不到符号，`ld -r`/`ar`/whole-archive 三法皆失败）；③ fmt 门禁只格式化本仓库自有文件（rustfmt 会顺着 `#[path]` 进入 upstream）。
- **顺手修出的产品语义**：`play_emotion` 落实官方「一次只播一个情绪、期间请求丢弃」——此前每日任务完成的庆祝会覆盖孩子刚点的情绪（冒烟里 curious 被 excited 顶掉即是此因）。
- **保留（刻意/边界，未改）**：BAM 执行器路径、齿隙编码器读数、传感器回报语义、麦克风替身、多鸭同世界、ToF 场景射线与相机帧流、命令延迟模型、运行期增删技能、社区策略门禁、setMode 整机重建、updaterd/configd 族、robotctl CLI、chorale 乐谱、head_imu、API_VERSION 握手、coverage 基线——均在对比报告 §0.2 逐条登记理由。

### 四之二十九、情绪图鉴 + 语音闲聊（2026-11-18 夜）

COMPANION 头脑风暴里仅剩未做的两项落地：

- **情绪图鉴·把戏学校**：核心 `GET /collection` 返回 13 个情绪与 manifest 技能的**真实次数与首次解锁日期**。零编造规则:`Life.unlocks` 只在计数出现时记录当天;升级前的历史计数回填为「日期未知」(界面显示 —);`POST /life` 恢复时只接受规范键与 `YYYY-MM-DD`,客户端写不进假日期。双端各有图鉴页(复用 Dex 路由),点行即用现有技能/情绪通道去练习。
- **语音闲聊**：AI 伙伴页内新增聊天区。用户输入(或一键取回 `last_heard`)→ 同一 OpenAI 兼容端点、纯文本回复 → 气泡显示 + 本机 TTS 朗读(音频只在手机)。提示词禁止编造,只许引用 `/state` 的真实数字;未配置模型时禁用。
- **验证**：本地 Rust 252+9 / mujoco 282+9、Android BUILD SUCCESSFUL(85 JVM 测试)、六道契约门禁全绿;Swift 与 TTS 实机由 CI/真机裁决。QA 对抗复审结果见 `docs/QA_FEATURE_BATCH_2026-11-18.md`(若复审有 must-fix,已按档修完再推)。

### 四之三十、图鉴徽章 + 教它说话（2026-11-18 深夜）

第三批功能点，延续「零编造 + 手机是外设」两条边界：

- **图鉴徽章**：情绪图鉴顶部九枚徽章（初次心动/十三种心情/第一课/把戏毕业/十·百·千次心情/十·百次把戏），条件全部来自 `/collection` 的真实计数；进度显示真实分数，空技能表不算毕业，重复行去重防灌水。双端纯壳层实现。
- **教它说话**：孩子给鸭子自己的语音词（≤24 字）绑定真实目标（13 情绪、manifest 技能、坐下/站起/捡东西）。核心 `/config {"voice_aliases": …}` 整表校验（目标必须是当前已加载动作，非法整表拒绝）；`interpret_with` 自定义词先于内置词表、最长短语优先；执行走原有 trigger_skill/情绪/坐站/捡拾路径，暂停门与 policy 门全部继承。词表只在手机（UserDefaults/SharedPreferences），核心就绪后重放一次，核心不落盘。
- **验证**：核心 265+9 / mujoco 307+9（新增 10 条别名测试）；Android 106 JVM（新增 18）；iOS 新增 23 单测；六道契约门禁绿（Copy 336=336）。Swift/真机由 CI 与真机裁决。
- **QA**：3 维对抗审查 + 双镜头验证，结论与修复记录见 `docs/QA_BADGES_ALIASES_2026-11-18.md`。

### 四之三十一、技能热插拔 + 家长看板「日子」（2026-11-19）

第四批功能点，两个真缺口（对比报告 §5.1 与已拍板池的残余）：

- **技能热插拔**：`POST /config {"custom_skills":[...]}` 与 IPC `robot.setSkill/removeSkill`（参数逐字对齐官方 `SkillParams`）。整表校验：只收 `kind=episodic`、`name` 为裸文件名（拒绝 `/ \ ..`、前导 `/`、盘符前缀）、文件必须存在于策略目录；`path:"none"` 按官方语义关闭同名套装技能；accepting 前跑官方 `duck_control::policy::validate`。load 时合并、同名覆盖；坏条目不入表；文件暂时缺失只跳过本次并告警，表保留。下一次 reload 生效，`/skills`、`robot.skills`（`overridden` 标记）、vetbook 同步。
- **日子（家长看板）**：`DayRecord` 补齐八项真实每日增量（被摸/被抱起/摔倒/被喂/拍手/口哨/捡拾/坐下；旧存档缺字段=0），`GET /days` 返回已完成日、今天实时增量与本周合计（最近 7 天+today，全部同口径相减，钳零）。`sit_downs` 按官方 vetbook 的 label 边沿在运行时累加。恢复闭环携带 `days` 与 `day_started`：没有可信基线时重设为当前 life 并盖上今天，**绝不把终身计数算进今天**。双端「日子」页（工具页，2 跳）：本周卡 + 每日九项 + 今天行 + 空态 + 双语分享；离线与真空态分开。
- **验证**：核心 290+9 / mujoco 341+9（本批修复新增 13 条测试，含 6 处先红后绿）；Android JVM 全绿（DaysSheetTest 10）；六道契约门禁绿（台账 44 行/50 条目，Copy 350=350）。Swift/真机与真实 ONNX 热插拔由 CI 与真机裁决。
- **QA**：3 维对抗审查 28 agent，拦下 2 blocker（setSkill 路径穿越；冷启动把终身计数当今天）+ 4 major，全部修复并带红绿证据，见 `docs/QA_SKILLS_DAYS_2026-11-19.md`。

### 四之三十二、D1 排版与网格——深度重构第一批（2026-11-20）

总纲 `docs/UIUX_REFACTOR_DEEP_2026-11-19.md`（对抗评审结论在 §9）的 D1，按「契约先红、再施工」执行：

- **令牌三层落地（双端同名同值）**：`DuckTheme.swift`/`DuckTheme.kt` 在色层之上补 `type`（iOS 语义字体七档——display/title/body/label/caption/data/micro，动态字体免费生效；Android Type 10/11/13/15/20/28 sp）、`space`（4 基数 2–32）、`radius`（tiny/thumb/control/card/panel）、`icon`（12/15/18/22，不随字号缩放的几何量）。
- **裸值清零**：iOS 207 处、Android 143 处固定字号全部走令牌（主题文件外 `.system(size:`/`fontSize=`=0）；padding/spacedBy/Spacer/cornerRadius 全部收进 4 基数与五档圆角（off-grid=0）。棘轮 `check-ui-baseline.py` 重定基线并收紧到 0，新增 `ios/android_offgrid_spacing`、`ios/android_radius_literals` 四项；计数排除两个主题文件（令牌持有地）。
- **动态字体**：首屏固定高容器改自适应（iOS 拇指台/时间栏 `minHeight`，标签允许两行、去掉 `minimumScaleFactor` 压字；Android 同步）；新增 XCUITest `testFirstScreenSurvivesAccessibilityXXXL`（AX3 启动，五个动词 + 时间栏不出屏不重叠）与 Android 冒烟 `font_scale 2.0` 段（uiautomator 断言同语义）。
- **新门禁**：`scripts/check-ui-scope.py`（UI 批次触碰 `core/`、`upstream/`、`patches/` 即红，书面豁免除外）挂进 contracts.yml 与 ios.yml/android.yml 的本地检查组。
- **施工留痕（冒烟揪出两个真缺陷，均已修）**：① 时间轨 `height(48.dp)`→`heightIn(min=48.dp)` 是 D1 引入的真缺陷——内层 `fillMaxHeight()` 失去唯一上限后把整列吃掉（失败 dump 实测时间轨按钮 532dp、拇指台被挤成 16dp 残条、五个动词退出语义树），恢复固定高度；该行无随字号缩放内容，固定 48dp 本就正确。② 语言切换从不持久化（iOS 写 UserDefaults、Android 只改内存）——跨端 parity bug，`font_scale` 段的重启揭出；两端统一为切换即持久化，两处重复 handler 收敛为 `flipLanguage()`。冒烟自身同步加固：抽屉未开重试、双通道滚动，失败时 dump/截图/dumpsys/IME 状态全部落 artifact——本批三次定位全靠它。
- **验证**：本地 `compileDebugKotlin + testDebugUnitTest` 绿、八道静态门禁绿；CI 四工作流全绿（Android+Contracts 于 `86b5365`；iOS+Core 于 `20189e8`，其后提交未触碰两路径）。棘轮 `shell_kt_lines` 压回 3763 基线。
- **附**：D2 组件契约先行件已备于 `contracts/ui-components.json` + `scripts/check-ui-components.py`（未接线，随 D2 首推按「先红后绿」落门禁）。

### 四之三十三、D2 组件库定义批（2026-11-20）

深度重构第二批，按老板裁定「定义一次建齐、接入分两波」执行：

- **契约先行**：`contracts/ui-components.json` 冻结 11 个基础组件（两波）的双端文件、状态矩阵、令牌清单；`scripts/check-ui-components.py` 断言实现双端在位、源码 `STATE MATRIX:` 行与契约逐字相等、组件文件内零裸字面量。首推 `0d73354` 有意红（组件尚不存在），次推转绿——IA 批「契约先红、再施工」的既定纪律。
- **11 组件双端落地**：一波 DuckButton/DuckChip/DuckCard/DuckSheetHeader/DuckListRow；二波 DuckToggle/DuckSegmented/DuckProgress/DuckEmptyState/DuckErrorCard/DuckLoading。iOS 落 `Design/DuckButton.swift`、`DuckSurfaces.swift`、`DuckControls.swift`、`DuckStates.swift`；Android 落 `DuckComponents.kt` + Icons 补 check/close 两枚。每组件源码带状态矩阵行，只引用令牌，统一按压反馈（scale 0.96 + 变暗，减动效即无）。
- **手抄关闭钮收敛**：15 处逐字节相同的 xmark 块换成共享 `DuckCloseGlyph`（各 Button 保留自己的 action 与测试 id，零行为变化）；棘轮 `ios_close_button_files` 16→1；顺带修掉 ProgressKit 硬编码「关闭」（→ `Copy.close`）。
- **施工留痕**：iOS 首推红在 `.foregroundStyle(.duckX)` 隐式成员简写——令牌声明在 Color 上，ShapeStyle 上不可解析，5 处改显式 `Color.`（`feba979`）；棘轮学会识别组件实现层（DuckComponents.kt 的 `indication=null` 伴随 graphicsLayer 缩放/变暗，不计入 press_no_feedback）。
- **验证**：组件门禁 + 八道静态门禁绿；Android compile + JVM 绿（`68c4a07`）；iOS 编译绿（`feba979`）；Contracts 绿（`feba979`）。
- **下一波（接入）**：把组件用进高频面（抽屉行、sheet 头、按钮），每屏独立绿；DuckTheme.kt/MoreSheet.kt 归老板领地，接入时由老板侧合并。

### 四之三十四、D2 接入波 + D3 动效/触感（2026-11-20）

- **D2 接入波**（`030cadc`/`61f93e8`）：iOS 抽屉页的 14 处 `PageRow` 委托 `DuckListRow`，四模式选择器改 `DuckSegmented`；Android 抽屉 16 处 `MoreRow` 委托升级后的 `DuckListRow`（吸收 icon/badge/chevron）。调用点测试锚点保留。留痕：`030cadc` 首推红在入口台账锚点被换组件吞掉（`play-more-mode-`），`61f93e8` 以 `play-more-mode-stick` 恢复；该提交的 iOS「红」实为被后续提交取消，非编译失败。
- **D3 动效与触感**（`4cde149`，四工作流全绿）：`DuckMotion` 四档（quick 0.15 / base 0.25 / slow 0.40 / deliberate spring）双端；28 处内联动效全部收口（内联动效棘轮 0/0）。减动效单点：iOS `withDuckMotion` 与按压反馈、Android 在属地处读 animator scale。触感地图 12 个真实事件点（摸/技能/模式/表面切换/喂/路径完成/摔倒），路径与摔倒只在真实边沿响，首帧静默。棘轮新增 `ios/android_inline_motion`（0, max）与 `haptics_sites`（12, min）；`shell_kt_lines` 因真实接线两度有意重定（3772→3816→3831）。

### 四之三十五、D4–D7 收口（2026-11-20，合并推送，逐项记账）

四批文件在 `Shell.kt` 等同一文件上重叠（a11y 与 perf 同文件），无法按主题干净分推；合并为一次推送，CI 对整棵树判绿，账目逐项如下：

- **D4 无障碍**：舞台一元素 + 四自定义动作（向前走一步/左转/右转/摸摸它），两端走同一手势路径（250ms 摇杆脉冲，pet 走真实 /pet）；事件播报单点化——iOS 在 `moodText` 一处 `UIAccessibility.post`，Android 给事件 pill 加 `liveRegion`。新增 `scripts/check-contrast.py`：从两端主题文件现算 12 组 WCAG 组合（11 组 UI token + 品牌喙/画布），全过（最低 4.83:1），并断言两端同名 token 同值；已接三工作流。新增 Copy 键 5 个两端同推（358=358）。棘轮 a11y 下限升至 customActions 3 / announcements 2。
- **D5 状态接入**：Android 失败卡 → `DuckErrorCard`（retry 锚点保留）、唤醒文案 → `DuckLoading`；`DuckErrorCard` 增 `retryTag` 并修正 iOS 语义为 `.contain`（重试保持独立 VoiceOver 元素）。各表面三态矩阵仍为遗留（见下）。
- **D6 断点与平台差异契约**：`contracts/adaptive.json`（<600 / 600–899 / ≥900 三档，同一 IA）+ `contracts/platform-differences.json`（返回手势、sheet detent、Toast、触感 API、语言存储五条，逐条带理由）+ `scripts/check-ui-parity.py`（断言三档完整、每条差异非两端同文、两壳的 600 断点唯一）；三工作流已接。
- **D7 性能可测**：Android 调试构建挂壳层重组计数器（每 5s `duck-perf` 日志），冒烟读末值并记录（首个数值随 CI 落盘，**先记录后设上限**，未伪装成预算）；仪表 30 控件列的 Lazy 化与 iOS 107 `@Published` 拆分风险过高，明确列为遗留。
- **验证**：本地 Android compile + JVM 绿；十道静态门禁绿（含 contrast/parity 新门）；总纲 §2 基线表已刷新为施工后实测。

### 四之三十七、方向 A「房间里的物件」落地上线版（A1，2026-11-21）

老板在 `design-demos/A-complete.html` 三稿打磨后拍板方向 A（`design-demos/direction-approved.md` 存原话），本轮把它落进两端真实界面：

- **Live 主屏改骨架**：底部五个悬浮圆钮 → 床头柜木板上的四件东西（陶碗=喂食，碗里饭量=饥饿；卡盒=技能；铜铃=叫一声；摊开的手=拎起）。**木板本身就是抽屉把手**（点木头拉柜子，More 的 id/标签不变），首屏可点仍是 7 个。
- **墙上的两件东西**：木壳长条挂钟（`duck-time`，点开时间机器、长按冻结）＋圆形黄铜气压表（`duck-readings`，双针、密刻度，开仪表）。钟表形体/材质拉开，不抢鸭子。
- **取景挪位**：Live 不再有取景钮（7 个控件的预算），取景入口移到「更多 → 玩一玩 → 取景」（触发后页面自动交接进取景），`surface-toggle` 的 id 与台账锚点同步迁到 `MorePages.swift/.kt`。
- **令牌**：新增木/陶/铜/食物色 7 个（两端同名同值）；新组件只引用 `DuckSpace/DuckRadius`，棘轮 offgrid/radius 保持 0。
- **测试**：iOS 三处 Director 流程改走抽屉两跳（`openDirector()`）；Android 冒烟同一路径；首屏 `duck-time`/`duck-feed` 等 id 全部保留，AX3 首屏断言不变。
- **验证**：Android compile + JVM 绿；十道静态门禁绿；`shell_kt_lines` 3836→3593（删掉旧轨与拇指台）。Swift 编译由 CI 裁决（本机无 macOS）。


### 四之三十六、Dense-sheet AX3 扫掠收口（2026-11-21）

D4 遗留的「dense sheet 全表」由老板起头、本轮收口。两端 CI 全绿：Android `51e8193`、Contracts `51e8193`、iOS `0de1de1`（其后提交未触碰 iOS/core）。

**产品侧真缺陷（3 处，全部有 AX3 实测证据）**
1. **ensemble 芯片 42.25pt**：VoiceOver 命中框裁掉胶囊两端，低于 44 地板；地板抬到 48（全 App 控件标准高度）。
2. **iOS Neighbours sheet 无滚动容器**：AX3 下内容高于 sheet，未滚动 VStack 被垂直居中，头部（含 `peers-close`）被挤出屏幕——头部固定、内容滚动。
3. **Android AI sheet 头部在滚动内容里**：2x 字号 + IME/聚焦把标题与关闭钮顶出视野——头部固定、内容滚动（与 iOS 同一条规则）。

**测试侧硬化（8 处，每次按 dump 证据修）**：Daily quests 期望标题误用卡片小标题；Duck panel 的门是「鸭子的脸」而非分组标题「这只鸭子」；反向滑动 = bottom sheet 关闭手势（改为单向 + 四轮关闭重开抽屉复位滚动）；关页返回不改滚动位置导致目标行在视口上方；多按 BACK 退到桌面（`ensure_live` 用首屏动词指纹 + 无条件 `am start` 拉回前台）；时间钮点击重试；失败落盘 `dense-dump.xml`/`dense-screen.png`；iOS 测试日志与断言进 artifact 与步骤摘要（ci-logs 发布竞态曾吞掉现场）。

**性能可测层**：Android 调试壳层重组计数实测整轮 **2–3 次**（状态下沉本就达标），门禁设 50——只有整树重组回归才可能触发。

**判分更新**：七条口径第 2 条（动态字体/AX3/命中区）由「部分」升为**达标（首屏 + dense sheets 两端全量扫掠）**；第 5 条获得真实可测上限（Lazy 化与 @Published 拆分仍为遗留）。

### 四之三十七、桌面版遗物归档（2026-11-23）

产品只有 iOS + Android（README），M1 桌面 sim harness 的代码早已随桌面版移除；本次把剩下的遗物从工作树清出去。

- **入库**：`M1_REPORT.md` → `_archive/desktop-m1/M1_REPORT.md`（git 识别为移动）；`_archive/README.md` 说明历史与「勿删」清单；`docs/COMPARISON_OFFICIAL_VS_MOBILE_2026-11-18.md` 的 5 处引用改指新路径。
- **本机留痕（.gitignore，不入库）**：`_archive/builds/`（mujoco-310/cmake/lib、target-mj-r2/3/4、refrender、scratch 等 16 个构建目录）、`_archive/desktop-m1/scripts/`（jaw_* 桌面渲染试验 21 个、GLSL 着色器、traces/ 轨迹 CSV）。work/ 从 9.5G 降到 6.2G。
- **明确不动**：`work/mingw-binutils|ort-win|shim|mujoco-lib|tmp(包装脚本)|.venv|.cargo|.rustup` — 本机 Rust 核心测试回路（含 `--features mujoco`）依赖它们；`verify_odom_roller/`、`audit_fp/` 是被 git 跟踪的官方语义验证件。
- 十个静态门全绿后提交 `c7430bc`。

**本机软件卸载（老板逐项确认后执行）**：
- 全局 pip 桌面栈 `mujoco 4.0.1 / mujoco-core / mujoco-studio / mujoco-studio-wasm / glfw` — 已卸载。`scripts/reference-render.py` 改用 `work/.venv` 的 mujoco 3.10（与上游锁定版一致），头部注释已写明。
- 全局 rustup（`~/.rustup` + `~/.cargo`，MSVC 工具链，635 MB）— 已删；本机核心测试走 `work/.cargo` 便携 GNU 工具链（记忆 local-core-test-loop），PATH/注册表卸载项都指向 work/ 那套，全局只是残留。删后便携 cargo/rustc 1.99.1 复测正常。
- `C:\Program Files (x86)\Microsoft Visual Studio\2022` 与 `C:\Program Files\Microsoft Visual Studio\2022` 两个 0 KB 空壳 — 已删（需提权）。VS 2026 Insiders（11 月前的自有 IDE）与 VS Installer 保留。
- `work/.venv` 保留：build.rs 兜底头文件路径 + 运行时 mujoco.dll + reference-render 的渲染后端都靠它。

### 四之三十八、A1 冒烟三红定位与修复（2026-11-23）

拉两端 CI 产物按证据定位，不甩抖动。三处红同源于 Direction-A 架子的两处布局病，各端表现不同：

**病根一：板子贪心撑满全屏（iOS 实锤）**。`DuckShelf` 的板用 `frame(maxWidth:.infinity, minHeight:82)` + 内嵌 `Spacer`——`minHeight` 是地板不是天花板，chrome VStack 把剩余高度全给了最后一个元素，木板铺满整屏盖住鸭子。证据：`work/ci-ios-a1/smoke-logs/sim-drive/frame-1.png` 里鸭子完全不见、四物件浮在板中央。后果：`frames 1-2/2-3 differ (scene is live)` 两红（板静止→三帧全等），但 `--expect-shell` 颜色检查仍过（碗/手是亮像素，1582 px 骗过了它）。

**病根二：AX3 下物件列压住拉手文字（Android 实锤）**。旧结构先画板（可点）再画物件 Row（叠在上层）；2x 字号下技能物件列 `[93,157]×[494,608]` 与 更多 文字 `[139,163]×[491,539]` 在 x/y 都重叠，Compose 命中给后画的物件层——点 更多 开了技能环而非抽屉。证据：`dense-dump.xml` 失败瞬间是主屏（无 积木规则 行）、`dense-screen.png` 板仅 82px 而物件标签溢出到 608。11 个 dense 页里前面靠运气过，Blocks 页运气用完。

**修复（两端对齐原型语义 `.shelf-board{height:132px}`）**：板改固定 132pt/dp（`DuckShelf.boardHeight`），物件坐进板内居中、上移让出底部拉手；Android 拉手文字独立成 Column、**最后组合**（叠在物件之上）并给默认水波纹（`LocalIndication`，满足 press-feedback 棘轮）。iOS 规则编程 300ms 超时：写成功、读回 PASS，仅应答超时——A1 是 UI-only 提交不碰 core/intent 路径，判为板盖鸭引发的前序时序连带，板修好后随冒烟重跑验证（若仍红按真缺陷单查，不预设）。

**验证**：十静态门全绿；Android 本机 `assembleDebug testDebugUnitTest` 通过（修了 sizeIn import + combinedClickable 需显式 indication）。推 CI 双端复跑。

**后续四轮迭代到双端全绿（每轮拉产物按证据修，不猜）**：
1. `9a3dfe8` 后 Android 红「8 控件超预算 7」：拉手独立命中条时忘了板子还可点——板降为纯场景（`7641e99`）。同轮 iOS 揪出 `155f3c0` 的拼接事故 `playMore.tap()        openDirector()`（语法错+自我递归，UI 测试连红四轮）——补全三步门 More→玩一玩→取景（`63011b4`）。
2. `7641e99` 后双端同病同治：AX3 下「更多」字框落进技能列框（Android overlap 门）；iOS 板级 Button 的 frame=整个架子，XXXL 门判 duck-feed overlaps duck-more。**结构定论：上下双带**——物件行内容自适应在上、拉手满宽 48 独立条在下、木纹是容器背景（Android `d06cdb7`、iOS `b33a951`）。同轮修 sweep 开局敲旧板心的死木头逻辑。
3. `b33a951` 后 iOS 剩两项时序红（chain last=chirp、curious 读到 excited）：`wait_idle` 按 label 判空闲是假的（技能时间线期间 label 恒 walk），前一条时间线还占着鸭子——改轮询诚实字段+等 emotion 清空（`14e2a1c`，**iOS 转绿**）。Android 剩 This duck 页 witness 不中：先修「失败 dump 读远端旧树」（`2387f1b`）再放宽窗口（`ba3cf41`）——都不是主因。
4. **真根因**（a8 dump 直接终结争论）：`dense_sheet_opened` 精确集合匹配，但该页 感受它的心 只存在于长行标签 `感受它的心——由心情推出来的读数` 的前缀里——改子串匹配（`ce15170`，**Android 转绿**）。十一页能过是因为它们的 witness 词在别处以完整标签出现过。

**收口**：iOS `14e2a1c` ✅、Android `ce15170` ✅、Contracts `ce15170` ✅、Core `c5bc38c` ✅（UI 批次未碰 core/，check-ui-scope 把关）。A1 批次双端全绿。教训入记忆：Edit 工具把仓库 .sh 重写成 CRLF，改脚本后必须 normalize + `bash -n` 再提交（本轮踩两次，autocrlf 掩盖了入库 blob，只炸本机）。

### 四之三十九、A2 批次：柜子、时间机器与仪表的木/铜语言（2026-11-23）

按 `design-demos/A-complete.html` 的材质语言，三块表面换装，标识/IA/动词零改动：

- **抽屉 = 木柜**：`ModalBottomSheet` 容器色 `WoodDeep` + 内容列木纹纵渐变（`WoodHi→Wood→WoodDeep`）；分组标题改铜色（`Brass`）。iOS 用 `.presentationBackground(LinearGradient(wood))`，Android 用 `containerColor` + `Column.background(Brush.verticalGradient)`。原型 `.drawer` 的 30px 顶圆弧与内高光由 Material3 的 sheet 形状承担。
- **时间机器 = 木匣**：iOS 的 scrubber 包进木纹圆角匣（`.tm-case` 语言）：内描边黑影、琥珀播放头（径向渐变瓷珠+描边）、四角铜钉（`brassRivet`，径向渐变）；刻度读数改铜色。Android 的冻结/单步/倒放/倒回/分枝 chip 组进同款木匣（`Box` 渐变+黑影描边+左上两枚铜钉）。
- **仪表 = 铜表盘**：仪器 sheet 容器同木色（iOS `.presentationBackground`、Android `containerColor`），一眼两块木面同柜。
- **全柜一致**：原型里柜子的每一格都是木托盘（`.tray`），所以抽屉可达的每一页（规则/编舞/玩一玩/邻居/AI/任务/日记/明信片/图鉴/工具/这只鸭子/大按钮/AB/备份/别名/天数）两端统一木色容器；照片预览是 Dialog，保留暗底让照片最亮——两端同一例外。iOS 用 `CabinetTray` 修饰器（`route == .photo` 时跳过），Android 用 `CabinetTray` 令牌替换 17 处 sheet 的 containerColor。

**验证与收口**：十静态门全绿；Android 本机 `assembleDebug testDebugUnitTest` 通过。CI 全绿：iOS/Android/Contracts `78d204d`（Core 未触发——A2 全在 UI 层）。中途自查抓到一个编辑叠加出的多余 `}`（Swift 编译必挂），修复推 `08cc1a0` 后才进 CI。所有 testTag（more-title、time-pause.fill、time-play.fill、rewind-play、more-group-*）与行文字保持原样。截图验收：`dist/ui-b4-android-more.png` 木纹抽屉+铜色分组。**方向 A 批准套件（主屏/抽屉/时间机器/仪表）至此全部落地且双端全绿。**

---


### 四之四十、发布否决遗留收尾：截图矩阵 + 组件状态行为断言（2026-11-23）

- **遗留 5（截图矩阵，竖/横/XXL）**：Android 冒烟新增两格——横屏（旋转→等重排→截图→转回，PNG 宽>高断言）与 XXL（字号 2.0 时截 Live 屏）；iOS 新增 XXL 格（`simctl launch` 带 `UIPreferredContentSizeCategoryName=AccessibilityXXXL` 启动后截图）。三个新图进 workflow 的 dist 发布与 artifact 清单。iOS 横屏截图缺口如实登记：simctl 无宿主侧旋转 API；横屏行为本身由 `testLandscapeRoundTrip` 覆盖（旋转往返 + 技能投递 + 转回）。
- **遗留 1（组件状态行为测试）**：iOS 新增 `testComponentStatesMoveWithRealTaps`——DuckSegmented 的 selected trait 随真实点击迁移、DuckListRow 开关行（双人接机）的 selected trait 点击翻转且二次点击复原；Android 冒烟新增开关行 selected 翻转断言（读 uiautomator `selected` 属性，翻转+复原）。禁用态仍靠既有端到端路径（核心未就绪时控件 disabled + 冒烟可达性断言），视觉按下态不在快照基建内——如实保留为剩余缺口。
- **遗留 2（dense AX3 全表/44-48/焦点）**：核对后已由现有双端扫掠覆盖——Android 逐页断言首焦点+47dp 底线+不出屏；iOS `testDenseSheetsSurviveAccessibilityXXXL` 逐页 assertSheetFocus/assertHitTargets。QA 文档该条可视为关闭（文档为 3 天前快照）。

**收口（2026-11-23，双端全绿）**：iOS `02d4511` ✅、Android `260c6d0` ✅、Contracts ✅。截图矩阵三格已入库（`dist/ui-b1-android-land.png` 640x320、`ui-b1-android-xxl.png`、`ui-b1-ios-xxl.png`，ui-bot 提交 `fceb706`/`36714ab`）。组件状态断言探路时抓到两条真问题：① `DuckInput.p2On` 是普通 `@Volatile var`，开关行与 P2 chip **点了不重组**——改为 Compose 快照状态；② Compose 的 selected 语义在本机 uiautomator 上以 `checkable/checked` 暴露（comp-dump：checked 翻转、selected 恒 false），冒烟读取改为 checkable 时读 checked。另记一次自伤事故：一行式行尾归一化把 `smoke-test.sh` 清空并推送（`02d4511`），已从 `fd640a1` 逐字节恢复（`7396569`），教训入记忆（先读入变量再写回）。

### 四之四十一、官方之外的玩法三连（2026-11-24）

头脑风暴与排期见 `docs/PLAY_IDEAS_2026-11-24.md`。原则：不碰 `core/` 的 61 维观测与安全层，只用既有动词与 `/state` 真实字段。

1. **灯的编程**（`4350832`，双端绿）：核心 `rules` 早支持 `light` 动作（`runtime.rs:2735`，7 色+关灯）但两端编辑器都没暴露。两端补「灯」动作与颜色标签；Android 冒烟编程“拍手→蓝灯”，断言 `/state.leds.color` 翻蓝后清灯。
2. **声音涟漪**（iOS `159f6f9`、Android `da5fa92`，双端绿）：叫一声/情绪/问候时舞台扩散一圈琥珀涟漪。iOS 由 50 Hz tick 发布 `isSpeaking`（`controller_published` 棘轮**有意重定** +1，附注）；Android 由 Voice 的播放状态暴露 compose 状态。减动效折叠、走 `DuckMotion` 令牌、不抢触摸。
3. **成就庆祝**（iOS `414b198`，Android 同批绿）：`latestUnlock` 自 D5 发布却无人消费；首次解锁（启动时为 null，不会误触）触发鸭子真实 excited + 成功触感。涟漪随声音自动出现。

**验证**：十静态门全绿（含有意重定的 `controller_published 108`）；Android 本机 `assembleDebug + testDebugUnitTest` 过。iOS 冒烟绿；`testTheDuckVoiceIsHeard`（音频会话分类测试，与本改动无关）在负载高的 runner 上偶发红，同测试在涟漪轮次刚绿过——dispatch 复跑 45313689419 **成功**，按负载敏感归档。三玩法双端全绿：灯 `4350832`、涟漪 iOS `159f6f9`/Android `da5fa92`、庆祝 iOS `414b198`/Android 同批。

### 四之四十二、玩法续：路线明信片与回声游戏（2026-11-24）

4. **路线完成明信片**（`5b8ef1c`，随 `917b822` 一并双端绿）：走完全程后 15 秒内，画路条上出现「存成明信片」；点一下走 Director 同款快门（真实帧 + 房间），存成普通明信片。iOS 由 `pathDoneTick` 触发，Android 由 `/state.path` 的 active→false 转换触发（与已有的“走完全程啦”同源）。
5. **回声游戏**（`917b822`，双端绿）：内联在「玩一玩」页，免新路由。鸭子按顺序唱自己的四个合成音（chirp/coo/peck/inquire，`queue_sound` 真声），孩子依序拍回；每过一关加一声，5 关通关，错一声响 alarm（失败态），通关让鸭子用真实 excited 庆祝。iOS `EchoGame.swift`；Android `MorePages.kt` 内 `EchoGame`（`PathChip` 提为 internal 复用）。

**玩法池全部状态**（`docs/PLAY_IDEAS_2026-11-24.md`）：1 灯编程 ✅、2 声音涟漪 ✅、3 成就庆祝 ✅、4 路线明信片 ✅、6 昼夜（Daylight 既有）✅、7 回声游戏 ✅、9 定时（编辑器既有）✅；5/10（灯联动、情绪灯色）因灯只在仪表虚拟脸可见，标为收益低、暂缓；8（分枝社交）需双实例，留大版本。

**验证**：十静态门全绿；Android 本机 `assembleDebug + testDebugUnitTest` 过；`917b822` iOS/Android/Contracts 全绿。

### 四之四十三、鸭子本体的互动 + 性能记录 + 读屏播报（2026-11-24，双端全绿 `c0dfe4e`）

1. **身体分区可摸**（`d94e40b`）：此前 iOS 点鸭子一律“摸”，Android 主屏**点鸭子没有任何反应**。两端统一按身体分区回答：头→curious（抬头）、肚子→pet（咕咕）、脚→kick_left（踢）。画路/拎起/技能环/长按开环的手势让位不变（长按先置 ringOpen，抬手时 tap 分支自动让路）。
2. **挠痒痒**（`c0dfe4e`）：3 秒内连挠肚子 4 下→laugh（音效/情绪/触感都真）。窗口在壳层本地，每次触摸仍是真实 pet。
3. **读屏播报**（`18d25f6`）：回声游戏每阶段对 TalkBack/VoiceOver 播报（iOS announcement / Android polite live region），通关与错音同报。
4. **掉帧记录进门记录**（`e75837b`）：冒烟末段驱动“叫一声→拖动→开抽屉→返回”，`dumpsys gfxinfo` 记录 Janky 与 90/95/99 分位。首轮实测（模拟器软件 GL）：**Janky 83 帧 86.46%｜90th 57ms｜95th 150ms｜99th 350ms**——基线极高，上限按“先记录、再设限”另定；文档 `docs/PERF_BUDGET_2026-11-24.md`。

**验证**：`c0dfe4e` iOS/Android/Contracts 全绿；十门绿；Android 本机 `assembleDebug + testDebugUnitTest` 过。

### 四之四十四、可玩性审计与两批玩法落地（2026-11-24）

老板指令：100 条清单先审计，可玩性高的先做、差的全丢。审计见 `docs/PLAY_IDEAS_100_2026-11-24.md`「可玩性审计」节——100 条净剩 7 条，其余按「纯展示 / 语义冲突 / 单次玩具 / 无地基」四类丢弃。

**第一批**（`ef9dba6`，双端绿）：A12 头侧触摸转头（同一身体区域两种反应）；E57 猜拳（鸭子用自己的 sit/stand/kick 亮拳，出拳由 provenance 指纹+回合决定——同一只鸭子同一回合出同一拳，两只鸭子不同，是性格不是硬币；胜负说/触/显示，不记分不落盘）。
**第二批**：A1 心跳光环（`7077036`：iOS TimelineView 由墙钟算相位、Android 仅 bpm 存活时 30 Hz，减动效静止）、H74 自动存卡开关（`b50284c`，默认关）、B25 听见的气泡（`8cb6a8b`，Android 主屏补上——此前只在仪表页说话，主屏像聋了）、D36 羁绊里程碑（`f5e8d7b`：care.bond 过 0.5/0.8 各报一次，persist flag 防重报，鸭子 excited 庆祝）。
**丢弃复判**：E53 幸运骰并入丢弃——fork 是时间线实验语义，拿来开奖会混两条时间线。

**验证**：`f5e8d7b` Android + Contracts 绿；iOS 首次红在「大声惊吓」（`last_sound=coo`——pet 的 coo 与 bang 的 alarm 共享 100-tick 反应冷却，脚本注释里写明的负载敏感族），同代码复跑（dispatch `45374533435`，HEAD `8142b83`）**全绿**，按负载敏感归档。
**追加**（`0dc7495`）：A2 喂食张嘴——复判纠正审计误判（`/mouth` 意图两端真实驱动喙，TTS 已在用），喂食时嘴张开 0.6 s，有声音在响则让位；核心的睡着闭嘴上钳不动。
**追加**（`75c5dd1`）：猜拳场次比分——本页自己的记分（你 vs 鸭子），重开页归零、不落盘；两端同键。
**追加**（`0110369`）：掉帧门禁按"先记录、再设限"落地——三轮绿跑 band：Janky 47.4–86.5%、99th 121–350 ms；门禁只抓崩溃级退化（Janky ≤95%、99th ≤500 ms），解析不出即失败。CI 实测通过：`jank 47.41% | p99 150 ms`。
**遗留**：A24 声纹 / E48 识人跟随 / K98 真机回流 / K100 保真对拍需模型或核心工程，按红线单独立批。
**工具注记**：本机 `gh` 已不在旧路径（目录被新版本替换），CI 查询改用 git 凭据 token + GitHub 官方 API，轮询照旧。

### 四之四十五、新玩法补脚本化验收（2026-11-25）

QA 口径 7「每批验收必须可脚本化」的收尾：之前两批玩法（回声、猜拳）只有静态门与截图，没有行为断言——内联在「玩一玩」页的控件坏了，十门全绿也照样过。本批补上：

1. **Android 冒烟**：dense 扫掠在 AX3 字号下打开「玩一玩」后，新增 `playmore_games_check`——点「开始」必须到达「第 1 关」（鸭子真的把合成音排进队列、协程 550 ms 后才开拍键，状态文本是真实相位的证据）；点「石头」必须出现「鸭子出了 <手势>」，且场次比分行可读（正则读 `你 N · 鸭 M`）。失败时留 `games-dump.xml`/`games-screen.png`。顺带截 `ui-b4-android-games.png` 进 dist 与产物。`open_more_sheet` 加可选第 7 参（页面自己的行为检查），其余 12 张页零改动。
2. **iOS UI 测试**：`testThePlayPagesGamesAnswer`——滚动到回声块点 `echo-start`，`echo-status` 在 20 s 内变 `Round 1`；再滚到 `rps-rock` 点击，`rps-duck` 出现且以「The duck threw」开头，`rps-status` 不再停在 `Start`，比分行匹配 `You N · Duck M`。平局不改比分是合法结果，故只断言「可读」不断言「非零」。

**验证**：十门绿；冒烟语法 `bash -n` 过、新断言用合成 dump 单测过；Android 仅脚本/工作流改动，iOS 编译由 CI 判。

**首跑翻车与修复（run 45502701329）**：iOS 绿（新测试通过）；Android 冒烟红在「回声游戏没到第 1 关」。产物 `games-dump.xml` 说明真相：页面上有**三个「开始」**——回声状态文字、回声开始按钮、猜拳状态文字；`tap_text` 取第一个匹配，八轮全点在不可点的状态文字上（dump 里比分仍是 `你 0 · 鸭 0`）。修复：新增 `tap_text_in_control`，点「包住该文字的 clickable 容器」中心，用这次真实失败 dump 回测：开始→(55,452)、石头→(55,584)，都是芯片本体。`games-dump.xml`/`games-screen.png` 同时进 ci-logs 与产物，下次失败不用再下载 artifact。

**修复后全绿（run 45507605941，`836932a`）**：Android + Contracts 绿，iOS 在 `b0d4c44` 已绿且本批未动 iOS 代码。冒烟日志实证：`echo game: Start began round 1 (the duck sang its sequence)`、`rock paper scissors: the duck answered the throw and the tally row is readable`；截图 `dist/ui-b4-android-games.png` 同屏可见「回声游戏 第 1 关」与「猜拳 你赢了！你 1 · 鸭 0」。掉帧 `jank 58.53% / p99 150 ms`，门内。

### 四之四十六、舞台道具与鸭子自娱（2026-11-25）

老板指令：加更多道具和鸭子互动，鸭子也能自己和道具互动。红线照旧——道具是舞台地面上的场景元素，不碰 61 维观测与安全层，七控件预算一分未动。

**三个道具**（球/积木/坐垫）画在竖屏舞台地面（两端 `StageProps.swift`/`StageProps.kt`，场景层、无 clickable、不占触摸预算）。孩子点道具，鸭子用**真实动词**回应：点球→`kick_left`（官方 ball_kick 策略）/点积木→`ground_pick`/点坐垫→`sit`；道具只是目标，不自带假动画。

**鸭子自己玩**，信号全来自已发布的真实字段，不造假：
- `/state.idle.head`（核心自己的闲逛头部命令，takeover>0.1 时有效）→ 鸭子看向哪边，那边的道具亮起；
- 真实计数 `life.skills.kick_left|kick_right`、`life.ground_picks`、`life.sit_downs` 的增量 → 对应道具动一下（环、规则、邻居或 AI 触发的动作和手指触发的同一路计数；被拒的动作不计）；
- `care.asleep` → 道具静止，和鸭子一起安静。

**无障碍**：道具是场景，屏幕阅读器通过舞台元素新增的三个命名动作（踢球/捡积木/坐垫）触达；a11y 自定义动作棘轮（min 7）升到 10。

**验收**：Android `StagePropsTest`（5 例：身体区永不误中道具、地面分区命中、真实字段解析、无 takeover 的头部命令不算凝视、缺字段读 0）；Android 冒烟点球后断言 `life.skills.kick_left` 真增长（失败时回落到顶栏技能标签）；iOS `testTheBallTapStartsARealKick` 从舞台元素自己的坐标系点球、断言 `duck-last-skill` 出现踢。契约先改：features.yaml 增 `stage-props`（LEDGER_TOTAL 46→47）、ui-entries 增行、三个棘轮带注记重定（shell_kt_lines 3844、content_view_lines 1844、controller_published 109）。

**首跑翻车与修复（run 45528619458）**：iOS/Contracts/Core 绿，Android 冒烟红在「点球没起踢」。CI 截图说明真相：道具确实画在舞台上了（左橙球、右木块），但冒烟按**屏幕**百分比点击（16%,72%）落在了舞台下方的状态胶囊/栏上——舞台只占屏幕中间约 85..445px。修复：冒烟先从 dump 里读「鸭子舞台」节点的真实 bounds，再按舞台比例算球的位置（实测 (51,329)，与截图一致）；读不到节点时用截图实测的 52% 兜底；iOS 测试同样改成从舞台元素自己的坐标系点击。

**修复后全绿（run 45537353792，`69eb18f`）**：Android + iOS + Contracts 绿（Core 在前一提交 `d99e799` 已绿）。冒烟实证：`props: a ball tap really started a kick (life.skills.kick_left 0 -> 1)`；掉帧 `jank 57.02% / p99 250 ms`，门内。文档随后补进 COMPANION 第 18 条（`a22d9b9`，仅文档不触发 CI）。

**追加（2026-11-25 第二道具批）**：脑暴池与审计见 `docs/PROPS_IDEAS_2026-11-25.md`（13 个候选；明确不做会自己滚的球、没有真实动词的纸飞机、需要双实例的跷跷板、以及会重置 idle 破坏打盹的"鸭子定时自己发起技能"）。本批新增两个道具与一层自主互动：

- **呼啦圈**：点→官方 `roulade`；鸭子真实的翻滚计数（`life.skills.roulade`，环/规则/邻居/AI/手指同路）增量让圈转，它自己 `excited` 时圈也转；
- **镜子**：点→`curious` 情绪；鸭子自己的情绪时间线（`/state.emotion`）是唯一来源——好奇时镜面反光；
- **情绪到道具**（自主）：`laugh`/`sad` 让坐垫发暖，`excited` 转圈、球发亮，`curious` 照镜子；全部来自 `/state.emotion.name`，睡着时全静。

布局改为每侧两行（上：球/积木，下：呼啦圈/镜子，中间坐垫），`PropCues` 增 `rolls/emotion`（不加 @Published，控制器仍 109）。验收：Android 单测覆盖五个分区与 `emotion`/`rolls` 解析；冒烟点球断言踢计数、点呼啦圈断言 `life.skills.roulade` 增长（舞台坐标仍从 dump 的「鸭子舞台」bounds 算）；iOS 测试先点球验 `kick left`、隔 1.5 秒点呼啦圈验 `roulade`。a11y 命名动作从 3 个增到 5 个。

**验证（`3ffc15e`）**：Android + Contracts 绿；iOS 首跑红在已知负载敏感用例「大声惊吓」（`last_sound=coo`——pet 的 coo 与 bang 的 alarm 共享 100 tick 冷却，见四之四十四同族），同代码 dispatch 复跑（run 45548539490，`7e73f43`=代码 + 机器人截图提交）**205/205 全过**、该用例 `last_sound=alarm`，按负载敏感归档。

### 四之四十七、「自己玩」开关（2026-11-25）

老板指令：让它闲着时自己走去道具边玩。做成一枚舞台左上的开关（场景元素 + 舞台自己的命中区；iOS 上是真按钮，VoiceOver/XCUITest 直接可达），**默认关**、选择按端持久化。

打开后鸭子自己做主：先按真实走路策略朝道具那一侧走 1.2 秒（`input.vx/wz`，会打滑会摔都是真物理），再对选中的道具做真实动词（踢/滚/捡/坐/好奇）。触发与守卫全部是真实 `/state` 字段：醒着、未冻结、未被拎起、不在画路、没有情绪时间线、`idle.takeover > 0.1`（核心自己的闲逛环顾在管头）、`care.idle_s ≥ 10 s`、25 秒冷却、抽屉/技能环打开时不动。选哪个道具由鸭子自己的指纹 + 模拟 tick 决定：同一只鸭同一拍稳定、两只鸭不同。被核心拒绝的动作不计冷却，下一拍再试；成功才记冷却。代价写在明处：开着时它在动，打盹会更晚——所以默认关、必须亲手开。

**验收**：Android 单测 8 例（五个道具区、开关独立命中区、全部守卫逐项、选择的确定性、缺字段读 0 不冒充）；冒烟打开开关后在**没有任何道具点击**的情况下等 vetbook 计数真增长（最多 45 秒），随后关掉恢复静止；iOS `testTheDuckPlaysOnItsOwnWhenAsked` 打开开关、等顶栏出现真实技能判词、再关掉（选择持久化，测试必须给后面的用例留一只静止的鸭子）。契约与棘轮同步（shell 3897 / ContentView 1867 / published 109 / a11y 动作 12）。

### 四之四十八、镜子做鬼脸与道具的声音（2026-11-25）

- **镜子做不同情绪**：每次被接受的镜子点击让鸭子做一个不同的官方情绪，顺序固定（curious → laugh → excited → sad → yes），一轮五个。核心一次只演一个情绪、坐着也演不了，所以被拒的点击**不推进顺序**，冒烟按 `/state.emotion` 等演完再点。这就是"对着镜子做鬼脸"——情绪时间线自带官方嗓音。
- **道具的声音**：每个被接受的道具动作同时给鸭子自己的合成嗓音——踢球 `peck`、钻圈 `chirp`、捡积木 `greet`、坐垫 `coo`（核心官方 tag；`/state.last_sound` 可查）。镜子不加额外音效，情绪自己发声。安卓走 `DuckCore.queueSound`，iOS 走同一个 Voice 引擎（回声游戏用的那条）。

**验收**：Android 单测 9 例（新增：一轮五张脸互不重复、全是官方情绪）；冒烟先断言点球后 `last_sound=peck`，再站起鸭子、点镜子两次，断言 `life.emotions.curious`、`life.emotions.laugh` 各真增长（拒绝重试、演完再点）；iOS 测试在点球、点圈之后点镜子两次，断言顶栏判词先 `curious` 后 `laugh`。

### 四之五十、十二张游戏全部落地（2026-11-26）

全维度脑暴（`docs/PLAY_IDEAS_FULL_2026-11-25.md`）保留 12 条，先交付五张（四之四十九），本批补齐后七张，全部内联在「玩一玩」页：

| 游戏 | 真实地基 | 验收证据（Android 冒烟原文） |
|---|---|---|
| 左右开弓 | 官方 `kick_left/right` + `life.skills` | `kick rally landed 2 left and 2 right real kicks` |
| 情绪哑剧 | 13 官方情绪 + `life.emotions` | `mood charade performed mock and the right answer earned a real yes` |
| 表情木头人 | 情绪时间线时长当回合钟 | `mood freeze called the round (动起来/定住 shown while real moods play)` |
| 它今天有没有 | `life` 与 `life.day_started` 差值 | `did-it-happen answered the real pets delta with a real no` |
| 声音指挥官 | 核心房间声分类器 + 默认反应 | `sound Simon heard the synthetic clap and said so` |
| 反向指令 | 反向官方情绪（笑↔难过、兴奋↔好奇） | `reverse command made the duck sad (against laugh) and laugh (against sad)` |
| 照我做 | sit/stand/ground_pick/roulade 真实转变 | `posture mirror performed sit, stand, ground_pick and roulade` |
| 红灯停绿灯行 | 真实速度通道 + `life.metres`/`pose` | `red light stopped the duck; green walked 0.81 m` |
| 快进一天 | 核心 `care_time_scale` 60×/1× | `fast-forward set the real care clock to 60x and back to 1x` |
| 找一找 | 头=好奇、肚子=真摸、脚=真踢 | `find-it delivered curious, a real pet and a real kick` |
| 小账本 | 今日真实增量（`life` vs `day_started`） | `little ledger showed today's real facts and the duck nodded` |
| 传悄悄话 | 核心合成音 tag + 真实判词 | `whisper game queued peck and the guess earned a real yes` |

**验证（`92d02b2` Android+Contracts、`bac3b76` iOS）**：Android 冒烟十二行全过，AX3 密集扫掠的回声/猜拳复验也过（`echo game: Start began round 1`、`rock paper scissors: the duck answered the throw`），横屏 `640x320`，掉帧 `61.42% / p99 150 ms`，`smoke test passed`；iOS 的 `testTheFiveSmallGamesAreWired` 与 `testTheSevenMoreGamesAreWired` 在同一应用代码上绿。

**本批 CI 真因修复**（全部按日志修，不归抖动）：① 游戏段没先打开 Play more（脚本拼接误删）；② Compose testTag 不进 uiautomator 的 resource-id；③ 三个「开始」串台 → 各游戏开始键独立文案；④ 情绪游戏靠 `/state` 轮询翻页会跳段/串台 → 按官方时长排程；⑤ 长面板滚动会跨过单行标题 → 200 px 小步 + clickable 容器锚定（AX3 下同一页有死状态文字与真按钮同文案）；⑥ 反向指令对已在的姿态按「已如此」不加计数 → 改为反向情绪；⑦ 情绪被拒时各段重开一局重试（哑剧读新回合、照我做四势重放、找一找重放、账本重读、悄悄话重听）；⑧ 横屏轮询旋转、退出先回 Live；⑨ shellcheck SC2046。

**已知取舍**：悄悄话的冒烟断言是「点击必须换来核心真实的点头或摇头」+ 目标选择纯函数单测（读 `last_sound` 可能与入队竞争，不能作为唯一正确性证明）；iOS 侧对应的目标用同一秒级规则，行为等价。

**验证（`e092b74`）**：Android + Contracts 绿（iOS 在 `e29bfbe` 已绿：镜子/嗓音 UI 测试与首屏七控件都过）。冒烟实证五行：踢计数 `0 -> 1`、`last_sound=peck`、翻滚 `2 -> 3`、`mirror: two taps made two different real emotions (curious, then laugh)`、自娱计数 `7 -> 8`。两处 CI 教训：① `last_sound` 是单值字段，peck 的断言必须紧跟踢球，否则被呼啦圈的 chirp 覆盖（首跑红）；② `accessibilityAddTraits(.isButton)` 会让开关成为首屏第 8 个按钮、踩七控件预算——开关是场景元素，不带 button trait，语音辅助走舞台的「自己玩开关」命名动作（与 Android 同构）。

### 四之四十九、五张小游戏一次落地（2026-11-25）

出处：全维度脑暴（`docs/PLAY_IDEAS_FULL_2026-11-25.md`，四维 56 条审计后保留 12 条）里五条小成本高分项，一次做完，全部内联在「玩一玩」页（与回声/猜拳同款，不新增路由）：

1. **左右开弓**：节拍上交替左/右踢，每一下都是官方 `kick_left/right`；接受/拒绝以 vetbook `life.skills` 为准，六步完成鸭子 excited。
2. **情绪哑剧**：鸭子演 13 个官方情绪之一（答案由指纹+回合决定），演完开猜；猜中鸭子点头（`yes`）、猜错摇头（`no`）。
3. **表情木头人**：用鸭子自己的情绪时间线当回合钟——excited 段「动起来」、angry/yes 段「定住」，状态跟随 `/state.emotion`。
4. **它今天有没有**：用 `life` 与 `life.day_started` 的真实差值回答「今天走过吗/被摸过吗/摔过吗/吃过吗」，并展示真实数字；答案就是鸭子的 `yes`/`no` 情绪。
5. **声音指挥官**：出题「拍手让它笑 / 吹口哨让它好奇」，核心自己的房间声分类器听见后用官方默认反应回答；回合按 `sound_event.tick` 判定。

两端：`ios/DuckApp/PlayMoreGames.swift`、`android/.../PlayMoreGames.kt`；28 个文案键两端同键同值；Play more 页追加五段。

**验收**：Android 冒烟在真实 UI 上按「标题锚定的文字点击」驱动五个游戏（Compose 的 testTag 不进 uiautomator 的 resource-id，实测 dump 只有 `android:id/content`，所以不能用 tag）：节拍四次交替踢断言左右各 +2；读 `/state.emotion.name` 后点对应中文情绪 chip，断言 `life.emotions.yes` +1；木头人断言先后看到 excited 与 angry；今天卡断言情绪与真实 pets 差值一致；声音指挥官注入合成拍手后断言界面出现「听到了」。iOS `testTheFiveSmallGamesAreWired` 自上而下滚过五段：踢出判词、哑剧到「它在演什么？」、木头人横幅、今天卡给出 `Today:` 事实、声音指挥官进入聆听。

**验证（`0c2e7d2` Android+Contracts / `f8e6a78` iOS，2026-11-26）**：冒烟五行实证——`kick rally landed 2 left and 2 right real kicks`、`mood charade performed mock and the right answer earned a real yes`、`mood freeze called the round (动起来/定住 shown while real moods play)`、`did-it-happen answered the real pets delta with a real no`、`sound Simon heard the synthetic clap and said so`；`smoke test passed`，横屏 `640x320`，掉帧 `60.39% / p99 200 ms`。iOS 的 `testTheFiveSmallGamesAreWired` 在同一代码上绿。

**这一批的六次 CI 教训**（全部按真因修，不归抖动）：① 游戏段根本没有先打开 Play more（早前脚本拼接把「更多→玩一玩」整段替换掉了）——补回开页并加开页 dump；② Compose testTag 不进 uiautomator 的 resource-id（实测只有 `android:id/content`）——改为「标题锚定的文字点击」；③ 三个游戏共用「开始」导致点串台（日志里哑剧在木头人回合里演了 impatient）——开始键改为各说各话（开弓/开始猜/演一段）；④ 情绪游戏靠轮询 `/state` 翻页会被慢请求跳段并被别家情绪污染——改为按情绪官方时长排程（生气 2.5s、好奇 2.9s、崩溃 8.5s…取自上游关键帧）；⑤ 长面板需要 ~60 次慢速中屏拖动 + 先删远端 dump + 停滞探测；⑥ 横屏截图要轮询旋转（最多 16 秒）且先证明回到 Live 屏，否则拍到的是残留面板。

---

## 八、仓库与复现

- 仓库：`github.com/monsm/0637`
- Android/iOS 构建全部由 GitHub Actions 完成，产物在对应 run 的 Artifacts
- 上游固定：`pollen-robotics/microduck@da34a22` + `patches/duck-control-mobile.patch`
- CI 迭代记录见 [M2_PROGRESS.md](M2_PROGRESS.md)
