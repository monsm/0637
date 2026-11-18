# QA · 图鉴徽章 + 教它说话（2026-11-19）

- 对象：第三批功能点——图鉴徽章（双端，由 `/collection` 真实数据计算）、教它说话（自定义语音别名：核心 `/config voice_aliases` + 双端管理页与启动重放）。
- 方式：3 维对抗审查（core / ios / android）→ 每条发现双镜头验证 → QA 主笔放行判定。24 agent。
- 首判：**block**（1 major + 7 minor 全部确认成立）。随后三路修复（核心 / 契约登记 / 双端对齐），本地全绿。

## major 与修复

| # | 问题 | 修复 |
|---|---|---|
| 1 | **项目自己的契约规矩被绕过**：新功能未登记进 `features.yaml` / `ui-entries.json`（两个 gate 对未登记功能结构性失明）；两端入口不同页（iOS 仪表 / Android 鸭子面板）；Android 鸭子面板实数 7 控件超声明 ≤6；批内注释自认「绕开路由表」。 | 契约先红：台账 42→43、条目 46→49、扫描清单 +4 文件；新增 `alias` 行（duck-panel、2 跳、两端同 id）；徽章挂进既有 `dex` 行的 `ledger_ids`。两端入口统一到「这只鸭子」页；大字模式挪到工具页（两端同迁，id/tag 不变），duck-panel 回到 6。未改任何 caps、未新增路由 case。先红（anchor 缺失）后绿证据齐全。 |

## minor 与修复

| # | 问题 | 修复 |
|---|---|---|
| 2 | 短别名劫持内置短语（教「站」后「站起来」会踢、教「好」劫持「你好」） | 跨表全局最长匹配：别名须不短于最长命中内置短语（等长自定义胜）；内置表也改为全表取最长。测试覆盖「站/站起来」「好/你好/好的」、自定义长词胜内置短词、等长自定义优先。附带：`/heard` 回复的 `phrase` 现在是真正最长命中的短语。 |
| 3 | 字数上限单位不一致（壳按字素簇、核心按 Unicode 标量 → ZWJ emoji UI 判合法核心 400） | 统一契约：**上限 = 24 个 Unicode 标量**。核心文档与测试钉住（ZWJ 家庭 emoji = 7 标量），两壳改为 `unicodeScalars.count` / `codePointCount`。 |
| 4 | 大小写不归一（UI 显示「Sit」「sit」两条、核心只有一条，被覆盖那条仍标已教） | 双端入库/读盘即 `trim + lowercase`（与核心同序），归一键即字典键，替换目标；测试钉住「仅大小写不同是一个词」。 |
| 5 | `/heard` 失败丢上下文（只有 "no such skill"，label 被覆写，客户端只读状态码） | 失败体改为 `{ok:false, heard, phrase, error, reason}`，不动 label/notice；测试断言目标消失后响应仍带词与短语。 |
| 6 | `/heard` 缺 shutting_down 专用门（断电中说会动的词得到「press Start」的错补救） | 按 shutting_down → paused → policy 三道门；声音不受限；测试覆盖断电中 sit 拒绝、hello 照常。 |
| 7 | 「词表不泄漏 /state」测试空转（未 set_state，断言构造上不可能失败） | 先 set_state 标记再断言标记回来（证明管道非空），再断言别名不在 /state。 |

## 验证（修复后最终树，本地）

- Rust 默认 **270+9 全绿**、`--features mujoco --test-threads=1` **314+9 全绿**；本批核心新增 15 条测试，全部先红后绿。
- Android `compileDebugKotlin + testDebugUnitTest` 全绿（含 AliasStoreTest 11、CollectionBadgesTest 8）。
- 六道静态门禁全绿：copy-keys 336=336、ui-baseline 棘轮保持、ui-entries 49 行/93 锚点、routes、ui-features 43 行。
- Swift 编译、Xcode 单测/UI、真实 `/config` 与真机语音：由 CI 与真机裁决。

## 复审判定

三路修复后本地全绿；未再跑第三轮完整对抗复审（修复范围明确且每条都有先红后绿测试）。遗留跟进项：`/heard` 失败体字段变更需两端语音层展示（当前壳只读状态码，属既有行为）。
