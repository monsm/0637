# QA 推送前把关 · 官方语义补齐批次（2026-11-17）

- 审查方式：5 维对抗审查(官方对拍/ffi 接线/协议字节/双端壳/契约测试) → 每条发现双镜头验证 → QA 放行判定。34 个 agent。
- 判定：**block**（7 项 major 全部确认成立 + 1 项未跟踪文件致命项）→ 逐条修复 → 复跑全绿后重判。
- 审查对象：`docs/AUDIT_OFFICIAL_PARITY_2026-11-16.md` 批次的全部改动(24 文件 +4387 行)。

## 阻断项与处置

| # | 问题 | 处置 | 状态 |
|---|---|---|---|
| 0 | 新源文件全部未跟踪(ipc.rs/VetbookCard 双端/新测试),lib.rs 无条件 `pub mod ipc;`——干净检出三条 CI 全挂 | .gitignore 补齐 + 提交时 `git add` 全部批次文件、排除根目录会话草稿 | ✅ 提交时落实 |
| 1 | API 租约不刷新 deadman 时钟:纯 robot.move 驱动被 gate 每拍清零,limited_by 恒 ["deadman"](HEAD 旧序是通的,移植引入) | runtime.rs set_api_twist 同步刷新 last_twist/twist_fresh + 回归测试(10 拍 applied>0) | ✅ 工单 R |
| 2 | robot.state 帧四条早退(powered_off/Limp/Sitting/Resting)不组装,订阅者看冻结幽灵鸭(官方无条件组帧 main.rs:4103) | 组帧移到早退前 + 测试 | ✅ 工单 R |
| 3 | 45Hz 地板按墙钟计数,0.25×/0.5× 慢放(产品特性)被误判 Unhealthy;暂停恢复首拍误报约 1s | 按 clock.speed 归一 + 恢复首拍丢弃窗口 + 测试 | ✅ 工单 R |
| 4 | Checkpoint 漏收 driving_prev/stopped_driving_at/posture_checked/start_from_hold/volts_ema 五控制态 → fork 首拍假上升沿 reset 掉恢复的循环态、重跑姿态分类、分支与父轨迹分叉 | 五字段入 Checkpoint 读写 + fork 父子 20 拍轨迹一致测试 | ✅ 工单 R |
| 5 | disturb 门控未实现但 FIDELITY.md:147 写了假承诺(我基于未执行计划落笔,批次内自查发现的同类问题) | 真实现官方语义:驱动中换驱动网络→先回 home 再换(main.rs:3015-3106) + 测试;文档承诺随之成立 | ✅ runtime 侧工单 R;**ffi 接线待完成**(reload 点比对 walking slot→`replace_policy_disturbing(disturbs)`) |
| 6 | 分支会话 publish_ipc 用原主 health_result(原主不 tick 恒判 stalled),与 /state 的 active 矛盾 | 统一 active_runtime 来源 + 非空洞性测试(旧代码必红) | ✅ 工单 S |
| 7 | pad 离散事件走原主而非 active 分支(时间机器选中分支时按键打错鸭)+ pad Skill 无关机门 | 全臂改 on_active/active_runtime + 关机门对齐 intent 臂 + 端到端分支归属测试 | ✅ 工单 P |

## 推后项(有意不在本批修,立项排下一批)

QA 超上限未验证、工单 S 逐条裁决后确认成立、低危(物理手柄尚无真机用户)：

1. **DKRB 节奏**:ffi.rs:1922 `pad_report_at.take()` 在 `report_due` 判 false 时也消费了时间戳 → 实际隔拍一发(25Hz)而非官方 1s。改窥探不消费。
2. **Start-held Home 语义**:移动实现 `disable().and_then(enable)`=策略开;官方 go_home=disable+init=策略关、停 home(main.rs:2657-2683)。次要:enable 对已坐鸭可能重新 homing(官方 Init 保持 seat)。
3. **BodyPose 释放时机**:移动在**进入** BodyPose 时 release_body();官方 mapper 在**离开**时发 pose(active:false)(mapper.rs:44-49)。
4. **Rest 与 Shutdown 分离**:两者共用 request_shutdown;官方 robot.rest 落 Limp+舵机重启、不 power-off(main.rs:3130-3175)。手机无舵机重启是有意的,但 rest 不应等于关机(至少行为差异要有、或文档如实)。
5. **sit/stand 跨门语义**:intent 臂走幂等 sit/stand,pad 臂一律当 toggle——注释称有意,但同名跨门不同语义值得复核。

## CI 覆盖盲区(记录在案)

- core.yml 不编译 ffi.rs(无 mujoco feature)：IPC/pad/DKRB/ffi 接线在 core CI 是盲区,由 ios.yml/android.yml 的构建与模拟器冒烟间接覆盖。**后续立项**:core CI 增加 `cargo test --features mujoco` 步骤(需 MuJoCo 静态库构建步骤)。
- Swift/Kotlin 本地不可编译：双端新文件(VetbookCard/PadSenderAxisTests 等)由 ios.yml、android.yml 裁决。
- 本批未做真机/双手机/真实手柄验收(边界不变,见 AUDIT §五)。

## 首推 CI 结果与修复(2026-11-17 夜)

- Core ✅(51s)、Contracts ✅(14s)。
- Android ❌(4m50s):`compileDebugKotlin` 7 错,全部出自工单 G 的两处 import——`SelectionContainer` 包路径写错(实为 `androidx.compose.foundation.text.selection`,写成了 `androidx.compose.ui.text.selection`)、`VetbookCard.kt` 用 material3 风格 `Text` 漏 import `androidx.compose.material3.Text`。本地 gradle 8.10.2 + SDK 复现并修复:`compileDebugKotlin` 与 `testDebugUnitTest` 全绿。这正是「Swift/Kotlin 由 CI 裁决」条款兑现——本地施工时一次没编译过 Kotlin,CI 抓的。
- iOS:由 ios.yml 裁决(构建+XCUITest+模拟器会话断言)。
