# QA · 情绪图鉴 + 语音闲聊功能批（2026-11-18 夜）

- 对象：COMPANION 头脑风暴仅剩未做两项——#5 情绪图鉴·把戏学校、#9 语音闲聊。核心 `GET /collection`（真实次数 + 首次解锁日期，零编造）+ 双端图鉴页 + AI 伙伴页纯文本聊天与本机 TTS。
- 方式：4 维对抗审查（core / ios / android / contracts）→ 每条发现双镜头验证 → QA 主笔放行判定。29 agent。
- 首判：**block**（1 blocker + 8 major + 若干 minor）。此后三路修复并做定点复审。

## blocker 与修复

| # | 问题 | 修复 |
|---|---|---|
| 1 | `/life` 恢复原样采信 `unlocks_backfilled`；缺条目时 `note_unlocks` 用当天补写，把「日期未知（—）」变成伪造首次日期。无需恶意客户端：暂停时 tick 先 drain intents 再 early-return，跳过记录；且暂停中 pad 技能键、语音 `/heard`、pet 规则、`/night off` 都能绕过计数。 | 恢复时按「count>0 键全覆盖 + 合法日期」自行判定回填完整性，缺口一律补 `""`；`note_unlocks`/`set_life` 移到暂停 early-return 之前；暂停语义收敛到计数原语（`trigger_skill`/`play_emotion`/`celebrate_emotion` 暂停一律拒绝或跳过），pad/语音/规则/夜晚/LAN/AI/MCP 全部同门，Step 为时间机器明确例外。回归测试 5 条（回填缺口、pad、Heard、pet 规则、night），全部先红后绿。 |

## major 与修复

| # | 问题 | 修复 |
|---|---|---|
| 2 | day 全链路只校验长度/非空 | `is_calendar_day`（YYYY-MM-DD、2000–2100、闰年真实日历），应用到 `/quests/day`、QuestDay、tick 解锁门、note_unlocks、backup 恢复；`/collection` 过滤非法 first_day。测试：2026-02-30 等非法日拒绝、年份越界（>2100/<2000）拒绝、2024-02-29 接受；前导 `+` 形态（2026-+1-18）在复审后补上逐位数字校验并加测。 |
| 3 | 解锁键写/收不一致（含空格/点/非 ASCII 的 manifest 名每次启动被重记成当天） | 恢复端改为「键必须存在于恢复后的计数表且 count>0」+ 合法日期；未知键丢弃。测试：`skill:my move.v2` 跨存档保留原日期。 |
| 4 | iOS 图鉴显示 wire 名 | `SkillLabel.of` 映射（kick_left→"kick left"、roulade→"翻滚"），未知名保留真实名；带语言重载与单测。 |
| 5 | iOS 闲聊请求缺 last_heard/carrying/powered_off | compact 列表补齐；测试断言请求体含真实 heard 文本。 |
| 6 | Android 锁定行仍显示「First unlocked」 | 展示模型锁定即 `firstDay=null`；核心对 count==0 不再发送 first_day；两端解析兼容缺键；JVM 测试钉住。 |
| 7 | Android targetSdk 35 缺 TTS_SERVICE 查询（引擎发现受限） | Manifest `<queries>` 补 `android.intent.action.TTS_SERVICE`；JVM 静态断言 Manifest 同时含 RecognitionService 与 TTS_SERVICE。 |

## minor 与修复

- iOS/Android `configured` 只看 endpoint 非空 → 统一为「开关开启 && 可解析 http(s) URL」；非法地址立即提示「先配置模型」，不再等 2 秒超时。
- iOS 图鉴练习后不重载 → 1.2s 后重载（对齐 Android）。
- iOS 发送后不清草稿 → `ChatComposer.take()` 发送即清空，纯空白不清。

## 验证（本地，修复后最终树）

- Rust 默认 **255+9 全绿**、`--features mujoco --test-threads=1` **295+9 全绿**；新增核心测试 **22 条**（收集页数据/回填/日历/键/暂停语义，全部先红后绿；暂停语义收敛在 `trigger_skill`/`play_emotion` 计数原语，规则、夜晚、pad、语音、LAN、AI、MCP 同一门，Step 为时间机器例外）。
- Android `compileDebugKotlin + testDebugUnitTest` **88 全绿**（CollectionTest 7、VoiceChatTest 11 含 Manifest 静态断言）。
- 六道静态门禁全绿（copy-keys 319=319：旧键未删未改、两端对称新增 11 键 308→319；ui-baseline 未增长；ui-entries 87 锚点；routes；features）。
- Swift 编译、Xcode 单测/UI 测试、真实 `/collection` 联调、真实模型/TTS 音频、模拟器 UI 走查：由 CI 与真机裁决。

## 定点复审

对 7 项 must-fix 与回归测试做 2 镜头（核心 / 双端壳）定点复核，标准：0 blocker 且无「正常路径被误拒」的 major；结论见提交记录与复审聊天摘要。

## 后续登记（非本批）

- 旧 `DexSheet`（iOS/Android）与 `DexStore` 在 .dex 路由改渲染图鉴后已无调用点；`DuckController.updateDex` 仍在每轮 life 更新里计算。建议下一批删除并让控制器停止计算，顺带缩小 ui-baseline 债务。
