# QA · 技能热插拔 + 家长看板「日子」（2026-11-19）

- 对象：第四批功能点——技能热插拔（`/config custom_skills` + IPC `robot.setSkill/removeSkill`，官方语义）与家长看板「日子」（`GET /days` + 双端页面；DayRecord 补八项真实每日增量）。
- 方式：3 维对抗审查（core-skills / core-days / shells）→ 32 条对抗验证 → QA 放行判定。28 agent。
- 首判：**block**（2 blocker + 4 major + 6 minor 确认成立）。三路修复后全绿。

## blocker 与修复

| # | 问题 | 修复 |
|---|---|---|
| 1 | **setSkill 路径穿越**：`path` 缺省时技能名被拼成 `{name}.onnx`，`../../evil` 或绝对路径可越出 policies 目录加载任意 ONNX（name 只查空名/daemon 名）。 | `validate_skill_name` 改为裸文件名校验（拒绝 `/ \ ..`、前导 `/`、`:`——Windows 盘符前缀会被 `Path::join` 整体替换）；`merge` 对派生文件名二次校验（旧表也越不出去）；IPC 门口同步。测试覆盖七种恶意名 + 目录外真实 `.onnx` 不被载入。 |
| 2 | **冷启动把终身计数当今天**：启动时 `/quests/day` 先落地，`roll_day` 把基线快照成近零，tick 末尾才应用 `RestoreLife`（只改 life 不动基线）→ `/days.today` 与次日行把一辈子算成一天；`days`/`day_started` 也不进恢复闭环，重启历史清空。 | `Book::restore_days(saved, today)`：恢复已完成行（日历日过滤、30 行上限）；基线仅当保存日期=今天才采纳，否则**重设为当前 life 并盖上今天**（钳到 `[0, life]`）；`summary` 发布 `day_started`。测试覆盖两种到达顺序、同日回灌、跨日回灌。 |

## major 与修复

| # | 问题 | 修复 |
|---|---|---|
| 3 | `setSkill path:"none"` 被当作真实文件校验必然拒绝（官方意为关闭同名内建技能） | 哨兵原样存储、跳过文件校验；merge 时从内置/套装技能表**移除**该名（等价上游 `resolved_path → None` + `retain`）；与上游不符的注释已改写。测试：关 roulade → 列表消失 → removeSkill 恢复。 |
| 4 | setSkill 缺官方门口的 ONNX 校验：非 ONNX 被 accepted，坏条目永久留在表里，此后 reload/模式切换持续失败 | 接受前调用 `duck_control::policy::validate`（与官方同一条件：仅 `error.path().is_some()` 时拒绝）；测试：坏文件被拒且表不被污染、随后 reload 仍成功。 |
| 5 | Android 把「核心未回应/坏 JSON」与「确无完整日」合并成空态，首帧还会闪假空态 | 三分支：未加载不画 → 失败显示「没有回应」→ 真空才显示「还没有一整天的记录」；首帧不闪。 |
| 6 | 双端每日行字段不一致（iOS 6 项、Android 9 项） | 统一 9 项（陪伴/米/翻滚/被摸/被抱起/摔倒/被喂/拍手/口哨），iOS 日行与周卡、分享同源。 |

## minor 与修复

- 技能表并发：收集与判定同一临界区、merge 全程只读，消除并发误删。
- 文件缺失条目：只跳过本次 load 并告警，进程表保留（文件回来即恢复），不再永久删除。
- 顺序偏差（BTreeMap 名字序 vs 官方文件条目序）在文档如实登记；`robot.do` 按精确名不受影响。
- `sit_downs` 补运行时边沿计数（对齐官方 vetbook 的 label 边沿），日行不再恒 0。
- 与事实相反的注释（两端各一处）已改正。

## 验证（修复后最终树，本地）

- Rust 默认 **290+9 全绿**、`--features mujoco --test-threads=1` **341+9 全绿**；本批修复新增 13 条测试，全部先红后绿（含 6 处临时回退的红证据）。
- Android `compileDebugKotlin + testDebugUnitTest` 全绿（DaysSheetTest 10）。
- 六道静态门禁全绿：copy-keys 350=350、ui-baseline 棘轮保持、ui-entries 50 行/95 锚点、routes、ui-features 44 行。
- Swift 编译/Xcode 单测、真实 `/config custom_skills` 与真机看板：由 CI 与真机裁决。

## 边界（如实）

- `ground_pick`/`sit_toggle` 是官方 daemon 所有技能，官方 `setSkill` 同样先拒绝；`"none"` 关闭适用于套装声明的技能（如 roulade）。
- 自定义技能文件只从 policies 目录内的裸文件名加载（手机无绝对路径概念，官方板子上是绝对路径——刻意形状差，代码注释已登记）。
- `DayRecord` 的 `sit_downs` 现由运行时 label 边沿累加；旧存档缺字段=0。

## CI 首跑（2026-11-19）

- **Core ✅、Contracts ✅、Android ✅。**
- **iOS 第一轮**：`the lead chorus is published (role=)` 失败。会话日志显示 POST 已被核心接受（`queued:true`），其后合唱未在轮询窗内出现；同轮「加入邻居合唱并完成」两项均 PASS，本轮代码未触碰 chorus 路径。**重跑后该检查通过**——归档为 CI 环境停顿（/state 快照按 tick 刷新，渲染循环停顿时读不到刚接受的合唱），非代码缺陷。
- **iOS 第二轮**：`English speech maps to the same table` 失败，响应为上一批「情绪一次一个」的丢弃体 `{ok:true, dropped:"an emotion is already playing", heard, phrase}`——**缺 `skill` 字段**，冒烟按 `"skill":"excited"` 断言。这是真问题：丢弃/失败回答应始终告诉客户端「本意是哪个动词」。已修：两种回答都带 `skill` 或 `sound` 字段（`7ea7da5`），并加断言；本地 290+9 / 341+9 全绿。此修复对通用客户端同样有意义：语音遥控连发第二个情绪时仍能知道它被理解成了什么。
