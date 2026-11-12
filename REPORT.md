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
| **MCP** | `/mcp` JSON-RPC：initialize / tools/list / tools/call，12 个工具从 intent 表生成 | 单元测试 3 条（初始化、未知工具失败闭合、限幅）；CI 断言工具列表与 `duck_state` |
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
- 早安明信片：Director 照片配一句真实发生的事（互动/走米数），「收藏」进本地明信片架并逐张分享。
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
- **两端**：仪表表「晚安」→ 变成「已经安静 X 分钟」+「早安」；早晨弹「早安！昨晚安静了 X 分钟」。
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
- **两端**：仪表表入口 + 独立页面 + 系统分享；iOS（DiarySheet.swift）与 Android（DiarySheet.kt）各自复用已有 AI 传输层。
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

FIDELITY.md 的状态表停了三天，说相机与麦克风「未开始」——其实 2026-11-11 就做完了；意图 API 从 11 条路由长到 54 条；LiDAR 的 8×8 ToF 降采样**只有单元测试在调**，两端都没有深度数据源。表里现在写的是真的，包括仍然没做的两件（Android 没接手机 IMU；没有 ARKit/ARCore 跟踪）。

### 九、块程序：核心拒绝的导入不算导入

两端都有同一种撒谎：核心用 `{"ok":false}` 拒绝一份积木程序，两端却已经把 payload 存进本地、还报「导入了 N 条」——孩子看到一堆不生效的积木，下次启动还把这份程序原样交回去。Android 更狠：核心的程序还没读到时（刚启动、或 1.5 s 读超时），它拿**空表**去合并导入物，孩子原有的规则被整个替换。

现在：两端先显示新程序，只有核心真的收下才落盘；被拒绝就把旧程序放回去并说一句原因（iOS 的旧程序回滚还比对了「这仍是你看到的程序」，慢到的回答不会覆盖更新的编辑）。Android 在读到核心程序之前拒绝粘贴，并说明原因。

### 验证

- **Core 工作流**：143 条核心单测 + 8 条 HTTP 边界探针，全绿（本批之前是 136 条全绿，其中 8 条上游测试还被跳过）。
- **双端**：iOS 在 `728409f` 上绿（首跑红在邻居 greet 检查，同一 commit 重跑即绿——这是脚本历史上修过一轮的偶发项，不是回归）；Android 在 `728409f` 上因 Kotlin 解析问题红，修好后随下一轮验证。
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
| 性能指标（fps / 控制 Hz） | ✅ 两端实测并发布在 `/state.performance`，仪表表显示；CI 两端断言 `ticks_per_second > 0`，绿跑实测 iOS 55.2 fps / 50.0 Hz、Android 模拟器 14.8 fps / 50.5 Hz。功耗未采集（需真机电量计） |
| 物理版情绪 / 人格 | ✅ 官方 `robotd/emotions/` 13 个关键帧时间线 + 48 kHz 声音原样接入，`/skills`、`/skill`、MCP、技能环可发现；50 Hz 循环按 `Frame::apply` 语义驱动头/身体/喙，声音走现有扬声器通道，checkpoint 可恢复播放。IdleHead 张望/呼吸/瞌睡随 checkpoint 精确重放 |
| 声音反应 | ✅ 7 个官方合成音全部可用；被抱起 `coo`、持续摔倒 `alarm`、久静后重新驱动 `inquire`（上升沿 + 冷却）；拍手/口哨分类为官方 `laugh`/`curious` 并发布事件；情绪音为官方 wav |
| 虚拟脸灯 | ✅ `robotd/src/leds.rs` 的纯语义（状态 4 档、电量表 5 档 + 3% 滞回、7 色手电筒）发布在 `/state.leds`，`/flashlight` 与 MCP `duck_flashlight` 可控制；无 I²C 硬件，故为状态等价 |
| 中文语音 | ✅ 系统级离线识别（iOS `SFSpeechRecognizer` zh-CN / Android `SpeechRecognizer` zh-CN），用时才申请权限；文字走 `POST /heard` 映射到技能/情绪/问候，CI 直接测文字链路（模拟器无麦克风，识别本身须真机验证） |
| 凝视野 IK | ✅ `POST /look` + MCP `duck_look`：`HeadFk::look_at` 基于训练 MJCF 的正运动学求解，受舵机行程限制；`/state.pose` 供客户端换算躯干坐标 |
| wheee 长按 | ✅ `POST /wheee {"hold":true}`：官方 start/loop/end 三段经 `next_sound` 顺序流式播放，松开收尾；MCP `duck_wheee`、两端长按 quack 按钮 |
| 开机姿态判定 | ✅ 上游 `posture.rs` 原样编译，其单元测试在核心里跑通；数字版 MuJoCo 永远从 HOME 启动，故没有「折叠开机」输入场景，判定作为 API 保留而不造假路径 |
| 仍未移植 | 多鸭合唱 chorale（需多机无线电）、ToF 特雷门琴（需头部深度传感器）、pad 组合摇杆模式、`robot.rest` 的完整告别分支、渲染多光源/阴影贴图 —— 需硬件或超出手机范围 |
| 第三轮 QA 的未关项 | 15 条里关了 5 条（核心门禁、礼物与夜、摔倒数、合唱形状、API 鉴权）。**仍排着**：规则导入的失败处理（两端都把核心的拒绝当成功，Android 还会拿空表去合并）、peer id 只有 32 位且 pid 被掩掉、合唱 id 只含 tick、日记没有「今天」（核心掉线会说「今天什么都没发生」）、模型输出零校验、Android 没接手机 IMU。逐条见 [docs/QA_ROUND3.md](docs/QA_ROUND3.md) |
| 界面的账 | 设计审计（[docs/UI_AUDIT_2026-11-12.md](docs/UI_AUDIT_2026-11-12.md)）：Live 页面还露着引擎（状态串、原始技能名、tick 计数）、没有令牌层（9 个强调色/12 种字号/8 种圆角散在各处）、iOS 全部固定字号（171 处，Dynamic Type 完全无效）、手势没有无障碍等价物。方案在 [docs/UI_TOKENS.md](docs/UI_TOKENS.md) 与 [docs/STAGE_LOOK.md](docs/STAGE_LOOK.md) |

---

## 八、仓库与复现

- 仓库：`github.com/monsm/0637`
- Android/iOS 构建全部由 GitHub Actions 完成，产物在对应 run 的 Artifacts
- 上游固定：`pollen-robotics/microduck@da34a22` + `patches/duck-control-mobile.patch`
- CI 迭代记录见 [M2_PROGRESS.md](M2_PROGRESS.md)
