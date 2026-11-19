# QA · SENSORS 批（深度障碍 / AR 地板 / 声音摸摸 / 声音修复）（2026-11-19）

- 对象：第五批功能点——#4 深度障碍（双端 AR 深度扫描→静态 AABB 注入 MuJoCo）、#20 AR 地板手机侧接线（`POST /floor` 偏移语义）、声音摸摸（官方 `pet_detect.onnx` 经 `/sensor` PCM16 → 核心 log-mel+CNN）、喂食/礼物/邻居 replied 的「裸 yes 无声」修复，以及对抗审查中追出的分支分发/Android 深度解码等缺陷。
- 方式：5 维对抗审查（core-pet / core-world / iOS / Android / 契约）→ 每条发现双镜头（正确性 / 契约与诚实）反驳式验证 → 修复 → 定点复核。47 agent，420 万子代理 token。
- 首判：21 条发现；双镜头双方确认 **17** 条、分歧 **1** 条（QA 裁决为真并修）、双方驳回 3 条。修复后本地双套 + Android + 六道门禁全绿。

## blocker/major 与修复

| # | 问题 | 修复 |
|---|---|---|
| 1 | **分支分发二次换枝（本批新路径）**：`apply_intent` 已把选中分支换入 `core.runtime`，新增的麦克风 PCM 臂却用 `on_active` 再换一次 → PCM 喂给未选中的原始运行时；A/B 会话中可见分支永远 `petting:false`，隐藏原始被记账/coo，且分支暂停门被绕过。`FloorHeight`、`ParentBudget` 同病。 | 三个臂改为直接 `core.runtime.*`。测试：Fork+Select 后 PCM 只到分支（含暂停门）、地板偏移落分支、预算落分支；旧 selected=0 测试保留。 |
| 2 | **Chorus/Ensemble 同类**：写路径用 `on_active` → 合唱/合奏落在原始运行时；臂内读也用 `active_runtime`（dispatch 期间它指向原始）。 | 写改 `core.runtime.start_chorus/start_ensemble`；读改 `core.runtime`。守门测试：选中分支唱、原始不唱；`apply_intent_active` 顶部写死「本函数内 core.runtime 就是选中分支，禁止 on_active/active_runtime」并列出 global 豁免名单。 |
| 3 | **RestoreBackup 同类**：global 意图内部直接写 `core.runtime`（原始）恢复 care/rules，而 `Backup` 读选中分支。 | care/rules 改用 `on_active`（global 上下文中它才是正确换枝器），响应在分支上读出；QuestDay 保持 `active_runtime`（global 正确读法）。测试：备份落分支、原始不动。 |
| 4 | **Android DEPTH16 解码错**：把整 16 位当毫米，忽略「低 13 位=距离、高 3 位=置信度」的官方位布局 → 任何 confidence>0 的像素距离 ≥8192 mm 被过滤；真机扫描几乎必空。 | `Depth16` 纯函数：`range=raw&0x1FFF`、`confidence=raw>>13`，保守只收 100% 置信度（步长 20%，家具边缘正是置信度下降处），范围 100–4000 mm；logcat 打 bucket 分布供真机校准。合成帧测试钉住 0x2320/0x4320 两例。 |
| 5 | **Android 相机不筛朝向**：可能选中前置 ToF/深度相机，扫的是用户背后的房间。 | 只接受 `LENS_FACING_BACK`；只有前置深度→`NO_SENSOR` 诚实隐藏。纯函数测试：仅前置/前后都有/无深度。 |
| 6 | **PCM rate 无界放大**：`take_pcm16` 只要求 rate>0；`rate=1` 把 1 万样本重采样成 1.6 亿 f32 → OOM。 | rate 仅收 8 000…48 000；解码样本上限 2 s（32 000）。坏载荷剥离后丢弃。测试：0/1/7999/48001 拒绝，8000/16000/44100/48000 接受，32 000 接受/32 001 拒绝。 |
| 7 | **`sound_pets` 不随存档恢复**：已进 `/life` 与 summary，但 `RestoreLife` 逐字段重建漏了它 → 每次重启归零。 | 恢复补该字段；测试 7→恢复→7，旧档缺字段=0。 |
| 8 | **`PeerEvent::Replied` 裸 `"yes"`**：不在 `queue_sound` 白名单，邻居回应静默。 | 与 feed/gift/end_night 同款：yes 情绪已加载→`emotion:yes`，否则 `chirp`；两种构建测试钉住。 |
| 9 | **暂停 ring 拼接**（分歧项，QA 裁决为真）：暂停只跳过推理不喂模型，ring 保留 → 恢复后首窗把暂停前旧音频与恢复后新音频拼接，可能把暂停前的抚摸计成恢复时的新 Start。 | `set_paused(false)` 转换点清 ring；保留 verdict。测试 buffered==0。 |
| 10 | **契约注记失实（QA 自纠）**：`features.yaml` 的 tof 行声称「双端接真深度源」，实际深度扫描只喂 `obstacles.xml` 与 `/floor`，从未上报 depth 读数，8×8 网格恒空。 | 注记改为事实陈述；`to_tof_grid` 语义本批未动（仍按此前报告的 deviation 边界）。 |

## minor 与修复

| # | 问题 | 修复 |
|---|---|---|
| 11 | `obstacles.xml` 合并写盘失败会 `?` 上抛、阻断建核，违背「坏扫描不能挡住鸭子」。 | 写失败 warn + 回退普通场景；不可写路径测试。 |
| 12 | 未闭合 `<!--` 注释被原样保留，文本查找把注释里的 `<worldbody>` 当真 → 合并畸形 XML。 | `strip_comments` 未闭合→`None`，拒绝该扫描回退普通场景；QA 样本先红后绿。 |
| 13 | shim `duck_mj_ncon` 未截断而接触对缓冲只写 512 对 → ncon>512 越界读。 | 返回 `min(ncon,512)`，`contacts()` 双保险；100 个重叠 box 测试。 |
| 14 | Android 内参未按 `preCorrectionActiveArraySize → DEPTH16 输出尺寸` 缩放 → 点云方向错。 | 纯函数按宽/高分别缩放 fx/fy/cx/cy；同尺寸不变；缺失回退近似并注释。 |
| 15 | Android `obstacles.writeText` 未隔离：ENOSPC/只读会打断建核。 | staging 独立 `runCatching`，失败如实降级并继续建核；不显示假箱数。 |
| 16 | Android obstacles.xml 头注释写「one-second scan」，实际只取一帧。 | 改为「one depth frame captured at launch」；测试钉住新文案。 |
| 17 | 文档漂移：COMPARISON 仍记 pet-detect not_doing、feed/gift 无声；BRAINSTORM 把 #20/#4 记未做；COMPANION 未提声音摸摸。 | COMPARISON 增 §0.3 后记更新；BRAINSTORM 台账增「八（S1）」；COMPANION §4 与功能清单 16/17 更新；iOS README 已随工单更新。 |

## 验证（修复后最终树，本地）

- Rust 默认 **313+9 全绿**、`--features mujoco --test-threads=1` **388+9 全绿**（基线 310+9 / 375+9；本批新增 20+9 条测试，全部先红后绿，含 8 项分发放行、DEPTH16 合成帧、rate/样本上限、RestoreLife/RestoreBackup、暂停 ring、注释/写盘/接触上限）。
- Android `compileDebugKotlin + testDebugUnitTest` 全绿：**148 测试**（基线 136；DepthScanTest 9、PetSoundTest 10、其余既有）。
- 六道静态门禁全绿：copy-keys **353=353**、copy-values 339 一致、ui-baseline 棘轮保持（iOS 侧未增长，`@Published` 保持 107）、ui-entries 50 行/95 锚点、routes、ui-features **台账 46 行**（新增 depth-obstacles / pet-sound 两行）。
- `cargo check --features mujoco` 无错误；Cargo.lock 净变化 1 行（`ort` 直接依赖，复用 duck-control 同版，无新 crate）。
- 61 维观测与安全层：`sensors.rs` 纯加（pcm 剥离/解码），mujoco/runtime 改动仅本批意图（注入/地板/hook 与测试），布局与顺序未动；受保护路径门禁按设计只发警告。

## 边界（如实）

- **真机限定**：LiDAR/DEPTH16 的实际置信度分布、扫描精度、麦克风灵敏度只能在真机验收；模拟器/无深度设备整体隐藏并发诚实提示。Android 只收 100% 置信度像素（真机 logcat 有 bucket 分布可校准）；iOS 只在已获相机授权时扫描（首装不弹，Director 授权后下次启动生效）。
- **模型**：官方 `pet_detect.onnx`（上游原模型，未自训）随包才启用；无模型静默关闭、零计数零推理。特征链逐常量对齐上游并有契约 pin 测试，但未做官方 Python 向量对拍（本机不可用），以「FFT vs 直接 DFT」「静音=ln(1e-6) 地板」「440 Hz 落带」自洽断言替代。
- **`/floor`**：量纲钉死为「相对默认地面偏移 −0.1…0.3 m」，两端壳在检测面即鸭脚地面时诚实发 0（非零偏移需渲染层把鸭子锚进 AR 世界，本批不做）；每次启动扫一次，家具不随房间变化（文案写明）。
- **`sound_pets`**：进 `/life` 与 `/state.pet_detect`，刻意不进 DayRecord 八字段日行（尚无按日记账语义）。
- **8×8 ToF 网格**：仍无 depth 上报源，空就是空。
- **`Intent::Step`** 的暂停-恢复会清 pet ring（每次 step 视为新收听会话；时间机器例外）。
- **#21 自愈爬起**：仍 blocked——官方未发布 `standup.onnx` 权重，不自训、不编造。

## CI 首跑（2026-11-19）

- `ece665e`（本批四提交）：**Contracts ✅、iOS ✅、Android ✅**；**Core ❌**——blocking fmt：QA 修复的几处换行未过 `rustfmt --check`。修复（同配置本地 `FMT-OK`）后推送。
- `6dc7ed9`：**Core ❌**——clippy `dead_code`：测试支撑 API `load_pet_detector_with_config` 在默认 profile（`ffi.rs` 不编译）无调用者。修复方向先误删，随后确认 `ffi.rs` 测试实际调用它。
- `7622469`：**Core ❌**——mujoco job 编译失败（E0599，方法被删）；同轮 `tests`/clippy 已绿。
- `ea4b904`：**Core ✅、iOS ✅、Android ✅**。方法改为 `#[cfg(all(test, feature = "mujoco"))]`（默认无此方法=无 dead code；mujoco 测试可用），并清掉 ABI 测试里的 4 处嵌套 `unsafe` 警告。
- **Contracts** 于 `ece665e` ✅；后续 core-only 提交按 `paths` 过滤不触发（本批契约/文档改动已在 `ece665e` 验证）。最终四工作流按其路径触发全绿。
- CI 结论：Swift 编译/模拟器冒烟、Android 模拟器冒烟、Core 默认 + mujoco + fmt + clippy 均通过；真机 LiDAR/DEPTH16/麦克风灵敏度仍由真机验收。