# UI/UX 重构整合 · 会议纪要（2026-11-15）

> 会议：阿衡主持（统筹/收口）· 与会：小满（产品兼设计）、小鹿（双端前端）、老周（架构兼后端）、小盯（测试兼 CI）
> 基线：`2458dec`（三工作流全绿，会议从绿基线起跑）
> 会议材料：七路读码单（会前铺底，每条带 `文件:行号` 证据）→ 四人 12 条方向发散 → 四镜头对抗评审（功能保全/双端成本/红线冲突/可测性）→ 阿衡综合拍板
> 红线（全程未触碰）：61 维观测与安全层不动 · 渲染只动 shader uniform/SwiftUI/Compose 层 · 离线优先 · 事实不编造 · 核心进程不上网 · 产品只有 iOS+Android

---

## 一、一页结论

**这版 App 的「乱」已经换型了：11-12 审计时的「调试面板脸」治好了六成，但 11-14 起的七批功能洪峰把 B4 拍板的 IA 预算冲爆，视觉 token 债务（B2）一环未动，两端入口已经分化。**

| 面 | 一句话 |
|---|---|
| 信息架构 | B4 抽屉预算「4 组 × ≤4 行」被击穿：iOS 25 行（单组 16 行）；11 项新功能 iOS 进抽屉、Android 塞仪表，两端入口分化 |
| 导航 | 弹层状态机裂成（iOS）三棵树 /（Android）14 个散布布尔；仪表内还能叠两层 sheet；技能环按返回直接退出应用 |
| 页面组织 | `Shell.kt` 4078 行独揽全 UI 37.5%，网络解析住在 UI 文件；`ContentView.swift` 1919 行管 18 项职责 |
| 视觉令牌 | B2 从未落地：`DuckTheme.swift/.kt` 不存在；固定字号不降反增（iOS 171→193、Android 105→143） |
| 交互模式 | 长按劫持、摸鸭固定矩形、按压零反馈、无障碍替身 0 处——审计 P1 一批原样 |
| 双端一致性 | Copy 键集 264=264 有 CI 断言（✅）；但同名 key **值**漂移 ≥9 处无守护；成长手册/成就墙 iOS 独有且数据链不同 |
| 功能完整性 | 三个实 bug：iOS DKPD 发送端从未上屏、first_gift 成就永不解锁、ensemble 无邻居整个消失；日子行 iOS 加载不渲染 |

**会议结论（阿衡拍板）：走「护栏契约先行 → 双端止血 → DuckTheme 色批 → 单路由合一 → IntentClient 抽取」主轴（B1–B8 批次表见 §5）；IA 4×4 重排搁置待拍板，本会只吸收其台账 schema。今晚开工 B1（零 UI 护栏契约网）。**

---

## 二、现状记分牌（会议材料，证据留痕）

> 七路读码：iOS 结构 / Android 结构 / 视觉 token / Copy parity / 双端入口地图 / 文档对账 + 完备性批判。数字口径全部经阿衡 grep/读码复核；对抗镜头又纠出三处，已在下文订正（标 ⚑）。

### 2.1 信息架构

| # | 发现 | 证据 | 级别 |
|---|---|---|---|
| IA-1 | B4 抽屉预算被击穿：iOS 抽屉 **25 行**（玩4/一起1/记录4/这只鸭子16）；预算「4 组×≤4 行」由代码文件头注释自认，同文件即违例 | `MoreSheet.swift:9-18`（注释）、`:59-258`；复核 `grep -c 'id: "more-'` = 25 | P0 |
| IA-2 | 11 项新功能两端入口分化：iOS 放抽屉、Android 全塞「这只鸭子」仪表 sheet（心跳/家长时长/房间灯/罗盘/备份/ToF/AB/推把/倒放/HUD/双人接机） | iOS `MoreSheet.swift:145-257` vs Android `Shell.kt:3392-3680`；B4 §0 要求「一一对应」 | P0 |
| IA-3 | 8 项功能超 ≤2 跳（合唱/礼物/配对码/DKPD 收发/ensemble/LED/定时器），藏在邻居页或 sheet 内层 | iOS `PeersSheet.swift:100-168`、`DanceSheet.swift:16-17`、`RulesSheet.swift:140-162`；B4 §2 要求全部 ≤2 | P1 |
| IA-4 | Android `ensemble` 在无邻居分支内整个消失（iOS 是禁用态常显）——**无邻居是常态，等于默认态丢功能** ⚑ 功能保全镜头升级为第三个实 bug | `Shell.kt:1401-1501` vs iOS `PeersSheet.swift:135-153` | P0 |
| IA-5 | 横屏是另一套外壳/菜单（iOS 横屏底栏 10 可点 + ⋯ 菜单 13 项；Android ⋯ 菜单 12 项），与竖屏抽屉不同构 | `ContentView.swift:1293-1535`；`Shell.kt:802-830,1863-1919` | P1 |
| IA-6 | 旧入口与抽屉并存：仪表内仍有日记/明信片/图鉴/AI/语言/晚安/手电筒第二套按钮；手电筒 iOS 4 入口、语言 3 入口 | `InstrumentSheet.swift:40-103,274-296,371-376`；`ContentView.swift:1320,1337,1466` | P2 |
| IA-7 | 「每日任务/进度」同名功能双端数据链不同：iOS=成长手册（本机 ProgressStore + 7 日瓦片 + 成就墙），Android=3 任务 + 连续天数。**B4 §7 明文「只改入口不改账」——重排不得顺手统一数据** ⚑ 红线镜头 | `ProgressKit.swift:413-600` vs `Shell.kt:2851-2933`、`MoreSheet.kt:206-235` | P1 |

### 2.2 导航结构

| # | 发现 | 证据 | 级别 |
|---|---|---|---|
| NV-1 | iOS 弹层状态机裂成三棵树：ContentView `Modal` 枚举（8 case）+ MoreSheet `Destination` 枚举（12 case，重复注册 rules/dance/peers/ai 等）+ InstrumentSheet 4 散 bool；⚑ **订正：ContentView.swift:210-234 的 5 个散 bool 不是「僵尸」——是在用 onChange 适配层**（showInstrument 写入点 `:687,:1388`，showRules/showPeers/showAI/showDance 写入点 `:1344-1362,:1514`，是抽屉外行打开 sheet 的唯一机制），删掉会丢五条入口 | `ContentView.swift:48-71,207-234`；`MoreSheet.swift:32-51,266-334`；`InstrumentSheet.swift:21-27` | P1 |
| NV-2 | Android 散布 show* 布尔**实测 14 个**（Shell.kt 12 顶层 + `:3094` showDetails + `MoreSheet.kt:85` showModes；摸底简报的 17 有误 ⚑）；无状态机；仪表内 3 钮只 set 不关 showSheet → 可叠两层 sheet（审计 P1-20 原样） | `Shell.kt:987-990,128-146,3094,3393-3395`；`MoreSheet.kt:85` | P1 |
| NV-3 | Android `rememberSaveable` 全仓 0：进程死亡/uiMode 变更丢 Shell 82 处 remember + 整载 core；旋转靠 manifest configChanges 侥幸不重建 | `grep rememberSaveable` = 0；`AndroidManifest.xml:40,42` | P1 |
| NV-4 | Android `BackHandler` 全仓 0：技能环是普通 Box 遮罩，环开着按系统返回退出应用；现有 smoke 的 3 处 KEYCODE_BACK 是清理动作，**没有任何断言「返回关最上层且不退应用」** ⚑ 可测性镜头 | `grep BackHandler` = 0；`Shell.kt:920,2713-2717`；`smoke-test.sh:1152,1325,1393` | P1 |
| NV-5 | `Shell.kt` 4078 行独揽全 UI 37.5%：12 类职责，HTTP 网络解析住在 UI 文件（8 处 `127.0.0.1:8765` URL()/HttpURLConnection/org.json）；80+ composable、82 mutableStateOf、13 裸 Thread | `wc -l`；`Shell.kt:177-221,2447-2527,3879-4070` | P1 |

### 2.3 页面组织（两端）

| # | 发现 | 证据 | 级别 |
|---|---|---|---|
| PG-1 | `DuckController.swift` 2816 行/107 @Published，同持 DanceStudio/PostcardShelf/PadSender/相机/AI 客户端/埋点 | `DuckController.swift:12-17,1503`；`grep -c @Published` = 107 | P1 |
| PG-2 | iOS 时间轨占比公式在 ContentView 与 InstrumentSheet 各写一份且已漂移（一处夹取一处不夹）→ 同一进度条两处读数不一 | `ContentView.swift:777-782` vs `InstrumentSheet.swift:412-415` | P1 |
| PG-3 | Android 仪表 sheet 一块滚动列塞 ~30 控件（普通 Column 非 LazyColumn）；审计「12 功能挤一屏」恶化成 30 | `Shell.kt:3224-3695` | P2 |
| PG-4 | 语言切换靠手动 `languageTick &+= 1`，新 sheet 忘接就固化旧语言（ProgressSheet 即受害者） | `ContentView.swift:85,105`；`InstrumentSheet.swift:124`；`MoreSheet.swift:55` | P2 |
| PG-5 | ⚑ 订正：Android 关闭钮**已共享**（`Shell.kt:2230` CloseChip，14 处调用）；「15 份手抄关闭钮」只在 iOS 成立（15 个 .swift 文件各抄一份 48pt xmark） | `Shell.kt:2230`；iOS `grep '"xmark"'` = 15 文件 | P2 |

### 2.4 视觉令牌（B2 从未落地）

| # | 发现 | 证据 | 级别 |
|---|---|---|---|
| TK-1 | `UI_TOKENS §7` 指定的 `ios/DuckApp/Design/DuckTheme.swift` 与 `android/.../microduck/DuckTheme.kt` **不存在**；token 以 3 个功能文件的私有 Color 扩展散落（私有扩展同名共存不报编译错，漏替换也绿——需独立断言 ⚑），注释自认「until B2 grows」 | `Glob Design/**` = 0；`ProgressKit.swift:397-409`；`MoreSheet.swift:505-513`；`MoreSheet.kt:48-50` | P0 |
| TK-2 | iOS `.system(size:)` 171→**193** 处（14 档字号）；⚑ 订正：缺 `design:.rounded` 实测 **64** 处（简报 39 有误）；语义字体仅 ContentView，动态字体在其余界面文件失效 | `Grep count` 193；缺 rounded 64 | P0 |
| TK-3 | Android `fontSize` 105→**143** 处（11 档、9.sp 仍 3 处）；`MaterialTheme.colorScheme/typography` 全 App 0 引用 | `Grep count` 143、typography 0 | P0 |
| TK-4 | `.preferredColorScheme(.dark)` 10→**16** 处、project.yml 有 `GENERATE_INFOPLIST_FILE: YES` 但无 `UIUserInterfaceStyle`；⚑ Android `MainActivity.kt:128` 已有 `MaterialTheme(darkColorScheme())` 空参壳 | `Grep count` 16；`project.yml:48-76`；`MainActivity.kt:128` | P1 |
| TK-5 | 强调色 9 值换血后仍在：残留 4FC3F7×6/80DEEA×3/FFB74D×3/FFD54F + iOS 系统色 31 处；同一「选中态」iOS cyan vs Android #FF8A3D——**§2.5 已禁系统蓝、§7 已定 accent.action=#FF8A3D，选中态统一到 #FF8A3D 不是开放设计题** ⚑ 红线镜头 | `Shell.kt:1274,1279,1426`；`ContentView.swift:840` vs `Shell.kt:2171` | P1 |
| TK-6 | 动效 0 token（iOS 27 处内联 4 时长×3 曲线、Android AnimatedVisibility 2 处）；减动效 iOS 1 处/Android 0；触感全 App 2 处 | `Grep count` withAnimation 27；`Shell.kt:562,881` | P1 |
| TK-7 | 圆角 iOS 8 档/Android 10 档；拇指台外壳两端漂移（高 82 vs 90、横 padding 10 vs 6、间距 8 vs 4） | `cornerRadius` 26 处；`RoundedCornerShape` 23 处；`ContentView.swift:812-813` vs `Shell.kt:2081-2085` | P1 |
| TK-8 | Android 无相机合成仍是冷蓝渐变 `(14,19,26)→(23,28,36)`（非 Live 暖房一套）；Director 接触阴影盘未实现 | `PhotoCapture.kt:38-46`；`Renderer.swift:447-455,534` | P1 |
| TK-9 | brand 资产缺口：无 SVG 源、无 colors.json、无 mipmap-anydpi-v26；`ic_launcher_round` 5 密度与方形图 md5 相同全是假的；iOS AppIcon 无变体 | `ls brand/`=7 PNG；`md5sum` 5 密度相同 | P1 |

### 2.5 文案（Copy）

| # | 发现 | 证据 | 级别 |
|---|---|---|---|
| CP-1 | Copy 键集 **264=264** 且 CI 有断言（`scripts/check-copy-keys.py` 挂两工作流）——B4 §6.4 达成 | 本地实跑；`ios.yml:145-148`、`android.yml:34-37` | ✅ |
| CP-2 | 同名 key **值**漂移 **≥9 处**无守护：waking「叫醒 vs 唤醒」、faceStatus「健康 vs 正常」（switch 型函数，现有脚本根本解析不到 ⚑）、rewindPlay/netPad/roomLampNote（平台事实差，需拍板）/flashlight/heartRowNote/aiNote/abNote/backupReady；`work/cmp_copy_values.py` 未提交、无退出码、6 条插值误报 | 实测 15 条 diff（6 误报+9 真实）；`Copy.swift:161-169` vs `Copy.kt:547-553` | P1 |
| CP-3 | iOS `ProgressKit.swift` 成长手册 31 行中文字面量绕 Copy（英文系统显示中文）；Android 无成长手册/成就墙页 | `ProgressKit.swift:452,466,482,486,501,506,556,590,618`；Android `grep 成就` = 0 | P1 |
| CP-4 | 8 个 Android 开关复用「开/关」文案（`Copy.flashlightOn/Off` 被网控手柄/心跳/房间灯/罗盘/倒放/AB/HUD/双人接机 8 处共用），TalkBack 无法区分对象；⚑ `:3518` 是 flashlightOff vs roomLamp 非对称复用，键要按功能设计 | `Shell.kt:1388,3479,3518,3544,3627,3666,3673,3677` | P2 |
| CP-5 | 新增 Copy 键必须**两端同推**（check-copy-keys 比集合相等），单端加键当天红——任何文案批次都要成对推 ⚑ 成本镜头 | `scripts/check-copy-keys.py:65-72` | 制约 |

### 2.6 交互模式与无障碍

| # | 发现 | 证据 | 级别 |
|---|---|---|---|
| IX-1 | 长按 0.35s 劫持未修（path 模式按住照样弹技能环）；松手判定改法未采纳 | `ContentView.swift:131-135`；`Shell.kt:2294-2300` | P1 |
| IX-2 | 摸鸭热区仍是写死矩形 x25-75%/y33-68%（鸭子走远摸不到）；Renderer 投影方案未实施 | `ContentView.swift:977-980`；`Shell.kt:2378-2379` | P1 |
| IX-3 | 无障碍替身 0：customActions/liveRegion/`UIAccessibility.post` 双端 0 命中；TalkBack 用户走不了第一步；新功能又加了 7 个视觉 onChange 通知 | 双端 `Grep` 0；`ContentView.swift:265-357` | P1 |
| IX-4 | iOS 技能环 transition 死代码（三处 showRing 均无 withAnimation）；⚑ Android `indication=null` 实测 **13** 处按压零反馈（简报 14 有误） | `ContentView.swift:131-135,795,1402,1739-1743`；grep indication=null 13 | P1 |
| IX-5 | 残触控/对比度尾巴：iOS 明信片分享钮 44×36；iOS .tertiary 3 处、DexSheet 0.4alpha/9sp；仪表 scrubber accent 60% | `PostcardShelfView.swift:68`；`AISheet.swift:104`；`DexSheet.kt:108`；`InstrumentSheet.swift:421` | P2 |

### 2.7 功能完整性（读码单揭出的实 bug）

| # | 发现 | 证据 | 级别 |
|---|---|---|---|
| FE-1 | **iOS DKPD 发送端从未上屏**：pad-send chip 嵌在「逗邻居笑」action 闭包里只构造不渲染（`107-113` 构造后丢弃、`114` 才是真调用）；`togglePad` 无任何 UI 入口，`PadSender.swift` 101 行成孤岛——台账「#11 发送端 iOS ⏳」**属实**（编译能过 ≠ UI 可达；且 chip 自带合法 id，**源码级 tag 断言对死链永远绿**——tag 断言必须取运行时层级 dump ⚑） | `PeersSheet.swift:103-115`；`PadSender.swift:15`；`DuckController.swift:1506` | P0 |
| FE-2 | **iOS first_gift 成就永不解锁**：`ProgressStore.record(.peer)` 全仓零调用（唯一命中是 `ProgressKit.swift:34-38` 注释）；⚑ 正确挂点是 `gift_event` 成功分支（`DuckController.swift:2168-2185`），`giftPeer`（`:1563-1565`）是 fire-and-forget——**在点击处记账会把核心拒绝/节流也记成成就 = 编造事实**；iOS 无 unit-test target（project.yml 只有 DuckUITests），接线前需先建 | `grep record(.peer` 仅注释；`ProgressKit.swift:373` 是成就定义不是路径 | P0 |
| FE-3 | iOS 日子行数据加载了不渲染（`vetbookDays` 仅 `:1473,:2217` 两处、无视图消费）；Android 已渲染。⚑ 是**渲染缺失**不是数据链分歧——修渲染，不统一数据 | `DuckController.swift:1473,2217` vs `Shell.kt:3049-3055` | P1 |
| FE-4 | 地面高度 POST /floor、轨迹导出 GET /export/trajectory 只有核心路由，两端 UI 树 0 命中——**七批台账口径为「已落地」，实为 no-ui**；功能台账须按 22+2 起账（+2 就是这两项） ⚑ 功能保全镜头 | `core/.../intent.rs:1067,1083`；两 UI 树 `Grep` 0 | P2 |
| FE-5 | 顶部每日任务「火苗」死分支：`topBar(showProgress:)` 唯一调用不传参（默认 false），永不渲染 | `ContentView.swift:1148,1277-1292` | P2 |

### 2.8 测试与验收（批判员补的盲区）

| # | 发现 | 证据 | 级别 |
|---|---|---|---|
| TQ-1 | iOS 仅 7 个 XCUITest 全部针对七批之前的功能骨架；⚑ Android `androidTest` 目录不存在、无 androidx.test 依赖、CI 无 connectedAndroidTest；JVM 单测 2 文件 **6** 用例；七批新功能两端零 UI 断言；iOS 无 unit-test target | `DuckUITests.swift` 7 用例；`app/build.gradle.kts` 仅 junit | P1 |
| TQ-2 | 性能零基线：107 @Published、Android 5Hz 轮询过 4078 行组件树整树重组、仪表 30 控件墙非 LazyColumn；低配机表现未知 | `Shell.kt:1986-1989,3109-3114,3229` | P2 |
| TQ-3 | 空态/错误态系统覆盖缺：Copy 字典错误/离线词仅 2 个 key（retry/aiOffline）；Android 失败反馈 5 处 Toast 无内联错误态 | `Copy.swift:61,614`；`Copy.kt:151,449`；`Shell.kt:1542,1615,1620,1626` | P2 |
| TQ-4 | dist 截图现货 17 张全是 11-15 02:00（早于 HEAD 20:23）且由 `[skip ci]` 提交——断言挂截图步骤覆盖不到该类提交，需 workflow_dispatch 补跑 ⚑ | `ls dist/`；`android.yml:289-317` | P2 |
| TQ-5 | **CI 触发盲区** ⚑：三工作流全是 push+workflow_dispatch、无 pull_request；`ios.yml:5-11`/`android.yml:5-15` 的 paths **不含 scripts/contracts/docs**——纯脚本/契约改动两个 App 工作流根本不跑，必须新建独立轻量 contracts.yml（ubuntu + workflow_dispatch）并显式挂进两工作流 | `.github/workflows/ios.yml:5-11`、`android.yml:5-15`、`core.yml:16-22` | 制约 |
| TQ-6 | 现有护栏覆盖矩阵 ⚑：Copy 键集（key 非 value）；首屏 ≤7（只数 Live）；iOS 7 XCUITest；Android smoke 主要是 8765 intent 会话 + 首屏文本/≤7/抽屉 17 行文本；`check-frame.py` 只判空屏/两帧变化，**不是金色截图、无行数/OCR**——「截图验组数」做不到 | `DuckUITests.swift:410-413`；`smoke-test.sh:1210,1268-1305`；`check-frame.py` 头注释 | 制约 |

---

## 三、发散方向（四人 12 条，会后按对抗结论归四簇）

> 原始产出 12 条（每人 3 条，见会议工作流存档 `wf_76aac47a-f4d/journal.jsonl`）。对抗镜头判定：**12 条实为 4 个工程簇**，同簇必须收敛为一个 owner + 一套文件名，否则 CI 互相打架。

| 簇 | 原始方向（谁提的） | 一句话主张 |
|---|---|---|
| **A · 护栏/契约** | 老周「契约先行·冻结外壳」、小盯「护栏网先行」（吸收小鹿「入口契约重排」的 23 行全账 + 老周「注册表驱动重排」的 enabled-when schema） | 零 UI 改动，四层静态断言（Copy 值 / token 棘轮 / 入口台账 / 网络白名单）+ 独立 contracts.yml——所有重排的第一推 |
| **B · 止血** | 小满「止血式机械收敛」+ 小盯「导航收拢为一路」的三个断点接线 | 接回断掉的功能（pad-send/first_gift/ensemble/日子行）+ 8 开关对象名 + 文案值归一 + DuckTheme 13 色——不挪一个入口 |
| **C · 单路由** | 小鹿「路由树合一」（骨架）+ 小盯 reducer/整批纪律 + 老周横屏同表；小满「单路由重组」被否（见 §4） | iOS 三棵树 + Android 14 布尔收成一张路由表：adapter 先转发后删旧、overlay/sheet 两字段、纯 Kotlin reducer + JVM 单测 |
| **D · IA 4×4 重排** | 小满「动线契约重排」+ 小鹿「入口契约重排」+ 老周「注册表驱动重排」的搬运部分 | 4 组 × ≤4 行首层预算 + 四目的页 + 22+2 功能 ≤2 跳——**全部搁置待拍板**（见 §5） |

各方向完整版（收益/代价/风险/功能保全/CI 计划）存会议工作流 journal，要点已并入 §5 批次表。

---

## 四、对抗评审与查重（四镜头）

### 4.1 裁决汇总

| 方向（簇） | 功能保全 | 双端成本 | 红线冲突 | 可测性 | 结论 |
|---|---|---|---|---|---|
| A 护栏/契约 | 有条件推荐 | **推荐**（全仓最便宜、一次推送可验） | **推荐**（唯一零 UI 回归方向） | **推荐**（Windows 上唯一分钟级判绿） | ✅ **采纳为 B1** |
| B 止血 | 有条件→推荐（三断点全接线） | 有条件（拆单端单推） | 有条件（first_gift 挂点、Copy exceptions） | **推荐**（条件：先建 fixture/unit target） | ✅ **采纳为 B2–B6** |
| C 单路由 | 有条件（别删适配层） | 有条件（5-6 推） | 有条件（overlay/sheet 分字段） | 有条件（纯 reducer JVM 测试是全场最优牌） | ✅ **采纳为 B7** |
| D IA 重排 | 有条件 | 否（10-12 推、规格未决、必炸现有断言） | 否/搁置（16 行 vs 22+2 数学冲突、§5 无权威、三套互斥分组） | 否（grep 证不了 ≤2 跳） | ⏸ **搁置待拍板**（只吸收台账 schema） |

### 4.2 对抗镜头的十大纠错（会议产出，后续施工必须遵守）

1. **「5 个僵尸 bool」是误判**：`ContentView.swift:210-234` 是在用 onChange 适配层（写入点 `:687/:1344-1362/:1388/:1514`），删掉丢五条入口。路由重构必须「adapter 先转发、后删旧」，不许按「僵尸」清理。
2. **计数基线一律脚本现场算，禁抄简报**：show* 14（非 17）、Android distinct testTag 67 次/53 唯一（非 96）、JVM 6 用例（非 5）、indication=null 13（非 14）、缺 rounded 64（非 39）。
3. **first_gift 的记账挂点只有一处合法**：`gift_event` 成功分支（`DuckController.swift:2168-2185`）；`giftPeer` 是 fire-and-forget，点击处记账 = 把核心拒绝记成成就 = 编造事实。接线前先建 iOS unit-test target + 成功回执路径。
4. **pad-send 的 UI 断言需要 DEBUG 邻居 fixture**：chip 仅在 `peer.paired && !addr.isEmpty` 时渲染，模拟器单机无邻居、现有 launch arg 只有 `-duck.openMore`；Android smoke 已有 UDP 假邻居播种（`smoke-test.sh:456-591`）可借鉴。
5. **源码级 tag 断言对死链永远绿**（pad-send 带合法 id 却不入树）：tag 断言必须取运行时层级 dump（XCUITest/uiautomator），源码 grep 只当辅助。
6. **「core.yml 有 61 维/离线断言」不存在**（`core.yml:71-91` 仅 cargo test）——任何方案不许宣称有这个门禁；要加须新写并挂对工作流。
7. **Copy 值归一必须带 exceptions**：`roomLampNote/netPad/rewindPlay` 是平台事实差，自动归一会让一端说假话；插值 `\(x)` vs `$x` 必须归一否则 6 条误报天天红；faceStatus 是 switch 型函数，现有正则根本解析不到。
8. **新增 Copy 键两端同推**，否则 264=264 键集断言当天红；口径改说「键集相等」不说「保 264」。
9. **CI 触发盲区**：ios.yml/android.yml 不监听 scripts/contracts/docs；无 pull_request 触发——契约必须独立 contracts.yml（push+workflow_dispatch）+ 显式挂两工作流；`[skip ci]` 截图提交要 dispatch 补跑。
10. **B4 §5 不是重排的权威**：§5 只有 13 行旧映射，11 项新功能无行可依；三套互斥分组（定时器落玩组/鸭子组/待定）谁都不能自称「照 §5」——IA 重排必须回会议拍板，不能各拍各的。真正可引用的先例只有 §5:116「时间类控制定在时间栏」。

### 4.3 跨方向查重

- 12 条方向收敛为 4 簇（§3）；每簇一个 owner + 一套文件名：`DuckTheme`（色）、`DuckRoute`（路由）、`contracts/`（契约台账）、`features.yaml`（入口台账 schema）。
- 老周簇内冲突（HTTP 白名单不搬家 vs IntentClient 搬家）→ 排序消解：先白名单登记（B1），后搬家（B8）。
- 三份 DuckTheme 提案收敛到 UI_TOKENS §7 唯一路径；五份入口清单收敛成一份 fixture。

---

## 五、推荐方案与分批施工表（阿衡拍板）

**主轴：护栏契约(B1) → 双端止血(B2-B5) → DuckTheme 色批(B6) → 单路由合一(B7) → IntentClient 抽取(B8)；IA 4×4 搬运搁置待会议，先只保留台账。**

理由（五者平衡）：① 回归面单调升，每批踩在前一批棘轮上，绿基线随时可回退；② 盲改环境里只有纯 Python grep 与 JVM 单测能分钟级判活，路由逻辑抽纯 reducer 正是为此；③ 功能保全靠台账不靠承诺（22+2 起账、conform-b4 区分合规/违规态）；④ 事实红线决定接线方式（first_gift 只挂 gift_event）；⑤ 孩子动线最痛的三处（返回退应用/双层 sheet/ensemble 消失）用现状 bool 上 ~30 行热修先修，不等路由机。

### 分批施工表

| 批 | 内容 | 负责人 | 验收标准 | 依赖 |
|---|---|---|---|---|
| **B1 护栏契约网** | 零 UI：`work/cmp_copy_values.py` 修插值误报+退出码迁 `scripts/check-copy-values.py`；token/导航/入口台账（22+2 起账、conform-b4/not-implemented 字段），**棘轮基线由脚本在 2458dec 现算入库**；新建独立 `contracts.yml`（ubuntu、秒级、workflow_dispatch、监听 scripts+contracts+docs+ios+android）并挂 ios.yml/android.yml | 小盯 | contracts.yml 全绿；各脚本本机 python3 退出码可跑；基线数字全部代码现算（show*=14、testTag=67、indication=13、193、143、16） | 可立即开工 |
| **B2 Android 导航热修** | 现状 bool 上约 30 行：BackHandler 先关技能环再关 sheet 再退栈；仪表 3 钮先关 showSheet 再开目标；ensemble 无邻居改禁用常显（移出 `Shell.kt:1401` isEmpty 分支）；smoke-test.sh 加返回键序列 | 小鹿 | smoke 新增：环开+BACK 后 app 存活且环关；uiautomator 同刻仅一层浮层；无邻居 ensemble chip 存在且 disabled；show* 计数不升 | B1 |
| **B3 iOS 止血：pad-send+日子行** | pad-send chip 移出 gift-emotion action 闭包（`PeersSheet.swift:103-115`，giftPeer 原调用保留）；vetbookDays 补视图消费；先加 DEBUG 邻居 fixture 再写断言 | 小鹿 | XCUITest 新增：pad-send 与 gift-emotion 双 tag 可见可点、日子行行存在；gift-emotion 原触发不回归 | B1 |
| **B4 first_gift 接线** | 在 `gift_event` 成功分支（`:2168-2185`）record(.peer) 一次，**严禁点击点记账**；新增 iOS unit-test target 验 ProgressStore/Achievement 解锁链 | 小鹿 | record(.peer) 真调用计数 ≥1（排除注释）；单测：仅 gift 事件成功解锁 first_gift，postJSON 拒绝/节流不解锁 | B3（复用 fixture/target 基建） |
| **B5 文案归一+8 开关键** | 真漂移值归一；roomLampNote/rewindPlay/netPad 平台事实进 exceptions 待老板拍板，禁止自动归一；Shell.kt 八处 flashlightOn/Off 复用改独立对象名键，**新键两端同推** | 小满 | check-copy-values 绿且 allowlist 只缩不扩；check-copy-keys 集合相等；Shell.kt 八处 Copy.flashlight* 复用 grep=0 | B1 + 拍板平台事实键 |
| **B6 DuckTheme 色批 P0** | 按 UI_TOKENS §7 钉死路径建 `DuckTheme.swift/.kt` 落 13 色（RGBA 归一断言），选中态统一 #FF8A3D；project.yml 加 `INFOPLIST_KEY_UIUserInterfaceStyle: Dark`，**先加 dist 截图暗色像素守卫再删 16 处 preferredColorScheme**；PhotoCapture 无相机回退改暖石+光晕 | 小鹿 | check-ui-tokens 绿：两端同名同值、私有扩展=0、preferredColorScheme=0；暗色像素下限过；DanceSheet state.warn 未被替换 | B1 |
| **B7 单路由合一** | `DuckRoute.swift/.kt` + 纯 Kotlin ShellOverlayReducer + JVM 单测；两段式：先 adapter 转发（含在用 5 bool 适配层）再删旧状态；overlay/sheet 两字段；Saver 只存 id+照片路径；横屏菜单消费同一路由表；逐路由 XCUITest **先补后删** | 小鹿 | check-routes 绿：case 集=登记子集、点名 show* 清单=0、BackHandler/saveable ≥1、route-* 前缀；JVM reducer 测试绿；ai-open 等旧断言不红 | B1-B4 全绿 |
| **B8 IntentClient 抽取** | Shell.kt 八处 127.0.0.1:8765 URL()/HttpURLConnection/org.json **逐字搬出** UI 文件，零缓存零重试零语义变更；JVM JSON fixture 解析单测；PadSender UDP 与 AIBrain 用户端点不动 | 老周 | smoke 8765 全会话绿；Shell.kt 内 URL(/HttpURLConnection grep=0；fixture 单测证明搬迁前后解析结果同构 | B7（避免 Shell.kt 双改冲突） |

### 搁置/被否（记录原因，防止回锅）

| 项 | 处置 | 原因 |
|---|---|---|
| IA 4×4 重排搬运（三套分组方案） | ⏸ 搁置 | 16 行预算 vs 22+2 项数学冲突且无子页控件上限；§5 只有 13 行旧映射无权威；三套互斥分组互引失效；必炸 smoke needed 元组与 5 处 openMore 断言。**前置条件：老板/会议拍板 11 项新功能分组 + 每目的页控件上限**，之后按「契约先红、再搬入口」开工 |
| Android 补成长手册/成就墙页 | ❌ 本会否 | 是新产品不是重排：Android 无 ProgressStore 事件账，盲改环境新页有 FIDELITY §0.1 假数风险；另立产品批次 |
| 字体 7 档/圆角/动效批 | ⏸ 搁置 | UI_TOKENS §8 定字体 P1（卡中文圆体许可未拍板）动效 P2；193+143 处替换 ≈10-15 推，与一次推送一验证纪律相乘不可行；MaterialTheme.typography 牵动全部默认样式，有编舞对比度回归面。色批验收后再议 |
| 运行时读 JSON 渲染抽屉 | ❌ 否 | 新失败面：打包漏一处=空抽屉但编译绿；改为静态 fixture 对账 |
| 「顺手统一」每日任务数据链 | ❌ 否 | B4 §7 明文只改入口不改账 |

---

## 六、红线与不做清单（本次会议重申）

1. **鸭子的 61 维观测与安全层不因任何 UI 改动而变**；渲染层只许动 shader uniform / SwiftUI / Compose 层。
2. **离线优先**：本地存储、断网全功能可用、核心进程不上网；UI 层现有 HTTP 全部是 `127.0.0.1:8765` 回环（8 处，B8 搬家时逐字保持语义），AIBrain 的用户自填端点是已验收功能、显式 allowlist。
3. **事实不编造**：明信片/日记/标签只用真实计数；first_gift 只能挂 gift_event 成功分支；ToF 等无数据源项标 not-implemented，禁止渲染假数。
4. **七批新功能是产品已有资产，重构不得藏没、不得假删**（22+2 台账逐项去向）；产品只有 iOS+Android 两端。
5. 已验收的不回退：B1 状态人话、B3 舞台暖房/冷网格分家、明信片成片（iOS）、编舞对比度、首屏 ≤7 控件、Copy 键集 CI 断言、日光可注入（check-frame 不假红）。
6. **不许宣称不存在的门禁**（core.yml 无 61 维/离线断言）；新门禁须新写并挂对工作流。

---

## 七、会议留痕

| 谁 | 说了什么（浓缩） | 结果 |
|---|---|---|
| 阿衡 | 会议从绿基线 `2458dec` 起跑；先铺七路读码单再发散；要求每条方向给 20+ 功能去向与 CI 验证路径；四镜头对抗后拍板主轴 B1→B8，IA 搬运搁置回拍板 | 采用（本文 §5） |
| 小满 | ①「25 行抽屉不叫抽屉，30 控件墙不叫入口」——先止血接回真丢的功能和视觉底子，第二批做分组重排；②方向二（单路由）只是把墙修稳不是产品进步；③动线契约重排：4×4 是首层预算不是藏功能借口，家长工具与孩子玩点分开 | ①采纳为主轴止血批（B2-B6）；②其单路由独立案被否（声称补发送端却零动作触碰 PeersSheet、show* 计数错），骨架并入 B7；③搬运搁置待拍板，倒放落时间栏的直觉被记录为唯一可引先例 |
| 小鹿 | ①机械归位不换骨：DuckTheme 双端 13 色、SheetCloseButton/ToggleChip 组件化、分推可回退、诚实登记不硬修；②路由树合一：overlay/sheet 两字段、adapter 先转发后删旧、纯 reducer JVM 测试——12 方向里最稳的迁移设计；③入口契约重排：23 行全账+缺口诚实标 implemented:false | ①色批采纳（B6）；关闭钮组件化记入后续批次（Android CloseChip 已共享，只做 iOS 侧）；②采纳为 B7 骨架；③台账 schema 并入 B1，运行时 JSON 被否、搬运搁置 |
| 老周 | ①契约先行冻结外壳：零 UI 四层断言+独立轻量工作流，是全部方向的第一推；exceptions.yaml 显式化平台差异；②单路由机重组：横屏菜单复用竖屏同一路由表；IntentClient 把网络解析搬出 UI 文件（逐字搬家零语义变更）；③注册表驱动重排：features.yaml 的 enabled-when+数据链+not-implemented 是最值得当唯一事实源的 schema | ①与护栏网合并为 B1（四层断言+独立 contracts.yml）；②横屏同表并入 B7、IntentClient 单列为 B8（排 B7 后避免 Shell.kt 双改）；③schema 并入 B1 台账，其 Android 补新页/统一数据链部分被否（新产品+改账，红线） |
| 小盯 | ①护栏网先行：所有阈值由脚本在 2458dec 现算入库，棘轮只锁现值只降不升，先有断言再动刀；②三个断点（pad-send/first_gift/ensemble）是日常动线真伤，现状 bool 上 ~30 行热修即可，不必等路由机；③tag 断言必须取运行时层级 dump；受保护路径断言做成 CI；「PR 触碰核心即红」当前不可实现（无 pull_request 触发），按 push 实现并允许核心负责人显式豁免 | ①采纳为 B1 核心纪律（基线现算、棘轮只降不升）；②采纳为 B2-B4 止血批；③进 B1 台账设计；「受保护路径断言」列为 B1 的一部分（push 事件+豁免机制） |

**会中订正（对抗镜头纠错，全组认账）**：见 §4.2 十条。

---

## 八、开工节奏

- **今晚（本会话）**：B1 护栏契约网——小盯出脚本（本机 python3 全跑通、退出码非零即红），阿衡收口提交并推送，CI 轮询 ≤30s 一次，contracts.yml + 双端工作流全绿才算交付。UI 修复今晚一律不动（pad-send 无邻居 fixture，改了也无法证明修好——诚实优先）。
- **随后两批**（按 B1 绿灯放行）：B2 Android 导航热修（小鹿，单端单推）→ B3 iOS 止血（小鹿，先 fixture 后断言）。每批一次推送一次验证，失败归因单端化。
- **再后**：B4 first_gift（需先建 unit-test target）→ B5 文案归一（等老板拍板 roomLampNote/rewindPlay/netPad 三处平台事实差）→ B6 色批 → B7 路由 → B8 网络抽取。
- **回拍板项**（不阻塞 B1-B4）：IA 4×4 重排的 11 项新功能分组与每目的页控件上限；roomLampNote/rewindPlay/netPad 三键的平台事实差；中文圆体许可（字体批的前置）。
- 每批完成写 REPORT.md；全部批次完成标准=双端 + CI 全绿；发布前小盯一票否决。

---

## 九、回拍板（2026-11-16 老板已拍，三项全部落定）

1. **IA 4×4 重排 —— 已拍板：开工，已实现。** 分组取小满版骨架（三套方案里唯一引用 §5:116「时间类控制定在时间栏」的一版），按老板新约束「每目的页控件上限 ≤6」重算：首层 4 组 × 4 行 = 16 行；玩/一起/记录/这只鸭子四组；新设「玩一玩 / 鸭子的脸 / 工具」三个目的页，每页 ≤6 控件；倒放归时间栏；定时器留在积木规则页内。`contracts/features.yaml` 是唯一事实源，`scripts/check-ui-features.py` 守 4×4、≤6、台账、enabled-when 与两端一一对应——按「契约先红、再搬入口」执行（首跑红在 iOS 抽屉 25 行，搬运后转绿）。iOS 横屏 ⋯ 菜单改为与竖屏抽屉完全同序的四组，消费同一张路由表。
2. **平台事实差三键 —— 已拍板：各端说各的真话，进 exceptions，不许统一措辞。** `roomLampNote`（iOS 无照度 API）、`rewindPlay`（Rewind play vs Rewind）、`netPad`（UDP pad (port 4210) vs UDP pad (4210)）恢复各端真值；`contracts/copy-exceptions.json` 的 `platform_facts` 冻结为这三键，`check-copy-values.py` 断言集合相等——长要开会、缩要改值。
3. **中文圆体许可 —— 已拍板：暂不放包。** 字体档位收敛批继续搁置，等许可核实后再议；本轮不做任何字形替换。
