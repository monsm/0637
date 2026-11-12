# 视觉规范 v1 · MicroDuck 数字版

> 阿雅（UI/UX）· 2026-11-12 · 配套文档：`docs/UI_AUDIT_2026-11-12.md`、`docs/STAGE_LOOK.md`、`docs/UI_DIRECTIONS.md`

---

## 0. 先说现状：这套系统不存在

盘点结果（`audit/03-visual-system.md` 实测，两端共约 16,700 行 UI）：

| 类别 | 现状 |
|---|---|
| 颜色 token | **0**。iOS 全仓 `extension Color` / `enum Theme` / `Palette` **0 命中**；Android 无 `Color.kt` / `Theme.kt`，`MaterialTheme.colorScheme.*` 全 App **0 引用** |
| 字体 token | **0**。iOS 171 处 `.system(size:)`（12 档字号、40 个唯一字面量、41 处漏 `design: .rounded`）；Android 105 处 `fontSize = N.sp`（10 档），`MaterialTheme.typography` 0 引用 |
| 间距 / 圆角 | **0**。iOS 间距并集 16 个值（含 1/3/5/9/18 这种手调值）、圆角 7 个；Android 间距 14 个、圆角 8 个，两端同语义不对齐 |
| 阴影 / 层级 | iOS 全 App **1 个** `.shadow()`；Android **0 个**，层次全靠 `Color.Black.copy(alpha=0.35f)` 假造 |
| 动效 | iOS 25 处内联（4 时长 × 3 曲线，**0.2 同时配 easeIn 与 snappy**）；Android **0 token** |
| 深浅色 | iOS 10 处 `.preferredColorScheme(.dark)` 逐视图硬写；Android `darkColorScheme()` 零覆盖项 |
| 强调色 | **9 个值**抢同一个语义：`#4FC3F7` / `#80DEEA` / `#00FFFF` / iOS 系统 `accentColor` / `#0091FF` / `#FFD166` / `#FFB74D` / `#FFA726` / `#FFD54F` |
| 深色表面 | **7 个值**（iOS 3 + Android 4），同一个底色还有 3 种写法（`Color(red:)` / `Color(0xFF)` / `Color.rgb()`） |

**现在建 token 是"把散落的字面量归位"，不是"重构稳定的系统"。这是唯一一次能低成本做这件事的窗口。** 但不要一次建全套——全量 token 化要同时改两端 300+ 个调用点，而交互形态还在动，两件事叠在一起做，出了事分不清是谁的锅。

---

## 1. 原则

1. **鸭子是主角**：任何时刻屏幕上 ≥ 70% 的像素属于鸭子与它脚下的地面。
2. **两种视觉语言**：Live 暖、软、几乎没有数字；Instrument 冷、密、有数字。仪表盘的语气不许漏进 Live。
3. **数字只在两种情况下出现**：时间机器（那是刻度）与状态摘要（那是事实）。其他地方给姿态、给颜色、给动作。
4. **一个语义一个 token**。现在同一个意思有 N 个值，这是最大的债。
5. **只做深色**。3D 场景是暗的、相机取景器是暗的，强光模式会让鸭子失真。别浪费时间做浅色。

---

## 2. 颜色

### 2.1 舞台（渲染层，不是 UI 层）

见 `docs/STAGE_LOOK.md`。UI token 表里只留两个引用：

| token | 值 | 用处 |
|---|---|---|
| `stage.floor.live` | `#38322C` | Live 地面 albedo（暖石，粗糙度 ≥ 0.9，不画格） |
| `stage.floor.instrument` | `#24262B` | Instrument 地面（冷灰）+ 0.2 m 冷蓝棋盘 |

### 2.2 UI 层

| token | 值 | 用处 |
|---|---|---|
| `bg.root` | `#161513` | 面板底（暖黑，不是冷蓝黑） |
| `bg.surface` | `#221F1D` | 卡片 / 浮层 / sheet |
| `bg.scrim` | `#000000` @ 55% | 遮罩 |
| `text.primary` | `#F6F1E8` | 主文字（暖白，不用纯白） |
| `text.secondary` | `#F6F1E8` @ 70% | 次文字 |
| `text.tertiary` | `#F6F1E8` @ 50% | 三级 / 禁用 / 说明 |
| `text.onAccent` | `#1A1208` | 橘底上的字 |
| `accent.action` | `#FF8A3D` | 主行动（跟鸭子喙同源） |
| `accent.data` | `#7FB2D9` | 数据 / 刻度 / 仪表（冷） |
| `accent.info` | `#B08FD9` | 时间机器 / 回放 |
| `state.good` | `#86D6A4` | 状态好 |
| `state.warn` | `#E8B45E` | 状态警告 |
| `state.bad` | `#E8836B` | 状态坏 |
| `overlay.hint` | `#FFFFFF` @ 12% | 需要透明时的中性高亮（**下限 12%，且只用于图形，不能用于文字**） |

> `text.tertiary` 定在 **50%** 而不是 45%：45% 在 `bg.root` 上只有 4.14:1，达不到正文 4.5:1；50% 是 4.82:1。

### 2.3 品牌橙有三个值，三者关系必须写死

| token | 值 | 归属 | 说明 |
|---|---|---|---|
| `brand.duck.orange` | `#FF671F` | 3D 材质（MJCF 物理材质，**不可改**） | `robot_groundcontact_backlash.xml:403-440` |
| `accent.action` | `#FF8A3D` | UI | 比 3D 亮一档，深底上更清楚；**它是 UI 里的"品牌橙"** |
| 图标喙 / 脚 | `#FF671F` | brand/ 资产 | 图标必须用 3D 那个橙，否则"图标和 App 里不是同一只鸭" |

同理舞台底色要统一：App 的舞台底 `#0E131A` 与图标底 `#10141C→#191F2D` 现在是两个值，改完舞台后两端都用 `brand.canvas`（见 `docs/STAGE_LOOK.md`）。

### 2.4 对比度（已按 WCAG 2.1 复算）

| 组合 | 比值 | 判定 |
|---|---|---|
| `text.primary` on `bg.root` | 16.4 : 1 | ✅ |
| `text.secondary` @70% on `bg.root` | 8.45 : 1 | ✅ |
| `text.tertiary` @50% on `bg.root` | 4.82 : 1 | ✅ |
| `accent.action` on `bg.root` | 7.88 : 1 | ✅ |
| `text.onAccent` on `accent.action` | 7.88 : 1 | ✅ |
| `accent.data` on `bg.root` | 8.15 : 1 | ✅ |
| `brand.duck.orange` on `brand.canvas` | 6.46 : 1 | ✅ |
| `accent.action` on `bg.surface` | 7.28 : 1 | ✅ |

### 2.5 禁止

- 系统默认蓝（SwiftUI `.blue` / `accentColor`、Compose `Color.Blue`）不许出现在产品界面；仪表盘也用 `accent.data`。
- 颜色不是唯一信息通道：任何状态都要配图标或文字（画路走模式、驾驶模式、规则 chip 选中态现在全靠颜色，要补勾 / `isSelected`）。
- 一个界面只允许一个 `accent.action`；第二个强调色只能是 `accent.data` / `accent.info` 这类功能色。
- 半透明白字不低于 50%（`text.tertiary` 是下限）；低于 3:1 的图形对比度一律不合格。

---

## 3. 字体

字体族：iOS `system(design: .rounded)`；Android 目前只有 Roboto。**SF Rounded 与 Roboto 都没有中文字形** → 中文界面里"圆润"自动消失，只有英文看得到。中文圆体要不要随包放（思源黑体圆体 / HarmonyOS Sans SC）需要拍板，见 `docs/UI_AUDIT_2026-11-12.md` 第七节。

| token | 字号/行高 | 字重 | 用处 |
|---|---|---|---|
| `type.display` | 28/34 | semibold | 页面大标题 |
| `type.title` | 20/26 | semibold | 区块标题 |
| `type.body` | 15/21 | regular | 正文 |
| `type.label` | 13/18 | medium | 按钮、chip |
| `type.caption` | 11/14 | medium | 副文、时间戳 |
| `type.data` | 11/14 | monospaced | 只有刻度、tick、ID |
| `type.micro` | 10/12 | monospaced | 只有仪表刻度 |

规则：

- 中文正文不小于 13 pt；**不用 9 pt 以下**（现在的 9 pt 只允许出现在仪表刻度区）。
- `design: .rounded` 固化进 token 定义，**不许逐处手写**（现在 41 处漏写，而且 16/18/19 pt 全是直角——越大越不圆）。
- 数值用等宽（tabular），免得跳动；`.monospacedDigit()` 现在全仓 0 处。
- **iOS 必须用语义字体**（`.body` / `.caption` …）或 `Font.custom(_:size:relativeTo:)`，否则动态字体永远不生效；Android 的 `sp` 已有，但固定 `dp` 容器要改成 `heightIn(min:)`。

---

## 4. 间距、圆角、层级

- 间距基数 4：`4 / 8 / 12 / 16 / 20 / 24 / 32`。屏幕左右安全边距 16。**1/3/5/9/18 这种手调值全部收掉。**
- 圆角：`radius.chip` = 胶囊 · `radius.control` = 14 · `radius.card` = 18 · `radius.panel` = 24 · `radius.thumb` = 12。
- 层级三层：
  1. **chrome 层**（顶栏卡 / 拇指台 / 时间栏）——半透明模糊 + 顶部 12% 白渐变遮罩（保白字可读）
  2. **sheet 层**——不透明 `bg.surface`
  3. **overlay 层**（遮罩 / 弹窗）——`bg.scrim` + `elevation.overlay`
- **同一层不许混用**：`ultraThinMaterial` 与实色卡不能同屏出现（iOS 现在 24 处材质 + 2 处实色）。容器用材质 + 1 px 描边，控件用纯色半透明，选中态才用实色。

---

## 5. 动效

| token | 时长 | 曲线 | 用处 |
|---|---|---|---|
| `motion.quick` | 0.15 | easeOut | 按钮反馈、chip 状态 |
| `motion.base` | 0.25 | easeInOut | 状态切换、胶囊淡入淡出 |
| `motion.slow` | 0.40 | easeOut | sheet 出现、相机切换 |
| `motion.deliberate` | 0.60 | spring(0.32, 0.9) | 技能盘展开、时间轴升起 |

- 减动效：`prefers-reduce-motion` / 动画时长 0 时 → 瞬时，但**不能取消必要的状态反馈**（颜色或文字提示要留下）。两端现在都是 0 处处理。
- 每个动作要有**按下反馈 + 触感**：喂食 success、技能 medium、摔倒 error、路径完成 success（现在全 App 只有摸鸭子 1 处触感）。

---

## 6. 触控与可达

- 最小命中区：iOS 44 pt / Android 48 dp。
- 图标按钮一律 frame 48；同一排圆钮直径必须一致（现在 44/46/50/56 混用）。
- 主行动放在拇指弧区（屏幕下半）；顶栏只留"必须随时可见"的东西；**功能入口不许只放在右上角**（现在「⋯」离拇指弧约 750 pt）。
- 一屏之内，鸭子之外的可点元素 ≤ 7 个（现在 iOS 竖屏 11、Android 竖屏 9）。
- 核心手势（走 / 转身 / 摸）必须有**无障碍替身**：自定义 action "向前走一步 / 左转 / 右转 / 摸摸它"。

---

## 7. 双端映射（同名同值）

| token | iOS（SwiftUI） | Android（Compose） |
|---|---|---|
| `bg.root` | `Color(red: 0.086, green: 0.082, blue: 0.075)` | `Color(0xFF161513)` |
| `bg.surface` | `Color(red: 0.133, green: 0.122, blue: 0.114)` | `Color(0xFF221F1D)` |
| `bg.scrim` | `Color.black.opacity(0.55)` | `Color.Black.copy(alpha = 0.55f)` |
| `text.primary` | `Color(red: 0.965, green: 0.945, blue: 0.910)` | `Color(0xFFF6F1E8)` |
| `text.secondary` | `text.primary.opacity(0.70)` | `Color(0xFFF6F1E8).copy(alpha = 0.70f)` |
| `text.tertiary` | `text.primary.opacity(0.50)` | `Color(0xFFF6F1E8).copy(alpha = 0.50f)` |
| `text.onAccent` | `Color(red: 0.102, green: 0.071, blue: 0.031)` | `Color(0xFF1A1208)` |
| `accent.action` | `Color(red: 1.0, green: 0.541, blue: 0.239)` | `Color(0xFFFF8A3D)` |
| `accent.data` | `Color(red: 0.498, green: 0.698, blue: 0.851)` | `Color(0xFF7FB2D9)` |
| `accent.info` | `Color(red: 0.690, green: 0.561, blue: 0.851)` | `Color(0xFFB08FD9)` |
| `state.good` | `Color(red: 0.525, green: 0.839, blue: 0.643)` | `Color(0xFF86D6A4)` |
| `state.warn` | `Color(red: 0.910, green: 0.706, blue: 0.369)` | `Color(0xFFE8B45E)` |
| `state.bad` | `Color(red: 0.910, green: 0.514, blue: 0.420)` | `Color(0xFFE8836B)` |

代码落点：

- iOS 新建 `ios/DuckApp/Design/DuckTheme.swift`：`Color` 扩展 + `Font` 扩展 + 间距 / 圆角 / 动效常量。
- Android 新建 `android/app/src/main/java/ai/duck/microduck/DuckTheme.kt`：`Color` / `Typography` / `Shape` / `Spacing`。
- iOS 强制深色收成一处：`project.yml` 加 `INFOPLIST_KEY_UIUserInterfaceStyle: Dark`，删掉 10 个 `.preferredColorScheme(.dark)`。
- Android 把 `darkColorScheme()` 填上，取代现在 8 处手写色。

**加 token 必须两端一起加**，并在 CI 里加一条断言比对两端同名 token 值——现在的问题（一个底色 3 种写法、图标和 3D 两个底色、三个品牌色）**全部是跨端漂移，没有断言就一定会再漂回去**。

---

## 8. 落地顺序

| 批次 | 内容 | 风险 |
|---|---|---|
| **P0** | 只建**颜色**一层（上面 13 个值）+ 清掉 9 个主色 / 7 个深色表面 / 4 个高亮值；顺手修图标 | 低（取值已定死，代码侧只是替换） |
| **P0.5** | 补三件事：品牌橙两个 token 的关系写死（§2.3）；舞台底色与图标底色统一；文案表同步机制（现在 iOS 212 条 / Android 182 条中文，靠人记） | 低 |
| **P1** | 字体（`type.*` 七档 + 补 41 处 `design:` + 动态字体）+ 间距 4 基数 + 圆角统一。**中文圆体许可是唯一的硬阻塞** | 中（视觉面广，先做 iOS 再 Android 分开合） |
| **P2** | 动效 token 化 + 减动效 + elevation + 层级材质统一 | 低（不影响功能，可以最后做） |
