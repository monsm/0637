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
| 相机 / LiDAR / AR 模式 | ✅ 系统相机预览层作背景（Director 模式）；LiDAR 深度读数与 8×8 ToF 降采样在 `/sensors`；未做 ARKit/ARCore 跟踪与平面锚定 |
| 扬声器 | ✅ 官方声码合成器编译进核心，两端已接扬声器 |
| 麦克风 | 读数通道已实现（`/sensors`，可作惊吓/咕咕的反应源）；真机常开监听未实现 |
| roller 模式 | ✅ 整套 bundle + roller 场景切换；`/mode`、MCP `duck_mode`、两端界面切换 |
| 电池关机 | ✅ 坐下→憩姿→断力矩；空电量（EMA ≤ 6.6 V）自动触发，`/power` 重新上电 |
| 里程计 `/pose` | ✅ 官方接触式里程计编进核心，随 checkpoint 倒带 |
| 手柄三种摇杆模式 | ✅ 两端顶栏：走/转、看、姿态（蹲+侧倾） |
| 渲染器 PBR / 阴影 / 帧间插值 | ✅ PBR + 接触阴影 + 两 tick 位姿插值；未做多光源/阴影贴图 |
| Android 真机实测 | CI 仅覆盖模拟器（API 30 x86_64）；arm64 真机未跑 |
| 性能指标（fps / 控制 Hz） | ✅ 两端实测并发布在 `/state.performance`，仪表表显示；CI 两端断言 `ticks_per_second > 0`，绿跑实测 iOS 55.2 fps / 50.0 Hz、Android 模拟器 14.8 fps / 50.5 Hz。功耗未采集（需真机电量计） |

---

## 八、仓库与复现

- 仓库：`github.com/monsm/0637`
- Android/iOS 构建全部由 GitHub Actions 完成，产物在对应 run 的 Artifacts
- 上游固定：`pollen-robotics/microduck@da34a22` + `patches/duck-control-mobile.patch`
- CI 迭代记录见 [M2_PROGRESS.md](M2_PROGRESS.md)
