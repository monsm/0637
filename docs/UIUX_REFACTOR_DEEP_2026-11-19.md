# UI/UX 深度重构总纲 · 天花板版（seed v0，待对抗评审）

> 2026-11-19 · 起草：小宝（老板助理）· 基线 `964b872`（四工作流全绿）
> 定位：上一轮 B1–B8 + IA 4×4 解决了「止血与结构」；这一轮是全维度深挖，把 Duck 的界面做到「每个像素有规矩、每个状态有交代、每个人都能用」。
> 用法：本文是施工底稿。对抗评审逐条证伪、补漏、改判后，把结论并入 §9，然后按批次直接施工。

---

## 0. 一句话

**从「功能齐、颜色统一了」走到「系统级完美」：令牌零裸字面量、组件有状态矩阵、排版动态字体不破版、无障碍全链路可用、动效可打断可降级、状态三层齐备、性能有预算、双端有对照表。**

---

## 1. 天花板判定（Definition of Done）

| 维度 | 判定标准（全部可脚本/可截图验证） |
|---|---|
| 令牌 | 主题文件之外 **0 处裸值**：色/字号/间距/圆角/时长。CI 棘轮锁死，只降不升。双端同名同值。 |
| 组件 | 11 类基础组件全部来自组件库（单端一份实现），裸 `ButtonStyle`/手写卡片 **0 处**；每组件状态矩阵（默认/按下/禁用/选中/聚焦）有快照测试。 |
| 排版 | 7 档 `type.*`；iOS 动态字体到 accessibility3 **不截断、不重叠、不破版**（XCUITest 断言）；数字一律等宽；中文正文 ≥13 pt。 |
| 无障碍 | 舞台 4 个自定义动作（走一步/左转/右转/摸摸它）；状态变化必播报（摔倒/礼物/合唱/路径完成/入夜）；焦点顺序=视觉顺序；对比度文字 ≥4.5:1、图形 ≥3:1；命中区 ≥44/48；颜色永不单独承载信息。 |
| 动效 | 全部走 `motion.*` 4 档令牌；过渡可打断；减动效开启时瞬时但不丢状态反馈；列表进出场统一编排。 |
| 状态 | 每个有数据/网络/硬件的表面都有 加载/空/错误 三态 + 重试出口；错误文案是人话，不露协议串。 |
| 性能 | Android 5Hz 轮询不再触发整棵壳层重组（状态下沉到叶子）；长列表一律 Lazy；sheet 首帧 ≤150 ms（模拟器冒烟实测，写进 CI 记录）。 |
| 双端 | 同名功能同名入口、同顺序；差异只允许手势/返回键这类平台事实，进 `exceptions` 并写明理由；Copy 键集与值门禁继续全绿。 |
| 信息架构 | Live 首屏鸭子连同地面 ≥70% 画面、可点元素 ≤7；抽屉 4 组 × 每页 ≤6 控件；任意功能 ≤2 跳；横屏与竖屏同一张路由表。 |

---

## 2. 现状基线（2026-11-19 实测，施工前后都要重算）

| 项 | 现状（2026-11-20 施工后实测） |
|---|---|
| 颜色令牌 | ✅ `DuckTheme.swift` / `DuckTheme.kt`；主题外色字面量：iOS 4 / Android 0（棘轮） |
| 字体令牌 | ✅ seven-gear Type 双端；主题外裸字号 **0 / 0**、`ios_missing_rounded` 0；数值走 `duckData/Micro`（等宽） |
| 间距/圆角 | ✅ 4 基数 Space + 五档 Radius；off-grid **0 / 0**、radius 裸值 **0 / 0** |
| 动效/触感 | ✅ `DuckMotion` 四档双端；内联动效 **0 / 0**（棘轮）；触感地图 12 个真实事件点；减动效单点收口 |
| 组件 | ✅ 11 组件双端（`contracts/ui-components.json` + 门禁）；iOS 手抄关闭钮 16→1；两壳的行已走组件层 |
| 无障碍 | 🟡 舞台 4 自定义动作 + 播报单点（pill liveRegion / moodText announcement）已落；**动态字体全表、焦点顺序、44/48 全审未完成**（D4 遗留） |
| 状态覆盖 | 🟡 三态组件已建；Android 失败卡/唤醒已接入；各表面三态矩阵未铺完（D5 遗留） |
| 性能 | 🟡 Android 调试重组计数已挂、冒烟已记录首个数值；仪表 30 控件列 Lazy 化与 iOS @Published 拆分**未做**（D7 遗留） |
| IA | ✅ IA 4×4；入口台账 50 条锚点 94 处全绿 |
| 断点/平台差异 | ✅ `contracts/adaptive.json` 三档 + `contracts/platform-differences.json` 5 条平台事实（各有理由），`check-ui-parity.py` 守门 |
| 已知红线 | 未触碰：61 维/核心/upstream 零改动（`check-ui-scope.py` 守门）；离线优先；事实不编造；只做 iOS+Android；中文圆体不随包 |

---

## 3. 目标态：设计系统

### 3.1 令牌分层（color 已有，本轮补齐五层）

- **type**（7 档，双端同名）：
  `display 28/34 semibold` · `title 20/26 semibold` · `body 15/21 regular` · `label 13/18 medium` · `caption 11/14 medium` · `data 11/14 mono` · `micro 10/12 mono`
  - iOS：语义字体映射（动态字体免费生效，尺寸与七档逐一吻合），文本一律 `design: .rounded`；`micro/data` 用 `.caption2` + monospaced，`micro` 由 10 收到 11（1 pt 偏差换全局动态字体，写进文档）。超大档 `.dynamicTypeSize(...accessibility3)` 封顶。映射：≥22→`display(.title 28)`；17–21→`title(.title3 20)`；15–16→`body(.subheadline 15)`；13–14→`label(.footnote 13)`；11–12→`caption(.caption2 11)`；≤10 非等宽→`caption`；等宽→`data`(11)/`micro`(≤10 也归 `data`，仪表刻度除外)。图标字形不随字号缩放，走 `DuckIcon.xs/sm/md/lg`（12/15/18/22）。
  - Android：`sp` + `fontScale`；固定 `dp` 容器改 `heightIn(min:)`；`MaterialTheme.typography` 接管默认。字号映射：9–10→`Micro(10)`；11–12→`Caption(11)`；13–14→`Label(13)`；15–16→`Body(15)`；17–20→`Title(20)`；≥24→`Display(28)`；数值显示补 `FontFeatureSettings "tnum"`。
- **space**：4 基数 `4/8/12/16/20/24/32`；屏幕边距 16；1/3/5/9/18 等手调值清零。
- **radius**：`chip=胶囊 / control=14 / card=18 / panel=24 / thumb=12`。
- **elevation**：chrome 材质 + 顶部 12% 白渐变遮罩；sheet 不透明 `bg.surface`；overlay `bg.scrim`+投影；同一层不许混用材质/实色。
- **motion**：`quick 0.15 easeOut / base 0.25 easeInOut / slow 0.40 easeOut / deliberate 0.60 spring(0.32,0.9)`。

### 3.2 组件库（单端一份实现，状态矩阵齐全）

1. `DuckButton`（primary / secondary / ghost / icon；按下 scale 0.94 + 亮度 +4% + 触感）
2. `DuckChip`（选中态=实色 + 勾选图标 + `isSelected` 语义）
3. `DuckCard`（surface + 1px 描边 + radius.card）
4. `DuckSheetHeader`（标题 + 关闭钮，48pt，双端同一实现，替代 iOS **16 个文件**的手抄 xmark —— `contracts/ui-baseline.json: ios_close_button_files=16`）
5. `DuckListRow`（主/次/尾三栏 + 点击反馈 + 分隔线令牌）
6. `DuckToggle`（开/关 + 对象名 Copy + TalkBack/VoiceOver 播报对象）
7. `DuckSegmented`（分段控件，选中态非纯颜色）
8. `DuckProgress`（线性/环形；刻度 `accent.data`；填充不透明）
9. `DuckEmptyState`（图标 + 一句人话 + 一个行动；不允许空白页）
10. `DuckErrorCard`（标题 + 原因人话 + 重试；启动失败/网络失败/硬件不可用共用）
11. `DuckLoading`（呼吸动画 + 人话；减动效时静态；“正在唤醒鸭子…”不露引擎名）

### 3.3 排版与布局规则

- 4 基数网格；左右 16；卡片内 16/12；列表行高 ≥ 56。
- 字号档位收敛到 7 档，**禁止新增档位**；9 pt 只允许仪表刻度（micro）。
- 数值等宽（`.monospacedDigit()` / `FontFeature`），避免跳动。
- 文本灰度：primary/secondary(70%)/tertiary(50%，下限)；低于 50% 禁止用于文字。

### 3.4 动效与触感

- 所有过渡用 `motion.*`，禁止内联时长/曲线。
- 可打断：sheet 拖拽中可反向；技能盘展开中可再点。
- 列表/抽屉进出场统一编排（`.transition(.move+opacity)` 组合）。
- 触感地图：喂食 success / 技能 medium / 摔倒 error / 路径完成 success / 摸鸭 light / 切模式 selection。
- 减动效：全局 `reduceMotion` → 动效瞬时；必要状态反馈保留（颜色/文案/触感）。

### 3.5 无障碍基线

- 舞台 `accessibilityLabel("鸭子舞台")` + 自定义动作 4 个；技能盘、时间轴、抽屉各有可达入口。
- 状态播报清单（两端同一份文案）：摔倒、被拎起、拍手/口哨识别、礼物收发、合唱邀请/开始、路径走完、入夜/早安、AI 动作与理由、规则触发。
- 焦点顺序=视觉顺序；sheet 打开聚焦首元素；关闭回焦打开处。
- 动态字体：全界面到 accessibility3；两列以上布局自动换单列；按钮文字不裁。
- 对比度：正文 ≥4.5、图形 ≥3；当前已知违例逐条清零（历史清单见 UI_AUDIT §P0-9/P1-8/9/10）。

### 3.6 状态系统

每个表面（Live、抽屉五页、仪表、明信片、日记、图鉴、邻居、规则、AI、设置）必须具备：
1. **加载**：骨架/呼吸动画 + 人话；≤300ms 不闪。
2. **空**：一句人话 + 一个可行动出口（不许“去别处找”）。
3. **错误**：人话原因 + 重试 + 不假成功；失败时控件不装成可点。

### 3.7 性能预算

- **Android**：5Hz 轮询驱动的状态不再放顶层 `mutableStateOf` 大对象；用 `snapshotFlow`/`derivedStateOf` 下沉到叶子；LazyColumn 化长列表（仪表 30 控件列、图鉴）。
- **iOS**：107 个 `@Published` 的 `DuckController` 拆出子状态对象（相机/AI/编舞/明信片），页面只订阅所需。
- 预算写进冒烟：sheet 打开首帧、滚动掉帧率、重组次数（Android 用 `RecompositionCounter` 日志断言上限）。
- 无法可靠自动测的项，写进 QA 清单并标明“未验证”，不伪装成已达标。

### 3.8 双端一致

- 入口对照表（features.yaml）继续 CI 校验：同名功能同名入口、同顺序。
- 平台事实差（返回键手势、iOS sheet detent、Android Toast→Snackbar）进 `exceptions`。
- 新增 Copy 键两端同推；颜色/字号/间距令牌值 CI 比对。

---

## 4. 信息架构目标态（在 IA 4×4 之上精修）

- Live：鸭子 ≥70% 画面；可点元素 ≤7（喂/技能/叫/摸 + 更多 + 时间栏必要项）。
- 抽屉：四组（玩 / 一起 / 记录 / 这只鸭子）× 每页 ≤6；一页一主题，不混功能。
- 任意功能 ≤2 跳；跨页跳转有明确返回路径。
- 横屏与竖屏同一路由表（`DuckRoute`），横屏只换布局不换架构。
- 空态/错误态在各入口就地呈现，不新增跳转。

---

## 5. 分批施工表（每批：本地全绿 + 六门禁 → 推 CI → 四工作流全绿）

| 批 | 内容 | 验收（可脚本优先） |
|---|---|---|
| **D1 排版与网格** | 建 `type.*`/`space.*`/`radius.*` 三层令牌；替换 iOS 207 处、Android 143 处字号；间距手调值清零；动态字体映射 | 主题外裸字号=0、裸间距手调值=0（棘轮基线重算）；iOS accessibility3 截图不破版；双端同名同值断言 |
| **D2 组件库** | 11 个组件双端落地；iOS 15 份手抄关闭钮收敛；交互表面接入组件库 ≥90% | 组件状态矩阵快照；裸 ButtonStyle/手抄卡=0；点击/禁用/选中行为测试 |
| **D3 动效与触感** | `motion.*` 令牌；内联时长清零；减动效全局；触感地图落地 | grep 内联 withAnimation 时长=0；reduceMotion 用例；触感调用点≥6 |
| **D4 无障碍硬仗** | 舞台自定义动作、状态播报全清单、焦点管理、对比度清零、命中区修正 | XCUITest/uiautomator：走一步/转身/摸到；播报断言；对比度脚本复算全过；44/48 检查 |
| **D5 状态系统** | 每个表面补 加载/空/错误 + 重试；错误文案人话化 | 每表面三态截图；协议串零露；失败分支可点重试 |
| **D6 IA 精修** | 入口一一对应复核；抽屉 ≤6/页；≤2 跳脚本化；横竖屏同表 | features.yaml 对照全绿；跳数断言；新旧首屏截图 |
| **D7 性能** | Android 轮询状态下沉/Lazy 化；iOS 拆 @Published 子对象；预算冒烟 | 重组计数上限断言；sheet 首帧记录；无整树重组日志 |
| **D8 收尾** | 截图矩阵进 CI（竖/横、三态、XXL 字号）；文档与 QA 记录 | 截图差异阈值；QA 一票否决签字；REPORT.md 记账 |

> 依赖：D2 依赖 D1 的令牌；D3 依赖 D2 的组件；D4–D6 可与 D3 并行（文件不重叠）；D7 后置（避免与 D2/D6 双改）。

---

## 6. 红线（复述，不许破）

61 维观测与安全层不动 · 离线优先 · 事实不编造（任何演示数据都是真值或空态）· 只做 iOS+Android · 每批记账 · 发布前 QA 一票否决 · 中文圆体不随包（老板已拍板，文档写明“中文用系统字形”）。

---

## 7. 对抗评审要点（评审员必答）

1. D1 全量替换 350 处字号，一次推送一验证的纪律下会不会拖成 10+ 推？可否按文件分包、每包独立绿？
2. 动态字体到 accessibility3：现有固定高度容器/多列布局具体哪几处会破？名单要落到文件行号。
3. 组件库 11 个是一次建齐还是按使用频次分两批？一次建齐的风险（API 未定就全量接入）怎么控？
4. 性能预算里哪些能自动化、哪些只能人肉？不能自动的要诚实标注，不许写“已达标”。
5. 与红线冲突检查：任何组件/动效都不得触碰渲染层除 shader uniform 之外的接口。
6. 成本：按当前节奏估算每批的 CI 轮次与时间，给老板一个总时长预期。

---

## 8. 施工纪律

- 每批一个主题、一次推送一验证；CI 红先修再前进。
- 每批结束：REPORT.md 记账 + QA 记录 + 截图。
- 发现总纲错误：改总纲再施工，不硬搬。
- 全部完成 = 四工作流全绿 + 截图矩阵 + QA 签字。

---

## 9. 对抗评审结论

> 评审：小宝（独立怀疑者，非总纲起草人视角）× 团队现场数据复核 · 2026-11-19 深夜
> 口径：每条先按「默认它不成立」审，能站住的才留；改动直接落到正文对应章节。

### 9.1 §7 六问的裁决

**Q1 · D1 全量替换 350 处会拖成 10+ 推？**
裁决：**不会，但必须分包。** 按文件簇分包，实测基数：iOS 207 处在 **16 个文件**（InstrumentSheet 54、RulesSheet 19、ProgressKit 16、VetbookCard 14、Dance/Diary/Peers 各 13、MoreSheet 12、AISheet 10、ContentView 8、DexSheet/PhotoPreview 各 7、Backup 6、AB/BigButtons/PostcardShelf 各 5）；Android 143 处在 **11 个文件**（Shell.kt 82、MainActivity 11、DiarySheet 10、MorePages 9、MoreSheet 8、Dex/Dance 各 6、Vetbook/Postcards 各 4、BigButtons 3）。每端拆 2–3 个提交（Live 壳层 / 仪表与抽屉 / 工具类小文件），每包独立绿后推。硬规则：一包触碰 >50 个调用点必须在中途落一个绿基线，不许一口气改完再推。iOS 关键路径 19–29 分钟/推，故 D1 控制在 3 推（Android 1 + iOS 2）。
改动：§5 D1 行补充「按文件簇分包，iOS/Android 分推，每包独立绿」。

**Q2 · 动态字体 accessibility3 具体哪几处会破？**
裁决：已知高危名单必须落到行号再开工（本轮实读复核，均为 HEAD 行号）：
- iOS 首屏：`ContentView.swift:752` 拇指台写死 `height: 82` + `:777` 52×52 圆钮 + `:788` 11 pt `lineLimit(1)` 标签；`:655` 时间栏 `height: 48`、`:636` 历史条 `height: 18`；`:1476–1491` 横屏小卡 96×14 / 88×58 / 46×46 / 16×12；`:1206–1382` 横屏底栏 9 个 48×48 圆钮。
- iOS 仪表：`InstrumentSheet.swift:392,620` 48×48 关闭钮；`:211,283,316,728` `lineLimit(1)` 行。
- Android 首屏：`Shell.kt:2185` 拇指台 `height(90.dp)`；`:2104` 时间栏 `height(48.dp)`；`:2293,2714,3355,3400,3423,3438,3509` `maxLines=1` 行。
- 共性：抽屉五页的两列/多列行（`MoreSheet`/`MorePages` 两端）在 AX 字号下不自动单列。
修法统一：固定 `dp`/`.frame(height:)` → `heightIn(min:)`/自适应；两列以上在 accessibility 档自动单列；`lineLimit(1)` 只允许等宽数据行。**明确禁止**用 `minimumScaleFactor` 压字换「不破版」。
改动：§3.1 type 行补充「D1 同批扫固定高容器」；§1 排版判定补「两列自动单列」。

**Q3 · 组件库一次建齐还是分两批？**
裁决：**两波。** 波一（高频、被最多表面用）：`DuckButton / DuckChip / DuckCard / DuckSheetHeader / DuckListRow`；波二（低频但补齐状态系统）：`DuckToggle / DuckSegmented / DuckProgress / DuckEmptyState / DuckErrorCard / DuckLoading`。准入棘轮：新代码必须用库；旧代码按屏幕批次迁移，不追求一天清零。
改动：§3.2 标注波次；§5 D2 拆 D2a/D2b。

**Q4 · 性能预算哪些能自动化？**
裁决：只有脚本能断言的进 CI——Android「轮询状态不在顶层大对象」「长列表 Lazy 化」用 grep/AST 断言；iOS `@Published` 计数用 grep 棘轮；sheet 首帧与掉帧率模拟器不可靠，**只记录不上门禁**，在 QA 清单标「未验证」。
改动：§3.7 补充「可自动/仅记录」两列口径。

**Q5 · 红线冲突**
裁决：成立但需硬门禁兜底：D1–D6 一律不碰 `core/**` 与渲染管线（shader uniform 除外）；加一条 CI：批次提交若触碰 `core/**` 必须显式豁免说明。无障碍改动只增语义，不改观测。
改动：§6 补一句。

**Q6 · 成本与总时长**
裁决：按近期节奏（每批 1 推 1 验证 + QA 复审），D1–D8 预计 5–7 个工作日当量；可压缩点：D4–D6 文件不重叠，允许三线并行；每批设 token 预算意识，先出可验证小步再扩面。**明确不做**：为凑「完美」反复重做已绿批次。
实测 CI：iOS 18m53s（历史 20–29 分钟）、Android 10m15s、Core 4m08s、Contracts 21s；iOS 是唯一关键路径。按 16–18 推估，CI 墙钟 **7.5–9 小时**，可与本地施工并行；今晚 D1 的 3 推约 1.0–1.5 小时。
改动：§8 补「并行三线」与「不做返工凑数」。

### 9.2 评审补漏（底稿缺失的维度）

1. **图形语言**：图标尺寸/线宽/圆端规则未定——补 `icon.md`：24/28/32 三档、线宽 2、圆端；图标不得混用描边与实心同一排。
2. **首次启动**：引导/权限请求时机（相机、麦克风、语音）。补进 D5：首启不弹系统权限，用在用时再问；被拒给「去设置」出口。
3. **本地化**：复数与日期格式（周合计、日期行）用系统 locale，不许手拼；进 D6 验收。
4. **声音反馈**：已有真实嗓音系统，UI 动效与声音事件需对表（摔倒闷响、路径完成 coo），D3 一并定义「事件—声音—触感」三件套。

### 9.3 改判后仍成立的结论

- 八批主轴成立，顺序 D1→D2→D3→D4/D5/D6 并行→D7→D8 成立。
- 令牌零裸字面量目标成立（分母已实测：iOS 207 / Android 143）。
- 「中文不圆」已拍板，文档写明即可，不再阻塞。

### 9.4 开工令

评审通过。**D1 立即开工**（按 9.1 Q1 分包、Q2 名单同批修）。评审结论已入本文；施工中发现新反例，回填本节并改正文，再继续。

### 9.5 老板七条天花板口径 → 门禁落点（逐条可判）

| # | 口径 | 落点 | 可脚本验收（缺一不算过） |
|---|---|---|---|
| 1 | 组件清单 + 双端状态矩阵 + 令牌引用 + 禁裸字面量 | §3.2、D2 | `contracts/ui-components.json` 冻结 11 组件 × 状态（默认/按下/禁用/选中/聚焦/错误）；`check-ui-components.py` 断言清单完整、每组件有状态定义、页面引用率；`check-ui-baseline.py` 主题外色/字号/间距/圆角/时长裸值 = 0 |
| 2 | 动态字体无截断、WCAG AA、VoiceOver/TalkBack 语义与顺序、减动效全局 | D1（首屏）+ D4（全表） | XCUITest 以 `UICTContentSizeCategoryAccessibilityXXXL` 启动断言首屏/五页元素存在且不出屏；Android 冒烟 `font_scale 2.0` + uiautomator；`check-contrast.py` 复算全部前景/背景组合；a11y 棘轮（custom actions/announcements/focus）只升不降；reduce-motion 用例 |
| 3 | 动效令牌、可打断、≤1 帧抖动、列表统一编排 | D3 | `grep` 主题外内联时长/曲线 = 0；`motion.*` 四档；减动效用例；列表过渡统一组件 |
| 4 | 同一 IA、明确宽度断点、横屏不是另一套 App | D6 | 断点表 `contracts/adaptive.json`（<600 / 600–900 / ≥900）；两端同一 `DuckRoute`；横屏菜单与竖屏抽屉同序断言 |
| 5 | 性能预算可测、CI 能盯的进门禁 | D7 | 可自动：重组计数日志上限、非 Lazy 长列 = 0、`@Published` 棘轮、sheet 打开计时记录；不可自动：真机帧率/内存/GPU 标「未验证」入 QA |
| 6 | 同名功能同名入口同顺序；平台差异进 exceptions 并写理由 | D6 | `features.yaml` 两端一一对应；`copy-exceptions.json` 的 `platform_facts` 冻结；`contracts/platform-differences.json` 每条含 reason |
| 7 | 每批验收可脚本化 | 每批 | 每批在 §5 与 REPORT.md 写「命令 + 退出码/断言」，截图只作辅证 |

### 9.6 D1 施工口径（令牌映射与门禁变更，先行落库）

- **新增令牌**
  - iOS `Design/DuckTheme.swift`：`DuckType.display/title/body/label/caption/data/micro`（语义字体，见 §3.1 映射）、`DuckSpace.xxs…xxxl`（2/4/8/12/16/20/24/32）、`DuckRadius.thumb/control/card/panel`（12/14/18/24）、`DuckIcon.xs/sm/md/lg`（12/15/18/22，不随动态字体缩放）。
  - Android `DuckTheme.kt`：`Type.Micro/Caption/Label/Body/Title/Display`（10/11/13/15/20/28 sp）、`Space`、`Radius`、`Icon`（Dp）。
- **门禁变更（同一提交）**
  - `check-ui-baseline.py`：计数时排除 `ios/DuckApp/Design/DuckTheme.swift` 与 `DuckTheme.kt`（令牌文件允许持值）；新增 `ios_offgrid_spacing` / `android_offgrid_spacing`（max，只统计 `padding/spacedBy/Spacer` 的非 4 基数调用）；`ios_font_literals`、`android_font_literals`、`ios_missing_rounded` 收口后基线重定为 0。
  - 新增 `scripts/check-ui-scope.py`（D0）：UI 批次改动路径白名单，碰 `core/**`、`upstream/**`、`patches/**` 即红（豁免需书面说明）。
- **顺序**：令牌 → Android 全量替换（本机 `compileDebugKotlin testDebugUnitTest` + 七门禁）→ iOS 大文件组 → iOS 小文件组。每推四工作流全绿才继续。
- **D1 动态字体先修**：`ContentView.swift:752/777/788/655/636` 与 `Shell.kt:2185/2104` 改 `heightIn(min:)`/自适应；首屏 AX3 断言随 D1 一起进 CI。