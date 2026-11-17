# 官方面功能覆盖 · 地毯式对抗审核（2026-11-16）

> 问题：「现在是官方支持的全部功能都支持了吗？」
>
> 方法：官方基准 = `upstream/microduck@da34a22` + `patches/duck-control-mobile.patch`（本地脏树 `git diff da34a22` 与补丁逐 hunk 对照一致）。82 个 agent：基线 2（补丁逐 hunk 分类 + 官方 108 条功能清单）、6 切片映射（policy / robotd / protocol / peripheral / apps / docs）、18 条候选 × 3 镜头对抗（官方侧怀疑者 / 移动侧怀疑者 / 范围裁决者，≥2 确认才存活）、18 条「已支持」声明反向抽查、完整性批评家补 4 条漏项 + 7 类盲区。只读取证：不 build、不跑测试、不改 `upstream/`。

## 结论直答

**没有全部支持。** 官方 108 条清单里，映射到移动产品且本次能保守确认支持的约 40 条（抽查 18 条「已支持」声明，16 真、2 假）。**13 个真 gap + 10 处文档漂移**（其中 2 处是「已支持」声明抽查不实）；约 25 条是已裁决不做 / 真机硬件边界；20 条候选因验证上限未裁决（见 §四）。

核心步态与安全「执行」层大体到位，但**策略管理、接管语义、协议可观测性、连接/媒体/感知长尾**尚未达到官方全功能等价。FIDELITY.md 的「行为 / 状态机 / 安全 100%」按字面不成立（见 §二 #9、#10）。

---

## 一、真缺口（13 条，3 镜头 ≥2 确认）

### 策略管理簇（直接影响用户导入自定义策略）

> **2026-11-17 修复状态**：本节 #1-#13 已随「官方语义补齐」批次全部接线落地（walk 槽覆盖+声明校验、扰动门控、电压缩放、姿态分类、拾取门控、limited_by、limp-fall 复位、robot.* NDJSON 服务含 50Hz 订阅推送、DKST/DKRB 回包、fingerprint 双端、嗓音种子、health 分档、vetbook 本地卡片——share 发布按拍板只做本地卡片）。推送前 QA 审查另发现并修复 5 项移植引入的新缺陷（API 租约不刷新 deadman 时钟、robot.state 早退冻结帧、45Hz 地板与慢放/暂停错配、Checkpoint 漏收五个控制态、分支会话 health 来源）。

1. **自定义策略的 walk 槽位不可由 manifest / 配置指定**
   - 官方：`robotctl policy load walk <repo>` / `robot.loadPolicy` 可把 walk 槽指向任意文件（`robotd-params/src/lib.rs:2330-2340,2384-2385`；`docs/policy-manifest.md:11,59`——注意官方是 **config 覆盖**，manifest 的 `slot` 字段只用于展示）。
   - 移动：`core/duck-mobile-core/src/manifest.rs:88-92` 的 `Mode::Walk` 分支不读 manifest，只按文件名找 `velstand.onnx` / `alpha_walking.onnx`，找不到即整次加载失败（`:109`）；Roller 分支反而读 manifest。
   - 后果：任何不含恰好名为 `velstand.onnx` 的自定义步态包无法导入，只能改名绕过；`ios/README.md:52-58` 的 Reload AI 承诺对此类集合不成立。

2. **策略热切换缺官方 disturb 门控（行走中换驱动网络不先回家）**
   - 官方：后台线程加载 → 关机中拒绝 → 若替换的是正在驱动的网络（`change_disturbs`），先回 home pose 再换，否则原地换并 carry_over（`robotd/src/main.rs:3015-3106`、`:4861-4870`）。
   - 移动：`runtime.rs:2059-2068` 立即原地换，仅 limp-fall 特判；`ffi.rs:338-373` 的 `duck_core_reload_policies` 无任何状态检查（HTTP `/reload` 还拒绝关机中，`ffi.rs:1337-1341`，但壳层不走它）。
   - 后果：行走中换网可能摔；FIDELITY.md:146 的「失败回退官方策略」只以「保留当前集合」弱成立，没有回官方集的回退。

### 运行时语义簇（静默差异，影响步态保真与接管行为）

3. **电压自适应缩放缺失（voltage_adapt / scale_mult 调用点写死 1.0）**
   - 官方默认开启：`scale_mult = nominal(7.4) / battery_v.clamp(6.0,9.5)` 每 tick 缩放动作（`main.rs:3800-3803,3863`；`robotd-params/src/lib.rs:2524-2525`）。
   - 移动：`runtime.rs:2561-2567` 与 `:3073-3081` 两处 `controller.step(..., 1.0)`；全核心无 `voltage_adapt/nominal_voltage/scale_mult` 引用。乘法机制本身在（`control.rs:7` 直接编译官方 `robotd/src/control.rs`）。
   - 后果：模拟电池随负载压降时（`mujoco.rs:984-1001` 的 `vin_effective` 确实喂进扭矩），动作幅度相对官方偏软，步态保真随电量漂移。

4. **策略接管前的姿态分类（站/坐/倒）与坐姿起立引导未接线**
   - 官方：接管前 `posture::classify` → `settle_then_rise()` / `leave_seat()` + `seed_from_pose(&hold)`；上电 ramp 时 `classify→enter_seat→SEAT_POSITION`（`main.rs:3690-3733,3751-3760,5090-5118`；`robotd/src/posture.rs:10-40`）。
   - 移动：`control.rs:23-24` 只把 posture 模块再导出；`runtime.rs` 无任何 `classify/settle_then_rise/seed_from_pose/enter_seat/leave_seat` 调用；`enable()` 恒用 `DEFAULT_POSITION`（`:1130-1168`），at_home 捷径直接 Ready（`:1145-1149`）；`SEAT_POSITION` 仅定义无人用（`lib.rs:63`）。
   - 后果：从坐姿/非 home 姿态上电或恢复存档时，用步态网络硬拉回家——正是官方 posture.rs 警告的「把手伸进椅子里把它向后拽翻」。

5. **拾取分类器缺 `mode == Walk` 门控，轮式模式仍跑拾取**
   - 官方 `watching` 首项即 `policy_cfg.mode == Mode::Walk`，注释明写「Roller mode is not watched: the model has never seen wheels」（`main.rs:3340-3355`）。
   - 移动 `runtime.rs:2705-2712` 的 `watching = ready && settled && warm && free`，无 mode 判定；`ffi.rs:626-640` 切 roller 时把同一个 pickup 模型 `load_pickup` 进新 runtime。
   - 后果：轮式模式对带轮关节跑拾取分类器，可能误暂停轮式步态。

6. **安全层裁决原因 `limited_by` 不对外，deadman 自写且不产生 Limit**
   - 官方 `Safety::gate`/`apply` 返回 `Limit{Deadman,Range,NotFinite}`，robotd 汇总进 `robot.state.movement.limited_by`（`duck-control/src/safety.rs:85-100,366-477`；`main.rs:2643,4081,4115`；`duck-ipc-proto/src/lib.rs:4480`）。
   - 移动：自写 500ms deadman（`runtime.rs:2623-2630`，仅归零 twist + 一行日志）；所有 `safety.apply(...)` 的 `Ok(Applied)` 被 `?` 吞掉（`:2494,2519,2536,2570,2757,3106` 等）；全核心无 `.gate(/limited_by/Limit::`；`/state` 无此字段。
   - 后果：执行层钳位仍在，但外部 agent 看不到 deadman/范围/NaN 是否触发。

7. **limp-fall 交接不归零 twist，且缺 driving 沿的 reset_on_resume**
   - 官方：limp-fall 期间硬性 `twist_ema=[0;3]`（`main.rs:3385-3390`）；`driving && !was_driving` 且停顿 ≥ `RESET_AFTER_PAUSE(200ms)` 才 `controller.reset()`（`:3746-3761,4308-4312,168`）。
   - 移动：`runtime.rs:2729-2751` 的 Limp/Posing 分支在平滑之前 return，`twist_ema` 保留跌倒前旧值；无 `reset_on_resume/RESET_AFTER_PAUSE`；pickup Resume（`:2716`）无复位。
   - 后果：跌倒/被拿起放下后从旧行走指令与旧动作历史起振。

8. **`robot.health` 缺 degraded-vs-unhealthy 分档与 45Hz 速率地板**
   - 官方 `health()` 严格分开 degraded（不是发布的错）与 unhealthy（要回滚发布），含 `min_achieved_hz`（默认 45）与 stall 检测（`main.rs:876-1010`，尤其 `:950,:988,:993,:1002`；`deploy/robotd.toml:98,108`）。
   - 移动 `runtime.rs:2450-2454`：`healthy = imu_ready && !fallen && enabled`，`degraded` 恒等于 `!imu_ready`；实测 fps/ticks 只经 `duck_core_set_performance`（`ffi.rs:378`）作展示数字（`ffi.rs:1734`），从不回流健康判定；`leds.rs:71` 的 `Status::Degraded` 基本走不到。
   - 后果：控制环掉到 30Hz 或 override 加载失败仍显示「健康」。

### 协议 / 可观测性簇

9. **官方 JSON-RPC/NDJSON `robot.*` 协议面缺失（robot.move/subscribe/health 与 50Hz 推送流）**
   - 官方：`duck-ipc-proto` 的 `robot.health/move/head/do/subscribe`（API_VERSION=51，NDJSON over socket，订阅按订阅者降频 50Hz 推送；`lib.rs:1-52,706-783,972,3243-3253`）。
   - 移动：`intent.rs:905-1330` 全为本地 HTTP GET/POST，零 `robot.*` 方法名、零 NDJSON；iOS 1Hz 轮询 `GET /state`（`DuckController.swift:319,1270`）；唯一 JSON-RPC 是 MCP 三面（`mcp.rs:249-276`）。
   - 后果：按官方协议写的客户端不能原样驱动数字鸭，也没有服务器推送状态流。FIDELITY.md:70,147 的「软件协议 100% / 暴露 robot.move/subscribe/health」不成立。

10. **UDP pad 的 DKST 状态回包 / DKRB 报告包缺失，按钮位与长按语义错位**
    - 官方三包：DKPD 输入 + DKST 状态回执（verdict/counts）+ DKRB 报告包（health/posture/refused，拒绝保留 30s；`pad-map/src/wire.rs:162-344`、`buttons.rs:11-35`、`hold.rs:7-19`、`netpadd/src/main.rs:834-920`）。
    - 移动：`pad.rs:47-69,127-147` 只解析 DKPD、无 `send_to`；`ffi.rs:1889-1904` 只映射 3 个硬编码位（0x0040→sit、0x0001→roulade、0x0002→kick_left），缺 D-pad 四模式 / Home / RT / LT / 可绑定六键 / 1.5s·2s·4s 长按阈值。按官方位表 START=bit6、A=bit0、B=bit1，与官方默认（A=sit_toggle、B=ground_pick）整体错位。
    - 后果：官方 `netpadd`/`duckctl udp-pad` 发送端连到数字鸭收不到任何回执与健康报告；按钮语义错位。

11. **Android 双端接口缺 fingerprint（附 enable/disable 双端未使用）**
    - `ffi.rs` 导出 49 个 C 函数、`duck_core.h` 一一对应（脚本双向 diff=0）；`jni.rs` 只包 46 个入口，缺 `duck_core_fingerprint/enable/disable`；`DuckCore.kt:250-295` 无对应 external fun。
    - 后果：Android 仪器页拿不到 iOS 已展示的出处指纹（Provenance SHA-256；iOS `InstrumentSheet.swift:92,714`）；`Copy.kt:202` 有 provenance 文案无数据源。`enable/disable` 实际双端都未调用，属公共库存，不是不对称。

### 承诺与实现直接矛盾 / 批评家补项

12. **每只鸭唯一嗓音写死 `0xD0C`**
    - 官方：`robotd/src/sound.rs:699` 回落 `sounds::hardware_seed()`（读 SoC efuse 派生，「two seeds sound like two different creatures」；`sounds/src/lib.rs:126-167`）。
    - 移动：`runtime.rs:875` `Voice::new(0xD0C)` 是全仓唯一生产构造点；`voice.rs:30-36` 注释自称「seed 是构建的 provenance fingerprint，不同构建不同嗓子」，但 provenance 在 `ffi.rs:253` 才在 runtime 构造之后算出，`voice_seed()` getter（`runtime.rs:1009`）从不被喂指纹；FIDELITY.md:133 承诺「按种子确定音色」。
    - 后果：所有构建、所有手机同一把嗓子——承诺、注释、实现三者互相矛盾。改动小，应尽快修。

13. **`vetbook.share` 发布链缺失（只有本地导出）**
    - 官方：`duck-ipc-proto/src/lib.rs:931`（发布到 HF 公开数据集、返回可分享卡片 URL、可选进 Space 画廊；`:463-478,3043-3065`）。
    - 移动：仅 `intent.rs:918-919` 的 `/export/vetbook.{json,md}` 本地导出（`export.rs:13`），无发布/分享卡片/Space 登记，也无承载它的 account/OAuth 层。
    - 范围镜头投 out_of_scope（依赖官方账号与 updaterd），官方+移动两票判 gap，按 2:1 收 gap；上报时如实标注依赖账号层。

> 注：批评家补的第 4 条 `telemetry.*` 经范围镜头复核为 **out_of_scope**（updaterd 板级服务，手机无 updaterd/OTA；官方+范围两票），与工作流聚合时按首票误记为 gap 的口径不同，本报告已按多数票归入 §三。

---

## 二、文档漂移（10 条）

1. **manifest 声明校验未实现**：FIDELITY.md:146 称校验 `model_api / robot.model`；三棵树 grep 这两个字段零命中，只有官方张量形状检查兜底。官方拒绝发生在 updater 安装层（`updater/src/policy.rs:1226-1266`、`docs/policy-manifest.md:44-48`），移动端无对应物。
2. **`/skills` 无条件列举内建技能**：官方按「背后策略已加载且未被遮蔽」条件列举（`main.rs:4880-4916,5335-5346`）；移动 `ffi.rs:539-554` 无条件前置 sit/stand/ground_pick，reload 成自定义集后仍 advertised、调用时才拒。
3. **壳层丢弃 reload 失败原因**：C ABI `duck_core_reload_policies` 把带完整原因的 `PolicyError` 压成 `-1`（`ffi.rs:352-357`）；iOS `_ =` 忽略返回值（`DuckController.swift:2820-2832`），A/B 页 1.2s 后固定显示「已加载」（`ABSheet.swift:43-49`）；Android `MorePages.kt:154` 也不检查。失败时用户看到成功。
4. **电量百分比两个口径**：官方单一映射 6.6–8.2V（`model.rs:171-190`），`robot.health` 与 LED 同源；移动 `leds.rs:5-7` 自称同阈值，但灯档吃 `runtime.rs:2447-2449` 的 6.0–8.4V 窗口、`/state` 又用官方 `battery_percent`。6.6V 时灯面读 25%（Gauge::One）而官方 0%（Critical），差两档。
5. **「54 条路由」过时**：FIDELITY.md:25 / REPORT.md:509 称 54 条；`intent.rs:905-1330` 实测 69 个 (method,path) 匹配臂、62 条唯一路径（两路独立复核一致），名单缺 9 条已登记路径（`/backup /backup/restore /export/trajectory /floor /parent /pad /ensemble /peers/pair /quests/day`）。报告里引用路由数时应写死口径。
6. **「MCP 12 个工具、从 intent 表生成」失实**：`mcp.rs:21-218` 手写 25 个 `duck_*` 工具，`:283-407` 手写逐工具映射，并非程序化同源；`mcp.rs:4-6` 的「cannot drift」注释无机制保证。
7. **Origin 拒绝是前缀匹配**：COMPANION.md:82 / REPORT.md:504-505 称「网页一律被拒」；`intent.rs:791-798` 对四个 localhost 前缀做 `starts_with`，`http://localhost.attacker.tld` 这类 origin 会过门，回环分支无条件放行（`:799-805`）。影响面限于同机 webview，不是可远程利用漏洞，但文档绝对化描述不成立。
8. **iOS DKPD 发送端摇杆轴序对调**：官方线序 `[lx, ly, rx, ry]`（`pad-map/src/wire.rs:26-33`）；iOS `PadSender.swift:49-52` 注释写对、offset 14/16 写反（-stickY / stickX），接收端按 lx→-wz、ly→+vx 解后，推前变转向、侧推变前进；Android `PadSender.kt:53-54` 正确。iOS 侧缺陷，非设计选择。
9. **「50Hz tick 顺序与官方一致」抽查不实**：移动顺序 bring-up/shutdown → deadman，官方是 `safety.gate(deadman)` → bring-up → shutdown；移动无 `Safety::gate` 调用，deadman 为自建 500ms、不产生 Limit，且在 Limp/Homing/shutdown 提前 return 的 tick 上不执行；limp-fall 出 target 也在平滑之前（官方在 pickup/平滑之后统一出 target）。详见 `runtime.rs:2418-3106` vs `main.rs:2562-4080`（官方注释亦见 `main.rs:1957`）。
10. **「Limp→Homing→Ready 与 2s 力矩爬升一致」抽查不实**：常量与线性插值属实，但 `runtime.rs:1145-1149` 的 at_home 捷径直接 Ready、`:2533-2535` 的 `homing_is_static()` 让无行程 Homing 立即完成；官方 enable 一律进 Homing、满 2s 才转 Ready（`main.rs:3478-3544`）。MuJoCo 加载即 home 位（`mujoco.rs:362,400-408`），正常启动约 0.5s 就 Ready，2s 爬升实际不发生——直接击中 FIDELITY.md:68 的「行为/状态机 100%」。

---

## 三、裁决不做 / 已知边界（不计缺陷，但须如实上报）

- 老板拍板不做清单（REPORT.md §四之二十五）：Android 正式签名、AI 内置模型服务、中文圆体。
- `telemetry.*`：updaterd 板级服务（`duck-ipc-proto/src/lib.rs:967-969`、`updater/src/telemetry.rs`、`docs/robot/telemetry.md`），手机无 updaterd/OTA——out_of_scope。
- LiDAR/ARKit/ARCore 跟踪；端内 8×8 ToF 无深度源（`/sensors` 深度行恒空）、ToF theremin 用手机传感器顶替。
- 真机验证未做（iOS 签名凭据、Android arm64、CI 无物理设备）。
- Android 手机 IMU 扰动未接（只有 ROTATION_VECTOR/LIGHT；`DuckCore.push` 零调用），iOS 倾斜看独有——已知边界。
- 桌面 sim harness 已移除；DKPD 发送端是外部工具；手机只是外设，61 维观测与安全层不许改。
- 连接/配对供给链（btd/duck-ble、nfcd、configd、padd 实体手柄、pad-imu）、updater/OTA、account/OAuth、WebRTC/mediad 媒体管线、duck-detect/pet-detect/pad-imu/uyvy/duck-ether、多鸭 chorale 无线电合唱、RL StandUp、roller swizzle 变体、Dynamixel 总线/板级热/LED 硬件——`docs/BRAINSTORM_2026-11-14.md` §A/§C 已登记的刻意缺失或手机硬件边界。

## 四、未裁决候选（20 条，超验证上限，未对抗验证）

`duck-ble/btd` BLE GATT 通道、sounds 变体表（官方 7 音色 12/12/6/10/10/10/10 变体）、ToF 8×8 降采样、`robot.theremin`、chorale 音乐半（声部/音高/节拍时间基）、netpadd/pad-map 接收端语义（与 gap #10 同主题）、intent 路由清单逐项（与 drift #5 同主题）、触屏施力/抱起（Android 摇杆模式缺 carry）、Director/策略 A/B 试穿、积木规则 timer 触发（iOS 提供、核心永远拒绝；Android 读回丢失）、多机邻居 LAN 动词面、明信片文案、Android 麦克风分析窗长、FIDELITY 路由枚举、fixture 门禁覆盖面、BAM 锁定与三工作流自拉上游声明、REPORT §七时效性、若干量化描述（里程计路由名、care 节拍）。

**这批不是被推翻，是没来得及验**——报告不得把「未验证」读成「支持」。

## 五、审计盲区（本轮够不到，需运行期验收补）

1. 真机实测（传感器噪声、IMU 抖动、发热、功耗、真实接触）。
2. 50Hz / <2ms per tick 的真机性能预算与降频行为。
3. 与官方 duck-sim 的轨迹逐步对齐（FIDELITY 的唯一定义 100%，CI 无对拍步骤，本次也没重跑）。
4. 61 维观测逐字节对拍（布局同源风险低；真正未对拍的是 MuJoCo 采样+齿隙+电压 → obs 的输入链）。
5. AI 伙伴真实 LLM / 真实双手机邻居 / 真实相机。
6. iOS 横屏与 Android 完整交互面（未跑 XCUITest/Compose UI 层级）。
7. 上游调度器 8 条 `#[ignore]` 测试是否真在 CI 跑绿（审计禁跑 cargo）。

## 六、补丁审计

9 个被改文件全部判 `mobile_adaptation`，**无 dropped_official_behavior、无 silent_behavior_change**。两处需留意（不是缺陷，是风险面）：
- `safety.rs:357-359` 的 `io_mut()` 把 `RobotIo` 可变句柄暴露到安全层之外；
- `policy.rs` 的 `restore_state/set_state_bytes` 对长度不匹配快照做重置而非报错。

## 七、建议优先级

1. **策略管理**：walk 槽支持 manifest/配置指定 + 热切换 disturb 门控 + reload 失败原因回传壳层（`ios/README.md:52-58` 有明确承诺，用户可直接撞上）。
2. **运行时语义**：voltage_adapt、posture/limp-fall/pickup 三处语义（直接影响步态保真与摔倒/被拿起后的行为）。
3. **安全/健康可观测性**：`limited_by`、degraded/unhealthy、45Hz 地板 + JSON-RPC/NDJSON 兼容层（影响外部 agent 生态）。
4. **嗓音种子 `0xD0C`**：改动小、承诺与实现直接矛盾，尽快修。
5. **文档纠偏**：FIDELITY 的「100%」表述、54 路由数、MCP 12 工具、Origin 绝对化、电量口径——按代码事实改写。

## 八、元观察（诚实标注）

- 本轮 22 条存活发现 **0 条被对抗推翻**（killed=[]），抽查仅 2 条不实。一次地毯式对抗审核若几乎全存活，更可能的解释是反证阶段偏弱而非幸存者全高置信——下一轮应专门做 kill-pass：对每个 gap 强制问「官方侧这条是否真有对应承诺/代码，移动端是否只是换词实现」。
- 路由计数存在三种口径（54 / 62 / 64-69），本报告采用两路独立复核一致的 **69 臂 / 62 唯一路径**（同 GET/POST 记两臂、按路径去重）；引用时必须写死口径。
